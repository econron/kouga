use kouga_model::{Uuid, sqlx};
use kouga_test::TestDb;
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    process::Command,
    thread,
    time::{Duration, Instant},
};
use taskboard_grpc::BoardService;
use taskboard_rpc::rpc::{CreateTaskRequest, board_client::BoardClient, board_server::BoardServer};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Request, metadata::MetadataValue};

fn smtp_sink() -> (u16, thread::JoinHandle<(String, Vec<String>)>) {
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
        let mut transcript = Vec::new();
        let mut data = false;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    break;
                }
                Err(error) => panic!("SMTP read: {error}"),
            }
            transcript.push(line.clone());
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
        (body, transcript)
    });
    (port, handle)
}

#[tokio::test]
async fn grpc_create_task_enqueues_once_and_worker_delivers() {
    let Ok(base_url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let isolated = TestDb::from_env(concat!(env!("CARGO_MANIFEST_DIR"), "/../../migrations"))
        .await
        .unwrap();
    let db = isolated.db();
    let schema: String = sqlx::query_scalar("SELECT current_schema()")
        .fetch_one(db)
        .await
        .unwrap();
    let owner = Uuid::new_v4();
    let project = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users(id,email,password_hash) VALUES($1,'grpc-mail@example.test','unused')",
    )
    .bind(owner)
    .execute(db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO projects(id,owner_id,slug,name) VALUES($1,$2,'grpc-mail','gRPC Mail')",
    )
    .bind(project)
    .bind(owner)
    .execute(db)
    .await
    .unwrap();
    let token = kouga_auth::issue_token(db, owner, Duration::from_secs(60))
        .await
        .unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tonic::transport::Server::builder()
        .add_service(BoardServer::new(BoardService::new(db.clone())))
        .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async move {
            let _ = stopped.await;
        });
    let handle = tokio::spawn(server);
    let mut client = BoardClient::connect(format!("http://{address}"))
        .await
        .unwrap();
    let mut request = Request::new(CreateTaskRequest {
        project_id: project.to_string(),
        title: "gRPC notification".into(),
    });
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {token}")).unwrap(),
    );
    let task = client.create_task(request).await.unwrap().into_inner();
    let task_id = Uuid::parse_str(&task.id).unwrap();
    let jobs: Vec<(Uuid, i32, String)> = sqlx::query_as(
        "SELECT id, version, queue FROM kouga_jobs WHERE name='taskboard.task_created' AND payload->>'task_id'=$1",
    )
    .bind(task_id.to_string())
    .fetch_all(db)
    .await
    .unwrap();
    assert_eq!(jobs.len(), 1);
    let (job_id, version, queue) = &jobs[0];
    assert_eq!(*version, 2);
    assert_eq!(queue.as_str(), "task-mail");

    let (port, smtp) = smtp_sink();
    let join = if base_url.contains('?') { '&' } else { '?' };
    let url = format!("{base_url}{join}options=-csearch_path%3D{schema}");
    let result = Command::new(env!("CARGO_BIN_EXE_task-notice-worker"))
        .arg("--once")
        .env("DATABASE_URL", url)
        .env("KOUGA_ENV", "test")
        .env("KOUGA_SMTP_HOST", "127.0.0.1")
        .env("KOUGA_SMTP_PORT", port.to_string())
        .env("KOUGA_SMTP_LOCAL", "1")
        .env("KOUGA_MAIL_FROM", "sender@example.test")
        .status()
        .unwrap();
    assert!(result.success());
    let effect_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM task_notice_effects WHERE job_id=$1 AND task_id=$2",
    )
    .bind(job_id)
    .bind(task_id)
    .fetch_one(db)
    .await
    .unwrap();
    assert_eq!(effect_count, 1);
    let status: String = sqlx::query_scalar("SELECT status FROM kouga_jobs WHERE id=$1")
        .bind(job_id)
        .fetch_one(db)
        .await
        .unwrap();
    let (mail, transcript) = smtp.join().unwrap();
    assert_eq!(status, "succeeded", "SMTP transcript: {transcript:?}");
    assert!(
        mail.contains("gRPC notification"),
        "SMTP transcript: {transcript:?}"
    );

    stop.send(()).unwrap();
    handle.await.unwrap().unwrap();
    isolated.close().await.unwrap();
}
