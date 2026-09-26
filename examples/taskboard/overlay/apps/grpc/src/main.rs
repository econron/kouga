#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = kouga_telemetry::TelemetryConfig::from_env()?;
    if std::env::var_os("OTEL_SERVICE_NAME").is_none() {
        config.service_name = "taskboard-grpc".into();
    }
    let telemetry = kouga_telemetry::Telemetry::init(config)?;
    let address = std::env::var("KOUGA_GRPC_BIND")
        .unwrap_or_else(|_| "127.0.0.1:50051".to_owned())
        .parse()?;
    let db = kouga_model::db::connect(
        &std::env::var("DATABASE_URL")?,
        5,
        std::time::Duration::from_secs(5),
    )
    .await?;
    tonic::transport::Server::builder()
        .layer(tower::util::MapResponseLayer::new(
            kouga_grpc::normalize_message_size_status,
        ))
        .add_service(
            taskboard_rpc::rpc::board_server::BoardServer::new(taskboard_grpc::BoardService::new(
                db,
            ))
            .max_decoding_message_size(4 * 1024 * 1024),
        )
        .serve_with_shutdown(address, shutdown())
        .await?;
    let _ = telemetry.shutdown(std::time::Duration::from_secs(5)).await;
    Ok(())
}

async fn shutdown() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = term.recv() => {},
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}
