#![cfg(feature = "otel")]

use axum::{body::Body, extract::ConnectInfo};
use http::{Request, StatusCode};
use kouga_db::Db;
use kouga_http::{Endpoint, HttpOptions, Json, Operation, Router, State};
use kouga_mailer::{MailMessage, MemoryMailer};
use kouga_model::Uuid;
use kouga_queue::Enqueue;
use kouga_telemetry::{Sampling, Telemetry, TelemetryConfig};
use kouga_worker::{JobContext, JobError, Worker, WorkerOptions};
use opentelemetry_proto::tonic::{
    collector::trace::v1::ExportTraceServiceRequest, common::v1::any_value::Value, trace::v1::Span,
};
use prost::Message as _;
use sqlx::{
    Executor,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    process::Command,
    str::FromStr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

const REMOTE_TRACE: &str = "4bf92f3577b34da6a3ce929d0e0e4736";
const REMOTE_PARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
type Packets = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

#[derive(Debug, kouga_model::Model)]
#[model(table = "t27_records")]
struct Record {
    id: Uuid,
    name: String,
}

#[kouga_job::job(name = "t27_mail", version = 1, queue = "t27")]
struct MailJob {
    legacy: bool,
}

fn telemetry(service: &str) -> Telemetry {
    Telemetry::init(TelemetryConfig {
        service_name: service.into(),
        service_version: None,
        environment: Some("test".into()),
        endpoint: Some(std::env::var("KOUGA_T27_ENDPOINT").unwrap()),
        headers: HashMap::new(),
        traces: true,
        metrics: false,
        logs: true,
        sampling: Sampling::AlwaysOn,
        log_filter: "info".into(),
        export_timeout: Duration::from_secs(2),
    })
    .unwrap()
}

async fn database() -> Db {
    let url = std::env::var("KOUGA_TEST_DATABASE_URL").unwrap();
    let schema = std::env::var("KOUGA_T27_SCHEMA").unwrap();
    PgPoolOptions::new()
        .max_connections(4)
        .after_connect(move |conn, _| {
            let sql = format!("SET search_path TO {schema}");
            Box::pin(async move {
                conn.execute(sqlx::AssertSqlSafe(sql)).await?;
                Ok(())
            })
        })
        .connect_with(PgConnectOptions::from_str(&url).unwrap())
        .await
        .unwrap()
}

#[tracing::instrument(name = "t27.custom", skip_all)]
async fn business(db: Db) -> Uuid {
    let record = Record::create(
        &db,
        NewRecord {
            name: "public".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(record.name, "public");
    let job_id = MailJob { legacy: false }.enqueue(&db).await.unwrap();
    tracing::info!(operation = "mail_queued", "queued");
    job_id
}

fn request(path: &str, parent: &str, peer: Ipv4Addr) -> Request<Body> {
    let method = if path == "/send" { "POST" } else { "GET" };
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("traceparent", parent)
        .header("authorization", "Bearer t27-super-secret")
        .body(Body::from("body-secret"))
        .unwrap();
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from((peer, 3000))));
    request
}

async fn api() {
    let telemetry = telemetry("t27-http");
    let db = database().await;
    let mut options = HttpOptions::default();
    options
        .trusted_trace_peers
        .push(IpAddr::V4(Ipv4Addr::LOCALHOST));
    let app = Router::<Db>::new()
        .configure(options)
        .unwrap()
        .post(
            "/send",
            Endpoint::handler(
                |State(db): State<Db>| async move { Json(business(db).await.to_string()) },
                Operation::new("t27.send").response::<Json<String>>(),
            ),
        )
        .unwrap()
        .get(
            "/probe",
            Endpoint::handler(
                || async { Json("ok") },
                Operation::new("t27.probe").response::<Json<&str>>(),
            ),
        )
        .unwrap()
        .with_state(db);
    let (send, invalid, untrusted) = tokio::join!(
        app.clone()
            .oneshot(request("/send", REMOTE_PARENT, Ipv4Addr::LOCALHOST)),
        app.clone()
            .oneshot(request("/probe", "invalid", Ipv4Addr::LOCALHOST)),
        app.oneshot(request(
            "/probe",
            REMOTE_PARENT,
            Ipv4Addr::new(127, 0, 0, 2)
        )),
    );
    for response in [send, invalid, untrusted] {
        assert_eq!(response.unwrap().status(), StatusCode::OK);
    }
    telemetry.shutdown(Duration::from_secs(5)).await.unwrap();
}

fn worker_options() -> WorkerOptions {
    WorkerOptions {
        queues: vec!["t27".into()],
        concurrency: 1,
        poll_interval: Duration::from_millis(10),
        lease_duration: Duration::from_secs(1),
        job_timeout: Duration::from_secs(5),
        shutdown_grace: Duration::from_secs(5),
        max_attempts: 2,
        retry_base: Duration::from_millis(10),
        retry_max: Duration::from_millis(10),
    }
}

async fn worker() {
    let telemetry = telemetry("t27-worker");
    let db = database().await;
    let mailer = Arc::new(MemoryMailer::new());
    let mut worker = Worker::new(db.clone(), mailer.clone(), worker_options()).unwrap();
    worker
        .register::<MailJob>(|job: MailJob, ctx: JobContext<MemoryMailer>| async move {
            if !job.legacy && ctx.attempt == 1 {
                return Err(JobError::Retryable("retry"));
            }
            if !job.legacy {
                MailMessage::new(
                    "sender@example.test",
                    "sensitive-recipient@example.test",
                    "Welcome",
                    "hello",
                )
                .unwrap()
                .deliver(ctx.state.as_ref())
                .await
                .unwrap();
            }
            Ok(())
        })
        .unwrap();
    let first = worker
        .run_once(1, Duration::from_secs(2), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(first.retried, 1);
    tokio::time::sleep(Duration::from_millis(30)).await;
    let second = worker
        .run_once(1, Duration::from_secs(2), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(second.succeeded, 1);
    sqlx::query("INSERT INTO kouga_jobs (name,version,queue,payload,available_at) VALUES ('t27_mail',1,'t27',$1,now())")
        .bind(serde_json::json!({"legacy":true})).execute(&db).await.unwrap();
    let legacy = worker
        .run_once(1, Duration::from_secs(2), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(legacy.succeeded, 1);
    assert_eq!(mailer.recorded().len(), 1);
    telemetry.shutdown(Duration::from_secs(5)).await.unwrap();
}

fn collector() -> (
    String,
    Packets,
    Arc<AtomicBool>,
    std::thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let received = Arc::new(Mutex::new(Vec::new()));
    let stopped = Arc::new(AtomicBool::new(false));
    let items = received.clone();
    let stop = stopped.clone();
    let handle = std::thread::spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut bytes = Vec::new();
                    let mut buffer = [0u8; 8192];
                    loop {
                        let count = stream.read(&mut buffer).unwrap();
                        if count == 0 {
                            break;
                        }
                        bytes.extend_from_slice(&buffer[..count]);
                        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                            let headers = String::from_utf8_lossy(&bytes[..end]);
                            let length: usize = headers
                                .lines()
                                .find_map(|line| {
                                    let (name, value) = line.split_once(':')?;
                                    name.eq_ignore_ascii_case("content-length")
                                        .then(|| value.trim().parse().ok())
                                        .flatten()
                                })
                                .unwrap_or(0);
                            if bytes.len() >= end + 4 + length {
                                break;
                            }
                        }
                        assert!(bytes.len() < 1_000_000);
                    }
                    let end = bytes
                        .windows(4)
                        .position(|part| part == b"\r\n\r\n")
                        .unwrap();
                    let path = String::from_utf8_lossy(&bytes[..end])
                        .lines()
                        .next()
                        .unwrap()
                        .to_owned();
                    items
                        .lock()
                        .unwrap()
                        .push((path, bytes[end + 4..].to_vec()));
                    stream
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .unwrap();
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(error) => panic!("collector: {error}"),
            }
        }
    });
    (endpoint, received, stopped, handle)
}

fn run_child(role: &str, endpoint: &str, schema: &str) {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "trace_flow", "--nocapture"])
        .env("KOUGA_T27_ROLE", role)
        .env("KOUGA_T27_ENDPOINT", endpoint)
        .env("KOUGA_T27_SCHEMA", schema)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{role}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for secret in [
        "t27-super-secret",
        "body-secret",
        "sensitive-recipient@example.test",
    ] {
        assert!(
            !output
                .stdout
                .windows(secret.len())
                .any(|part| part == secret.as_bytes())
        );
        assert!(
            !output
                .stderr
                .windows(secret.len())
                .any(|part| part == secret.as_bytes())
        );
    }
}

fn string_attribute<'a>(
    attributes: &'a [opentelemetry_proto::tonic::common::v1::KeyValue],
    key: &str,
) -> Option<&'a str> {
    attributes
        .iter()
        .find(|attribute| attribute.key == key)
        .and_then(|attribute| attribute.value.as_ref())
        .and_then(|value| value.value.as_ref())
        .and_then(|value| match value {
            Value::StringValue(value) => Some(value.as_str()),
            _ => None,
        })
}

fn int_attribute(
    attributes: &[opentelemetry_proto::tonic::common::v1::KeyValue],
    key: &str,
) -> Option<i64> {
    attributes
        .iter()
        .find(|attribute| attribute.key == key)
        .and_then(|attribute| attribute.value.as_ref())
        .and_then(|value| value.value.as_ref())
        .and_then(|value| match value {
            Value::IntValue(value) => Some(*value),
            _ => None,
        })
}

#[tokio::test(flavor = "multi_thread")]
async fn trace_flow() {
    match std::env::var("KOUGA_T27_ROLE").as_deref() {
        Ok("api") => return api().await,
        Ok("worker") => return worker().await,
        _ => {}
    }
    let Ok(url) = std::env::var("KOUGA_TEST_DATABASE_URL") else {
        return;
    };
    let (endpoint, received, stopped, server) = collector();
    let admin = kouga_db::connect(&url, 2, Duration::from_secs(5))
        .await
        .unwrap();
    let schema = format!(
        "kouga_t27_{}_{}",
        std::process::id(),
        Uuid::new_v4().simple()
    );
    admin
        .execute(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .await
        .unwrap();
    let scoped = PgPoolOptions::new()
        .max_connections(2)
        .after_connect({
            let schema = schema.clone();
            move |conn, _| {
                let sql = format!("SET search_path TO {schema}");
                Box::pin(async move {
                    conn.execute(sqlx::AssertSqlSafe(sql)).await?;
                    Ok(())
                })
            }
        })
        .connect_with(PgConnectOptions::from_str(&url).unwrap())
        .await
        .unwrap();
    scoped
        .execute(sqlx::raw_sql(kouga_queue::SCHEMA_SQL))
        .await
        .unwrap();
    scoped
        .execute(sqlx::raw_sql(include_str!(
            "../migrations/20260925000021_add_job_failure.up.sql"
        )))
        .await
        .unwrap();
    scoped
        .execute("CREATE TABLE t27_records (id uuid PRIMARY KEY, name text NOT NULL)")
        .await
        .unwrap();
    run_child("api", &endpoint, &schema);
    let saved: (String, serde_json::Value) =
        sqlx::query_as("SELECT traceparent,payload FROM kouga_jobs WHERE name='t27_mail'")
            .fetch_one(&scoped)
            .await
            .unwrap();
    assert!(saved.0.contains(REMOTE_TRACE));
    assert_eq!(saved.1, serde_json::json!({"legacy":false}));
    run_child("worker", &endpoint, &schema);
    let statuses: Vec<(String,)> =
        sqlx::query_as("SELECT status FROM kouga_jobs ORDER BY created_at")
            .fetch_all(&scoped)
            .await
            .unwrap();
    assert_eq!(statuses.len(), 2);
    assert!(statuses.iter().all(|(status,)| status == "succeeded"));
    stopped.store(true, Ordering::SeqCst);
    server.join().unwrap();
    let packets = received.lock().unwrap().clone();
    for secret in [
        "t27-super-secret",
        "body-secret",
        "sensitive-recipient@example.test",
    ] {
        assert!(packets.iter().all(|(_, body)| {
            !body
                .windows(secret.len())
                .any(|part| part == secret.as_bytes())
        }));
    }
    let mut spans: Vec<(String, Span)> = Vec::new();
    for (path, body) in packets
        .iter()
        .filter(|(path, _)| path.contains("/v1/traces"))
    {
        let export = ExportTraceServiceRequest::decode(body.as_slice())
            .unwrap_or_else(|error| panic!("{path}: {error}"));
        for resource in export.resource_spans {
            let service = string_attribute(&resource.resource.unwrap().attributes, "service.name")
                .unwrap()
                .to_owned();
            for scope in resource.scope_spans {
                spans.extend(scope.spans.into_iter().map(|span| (service.clone(), span)));
            }
        }
    }
    let http: Vec<_> = spans
        .iter()
        .filter(|(service, span)| service == "t27-http" && span.name == "kouga.http.request")
        .collect();
    assert_eq!(http.len(), 3);
    assert_eq!(
        http.iter()
            .map(|(_, span)| &span.trace_id)
            .collect::<std::collections::HashSet<_>>()
            .len(),
        3
    );
    assert_eq!(
        http.iter()
            .filter(|(_, span)| span.trace_id == hex_trace())
            .count(),
        1
    );
    let entry = &http
        .iter()
        .find(|(_, span)| span.trace_id == hex_trace())
        .unwrap()
        .1;
    let custom = spans
        .iter()
        .find(|(_, span)| span.name == "t27.custom")
        .unwrap()
        .1
        .clone();
    assert_eq!(custom.parent_span_id, entry.span_id);
    assert!(spans.iter().any(|(_, span)| span.name == "kouga.db.query" && span.parent_span_id == custom.span_id));
    let enqueue = spans
        .iter()
        .find(|(_, span)| span.name == "kouga.queue.enqueue")
        .unwrap()
        .1
        .clone();
    assert_eq!(enqueue.parent_span_id, custom.span_id);
    assert_eq!(enqueue.trace_id, entry.trace_id);
    let jobs: Vec<_> = spans
        .iter()
        .filter(|(service, span)| service == "t27-worker" && span.name == "kouga.job.run")
        .collect();
    assert_eq!(jobs.len(), 3);
    let linked: Vec<_> = jobs
        .iter()
        .filter(|(_, span)| span.links.len() == 1)
        .collect();
    assert_eq!(linked.len(), 2);
    assert_ne!(linked[0].1.span_id, linked[1].1.span_id);
    for (_, span) in &linked {
        assert_eq!(span.links[0].trace_id, enqueue.trace_id);
        assert_eq!(span.links[0].span_id, enqueue.span_id);
    }
    assert!(
        linked
            .iter()
            .any(|(_, span)| int_attribute(&span.attributes, "job.attempt") == Some(1))
    );
    let second = &linked
        .iter()
        .find(|(_, span)| int_attribute(&span.attributes, "job.attempt") == Some(2))
        .unwrap()
        .1;
    assert!(spans.iter().any(|(service, span)| service == "t27-worker"
        && span.name == "kouga.mail.send"
        && span.parent_span_id == second.span_id));
    assert!(jobs.iter().any(|(_, span)| span.links.is_empty()
        && int_attribute(&span.attributes, "job.attempt") == Some(1)));
    scoped.close().await;
    admin
        .execute(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .await
        .unwrap();
}

fn hex_trace() -> Vec<u8> {
    (0..REMOTE_TRACE.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&REMOTE_TRACE[i..i + 2], 16).unwrap())
        .collect()
}
