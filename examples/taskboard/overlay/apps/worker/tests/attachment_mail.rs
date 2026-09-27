use app_contracts::task_notice::AttachmentMailV1;
use bytes::Bytes;
use futures_util::stream;
use kouga_auth::CurrentUser;
use kouga_model::{Uuid, sqlx};
use kouga_queue::Enqueue;
use kouga_storage::{AmazonS3Builder, FileKind, Storage, Upload};
use kouga_test::TestDb;
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    process::Command,
    thread,
    time::{Duration, Instant},
};

fn smtp_sink() -> (u16, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut socket = loop {
            match listener.accept() {
                Ok((socket, _)) => break socket,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(10));
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
                    socket.write_all(b"250 accepted\r\n").unwrap();
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
        body
    });
    (port, handle)
}

fn run_worker(db_url: &str, port: u16) {
    let status = Command::new(env!("CARGO_BIN_EXE_task-notice-worker"))
        .arg("--once")
        .env("DATABASE_URL", db_url)
        .env("KOUGA_ENV", "test")
        .env("KOUGA_SMTP_HOST", "127.0.0.1")
        .env("KOUGA_SMTP_PORT", port.to_string())
        .env("KOUGA_SMTP_LOCAL", "1")
        .env("KOUGA_MAIL_FROM", "sender@example.test")
        .env("BOARD_STORAGE_BACKEND", "s3")
        .env(
            "BOARD_S3_ENDPOINT",
            std::env::var("KOUGA_TEST_S3_ENDPOINT").unwrap(),
        )
        .env(
            "BOARD_S3_BUCKET",
            std::env::var("KOUGA_TEST_S3_BUCKET").unwrap(),
        )
        .env("BOARD_S3_REGION", "us-east-1")
        .env("BOARD_S3_ALLOW_HTTP", "1")
        .status()
        .unwrap();
    assert!(status.success());
}

#[tokio::test]
async fn attachment_job_sends_s3_file_only_for_live_owner_record() {
    let (Ok(base_url), Ok(endpoint), Ok(bucket)) = (
        std::env::var("TEST_DATABASE_URL"),
        std::env::var("KOUGA_TEST_S3_ENDPOINT"),
        std::env::var("KOUGA_TEST_S3_BUCKET"),
    ) else {
        return;
    };
    let test = TestDb::from_env(concat!(env!("CARGO_MANIFEST_DIR"), "/../../migrations"))
        .await
        .unwrap();
    let db = test.db();
    let schema: String = sqlx::query_scalar("SELECT current_schema()")
        .fetch_one(db)
        .await
        .unwrap();
    let db_url = format!("{base_url}?options=-csearch_path%3D{schema}");
    let owner = Uuid::new_v4();
    let other = Uuid::new_v4();
    let project = Uuid::new_v4();
    let task = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,email,password_hash) VALUES($1,'attachment@example.test','unused'),($2,'other@example.test','unused')")
        .bind(owner).bind(other).execute(db).await.unwrap();
    sqlx::query(
        "INSERT INTO projects(id,owner_id,slug,name) VALUES($1,$2,'attachment','Attachment')",
    )
    .bind(project)
    .bind(owner)
    .execute(db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO tasks(id,project_id,owner_id,title) VALUES($1,$2,$3,'Attached')")
        .bind(task)
        .bind(project)
        .bind(owner)
        .execute(db)
        .await
        .unwrap();
    let s3 = AmazonS3Builder::from_env()
        .with_endpoint(endpoint)
        .with_bucket_name(bucket)
        .with_region("us-east-1")
        .with_allow_http(true)
        .build()
        .unwrap();
    let storage = Storage::s3(db.clone(), s3);
    let actor = CurrentUser { id: owner };
    let png = b"\x89PNG\r\n\x1a\nattachment payload";
    let file = storage
        .save(
            Upload {
                owner: actor,
                filename: "report.png",
                declared_content_type: "image/png",
                allowed: &[FileKind::Png],
                max_bytes: 10 * 1024 * 1024,
            },
            stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(png))]),
        )
        .await
        .unwrap();
    assert!(storage.attach(actor, file.id, "tasks", task).await.unwrap());
    let job = AttachmentMailV1 {
        task_id: task,
        file_id: file.id,
        owner_id: owner,
    }
    .enqueue(db)
    .await
    .unwrap();
    let (port, smtp) = smtp_sink();
    run_worker(&db_url, port);
    let body = smtp.join().unwrap();
    assert!(body.contains("multipart/mixed"));
    assert!(body.contains("report.png"));
    assert!(body.contains("iVBORw0KGgphdHRhY2htZW50IHBheWxvYWQ="));
    let status: String = sqlx::query_scalar("SELECT status FROM kouga_jobs WHERE id=$1")
        .bind(job)
        .fetch_one(db)
        .await
        .unwrap();
    assert_eq!(status, "succeeded");

    // A forged owner and a deleted file must not be mailed, even if queued directly.
    let forged = AttachmentMailV1 {
        task_id: task,
        file_id: file.id,
        owner_id: other,
    }
    .enqueue(db)
    .await
    .unwrap();
    run_worker(&db_url, port);
    let forged_status: String = sqlx::query_scalar("SELECT status FROM kouga_jobs WHERE id=$1")
        .bind(forged)
        .fetch_one(db)
        .await
        .unwrap();
    assert_eq!(forged_status, "succeeded");
    let deleted = AttachmentMailV1 {
        task_id: task,
        file_id: file.id,
        owner_id: owner,
    }
    .enqueue(db)
    .await
    .unwrap();
    storage.delete(actor, file.id).await.unwrap();
    run_worker(&db_url, port);
    let deleted_status: String = sqlx::query_scalar("SELECT status FROM kouga_jobs WHERE id=$1")
        .bind(deleted)
        .fetch_one(db)
        .await
        .unwrap();
    assert_eq!(deleted_status, "succeeded");
    test.close().await.unwrap();
}
