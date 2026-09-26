use axum::{
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Request as HttpRequest, StatusCode},
};
use kouga_test::{TestClient, TestDb};
use std::time::Duration;
use taskboard_grpc::BoardService;
use taskboard_rpc::rpc::{self, board_client::BoardClient, board_server::BoardServer};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Code, Request, metadata::MetadataValue};

async fn send(
    client: &TestClient,
    method: &str,
    path: &str,
    body: &str,
    token: Option<&str>,
) -> axum::response::Response {
    let mut request = HttpRequest::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    client
        .send(
            request
                .extension(ConnectInfo(std::net::SocketAddr::from((
                    [127, 0, 0, 1],
                    41335,
                ))))
                .body(Body::from(body.to_owned()))
                .unwrap(),
        )
        .await
}

async fn json(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 1_000_000).await.unwrap()).unwrap()
}

async fn register(client: &TestClient, email: &str) -> String {
    let response = send(
        client,
        "POST",
        "/auth/register",
        &format!(r#"{{"email":"{email}","password":"correct horse battery"}}"#),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    json(response).await["data"]["token"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn authorized<T>(payload: T, token: &str) -> Request<T> {
    let mut request = Request::new(payload);
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {token}")).unwrap(),
    );
    request
}

#[tokio::test]
async fn http_and_grpc_share_owner_scoped_board() {
    if std::env::var("TEST_DATABASE_URL").is_err() {
        return;
    }
    let isolated = TestDb::from_env(concat!(env!("CARGO_MANIFEST_DIR"), "/../../migrations"))
        .await
        .unwrap();
    let http = TestClient::new(taskboard::router().with_state(isolated.db().clone()));
    let alice = register(&http, "grpc-alice@example.com").await;
    let bob = register(&http, "grpc-bob@example.com").await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tonic::transport::Server::builder()
        .add_service(BoardServer::new(BoardService::new(isolated.db().clone())))
        .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async move {
            let _ = stopped.await;
        });
    let handle = tokio::spawn(server);
    let mut grpc = BoardClient::connect(format!("http://{address}"))
        .await
        .unwrap();

    assert_eq!(
        grpc.create_project(Request::new(rpc::CreateProjectRequest {
            slug: "unauthorized".into(),
            name: "No".into(),
        }))
        .await
        .unwrap_err()
        .code(),
        Code::Unauthenticated
    );
    assert_eq!(
        grpc.create_project(authorized(
            rpc::CreateProjectRequest {
                slug: "bad slug".into(),
                name: "No".into(),
            },
            &alice,
        ))
        .await
        .unwrap_err()
        .code(),
        Code::InvalidArgument
    );

    let response = send(
        &http,
        "POST",
        "/projects",
        r#"{"slug":"web","name":"Web"}"#,
        Some(&alice),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let project_id = json(response).await["data"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let task = grpc
        .create_task(authorized(
            rpc::CreateTaskRequest {
                project_id: project_id.clone(),
                title: "From gRPC".into(),
            },
            &alice,
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        json(
            send(
                &http,
                "GET",
                &format!("/tasks/{}", task.id),
                "",
                Some(&alice)
            )
            .await
        )
        .await["data"]["title"],
        "From gRPC"
    );
    assert_eq!(
        grpc.get_task(authorized(
            rpc::GetTaskRequest {
                id: task.id.clone()
            },
            &bob
        ))
        .await
        .unwrap_err()
        .code(),
        Code::NotFound
    );
    assert_eq!(
        grpc.create_task(authorized(
            rpc::CreateTaskRequest {
                project_id: project_id.clone(),
                title: "Other owner".into()
            },
            &bob
        ))
        .await
        .unwrap_err()
        .code(),
        Code::NotFound
    );
    assert_eq!(
        grpc.create_task(authorized(
            rpc::CreateTaskRequest {
                project_id: project_id.clone(),
                title: " ".into()
            },
            &alice
        ))
        .await
        .unwrap_err()
        .code(),
        Code::InvalidArgument
    );
    assert_eq!(
        grpc.create_project(authorized(
            rpc::CreateProjectRequest {
                slug: "web".into(),
                name: "Duplicate".into()
            },
            &alice
        ))
        .await
        .unwrap_err()
        .code(),
        Code::FailedPrecondition
    );

    grpc.complete_task(authorized(
        rpc::CompleteTaskRequest {
            id: task.id.clone(),
        },
        &alice,
    ))
    .await
    .unwrap();
    let count = json(
        send(
            &http,
            "GET",
            &format!("/projects/{project_id}/count"),
            "",
            Some(&alice),
        )
        .await,
    )
    .await;
    assert_eq!(count["data"]["completed"], 1);

    let mut expired = authorized(rpc::GetProjectCountRequest { project_id }, &alice);
    expired.set_timeout(Duration::from_nanos(1));
    let timeout = grpc.get_project_count(expired).await.unwrap_err();
    assert_eq!(
        kouga_grpc::normalize_client_timeout(timeout).code(),
        Code::DeadlineExceeded
    );

    stop.send(()).unwrap();
    handle.await.unwrap().unwrap();
}
