use axum::body::Body;
use http::{Request, StatusCode};
use kouga_cache::{PgCache, RateDecision, RateLimiter, SCHEMA_SQL, rate_limit};
use kouga_http::{Endpoint, HttpRequest, Json, Operation, Router};
use std::time::Duration;
use tower::ServiceExt;

#[tokio::test]
async fn shared_cache_and_rate_limit() {
    let Ok(url) = std::env::var("KOUGA_TEST_DATABASE_URL") else {
        eprintln!("skipping PostgreSQL integration test: KOUGA_TEST_DATABASE_URL is unset");
        return;
    };
    let db = kouga_db::connect(&url, 10, Duration::from_secs(3))
        .await
        .unwrap();
    let exists: bool = sqlx::query_scalar("SELECT to_regclass('kouga_cache') IS NOT NULL")
        .fetch_one(&db)
        .await
        .unwrap();
    if !exists {
        sqlx::raw_sql(SCHEMA_SQL).execute(&db).await.unwrap();
    }
    let namespace = format!("t23_{}", std::process::id());
    let cache = PgCache::new(db.clone());
    cache
        .set(
            &namespace,
            "k",
            &serde_json::json!(7),
            Duration::from_millis(20),
        )
        .await
        .unwrap();
    assert_eq!(
        cache.get(&namespace, "k").await.unwrap(),
        Some(serde_json::json!(7))
    );
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(cache.get(&namespace, "k").await.unwrap(), None);
    assert_eq!(cache.clean().await.unwrap(), 1);
    cache
        .set(
            &namespace,
            "k",
            &serde_json::json!(8),
            Duration::from_secs(1),
        )
        .await
        .unwrap();
    cache.delete(&namespace, "k").await.unwrap();
    assert_eq!(cache.get(&namespace, "k").await.unwrap(), None);

    let limiter = RateLimiter::new(db.clone(), &namespace, 3, Duration::from_secs(2)).unwrap();
    let mut checks = Vec::new();
    for _ in 0..12 {
        let limiter = limiter.clone();
        checks.push(tokio::spawn(
            async move { limiter.check("user").await.unwrap() },
        ));
    }
    let mut allowed = 0;
    for check in checks {
        match check.await.unwrap() {
            RateDecision::Allowed => allowed += 1,
            RateDecision::Denied { retry_after } => assert!(retry_after >= 1),
        }
    }
    assert_eq!(allowed, 3);
    assert_eq!(limiter.check("other").await.unwrap(), RateDecision::Allowed);
    sqlx::query("UPDATE kouga_rate_limits SET window_start = now() - interval '5 seconds' WHERE namespace = $1 AND key = 'user'")
        .bind(&namespace).execute(&db).await.unwrap();
    assert_eq!(limiter.check("user").await.unwrap(), RateDecision::Allowed);
    let http_limiter = RateLimiter::new(
        db.clone(),
        format!("{namespace}_http"),
        1,
        Duration::from_secs(2),
    )
    .unwrap();
    let app = Router::<()>::new()
        .middleware(rate_limit(http_limiter, |_request: &HttpRequest<()>| {
            Some("person".into())
        }))
        .get(
            "/limited",
            Endpoint::handler(
                || async { Json("ok") },
                Operation::new("limited.show").response::<Json<&str>>(),
            ),
        )
        .unwrap()
        .with_state(());
    let first = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/limited")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let denied = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/limited")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(denied.headers().contains_key("retry-after"));
    sqlx::query("DELETE FROM kouga_rate_limits WHERE namespace = $1")
        .bind(&namespace)
        .execute(&db)
        .await
        .unwrap();

    db.close().await;
    assert!(limiter.check("user").await.is_err());
    let unavailable = app
        .oneshot(
            Request::builder()
                .uri("/limited")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unavailable.status(), StatusCode::SERVICE_UNAVAILABLE);
    let value = cache
        .fetch(&namespace, "k", Duration::from_secs(1), || async {
            serde_json::json!(9)
        })
        .await;
    assert_eq!(value, serde_json::json!(9));
}
