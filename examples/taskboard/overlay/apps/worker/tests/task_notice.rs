use kouga_model::{Uuid, sqlx};
use kouga_test::TestDb;
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};

fn smtp_server(results: Vec<bool>) -> (u16, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut messages = Vec::new();
        for accept in results {
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(10))
                    }
                    Err(error) => panic!("SMTP accept: {error}"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            socket.write_all(b"220 local SMTP\r\n").unwrap();
            let mut reader = BufReader::new(socket.try_clone().unwrap());
            let mut body = String::new();
            let mut data = false;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 {
                    break;
                }
                if data {
                    if line == ".\r\n" {
                        messages.push(body);
                        socket
                            .write_all(if accept {
                                b"250 accepted\r\n"
                            } else {
                                b"451 retry later\r\n"
                            })
                            .unwrap();
                        break;
                    }
                    body.push_str(&line);
                } else if line.starts_with("EHLO") {
                    socket.write_all(b"250 local\r\n").unwrap();
                } else if line.starts_with("MAIL FROM:") || line.starts_with("RCPT TO:") {
                    socket.write_all(b"250 OK\r\n").unwrap();
                } else if line.starts_with("DATA") {
                    data = true;
                    socket.write_all(b"354 go\r\n").unwrap();
                }
            }
        }
        messages
    });
    (port, handle)
}

fn worker(url: &str, port: u16, pause: Option<u64>) -> Child {
    let mut command = Command::new(env!("CARGO_BIN_EXE_task-notice-worker"));
    command
        .arg("--once")
        .env("DATABASE_URL", url)
        .env("KOUGA_ENV", "test")
        .env("KOUGA_SMTP_HOST", "127.0.0.1")
        .env("KOUGA_SMTP_PORT", port.to_string())
        .env("KOUGA_SMTP_LOCAL", "1")
        .env("KOUGA_MAIL_FROM", "sender@example.test");
    if let Some(pause) = pause {
        command.env("TASKBOARD_TEST_PAUSE_AFTER_EFFECT_MS", pause.to_string());
    }
    command.spawn().unwrap()
}

async fn wait_for_effect(db: &kouga_model::Db, job: Uuid) {
    for _ in 0..100 {
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM task_notice_effects WHERE job_id=$1")
                .bind(job)
                .fetch_one(db)
                .await
                .unwrap();
        if count == 1 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    panic!("worker did not apply notification effect");
}

async fn wait_for_lease_expiry(db: &kouga_model::Db, job: Uuid) {
    for _ in 0..200 {
        let expired: bool = sqlx::query_scalar(
            "SELECT lease_until IS NOT NULL AND lease_until <= now() FROM kouga_jobs WHERE id=$1",
        )
        .bind(job)
        .fetch_one(db)
        .await
        .unwrap();
        if expired {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("killed worker lease did not expire");
}

#[tokio::test]
async fn killed_worker_reclaims_without_duplicate_effect_and_reads_old_payload() {
    let Ok(base_url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let isolated = TestDb::from_env("../../migrations").await.unwrap();
    let db = isolated.db();
    let schema: String = sqlx::query_scalar("SELECT current_schema()")
        .fetch_one(db)
        .await
        .unwrap();
    let join = if base_url.contains('?') { '&' } else { '?' };
    let url = format!("{base_url}{join}options=-csearch_path%3D{schema}");
    let owner = Uuid::new_v4();
    let project = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users(id,email,password_hash) VALUES($1,'receiver@example.test','unused')",
    )
    .bind(owner)
    .execute(db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO projects(id,owner_id,slug,name) VALUES($1,$2,'mail','Mail')")
        .bind(project)
        .bind(owner)
        .execute(db)
        .await
        .unwrap();
    let task = Uuid::new_v4();
    sqlx::query("INSERT INTO tasks(id,project_id,owner_id,title) VALUES($1,$2,$3,'crash test')")
        .bind(task)
        .bind(project)
        .bind(owner)
        .execute(db)
        .await
        .unwrap();
    let job: Uuid = sqlx::query_scalar("INSERT INTO kouga_jobs(name,version,queue,payload,available_at) VALUES('taskboard.task_created',2,'task-mail',jsonb_build_object('task_id',$1::text,'owner_id',$2::text),now()) RETURNING id")
        .bind(task).bind(owner).fetch_one(db).await.unwrap();
    let (port, smtp) = smtp_server(vec![true, true, false, true]);
    let mut first = worker(&url, port, Some(5000));
    wait_for_effect(db, job).await;
    first.kill().unwrap();
    first.wait().unwrap();
    wait_for_lease_expiry(db, job).await;
    assert!(worker(&url, port, None).wait().unwrap().success());
    let (status, attempts): (String, i32) =
        sqlx::query_as("SELECT status,attempt FROM kouga_jobs WHERE id=$1")
            .bind(job)
            .fetch_one(db)
            .await
            .unwrap();
    assert_eq!(status, "succeeded");
    assert_eq!(attempts, 2);
    let effects: i64 =
        sqlx::query_scalar("SELECT count(*) FROM task_notice_effects WHERE job_id=$1")
            .bind(job)
            .fetch_one(db)
            .await
            .unwrap();
    assert_eq!(effects, 1);

    // Simulate an HTTP producer that has not been updated: only the old v1 field exists.
    let old: Uuid = sqlx::query_scalar("INSERT INTO kouga_jobs(name,version,queue,payload,available_at) VALUES('taskboard.task_created',1,'task-mail',jsonb_build_object('task_id',$1::text),now()) RETURNING id")
        .bind(task).fetch_one(db).await.unwrap();
    assert!(worker(&url, port, None).wait().unwrap().success());
    let old_status: String = sqlx::query_scalar("SELECT status FROM kouga_jobs WHERE id=$1")
        .bind(old)
        .fetch_one(db)
        .await
        .unwrap();
    assert_eq!(old_status, "succeeded");

    // SMTP transient failure is retryable; the next delivery succeeds.
    let retry: Uuid = sqlx::query_scalar("INSERT INTO kouga_jobs(name,version,queue,payload,available_at) VALUES('taskboard.task_created',2,'task-mail',jsonb_build_object('task_id',$1::text,'owner_id',$2::text),now()) RETURNING id")
        .bind(task).bind(owner).fetch_one(db).await.unwrap();
    assert!(worker(&url, port, None).wait().unwrap().success());
    let retry_status: String = sqlx::query_scalar("SELECT status FROM kouga_jobs WHERE id=$1")
        .bind(retry)
        .fetch_one(db)
        .await
        .unwrap();
    assert_eq!(retry_status, "pending");
    sqlx::query("UPDATE kouga_jobs SET available_at=now() WHERE id=$1")
        .bind(retry)
        .execute(db)
        .await
        .unwrap();
    assert!(worker(&url, port, None).wait().unwrap().success());
    let final_status: String = sqlx::query_scalar("SELECT status FROM kouga_jobs WHERE id=$1")
        .bind(retry)
        .fetch_one(db)
        .await
        .unwrap();
    assert_eq!(final_status, "succeeded");
    let messages = smtp.join().unwrap();
    assert_eq!(messages.len(), 4);
    assert!(messages.iter().all(|body| body.contains("crash test")));
    let wrong_owner = Uuid::new_v4();
    let permanent: Uuid = sqlx::query_scalar("INSERT INTO kouga_jobs(name,version,queue,payload,available_at) VALUES('taskboard.task_created',2,'task-mail',jsonb_build_object('task_id',$1::text,'owner_id',$2::text),now()) RETURNING id")
        .bind(task).bind(wrong_owner).fetch_one(db).await.unwrap();
    assert!(worker(&url, port, None).wait().unwrap().success());
    let (status, reason): (String, Option<String>) =
        sqlx::query_as("SELECT status,failure_reason FROM kouga_jobs WHERE id=$1")
            .bind(permanent)
            .fetch_one(db)
            .await
            .unwrap();
    assert_eq!(status, "dead");
    assert_eq!(reason.as_deref(), Some("task owner mismatch"));
    isolated.close().await.unwrap();
}
