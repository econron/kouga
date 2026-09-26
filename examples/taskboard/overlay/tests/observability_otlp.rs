use axum::{
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::Request,
};
use kouga_telemetry::{Sampling, Telemetry, TelemetryConfig};
use kouga_test::{TestClient, TestDb};
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

struct Collector {
    endpoint: String,
    records: Arc<Records>,
    stop: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<()>,
}

struct Records(Mutex<Vec<(String, Vec<u8>)>>);

fn collector() -> Collector {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let records = Arc::new(Records(Mutex::new(Vec::new())));
    let stop = Arc::new(AtomicBool::new(false));
    let captured = records.clone();
    let stopped = stop.clone();
    let thread = std::thread::spawn(move || {
        while !stopped.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut data = Vec::new();
                    let mut chunk = [0u8; 8192];
                    while let Ok(read) = stream.read(&mut chunk) {
                        if read == 0 {
                            break;
                        }
                        data.extend_from_slice(&chunk[..read]);
                        if let Some(end) = data.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                            let headers = String::from_utf8_lossy(&data[..end]);
                            let length = headers
                                .lines()
                                .find_map(|line| {
                                    let (key, value) = line.split_once(':')?;
                                    key.eq_ignore_ascii_case("content-length")
                                        .then(|| value.trim().parse::<usize>().ok())
                                        .flatten()
                                })
                                .unwrap_or(0);
                            if data.len() >= end + 4 + length {
                                break;
                            }
                        }
                    }
                    let path = String::from_utf8_lossy(&data)
                        .lines()
                        .next()
                        .unwrap_or("")
                        .to_owned();
                    if let Some(end) = data.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                        captured
                            .0
                            .lock()
                            .unwrap()
                            .push((path, data[end + 4..].to_vec()));
                    }
                    let _ = stream.write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(error) => panic!("collector: {error}"),
            }
        }
    });
    Collector {
        endpoint,
        records,
        stop,
        thread,
    }
}

async fn post(
    client: &TestClient,
    path: &str,
    json: String,
    token: Option<&str>,
) -> serde_json::Value {
    let mut request = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = client
        .send(
            request
                .extension(ConnectInfo(std::net::SocketAddr::from((
                    [127, 0, 0, 1],
                    42425,
                ))))
                .body(Body::from(json))
                .unwrap(),
        )
        .await;
    assert!(
        response.status().is_success(),
        "{path}: {}",
        response.status()
    );
    serde_json::from_slice(&to_bytes(response.into_body(), 1_000_000).await.unwrap()).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn taskboard_exports_three_signals_without_secrets_and_persists_job_context() {
    if std::env::var("TEST_DATABASE_URL").is_err() {
        return;
    }
    let Collector {
        endpoint,
        records,
        stop,
        thread,
    } = collector();
    let telemetry = Telemetry::init(TelemetryConfig {
        service_name: "taskboard-http".into(),
        service_version: Some("0.1.0".into()),
        environment: Some("test".into()),
        endpoint: Some(endpoint),
        headers: HashMap::new(),
        traces: true,
        metrics: true,
        logs: true,
        sampling: Sampling::AlwaysOn,
        log_filter: "info".into(),
        export_timeout: Duration::from_secs(2),
    })
    .unwrap();
    let isolated = TestDb::from_env("migrations").await.unwrap();
    let client = TestClient::new(taskboard::router().with_state(isolated.db().clone()));
    let registration = post(
        &client,
        "/auth/register",
        r#"{"email":"otel-secret@example.invalid","password":"correct horse battery"}"#.into(),
        None,
    )
    .await;
    let token = registration["data"]["token"].as_str().unwrap();
    let project = post(
        &client,
        "/projects",
        r#"{"slug":"observed","name":"Observed"}"#.into(),
        Some(token),
    )
    .await;
    let project_id = project["data"]["id"].as_str().unwrap();
    post(
        &client,
        "/tasks",
        format!(r#"{{"project_id":"{project_id}","title":"Secret task title"}}"#),
        Some(token),
    )
    .await;
    let parent: Option<String> = kouga_model::sqlx::query_scalar(
        "SELECT traceparent FROM kouga_jobs WHERE name='taskboard.task_created' LIMIT 1",
    )
    .fetch_one(isolated.db())
    .await
    .unwrap();
    assert!(
        parent.is_some(),
        "queue context must be stored separately from payload"
    );
    telemetry.flush(Duration::from_secs(5)).await.unwrap();
    stop.store(true, Ordering::SeqCst);
    thread.join().unwrap();
    post(
        &client,
        "/projects",
        r#"{"slug":"collector-outage","name":"Still available"}"#.into(),
        Some(token),
    )
    .await;
    isolated.close().await.unwrap();
    assert!(
        tokio::time::timeout(
            Duration::from_secs(7),
            telemetry.shutdown(Duration::from_secs(2))
        )
        .await
        .is_ok(),
        "shutdown must be bounded when Collector is unavailable"
    );
    let records = records.0.lock().unwrap();
    for signal in ["/v1/traces", "/v1/metrics", "/v1/logs"] {
        assert!(
            records.iter().any(|(path, _)| path.contains(signal)),
            "missing {signal}"
        );
    }
    let payload = records
        .iter()
        .flat_map(|(_, bytes)| bytes.iter().copied())
        .collect::<Vec<_>>();
    let text = String::from_utf8_lossy(&payload);
    assert!(text.contains("taskboard-http"));
    assert!(text.contains("taskboard.http.business"));
    assert!(text.contains("taskboard.http.requests"));
    for secret in [
        "correct horse battery",
        "otel-secret@example.invalid",
        "Secret task title",
        token,
    ] {
        assert!(!text.contains(secret), "telemetry exposed a secret");
    }
    drop(records);
}
