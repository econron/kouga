//! Set KOUGA_TEST_DATABASE_URL to run against a disposable PostgreSQL database.

use chrono::{Duration as ChronoDuration, Utc};
use kouga_queue::{Enqueue, EnqueueOptions, SCHEMA_SQL, TraceMetadata};
use std::time::Duration;

#[kouga_job::job(name = "welcome", version = 1, queue = "mail")]
struct Welcome {
    user_id: i64,
}

#[tokio::test]
async fn enqueue_reads_back_and_respects_transaction() {
    let Ok(url) = std::env::var("KOUGA_TEST_DATABASE_URL") else {
        eprintln!("skipping PostgreSQL integration test: KOUGA_TEST_DATABASE_URL is unset");
        return;
    };
    let db = kouga_db::connect(&url, 2, Duration::from_secs(2))
        .await
        .unwrap();
    let schema_exists: bool = sqlx::query_scalar("SELECT to_regclass('kouga_jobs') IS NOT NULL")
        .fetch_one(&db)
        .await
        .unwrap();
    if !schema_exists {
        sqlx::raw_sql(SCHEMA_SQL).execute(&db).await.unwrap();
    }
    sqlx::query("CREATE TABLE IF NOT EXISTS kouga_t20_business (id bigint PRIMARY KEY)")
        .execute(&db)
        .await
        .unwrap();
    let business_id = Utc::now().timestamp_micros();

    let at = Utc::now() + ChronoDuration::hours(1);
    let mut tx = db.begin().await.unwrap();
    sqlx::query("INSERT INTO kouga_t20_business (id) VALUES ($1)")
        .bind(business_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    let id = Welcome { user_id: 7 }
        .enqueue_with(
            &mut tx,
            EnqueueOptions {
                available_at: Some(at),
                trace: TraceMetadata {
                    traceparent: Some(
                        "00-0123456789abcdef0123456789abcdef-0123456789abcdef-01".into(),
                    ),
                    tracestate: None,
                },
            },
        )
        .await
        .unwrap();
    let inside: (String, i32, String, serde_json::Value, chrono::DateTime<Utc>, Option<String>) =
        sqlx::query_as("SELECT name, version, queue, payload, available_at, traceparent FROM kouga_jobs WHERE id = $1")
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(
        (&inside.0, inside.1, &inside.2),
        (&"welcome".to_string(), 1, &"mail".to_string())
    );
    assert_eq!(inside.3["user_id"], 7);
    assert_eq!(inside.4, at);
    assert!(inside.5.is_some());
    let outside: i64 = sqlx::query_scalar("SELECT count(*) FROM kouga_jobs WHERE id = $1")
        .bind(id)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(outside, 0);
    tx.rollback().await.unwrap();
    let business_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM kouga_t20_business WHERE id = $1")
            .bind(business_id)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(business_count, 0);
    let outside: i64 = sqlx::query_scalar("SELECT count(*) FROM kouga_jobs WHERE id = $1")
        .bind(id)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(outside, 0);

    let mut tx = db.begin().await.unwrap();
    sqlx::query("INSERT INTO kouga_t20_business (id) VALUES ($1)")
        .bind(business_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    let committed_id = Welcome { user_id: 9 }.enqueue(&mut tx).await.unwrap();
    tx.commit().await.unwrap();
    let business_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM kouga_t20_business WHERE id = $1")
            .bind(business_id)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(business_count, 1);
    let payload: serde_json::Value =
        sqlx::query_scalar("SELECT payload FROM kouga_jobs WHERE id = $1")
            .bind(committed_id)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(payload["user_id"], 9);

    let id = Welcome { user_id: 8 }.enqueue_at(&db, at).await.unwrap();
    let (status, attempt): (String, i32) =
        sqlx::query_as("SELECT status, attempt FROM kouga_jobs WHERE id = $1")
            .bind(id)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!((status.as_str(), attempt), ("pending", 0));
    sqlx::query("DELETE FROM kouga_jobs WHERE id = $1")
        .bind(id)
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM kouga_jobs WHERE id = $1")
        .bind(committed_id)
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM kouga_t20_business WHERE id = $1")
        .bind(business_id)
        .execute(&db)
        .await
        .unwrap();
}
