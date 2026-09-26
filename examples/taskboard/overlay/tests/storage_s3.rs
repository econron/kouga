//! Run with TEST_DATABASE_URL and BOARD_S3_* against a disposable S3 service.
use axum::{
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Request, StatusCode, header},
};
use kouga_model::{Uuid, sqlx};
use kouga_test::{TestClient, TestDb};
use std::net::SocketAddr;

async fn send(
    client: &TestClient,
    method: &str,
    path: &str,
    body: Vec<u8>,
    token: Option<&str>,
    content_type: &str,
) -> axum::response::Response {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, content_type);
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    client
        .send(
            request
                .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 44444))))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
}

async fn json(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap()).unwrap()
}

async fn register(client: &TestClient, email: &str) -> String {
    let response = send(
        client,
        "POST",
        "/auth/register",
        format!(r#"{{"email":"{email}","password":"correct horse battery"}}"#).into_bytes(),
        None,
        "application/json",
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    json(response).await["data"]["token"]
        .as_str()
        .unwrap()
        .into()
}

#[tokio::test]
async fn s3_owner_failure_and_cleanup_across_app_instances() {
    if std::env::var("TEST_DATABASE_URL").is_err() || std::env::var("BOARD_S3_ENDPOINT").is_err() {
        return;
    }
    unsafe { std::env::set_var("BOARD_STORAGE_BACKEND", "s3") };
    let test = TestDb::from_env("migrations").await.unwrap();
    let db = test.db().clone();
    unsafe { std::env::set_var("KOUGA_ENV", "production") };
    assert!(taskboard::storage::open(db.clone()).is_err());
    unsafe { std::env::set_var("KOUGA_ENV", "test") };
    let first = TestClient::new(taskboard::router().with_state(db.clone()));
    let second = TestClient::new(taskboard::router().with_state(db.clone()));
    let alice = register(&first, "s3-alice@example.invalid").await;
    let bob = register(&first, "s3-bob@example.invalid").await;
    let project = send(
        &first,
        "POST",
        "/projects",
        br#"{"slug":"s3-files","name":"Files"}"#.to_vec(),
        Some(&alice),
        "application/json",
    )
    .await;
    assert_eq!(project.status(), StatusCode::CREATED);
    let project_id = json(project).await["data"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let task = send(
        &first,
        "POST",
        "/tasks",
        format!(r#"{{"project_id":"{project_id}","title":"S3 task"}}"#).into_bytes(),
        Some(&alice),
        "application/json",
    )
    .await;
    assert_eq!(task.status(), StatusCode::CREATED);
    let task_id = json(task).await["data"]["id"].as_str().unwrap().to_owned();
    let url = format!("/tasks/{task_id}/attachments");
    let png = b"\x89PNG\r\n\x1a\nactual data";
    let mut multipart = b"--s3\r\nContent-Disposition: form-data; name=\"file\"; filename=\"../unsafe.png\"\r\nContent-Type: image/png\r\n\r\n".to_vec();
    multipart.extend_from_slice(png);
    multipart.extend_from_slice(b"\r\n--s3--\r\n");
    assert_eq!(
        send(
            &first,
            "POST",
            &url,
            multipart.clone(),
            Some(&bob),
            "multipart/form-data; boundary=s3"
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(
            &first,
            "POST",
            &url,
            multipart,
            Some(&alice),
            "multipart/form-data; boundary=s3"
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let mut multipart = b"--s3\r\nContent-Disposition: form-data; name=\"file\"; filename=\"safe.png\"\r\nContent-Type: image/png\r\n\r\n".to_vec();
    multipart.extend_from_slice(png);
    multipart.extend_from_slice(b"\r\n--s3--\r\n");
    let uploaded = send(
        &first,
        "POST",
        &url,
        multipart,
        Some(&alice),
        "multipart/form-data; boundary=s3",
    )
    .await;
    assert_eq!(uploaded.status(), StatusCode::CREATED);
    let file_id = json(uploaded).await["data"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let file_url = format!("{url}/{file_id}");
    assert_eq!(
        send(
            &second,
            "GET",
            &file_url,
            vec![],
            Some(&bob),
            "application/json"
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    let downloaded = send(
        &second,
        "GET",
        &file_url,
        vec![],
        Some(&alice),
        "application/json",
    )
    .await;
    assert_eq!(downloaded.status(), StatusCode::OK);
    assert_eq!(
        downloaded.headers()[header::X_CONTENT_TYPE_OPTIONS],
        "nosniff"
    );
    assert_eq!(
        &to_bytes(downloaded.into_body(), 1024).await.unwrap()[..],
        png
    );

    let endpoint = std::env::var("BOARD_S3_ENDPOINT").unwrap();
    unsafe { std::env::set_var("BOARD_S3_ENDPOINT", "http://127.0.0.1:1") };
    let failed = send(
        &second,
        "DELETE",
        &file_url,
        vec![],
        Some(&alice),
        "application/json",
    )
    .await;
    assert_eq!(failed.status(), StatusCode::SERVICE_UNAVAILABLE);
    unsafe { std::env::set_var("BOARD_S3_ENDPOINT", endpoint) };
    let id = Uuid::parse_str(&file_id).unwrap();
    let state: String = sqlx::query_scalar("SELECT state FROM kouga_files WHERE id=$1")
        .bind(id)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(state, "delete_pending");
    assert_eq!(
        send(
            &first,
            "GET",
            &file_url,
            vec![],
            Some(&alice),
            "application/json"
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        taskboard::attachments::cleanup_once(db.clone())
            .await
            .unwrap(),
        1
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM kouga_files WHERE id=$1")
        .bind(id)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 0);
    test.close().await.unwrap();
}
