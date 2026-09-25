use kouga_auth::{Decision, authorize, issue_token};
use kouga_core::{Error, ErrorKind, Patch};
use kouga_grpc::{
    InFlight, normalize_client_timeout, normalize_message_size_status, require_bearer, to_status,
    validate_input, within_deadline,
};
use kouga_model::{Model, find};
use kouga_test::TestDb;
use kouga_validation::{Request as ValidatedRequest, ValidationError, ValidationErrors};
use std::{error::Error as StdError, time::Duration};
use tonic::{Request, Response, Status, transport::Server};
use tower::util::MapResponseLayer;
use uuid::Uuid;

mod rpc {
    tonic::include_proto!("kouga_fixture");
}

#[derive(Clone)]
struct Tasks {
    db: kouga_db::Db,
    owner: Uuid,
    inflight: InFlight,
}

#[allow(dead_code)]
#[derive(Model)]
#[model(table = "grpc_tasks")]
struct TaskRow {
    id: Uuid,
    owner_id: Uuid,
    title: String,
}

struct CreateInput {
    title: String,
}

impl ValidatedRequest for CreateInput {
    type Context = ();

    fn validate_sync(&self, errors: &mut ValidationErrors) {
        if self.title.trim().is_empty() {
            errors.push(ValidationError::new("required").at("title"));
        }
    }

    async fn validate_async(&self, _: &(), _: &mut ValidationErrors) -> Result<(), Error> {
        Ok(())
    }
}

fn patch_from_proto(
    value: rpc::UpdateRequest,
) -> Result<(Patch<String>, Patch<Option<String>>), Status> {
    let title = value.title.map_or(Patch::Missing, Patch::Value);
    let description = match value.description_change {
        None => Patch::Missing,
        Some(rpc::update_request::DescriptionChange::Description(value)) => {
            Patch::Value(Some(value))
        }
        Some(rpc::update_request::DescriptionChange::ClearDescription(true)) => Patch::Value(None),
        Some(rpc::update_request::DescriptionChange::ClearDescription(false)) => {
            return Err(Status::invalid_argument("clear_description must be true"));
        }
    };
    Ok((title, description))
}

#[tonic::async_trait]
impl rpc::tasks_server::Tasks for Tasks {
    async fn create(
        &self,
        request: Request<rpc::CreateRequest>,
    ) -> Result<Response<rpc::Task>, Status> {
        let _permit = self.inflight.try_acquire()?;
        let actor = require_bearer(request.metadata(), &self.db).await?;
        authorize(actor, &"create", &self.owner, |actor, _, owner| {
            if actor.id == *owner {
                Decision::Permit
            } else {
                Decision::Deny
            }
        })
        .map_err(to_status)?;
        let input = validate_input(
            CreateInput {
                title: request.into_inner().title,
            },
            &(),
        )
        .await?;
        within_deadline(Duration::from_millis(60), async {
            if input.title == "slow" {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            let mut tx = self
                .db
                .begin()
                .await
                .map_err(|error| to_status(kouga_db::DbError::from(error).into_core()))?;
            let id = Uuid::new_v4();
            sqlx::query("INSERT INTO grpc_tasks (id, owner_id, title) VALUES ($1, $2, $3)")
                .bind(id)
                .bind(actor.id)
                .bind(&input.title)
                .execute(&mut *tx)
                .await
                .map_err(|error| to_status(kouga_db::DbError::from(error).into_core()))?;
            kouga_db::commit_transaction(tx)
                .await
                .map_err(|error| to_status(error.into_core()))?;
            let task = find::<TaskRow, _>(&self.db, id)
                .await
                .map_err(|error| to_status(error.into_core()))?
                .ok_or_else(|| {
                    to_status(Error::new(ErrorKind::NotFound, "not_found", "Not found"))
                })?;
            Ok(Response::new(rpc::Task { title: task.title }))
        })
        .await
    }
}

#[test]
fn protobuf_presence_is_not_inferred_from_defaults() {
    let (title, description) = patch_from_proto(rpc::UpdateRequest::default()).unwrap();
    assert_eq!(title, Patch::Missing);
    assert_eq!(description, Patch::Missing);
    let (title, description) = patch_from_proto(rpc::UpdateRequest {
        title: Some(String::new()),
        description_change: Some(rpc::update_request::DescriptionChange::ClearDescription(
            true,
        )),
    })
    .unwrap();
    assert_eq!(title, Patch::Value(String::new()));
    assert_eq!(description, Patch::Value(None));
    let (_, description) = patch_from_proto(rpc::UpdateRequest {
        title: None,
        description_change: Some(rpc::update_request::DescriptionChange::Description(
            String::new(),
        )),
    })
    .unwrap();
    assert_eq!(description, Patch::Value(Some(String::new())));
    assert_eq!(
        patch_from_proto(rpc::UpdateRequest {
            title: None,
            description_change: Some(rpc::update_request::DescriptionChange::ClearDescription(
                false
            )),
        })
        .unwrap_err()
        .code(),
        tonic::Code::InvalidArgument
    );
}

#[tokio::test]
async fn real_server_auth_validation_policy_limits_and_transaction()
-> Result<(), Box<dyn StdError + Send + Sync>> {
    let Ok(url) = std::env::var("KOUGA_TEST_DATABASE_URL") else {
        return Ok(());
    };
    let isolated = TestDb::connect(
        &url,
        concat!(env!("CARGO_MANIFEST_DIR"), "/../kouga-auth/migrations"),
    )
    .await?;
    sqlx::query("CREATE TABLE grpc_tasks (id uuid PRIMARY KEY, owner_id uuid NOT NULL, title text NOT NULL)")
        .execute(isolated.db()).await?;
    let owner = Uuid::new_v4();
    let token = issue_token(isolated.db(), owner, Duration::from_secs(60)).await?;
    let foreign = issue_token(isolated.db(), Uuid::new_v4(), Duration::from_secs(60)).await?;
    let service = rpc::tasks_server::TasksServer::new(Tasks {
        db: isolated.db().clone(),
        owner,
        inflight: InFlight::new(1),
    })
    .max_decoding_message_size(64);
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    drop(listener);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        Server::builder()
            .layer(MapResponseLayer::new(normalize_message_size_status))
            .timeout(Duration::from_secs(1))
            .add_service(service)
            .serve_with_shutdown(addr, async {
                let _ = stop_rx.await;
            })
            .await
    });
    let mut client = loop {
        match rpc::tasks_client::TasksClient::connect(format!("http://{addr}")).await {
            Ok(client) => break client,
            Err(_) => tokio::time::sleep(Duration::from_millis(10)).await,
        }
    };
    let bare = client
        .create(rpc::CreateRequest {
            title: "one".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(bare.code(), tonic::Code::Unauthenticated);
    let call = |title: &str, token: &str| {
        let mut request = Request::new(rpc::CreateRequest {
            title: title.to_owned(),
        });
        request
            .metadata_mut()
            .insert("authorization", format!("Bearer {token}").parse().unwrap());
        request
    };
    let mut duplicate = call("one", &token);
    duplicate
        .metadata_mut()
        .append("authorization", "Bearer duplicate".parse()?);
    assert_eq!(
        client.create(duplicate).await.unwrap_err().code(),
        tonic::Code::Unauthenticated
    );
    assert_eq!(
        client
            .create(call("one", "invalid"))
            .await
            .unwrap_err()
            .code(),
        tonic::Code::Unauthenticated
    );
    assert_eq!(
        client
            .create(call("one", &foreign))
            .await
            .unwrap_err()
            .code(),
        tonic::Code::PermissionDenied
    );
    assert_eq!(
        client.create(call("", &token)).await.unwrap_err().code(),
        tonic::Code::InvalidArgument
    );
    assert_eq!(
        client
            .create(call(&"x".repeat(100), &token))
            .await
            .unwrap_err()
            .code(),
        tonic::Code::ResourceExhausted
    );
    let mut busy = client.clone();
    let slow_call = call("slow", &token);
    let slow = tokio::spawn(async move { busy.create(slow_call).await });
    tokio::time::sleep(Duration::from_millis(15)).await;
    assert_eq!(
        client.create(call("one", &token)).await.unwrap_err().code(),
        tonic::Code::ResourceExhausted
    );
    assert_eq!(
        slow.await?.unwrap_err().code(),
        tonic::Code::DeadlineExceeded
    );
    let mut client_deadline = call("slow", &token);
    client_deadline.set_timeout(Duration::from_millis(10));
    let timeout = normalize_client_timeout(client.create(client_deadline).await.unwrap_err());
    assert_eq!(timeout.code(), tonic::Code::DeadlineExceeded, "{timeout:?}");
    assert_eq!(
        client.create(call("one", &token)).await?.into_inner().title,
        "one"
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM grpc_tasks")
        .fetch_one(isolated.db())
        .await?;
    assert_eq!(count, 1, "only the successful call commits");
    isolated.db().close().await;
    assert_eq!(
        client.create(call("one", &token)).await.unwrap_err().code(),
        tonic::Code::Unavailable
    );
    let _ = stop_tx.send(());
    server.await??;
    drop(client);
    isolated.close().await?;
    Ok(())
}

#[test]
fn core_errors_do_not_expose_sources() {
    let status = to_status(
        Error::new(ErrorKind::Internal, "internal", "Internal error")
            .with_source(std::io::Error::other("secret")),
    );
    assert_eq!(status.code(), tonic::Code::Internal);
    assert!(!status.message().contains("secret"));
}
