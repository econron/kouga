//! Copy to tests/advanced.rs in a generated app after adding the dev dependencies
//! described in user-docs/tutorial.md. Requires TEST_DATABASE_URL.
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt, stream};
use kouga_auth::{CurrentUser, issue_token};
use kouga_channel::{Action, Channel, Options};
use kouga_model::{Model, Uuid, sqlx};
use kouga_storage::{FileKind, Storage, StorageError, Upload};
use kouga_test::TestDb;
use std::{io, time::Duration};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

#[derive(Clone, Debug, Model)]
#[model(table = "journey_projects")]
struct JourneyProject {
    id: Uuid,
    name: String,
}

#[derive(Clone, Debug, Model)]
#[model(table = "journey_tasks")]
#[belongs_to(JourneyProject, key = project_id, name = project)]
struct JourneyTask {
    id: Uuid,
    project_id: Uuid,
    title: String,
}

#[tokio::test]
async fn association_and_local_attachment() {
    if std::env::var("TEST_DATABASE_URL").is_err() {
        return;
    }
    let test = TestDb::from_env("migrations").await.unwrap();
    let db = test.db();
    sqlx::raw_sql(
        "CREATE TABLE journey_projects (id uuid PRIMARY KEY, name text NOT NULL); \
         CREATE TABLE journey_tasks (id uuid PRIMARY KEY, project_id uuid NOT NULL REFERENCES journey_projects(id), title text NOT NULL);",
    )
    .execute(db)
    .await
    .unwrap();
    let project_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    sqlx::query("INSERT INTO journey_projects(id,name) VALUES ($1,$2)")
        .bind(project_id)
        .bind("guide")
        .execute(db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO journey_tasks(id,project_id,title) VALUES ($1,$2,$3)")
        .bind(task_id)
        .bind(project_id)
        .bind("write docs")
        .execute(db)
        .await
        .unwrap();
    let rows = JourneyTask::query()
        .preload(journey_task::relations::project())
        .fetch_all(db)
        .await
        .unwrap();
    assert_eq!(rows[0].related.name, "guide");
    assert_eq!(rows[0].model.title, "write docs");
    assert_eq!(rows[0].model.project(db).await.unwrap().id, project_id);

    sqlx::raw_sql(kouga_storage::SCHEMA_SQL)
        .execute(db)
        .await
        .unwrap();
    let root = std::env::temp_dir().join(format!("kouga-docs-storage-{}", Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let storage = Storage::local(db.clone(), &root).unwrap();
    let owner = CurrentUser { id: Uuid::new_v4() };
    let stranger = CurrentUser { id: Uuid::new_v4() };
    let png = b"\x89PNG\r\n\x1a\n1234";
    let saved = storage
        .save(
            Upload {
                owner,
                filename: "image.png",
                declared_content_type: "image/png",
                allowed: &[FileKind::Png],
                max_bytes: 1024,
            },
            stream::iter([Ok::<Bytes, io::Error>(Bytes::from_static(png))]),
        )
        .await
        .unwrap();
    assert!(
        storage
            .attach(owner, saved.id, "journey_tasks", task_id)
            .await
            .unwrap()
    );
    assert!(matches!(
        storage.download(stranger, saved.id).await,
        Err(StorageError::NotFound)
    ));
    let download = storage.download(owner, saved.id).await.unwrap();
    let chunks = download.stream.collect::<Vec<_>>().await;
    let actual = chunks
        .into_iter()
        .flat_map(|chunk| chunk.unwrap().to_vec())
        .collect::<Vec<_>>();
    assert_eq!(actual, png);
    storage.delete(owner, saved.id).await.unwrap();
    assert!(matches!(
        storage.download(owner, saved.id).await,
        Err(StorageError::NotFound)
    ));
    drop(storage);
    test.close().await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

async fn next_text(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> String {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => break text.to_string(),
                Some(Ok(Message::Ping(payload))) => ws.send(Message::Pong(payload)).await.unwrap(),
                other => panic!("expected text, got {other:?}"),
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn websocket_ticket_subscribe_and_cross_instance_publish() {
    if std::env::var("TEST_DATABASE_URL").is_err() {
        return;
    }
    let test = TestDb::from_env("migrations").await.unwrap();
    let db = test.db();
    let actor = CurrentUser { id: Uuid::new_v4() };
    sqlx::query("INSERT INTO users(id,email,password_hash) VALUES ($1,$2,$3)")
        .bind(actor.id)
        .bind(format!("journey-{}@example.test", actor.id))
        .bind("unused")
        .execute(db)
        .await
        .unwrap();
    let bearer = issue_token(db, actor.id, Duration::from_secs(60))
        .await
        .unwrap();
    let options = Options {
        allowed_origins: vec!["http://localhost:3000".into()],
        ..Options::default()
    };
    let subscriber = Channel::start(db.clone(), options.clone(), move |user, action, name| {
        user.id == actor.id && name == "updates" && action == Action::Subscribe
    })
    .await
    .unwrap();
    let publisher = Channel::start(db.clone(), options, move |user, action, name| {
        user.id == actor.id && name == "updates" && action == Action::Publish
    })
    .await
    .unwrap();
    let ticket = subscriber.issue_ticket(&bearer).await.unwrap().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, subscriber.router()).await.unwrap();
    });
    let mut request = format!("ws://{address}/_kouga/ws")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("Origin", "http://localhost:3000".parse().unwrap());
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        format!("kouga, kouga-ticket.{ticket}").parse().unwrap(),
    );
    let (mut ws, _) = connect_async(request).await.unwrap();
    ws.send(Message::Text(
        r#"{"type":"subscribe","channel":"updates"}"#.into(),
    ))
    .await
    .unwrap();
    assert!(next_text(&mut ws).await.contains("subscribed"));
    publisher
        .publish(actor, "updates", serde_json::json!({"value": 1}))
        .await
        .unwrap();
    assert!(next_text(&mut ws).await.contains("\"value\":1"));
    ws.close(None).await.unwrap();
    drop(ws);
    server.abort();
    let _ = server.await;
    drop(publisher);
    test.close().await.unwrap();
}
