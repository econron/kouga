use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use kouga_auth::CurrentUser;
use kouga_db::Db;
use kouga_http::{Endpoint, Extension, Json, Operation, Router, auth::require_bearer};
use tower::ServiceExt;

fn router(db: Db) -> axum::Router {
    Router::<Db>::new()
        .middleware(require_bearer(|db: &Db| db))
        .get("/me", Endpoint::handler(
            |Extension(actor): Extension<CurrentUser>| async move { Json(actor.id.to_string()) },
            Operation::new("auth.me").response::<Json<String>>(),
        ))
        .unwrap()
        .with_state(db)
}

#[tokio::test]
async fn bearer_rejection_and_store_failure_are_distinct() {
    let db = sqlx::postgres::PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_millis(50))
        .connect_lazy("postgres://test:test@127.0.0.1:1/test")
        .unwrap();
    for header_value in [None, Some("Basic secret"), Some("Bearer bad")] {
        let mut builder = Request::builder().uri("/me");
        if let Some(value) = header_value {
            builder = builder.header(header::AUTHORIZATION, value);
        }
        let response = router(db.clone())
            .oneshot(builder.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[header::WWW_AUTHENTICATE], "Bearer");
    }
    let response = router(db.clone())
        .oneshot(
            Request::builder()
                .uri("/me")
                .header(header::AUTHORIZATION, format!("Bearer {}", "a".repeat(64)))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = to_bytes(response.into_body(), 1024).await.unwrap();
    assert!(!String::from_utf8_lossy(&body).contains("127.0.0.1"));
}
