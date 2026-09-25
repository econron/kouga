use kouga_telemetry::Health;

#[tokio::test]
async fn db_readiness_fails_when_required_pool_is_unavailable() {
    let Ok(url) = std::env::var("KOUGA_TEST_DATABASE_URL") else {
        return;
    };
    let db = kouga_db::connect(&url, 1, std::time::Duration::from_secs(2))
        .await
        .unwrap();
    let health = Health::new();
    let probe = || async { sqlx::query("SELECT 1").execute(&db).await.map(|_| ()) };
    assert_eq!(health.liveness().http_status(), 200);
    assert_eq!(health.readiness(probe()).await.http_status(), 200);
    db.close().await;
    assert_eq!(health.readiness(probe()).await.http_status(), 503);
    assert_eq!(health.liveness().http_status(), 200);
}
