use kouga_channel::{Channel, Options};
use std::time::Duration;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = kouga_model::db::connect(&std::env::var("DATABASE_URL")?, 5, Duration::from_secs(5))
        .await?;
    let origin = std::env::var("BOARD_CHANNEL_ORIGIN")?;
    let auth_check_interval = std::env::var("BOARD_CHANNEL_AUTH_CHECK_MS")
        .ok()
        .map(|value| value.parse::<u64>())
        .transpose()?
        .unwrap_or(1000);
    if !(100..=60_000).contains(&auth_check_interval) {
        return Err("BOARD_CHANNEL_AUTH_CHECK_MS must be 100..60000".into());
    }
    let channel = Channel::start(
        db,
        Options {
            allowed_origins: vec![origin],
            auth_check_interval: Duration::from_millis(auth_check_interval),
            ..Options::default()
        },
        taskboard_board::realtime::policy,
    )
    .await?;
    let bind = match std::env::var("BOARD_CHANNEL_BIND") {
        Ok(bind) => bind,
        Err(_) => format!(
            "0.0.0.0:{}",
            std::env::var("PORT").unwrap_or_else(|_| "3001".into())
        ),
    };
    let listener = tokio::net::TcpListener::bind(bind).await?;
    axum::serve(listener, channel.router())
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

async fn shutdown() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}
