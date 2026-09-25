use kouga_telemetry::{Sampling, Telemetry, TelemetryConfig};
use std::{collections::HashMap, time::Duration};

#[tokio::test(flavor = "multi_thread")]
async fn collector_outage_does_not_fail_business_work() {
    let telemetry = Telemetry::init(TelemetryConfig {
        service_name: "kouga-outage-test".into(),
        service_version: None,
        environment: None,
        endpoint: Some("http://127.0.0.1:1".into()),
        headers: HashMap::new(),
        traces: true,
        metrics: true,
        logs: true,
        sampling: Sampling::AlwaysOn,
        log_filter: "error".into(),
        export_timeout: Duration::from_millis(200),
    })
    .unwrap();
    let result = tracing::info_span!("business_work").in_scope(|| {
        tracing::error!(message = "collector_is_down");
        42
    });
    assert_eq!(result, 42);
    opentelemetry::global::meter("kouga-test")
        .u64_counter("work_count")
        .build()
        .add(1, &[]);
    let _ = tokio::time::timeout(
        Duration::from_secs(3),
        telemetry.shutdown(Duration::from_secs(1)),
    )
    .await
    .expect("shutdown must be bounded");
}
