use kouga_telemetry::{Sampling, Telemetry, TelemetryConfig};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[tokio::test]
async fn no_endpoint_keeps_local_logging_without_exporting() {
    let mut telemetry = Telemetry::init(TelemetryConfig {
        service_name: "kouga-local-test".into(),
        service_version: None,
        environment: None,
        endpoint: None,
        headers: HashMap::new(),
        traces: true,
        metrics: true,
        logs: false,
        sampling: Sampling::AlwaysOn,
        log_filter: "info".into(),
        export_timeout: Duration::from_secs(1),
    })
    .unwrap();
    let called = Arc::new(AtomicBool::new(false));
    let called_by_hook = called.clone();
    telemetry.on_shutdown(move |_| {
        called_by_hook.store(true, Ordering::SeqCst);
        Ok(())
    });
    tracing::info!(message = "local_only");
    telemetry.shutdown(Duration::from_secs(1)).await.unwrap();
    assert!(called.load(Ordering::SeqCst));
}
