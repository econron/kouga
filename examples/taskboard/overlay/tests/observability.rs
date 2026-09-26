use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
};
use kouga_http::{Endpoint, Json, Operation, Router};
use kouga_test::{TestClient, TestDb};
use std::sync::atomic::{AtomicUsize, Ordering};

static EFFECTS: AtomicUsize = AtomicUsize::new(0);

async fn probe() -> Json<&'static str> {
    EFFECTS.fetch_add(1, Ordering::SeqCst);
    Json("ok")
}

async fn request(client: &TestClient, path: &str) -> axum::response::Response {
    client
        .send(
            Request::builder()
                .uri(path)
                .extension(ConnectInfo(std::net::SocketAddr::from((
                    [127, 0, 0, 1],
                    42424,
                ))))
                .body(Body::empty())
                .unwrap(),
        )
        .await
}

#[tokio::test]
async fn readiness_and_rate_limit_share_database_across_http_instances() {
    if std::env::var("TEST_DATABASE_URL").is_err() {
        return;
    }
    let isolated = TestDb::from_env("migrations").await.unwrap();
    EFFECTS.store(0, Ordering::SeqCst);
    let router = || {
        taskboard::observability::routes_with_limit(
            Router::new()
                .get(
                    "/probe",
                    Endpoint::handler(probe, Operation::new("probe").response::<Json<&str>>()),
                )
                .unwrap(),
            2,
        )
    };
    let first = TestClient::new(router().with_state(isolated.db().clone()));
    let second = TestClient::new(router().with_state(isolated.db().clone()));
    assert_eq!(request(&first, "/ready").await.status(), StatusCode::OK);
    assert_eq!(request(&first, "/probe").await.status(), StatusCode::OK);
    assert_eq!(request(&second, "/probe").await.status(), StatusCode::OK);
    let denied = request(&first, "/probe").await;
    assert_eq!(denied.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(denied.headers().contains_key("retry-after"));
    assert_eq!(EFFECTS.load(Ordering::SeqCst), 2);
    isolated.db().close().await;
    assert_eq!(
        request(&second, "/ready").await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        request(&second, "/probe").await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(EFFECTS.load(Ordering::SeqCst), 2);
    isolated.close().await.unwrap();
}

#[tokio::test]
async fn taskboard_docs_are_development_only() {
    if std::env::var("TEST_DATABASE_URL").is_err() {
        return;
    }
    let isolated = TestDb::from_env("migrations").await.unwrap();
    let development = TestClient::new(
        kouga_openapi::serve(
            taskboard::router(),
            isolated.db().clone(),
            "Taskboard",
            "0.1.0",
            true,
        )
        .unwrap(),
    );
    assert_eq!(
        request(&development, "/openapi.yml").await.status(),
        StatusCode::OK
    );
    assert_eq!(
        request(&development, "/docs/swagger-ui-bundle.js")
            .await
            .status(),
        StatusCode::OK
    );
    let production = TestClient::new(
        kouga_openapi::serve(
            taskboard::router(),
            isolated.db().clone(),
            "Taskboard",
            "0.1.0",
            false,
        )
        .unwrap(),
    );
    assert_eq!(
        request(&production, "/openapi.yml").await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(&production, "/docs").await.status(),
        StatusCode::NOT_FOUND
    );
    isolated.close().await.unwrap();
}
