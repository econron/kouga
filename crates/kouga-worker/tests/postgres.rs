use std::{
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use kouga_queue::Enqueue;
use kouga_worker::{JobContext, JobError, Worker, WorkerOptions};
use sqlx::{
    Executor,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

#[kouga_job::job(name = "t21_fast", version = 1, queue = "t21")]
struct Fast {
    value: i32,
}
#[kouga_job::job(name = "t21_flaky", version = 1, queue = "t21")]
struct Flaky;
#[kouga_job::job(name = "t21_slow", version = 1, queue = "t21")]
struct Slow;
#[kouga_job::job(name = "t21_unknown", version = 1, queue = "t21")]
struct Unknown;

fn options() -> WorkerOptions {
    WorkerOptions {
        queues: vec!["t21".into()],
        concurrency: 2,
        poll_interval: Duration::from_millis(10),
        lease_duration: Duration::from_millis(120),
        job_timeout: Duration::from_secs(2),
        shutdown_grace: Duration::from_millis(500),
        max_attempts: 2,
        retry_base: Duration::from_millis(10),
        retry_max: Duration::from_millis(10),
    }
}

#[tokio::test]
async fn postgres_worker_lifecycle() {
    let Ok(url) = std::env::var("KOUGA_TEST_DATABASE_URL") else {
        return;
    };
    let admin = kouga_db::connect(&url, 2, Duration::from_secs(5))
        .await
        .unwrap();
    let schema = format!(
        "kouga_t21_{}_{}",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    );
    admin
        .execute(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .await
        .unwrap();
    let search_path = schema.clone();
    let db = PgPoolOptions::new()
        .max_connections(10)
        .after_connect(move |conn, _| {
            let sql = format!("SET search_path TO {search_path}");
            Box::pin(async move {
                conn.execute(sqlx::AssertSqlSafe(sql)).await?;
                Ok(())
            })
        })
        .connect_with(PgConnectOptions::from_str(&url).unwrap())
        .await
        .unwrap();
    db.execute(sqlx::raw_sql(kouga_queue::SCHEMA_SQL))
        .await
        .unwrap();
    db.execute(sqlx::raw_sql(include_str!(
        "../migrations/20260925000021_add_job_failure.up.sql"
    )))
    .await
    .unwrap();

    let count = Arc::new(AtomicUsize::new(0));
    let mut first = Worker::new(db.clone(), count.clone(), options()).unwrap();
    first
        .register::<Fast>(|job: Fast, ctx: JobContext<AtomicUsize>| async move {
            assert!(job.value > 0);
            ctx.state.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
    assert!(
        first
            .register::<Fast>(|_: Fast, _: JobContext<AtomicUsize>| async { Ok(()) })
            .is_err()
    );
    let mut second = Worker::new(db.clone(), count.clone(), options()).unwrap();
    second
        .register::<Fast>(|_: Fast, ctx: JobContext<AtomicUsize>| async move {
            ctx.state.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
    Fast { value: 1 }.enqueue(&db).await.unwrap();
    Fast { value: 2 }.enqueue(&db).await.unwrap();
    let (a, b) = tokio::join!(
        first.run_once(1, Duration::from_secs(1), CancellationToken::new()),
        second.run_once(1, Duration::from_secs(1), CancellationToken::new())
    );
    assert_eq!(a.unwrap().succeeded + b.unwrap().succeeded, 2);
    assert_eq!(count.load(Ordering::SeqCst), 2);

    let flaky = Arc::new(AtomicUsize::new(0));
    let mut worker = Worker::new(db.clone(), flaky.clone(), options()).unwrap();
    worker
        .register::<Flaky>(|_: Flaky, ctx: JobContext<AtomicUsize>| async move {
            if ctx.state.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(JobError::Retryable("temporary"))
            } else {
                Ok(())
            }
        })
        .unwrap();
    let id = Flaky.enqueue(&db).await.unwrap();
    assert_eq!(
        worker
            .run_once(1, Duration::from_secs(1), CancellationToken::new())
            .await
            .unwrap()
            .retried,
        1
    );
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(
        worker
            .run_once(1, Duration::from_secs(1), CancellationToken::new())
            .await
            .unwrap()
            .succeeded,
        1
    );
    let (status, attempt): (String, i32) =
        sqlx::query_as("SELECT status,attempt FROM kouga_jobs WHERE id=$1")
            .bind(id)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!((status.as_str(), attempt), ("succeeded", 2));

    let mut dying = Worker::new(db.clone(), Arc::new(()), options()).unwrap();
    dying
        .register::<Flaky>(|_: Flaky, _: JobContext<()>| async {
            Err(JobError::Retryable("still unavailable"))
        })
        .unwrap();
    let dead_id = Flaky.enqueue(&db).await.unwrap();
    assert_eq!(
        dying
            .run_once(1, Duration::from_secs(1), CancellationToken::new())
            .await
            .unwrap()
            .retried,
        1
    );
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(
        dying
            .run_once(1, Duration::from_secs(1), CancellationToken::new())
            .await
            .unwrap()
            .dead,
        1
    );
    assert_eq!(
        dying.failed(10).await.unwrap()[0].failure_reason.as_deref(),
        Some("still unavailable")
    );
    assert!(dying.retry_failed(dead_id).await.unwrap());
    assert!(dying.cancel_waiting(dead_id).await.unwrap());

    let invalid_id: uuid::Uuid = sqlx::query_scalar("INSERT INTO kouga_jobs (name,version,queue,payload,available_at) VALUES ('t21_fast',1,'t21',$1,now()) RETURNING id")
        .bind(serde_json::json!({"bad": true})).fetch_one(&db).await.unwrap();
    assert_eq!(
        first
            .run_once(1, Duration::from_secs(1), CancellationToken::new())
            .await
            .unwrap()
            .quarantined,
        1
    );
    let invalid_reason: String =
        sqlx::query_scalar("SELECT failure_reason FROM kouga_jobs WHERE id=$1")
            .bind(invalid_id)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(invalid_reason, "invalid payload");

    let unknown = Unknown.enqueue(&db).await.unwrap();
    assert_eq!(
        worker
            .run_once(1, Duration::from_secs(1), CancellationToken::new())
            .await
            .unwrap()
            .quarantined,
        1
    );
    assert!(
        worker
            .failed(10)
            .await
            .unwrap()
            .iter()
            .any(|job| job.id == unknown)
    );
    assert!(worker.retry_failed(unknown).await.unwrap());
    assert!(worker.cancel_waiting(unknown).await.unwrap());

    let slow_calls = Arc::new(AtomicUsize::new(0));
    let mut interrupted = Worker::new(db.clone(), slow_calls.clone(), options()).unwrap();
    interrupted
        .register::<Slow>(|_: Slow, ctx: JobContext<AtomicUsize>| async move {
            if ctx.state.fetch_add(1, Ordering::SeqCst) == 0 {
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
            Ok(())
        })
        .unwrap();
    let slow_id = Slow.enqueue(&db).await.unwrap();
    let handle = tokio::spawn(async move {
        interrupted
            .run_once(1, Duration::from_secs(1), CancellationToken::new())
            .await
    });
    for _ in 0..50 {
        let running: bool =
            sqlx::query_scalar("SELECT status='running' FROM kouga_jobs WHERE id=$1")
                .bind(slow_id)
                .fetch_one(&db)
                .await
                .unwrap();
        if running {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    handle.abort();
    let _ = handle.await;
    tokio::time::sleep(Duration::from_millis(180)).await;
    let mut recovery = Worker::new(db.clone(), slow_calls.clone(), options()).unwrap();
    recovery
        .register::<Slow>(|_: Slow, ctx: JobContext<AtomicUsize>| async move {
            ctx.state.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
    assert_eq!(
        recovery
            .run_once(1, Duration::from_secs(1), CancellationToken::new())
            .await
            .unwrap()
            .succeeded,
        1
    );
    assert_eq!(slow_calls.load(Ordering::SeqCst), 2);
    let status: String = sqlx::query_scalar("SELECT status FROM kouga_jobs WHERE id=$1")
        .bind(slow_id)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(status, "succeeded");

    let stale_calls = Arc::new(AtomicUsize::new(0));
    let mut stale = Worker::new(db.clone(), stale_calls.clone(), options()).unwrap();
    stale
        .register::<Slow>(|_: Slow, ctx: JobContext<AtomicUsize>| async move {
            ctx.state.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(200)).await;
            Ok(())
        })
        .unwrap();
    let stale_id = Slow.enqueue(&db).await.unwrap();
    let stale_handle = tokio::spawn(async move {
        stale
            .run_once(1, Duration::from_secs(1), CancellationToken::new())
            .await
    });
    for _ in 0..50 {
        let running: bool =
            sqlx::query_scalar("SELECT status='running' FROM kouga_jobs WHERE id=$1")
                .bind(stale_id)
                .fetch_one(&db)
                .await
                .unwrap();
        if running {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    sqlx::query("UPDATE kouga_jobs SET lease_until=now()-interval '1 second' WHERE id=$1")
        .bind(stale_id)
        .execute(&db)
        .await
        .unwrap();
    let mut rescuer = Worker::new(db.clone(), Arc::new(()), options()).unwrap();
    rescuer
        .register::<Slow>(|_: Slow, _: JobContext<()>| async { Ok(()) })
        .unwrap();
    assert_eq!(
        rescuer
            .run_once(1, Duration::from_secs(1), CancellationToken::new())
            .await
            .unwrap()
            .succeeded,
        1
    );
    assert_eq!(stale_handle.await.unwrap().unwrap().succeeded, 0);
    let status: String = sqlx::query_scalar("SELECT status FROM kouga_jobs WHERE id=$1")
        .bind(stale_id)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(status, "succeeded");

    let cancelled = Arc::new(AtomicUsize::new(0));
    let entered = Arc::new(Notify::new());
    let mut graceful = Worker::new(db.clone(), cancelled.clone(), options()).unwrap();
    graceful
        .register::<Slow>({
            let entered = entered.clone();
            move |_: Slow, ctx: JobContext<AtomicUsize>| {
                let entered = entered.clone();
                async move {
                    assert!(!ctx.cancellation.is_cancelled());
                    entered.notify_one();
                    ctx.cancellation.cancelled().await;
                    ctx.state.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            }
        })
        .unwrap();
    Slow.enqueue(&db).await.unwrap();
    let stop = CancellationToken::new();
    let stop_worker = stop.clone();
    let graceful_handle = tokio::spawn(async move { graceful.run_forever(stop_worker).await });
    tokio::time::timeout(Duration::from_secs(1), entered.notified())
        .await
        .unwrap();
    assert_eq!(cancelled.load(Ordering::SeqCst), 0);
    stop.cancel();
    assert_eq!(graceful_handle.await.unwrap().unwrap().succeeded, 1);
    assert_eq!(cancelled.load(Ordering::SeqCst), 1);
    assert_eq!(
        Worker::new(db.clone(), Arc::new(()), options())
            .unwrap()
            .run_once(1, Duration::from_millis(50), CancellationToken::new())
            .await
            .unwrap()
            .claimed,
        0
    );

    let mut timed_options = options();
    timed_options.concurrency = 1;
    let mut timed = Worker::new(db.clone(), Arc::new(()), timed_options).unwrap();
    timed
        .register::<Slow>(|_: Slow, _: JobContext<()>| async {
            tokio::time::sleep(Duration::from_millis(80)).await;
            Ok(())
        })
        .unwrap();
    Slow.enqueue(&db).await.unwrap();
    Slow.enqueue(&db).await.unwrap();
    assert_eq!(
        timed
            .run_once(2, Duration::from_millis(30), CancellationToken::new())
            .await
            .unwrap()
            .claimed,
        1
    );
    assert_eq!(
        timed
            .run_once(2, Duration::from_secs(1), CancellationToken::new())
            .await
            .unwrap()
            .claimed,
        1
    );

    let deadline_calls = Arc::new(AtomicUsize::new(0));
    let mut deadline_worker = Worker::new(db.clone(), deadline_calls.clone(), options()).unwrap();
    deadline_worker
        .register::<Fast>(|_: Fast, ctx: JobContext<AtomicUsize>| async move {
            ctx.state.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
    Fast { value: 3 }.enqueue(&db).await.unwrap();
    let mut lock = db.begin().await.unwrap();
    lock.execute("LOCK TABLE kouga_jobs IN ACCESS EXCLUSIVE MODE")
        .await
        .unwrap();
    let blocked = tokio::spawn(async move {
        deadline_worker
            .run_once(1, Duration::from_millis(30), CancellationToken::new())
            .await
    });
    tokio::time::sleep(Duration::from_millis(60)).await;
    lock.rollback().await.unwrap();
    assert_eq!(blocked.await.unwrap().unwrap().claimed, 0);
    assert_eq!(deadline_calls.load(Ordering::SeqCst), 0);

    let heartbeats_seen = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(AtomicUsize::new(0));
    let mut heartbeat_worker = Worker::new(db.clone(), heartbeats_seen.clone(), options()).unwrap();
    heartbeat_worker
        .register::<Slow>({
            let started = started.clone();
            move |_: Slow, ctx: JobContext<AtomicUsize>| {
                let started = started.clone();
                async move {
                    started.fetch_add(1, Ordering::SeqCst);
                    ctx.cancellation.cancelled().await;
                    ctx.state.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            }
        })
        .unwrap();
    Slow.enqueue(&db).await.unwrap();
    Slow.enqueue(&db).await.unwrap();
    let heartbeat_handle =
        tokio::spawn(async move { heartbeat_worker.run_forever(CancellationToken::new()).await });
    for _ in 0..50 {
        if started.load(Ordering::SeqCst) == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(started.load(Ordering::SeqCst), 2);

    db.close().await;
    assert!(heartbeat_handle.await.unwrap().is_err());
    assert_eq!(heartbeats_seen.load(Ordering::SeqCst), 2);
    admin
        .execute(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .await
        .unwrap();
}
