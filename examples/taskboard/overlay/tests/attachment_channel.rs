use axum::{
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Request, StatusCode, header},
};
use futures_util::{SinkExt, StreamExt};
use kouga_channel::{Channel, Options};
use kouga_model::{Uuid, sqlx};
use kouga_test::{TestClient, TestDb};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

struct ChannelProcess(std::process::Child);
impl Drop for ChannelProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn send(
    client: &TestClient,
    method: &str,
    path: &str,
    data: Vec<u8>,
    token: Option<&str>,
    content_type: &str,
) -> axum::response::Response {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, content_type);
    if let Some(token) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    client
        .send(
            req.extension(ConnectInfo(std::net::SocketAddr::from((
                [127, 0, 0, 1],
                41337,
            ))))
            .body(Body::from(data))
            .unwrap(),
        )
        .await
}
async fn json(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 1_000_000).await.unwrap()).unwrap()
}
async fn register(client: &TestClient, email: &str) -> (Uuid, String) {
    let data = format!(r#"{{"email":"{email}","password":"correct horse battery"}}"#);
    let response = send(
        client,
        "POST",
        "/auth/register",
        data.into_bytes(),
        None,
        "application/json",
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let data = json(response).await;
    (
        Uuid::parse_str(data["data"]["user"]["id"].as_str().unwrap()).unwrap(),
        data["data"]["token"].as_str().unwrap().to_owned(),
    )
}
fn multipart(png: &[u8]) -> Vec<u8> {
    let mut bytes = b"--fixture\r\nContent-Disposition: form-data; name=\"file\"; filename=\"report.png\"\r\nContent-Type: image/png\r\n\r\n".to_vec();
    bytes.extend_from_slice(png);
    bytes.extend_from_slice(b"\r\n--fixture--\r\n");
    bytes
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
                Some(Ok(Message::Ping(ping))) => ws.send(Message::Pong(ping)).await.unwrap(),
                other => panic!("expected WebSocket text: {other:?}"),
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn owner_attachment_cross_server_events_and_reset() {
    if std::env::var("TEST_DATABASE_URL").is_err() {
        return;
    }
    let root = std::env::temp_dir().join(format!("taskboard-files-{}", Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    // One test owns this generated application's process environment.
    unsafe {
        std::env::set_var("BOARD_STORAGE_ROOT", &root);
    }
    let isolated = TestDb::from_env("migrations").await.unwrap();
    let db = isolated.db().clone();
    let http = TestClient::new(taskboard::router().with_state(db.clone()));
    let (alice, token) = register(&http, "t37-alice@example.com").await;
    let (_, bob_token) = register(&http, "t37-bob@example.com").await;
    let project = send(
        &http,
        "POST",
        "/projects",
        br#"{"slug":"files","name":"Files"}"#.to_vec(),
        Some(&token),
        "application/json",
    )
    .await;
    assert_eq!(project.status(), StatusCode::CREATED);
    let project = json(project).await;
    let project_id = project["data"]["id"].as_str().unwrap();
    let task = send(
        &http,
        "POST",
        "/tasks",
        format!(r#"{{"project_id":"{project_id}","title":"Prepare"}}"#).into_bytes(),
        Some(&token),
        "application/json",
    )
    .await;
    assert_eq!(task.status(), StatusCode::CREATED);
    let task = json(task).await;
    let task_id = task["data"]["id"].as_str().unwrap();
    let upload_url = format!("/tasks/{task_id}/attachments");
    let png = b"\x89PNG\r\n\x1a\ncontent";
    assert_eq!(
        send(
            &http,
            "POST",
            &upload_url,
            multipart(png),
            Some(&bob_token),
            "multipart/form-data; boundary=fixture"
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    let uploaded = send(
        &http,
        "POST",
        &upload_url,
        multipart(png),
        Some(&token),
        "multipart/form-data; boundary=fixture",
    )
    .await;
    assert_eq!(uploaded.status(), StatusCode::CREATED);
    let file_id = json(uploaded).await["data"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let file_url = format!("{upload_url}/{file_id}");
    assert_eq!(
        send(
            &http,
            "GET",
            &file_url,
            vec![],
            Some(&bob_token),
            "application/json"
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    let downloaded = send(
        &http,
        "GET",
        &file_url,
        vec![],
        Some(&token),
        "application/json",
    )
    .await;
    assert_eq!(downloaded.status(), StatusCode::OK);
    assert_eq!(
        downloaded.headers()[header::CONTENT_DISPOSITION],
        "attachment"
    );
    assert_eq!(
        downloaded.headers()[header::X_CONTENT_TYPE_OPTIONS],
        "nosniff"
    );
    assert_eq!(
        &to_bytes(downloaded.into_body(), 1024).await.unwrap()[..],
        png
    );

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let schema: String = sqlx::query_scalar("SELECT current_schema()")
        .fetch_one(&db)
        .await
        .unwrap();
    let url = format!(
        "{}?options=-csearch_path%3D{schema}",
        std::env::var("TEST_DATABASE_URL").unwrap()
    );
    let server = ChannelProcess(
        std::process::Command::new(env!("CARGO_BIN_EXE_taskboard-channel"))
            .env("DATABASE_URL", url)
            .env("BOARD_CHANNEL_ORIGIN", "http://localhost:3000")
            .env("BOARD_CHANNEL_AUTH_CHECK_MS", "100")
            .env("BOARD_CHANNEL_BIND", address.to_string())
            .spawn()
            .unwrap(),
    );
    let mut ready = false;
    for _ in 0..50 {
        if tokio::net::TcpStream::connect(address).await.is_ok() {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(ready, "independent Channel process did not start");
    let request = |ticket: &str| {
        let mut req = format!("ws://{address}/_kouga/ws")
            .into_client_request()
            .unwrap();
        req.headers_mut()
            .insert(header::ORIGIN, "http://localhost:3000".parse().unwrap());
        req.headers_mut().insert(
            header::SEC_WEBSOCKET_PROTOCOL,
            format!("kouga, kouga-ticket.{ticket}").parse().unwrap(),
        );
        req
    };
    let ticket = Channel::start(
        db.clone(),
        Options {
            allowed_origins: vec!["http://localhost:3000".into()],
            ..Options::default()
        },
        taskboard::realtime::policy,
    )
    .await
    .unwrap()
    .issue_ticket(&bob_token)
    .await
    .unwrap()
    .unwrap();
    let (mut denied, _) = connect_async(request(&ticket)).await.unwrap();
    denied
        .send(Message::Text(
            format!(
                r#"{{"type":"subscribe","channel":"{}"}}"#,
                taskboard::realtime::owner_channel(alice)
            )
            .into(),
        ))
        .await
        .unwrap();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(2), denied.next())
            .await
            .unwrap(),
        Some(Ok(Message::Close(_))) | None
    ));
    let source = Channel::start(
        db.clone(),
        Options {
            allowed_origins: vec!["http://localhost:3000".into()],
            ..Options::default()
        },
        taskboard::realtime::policy,
    )
    .await
    .unwrap();
    let ticket = source.issue_ticket(&token).await.unwrap().unwrap();
    let (mut ws, _) = connect_async(request(&ticket)).await.unwrap();
    ws.send(Message::Text(
        format!(
            r#"{{"type":"subscribe","channel":"{}"}}"#,
            taskboard::realtime::owner_channel(alice)
        )
        .into(),
    ))
    .await
    .unwrap();
    assert!(next_text(&mut ws).await.contains("subscribed"));
    let task_url = format!("/tasks/{task_id}");
    assert_eq!(
        send(
            &http,
            "PATCH",
            &task_url,
            br#"{"title":"Prepared"}"#.to_vec(),
            Some(&token),
            "application/json"
        )
        .await
        .status(),
        StatusCode::OK
    );
    let event = next_text(&mut ws).await;
    assert!(event.contains("task_changed") && event.contains("updated") && event.contains(task_id));

    let old_ticket = source.issue_ticket(&token).await.unwrap().unwrap();
    let reset_token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    sqlx::query("INSERT INTO kouga_password_resets(token_hash,user_id,expires_at) VALUES ($1,$2,now() + interval '30 minutes')")
        .bind(Sha256::digest(reset_token.as_bytes()).to_vec()).bind(alice).execute(&db).await.unwrap();
    let reset = send(
        &http,
        "POST",
        "/auth/password/reset",
        format!(r#"{{"token":"{reset_token}","password":"new correct password"}}"#).into_bytes(),
        None,
        "application/json",
    )
    .await;
    assert_eq!(reset.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        send(
            &http,
            "GET",
            "/auth/me",
            vec![],
            Some(&token),
            "application/json"
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert!(source.issue_ticket(&token).await.unwrap().is_none());
    assert!(
        matches!(connect_async(request(&old_ticket)).await, Err(tokio_tungstenite::tungstenite::Error::Http(response)) if response.status() == StatusCode::UNAUTHORIZED)
    );
    let closed = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => continue,
                value => break value,
            }
        }
    })
    .await
    .unwrap();
    assert!(matches!(closed, Some(Ok(Message::Close(_))) | None));

    let key: String = sqlx::query_scalar("SELECT storage_key FROM kouga_files WHERE id=$1")
        .bind(Uuid::parse_str(&file_id).unwrap())
        .fetch_one(&db)
        .await
        .unwrap();
    let object_path = root.join(&key);
    std::fs::remove_file(&object_path).unwrap();
    std::fs::create_dir(&object_path).unwrap();
    let login = send(
        &http,
        "POST",
        "/auth/login",
        br#"{"email":"t37-alice@example.com","password":"new correct password"}"#.to_vec(),
        None,
        "application/json",
    )
    .await;
    assert_eq!(login.status(), StatusCode::OK);
    let new_token = json(login).await["data"]["token"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        send(
            &http,
            "DELETE",
            &file_url,
            vec![],
            Some(&bob_token),
            "application/json"
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(
            &http,
            "DELETE",
            &file_url,
            vec![],
            Some(&new_token),
            "application/json"
        )
        .await
        .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let state: String = sqlx::query_scalar("SELECT state FROM kouga_files WHERE id=$1")
        .bind(Uuid::parse_str(&file_id).unwrap())
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(state, "delete_pending");
    std::fs::remove_dir(&object_path).unwrap();
    assert_eq!(
        taskboard::attachments::cleanup_once(db.clone())
            .await
            .unwrap(),
        1
    );
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM kouga_files WHERE id=$1")
        .bind(Uuid::parse_str(&file_id).unwrap())
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(remaining, 0);
    drop(server);
    drop(source);
    drop(http);
    drop(db);
    isolated.close().await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
