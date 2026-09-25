use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::post,
};
use kouga_test::{TestClient, TestDb};

#[tokio::test]
async fn portless_requests_pass_through_normal_authorization() {
    let router = axum::Router::new().route(
        "/items",
        post(|headers: axum::http::HeaderMap, body: String| async move {
            if headers.get("authorization").is_none() {
                return StatusCode::UNAUTHORIZED;
            }
            if headers.get("x-allowed").is_none() {
                return StatusCode::FORBIDDEN;
            }
            if body.is_empty() {
                return StatusCode::BAD_REQUEST;
            }
            StatusCode::CREATED
        }),
    );
    let client = TestClient::new(router);
    let request = || Request::post("/items").body(Body::from("hello")).unwrap();
    assert_eq!(
        client.send(request()).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let request = Request::post("/items")
        .header("authorization", "Bearer test")
        .body(Body::from("hello"))
        .unwrap();
    assert_eq!(client.send(request).await.status(), StatusCode::FORBIDDEN);
    let request = Request::post("/items")
        .header("authorization", "Bearer test")
        .header("x-allowed", "yes")
        .body(Body::empty())
        .unwrap();
    assert_eq!(client.send(request).await.status(), StatusCode::BAD_REQUEST);
    let request = Request::post("/items")
        .header("authorization", "Bearer test")
        .header("x-allowed", "yes")
        .body(Body::from("hello"))
        .unwrap();
    assert_eq!(client.send(request).await.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn separate_schemas_keep_parallel_tests_isolated() {
    let Ok(url) = std::env::var("KOUGA_TEST_DATABASE_URL") else {
        return;
    };
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
    let (first, second) = tokio::try_join!(
        TestDb::connect(&url, fixture),
        TestDb::connect(&url, fixture),
    )
    .unwrap();
    sqlx::query("INSERT INTO items VALUES (1, 'first')")
        .execute(first.db())
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM items")
        .fetch_one(second.db())
        .await
        .unwrap();
    assert_eq!(count, 0);
    first.db().close().await;
    assert!(sqlx::query("SELECT 1").execute(first.db()).await.is_err());
    first.close().await.unwrap();
    second.close().await.unwrap();
}
