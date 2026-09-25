use kouga_telemetry::{Sampling, Telemetry, TelemetryConfig};
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

#[tokio::test(flavor = "multi_thread")]
async fn sends_three_signals_and_keeps_stdout_separate() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let paths = Arc::new(Mutex::new(Vec::new()));
    let stopped = Arc::new(AtomicBool::new(false));
    let paths_for_server = paths.clone();
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
    let meter = opentelemetry::global::meter("kouga-test");
    meter.u64_counter("safe_counter").build().add(1, &[]);
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
}
