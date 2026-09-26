use kouga_telemetry::{Sampling, Telemetry, TelemetryConfig, propagation};
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::any_value::Value;
use prost::Message as _;
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
use tracing::Instrument;

#[tokio::test(flavor = "multi_thread")]
async fn sends_three_signals_and_keeps_stdout_separate() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let paths = Arc::new(Mutex::new(Vec::new()));
    let traces = Arc::new(Mutex::new(Vec::new()));
    let stopped = Arc::new(AtomicBool::new(false));
    let paths_for_server = paths.clone();
    let traces_for_server = traces.clone();
    let stopped_for_server = stopped.clone();
    let server = std::thread::spawn(move || {
        while !stopped_for_server.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut data = Vec::new();
                    let mut buffer = [0u8; 8192];
                    while let Ok(n) = stream.read(&mut buffer) {
                        if n == 0 {
                            break;
                        }
                        data.extend_from_slice(&buffer[..n]);
                        if let Some(end) = data.windows(4).position(|window| window == b"\r\n\r\n")
                        {
                            let headers = String::from_utf8_lossy(&data[..end]);
                            let content_length = headers
                                .lines()
                                .find_map(|line| {
                                    let (name, value) = line.split_once(':')?;
                                    name.eq_ignore_ascii_case("content-length")
                                        .then(|| value.trim().parse::<usize>().ok())
                                        .flatten()
                                })
                                .unwrap_or(0);
                            if data.len() >= end + 4 + content_length {
                                break;
                            }
                        }
                    }
                    let first_line = String::from_utf8_lossy(&data)
                        .lines()
                        .next()
                        .unwrap_or("")
                        .to_owned();
                    if first_line.contains("/v1/traces")
                        && let Some(end) = data.windows(4).position(|window| window == b"\r\n\r\n")
                    {
                        traces_for_server
                            .lock()
                            .unwrap()
                            .push(data[end + 4..].to_vec());
                    }
                    paths_for_server.lock().unwrap().push(first_line);
                    stream
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .unwrap();
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(error) => panic!("collector accept failed: {error}"),
            }
        }
    });
    let telemetry = Telemetry::init(TelemetryConfig {
        service_name: "kouga-test".into(),
        service_version: Some("0.1".into()),
        environment: Some("test".into()),
        endpoint: Some(endpoint),
        headers: HashMap::new(),
        traces: true,
        metrics: true,
        logs: true,
        sampling: Sampling::AlwaysOn,
        log_filter: "error".into(),
        export_timeout: Duration::from_secs(2),
    })
    .unwrap();
    tracing::info_span!("custom_safe_span", operation = "test").in_scope(|| {
        tracing::error!(message = "custom_safe_log");
    });
    let roots = tokio::join!(
        async {
            let root = tracing::info_span!("entry_a");
            async {
                let context = propagation::capture();
                assert!(context.0.is_some());
                context
            }
            .instrument(root)
            .await
        },
        async {
            let root = tracing::info_span!("entry_b");
            async { propagation::capture() }.instrument(root).await
        }
    );
    assert_ne!(roots.0.0, roots.1.0, "parallel contexts must not mix");
    assert!(!propagation::extract(
        Some("invalid"),
        None,
        &tracing::info_span!("untrusted")
    ));
    let remote = tracing::info_span!("trusted_http");
    assert!(propagation::extract(
        Some("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"),
        None,
        &remote
    ));
    remote.in_scope(|| {
        assert!(
            propagation::capture()
                .0
                .unwrap()
                .contains("4bf92f3577b34da6a3ce929d0e0e4736")
        )
    });
    drop(remote);
    let first_attempt = tracing::info_span!("kouga.job.run", job.attempt = 1);
    assert!(propagation::link(
        roots.0.0.as_deref(),
        roots.0.1.as_deref(),
        &first_attempt
    ));
    first_attempt.in_scope(|| {});
    drop(first_attempt);
    let worker = tracing::info_span!("kouga.job.run", job.attempt = 2);
    assert!(!propagation::link(None, None, &worker));
    assert!(propagation::link(
        roots.0.0.as_deref(),
        roots.0.1.as_deref(),
        &worker
    ));
    async {
        tracing::info_span!("kouga.mail.send").in_scope(|| {});
    }
    .instrument(worker)
    .await;
    let meter = opentelemetry::global::meter("kouga-test");
    meter.u64_counter("safe_counter").build().add(1, &[]);
    telemetry.flush(Duration::from_secs(5)).await.unwrap();
    for signal in ["traces", "metrics", "logs"] {
        assert!(
            paths
                .lock()
                .unwrap()
                .iter()
                .any(|path| path.contains(&format!("/v1/{signal}"))),
            "{signal} was not flushed before invocation return"
        );
    }
    telemetry.shutdown(Duration::from_secs(5)).await.unwrap();
    stopped.store(true, Ordering::SeqCst);
    server.join().unwrap();
    let recorded = paths.lock().unwrap();
    for signal in ["traces", "metrics", "logs"] {
        assert!(
            recorded
                .iter()
                .any(|path| path.contains(&format!("/v1/{signal}"))),
            "missing {signal}: {recorded:?}"
        );
    }
    let exports: Vec<ExportTraceServiceRequest> = traces
        .lock()
        .unwrap()
        .iter()
        .map(|body| ExportTraceServiceRequest::decode(body.as_slice()).unwrap())
        .collect();
    assert!(exports.iter().flat_map(|batch| &batch.resource_spans)
        .filter_map(|span| span.resource.as_ref())
        .flat_map(|resource| &resource.attributes)
        .any(|attribute| attribute.key == "service.name"
            && matches!(attribute.value.as_ref().and_then(|value| value.value.as_ref()), Some(Value::StringValue(name)) if name == "kouga-test")));
    let spans: Vec<_> = exports
        .iter()
        .flat_map(|batch| &batch.resource_spans)
        .flat_map(|resource| &resource.scope_spans)
        .flat_map(|scope| &scope.spans)
        .collect();
    let entry = spans.iter().find(|span| span.name == "entry_a").unwrap();
    let job_spans: Vec<_> = spans
        .iter()
        .filter(|span| span.name == "kouga.job.run")
        .collect();
    assert_eq!(job_spans.len(), 2);
    assert_ne!(job_spans[0].span_id, job_spans[1].span_id);
    let worker = job_spans
        .iter()
        .find(|span| {
            span.attributes.iter().any(|attribute| {
                attribute.key == "job.attempt"
                    && matches!(
                        attribute
                            .value
                            .as_ref()
                            .and_then(|value| value.value.as_ref()),
                        Some(Value::IntValue(2))
                    )
            })
        })
        .unwrap();
    assert_eq!(worker.links.len(), 1);
    assert!(worker.attributes.iter().any(|attribute| {
        attribute.key == "job.attempt"
            && matches!(
                attribute
                    .value
                    .as_ref()
                    .and_then(|value| value.value.as_ref()),
                Some(Value::IntValue(2))
            )
    }));
    assert!(spans.iter().any(|span| span.name == "trusted_http"
        && span.trace_id == b"K\xf9/5w\xb3M\xa6\xa3\xce\x92\x9d\x0e\x0eG6"));
    assert_eq!(worker.links[0].trace_id, entry.trace_id);
    assert_eq!(worker.links[0].span_id, entry.span_id);
    assert_ne!(worker.span_id, entry.span_id);
    assert!(
        spans
            .iter()
            .any(|span| span.name == "kouga.mail.send" && span.parent_span_id == worker.span_id)
    );
}
