use app_contracts::task_notice::{AttachmentMailV1, TaskCreatedV1, TaskCreatedV2};
use futures_util::StreamExt;
use kouga_auth::CurrentUser;
use kouga_mailer::{MailMessage, SmtpMailer};
use kouga_model::{Db, Uuid, sqlx};
use kouga_worker::{JobContext, JobError, Worker, WorkerOptions};
use std::{sync::Arc, time::Duration};
use tracing::Instrument;

#[path = "../../../../src/storage.rs"]
mod storage;

struct State {
    db: Db,
    mailer: SmtpMailer,
    from: String,
}

async fn process(
    task_id: Uuid,
    owner_id: Option<Uuid>,
    ctx: JobContext<State>,
) -> Result<(), JobError> {
    async move {
    let state = &ctx.state;
    let row: Option<(Uuid, String, String)> = sqlx::query_as(
        "SELECT t.owner_id, t.title, u.email FROM tasks t JOIN users u ON u.id=t.owner_id WHERE t.id=$1",
    )
    .bind(task_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| JobError::Retryable("task lookup failed"))?;
    let Some((actual_owner, title, email)) = row else {
        // Deletion before delivery is not an error; do not mail stale task data.
        return Ok(());
    };
    if owner_id.is_some_and(|owner| owner != actual_owner) {
        return Err(JobError::Permanent("task owner mismatch"));
    }
    sqlx::query("INSERT INTO task_notice_effects(job_id,task_id) VALUES($1,$2) ON CONFLICT(job_id) DO NOTHING")
        .bind(ctx.job_id)
        .bind(task_id)
        .execute(&state.db)
        .await
        .map_err(|_| JobError::Retryable("notification effect failed"))?;
    // Fixture-only crash window: a process can die after the durable business effect
    // and before queue acknowledgement. Never enable this outside local tests.
    if std::env::var("KOUGA_ENV").as_deref() == Ok("test")
        && let Ok(raw) = std::env::var("TASKBOARD_TEST_PAUSE_AFTER_EFFECT_MS")
    {
        let millis: u64 = raw
            .parse()
            .map_err(|_| JobError::Permanent("invalid test pause"))?;
        tokio::time::sleep(Duration::from_millis(millis.min(30_000))).await;
    }
    let message = MailMessage::new(
        &state.from,
        &email,
        "Task created",
        format!("New task: {title}\n"),
    )
    .map_err(|_| JobError::Permanent("invalid task notification address"))?;
    message
        .deliver(&state.mailer)
        .await
        .map_err(|_| JobError::Retryable("task mail delivery failed"))?;
    opentelemetry::global::meter("taskboard")
        .u64_counter("taskboard.mail.sent")
        .build()
        .add(1, &[]);
    tracing::info!("taskboard notification delivered");
    Ok(())
    }.instrument(tracing::info_span!("taskboard.notification.send")).await
}

async fn old(job: TaskCreatedV1, ctx: JobContext<State>) -> Result<(), JobError> {
    process(job.task_id, None, ctx).await
}

async fn current(job: TaskCreatedV2, ctx: JobContext<State>) -> Result<(), JobError> {
    process(job.task_id, Some(job.owner_id), ctx).await
}

async fn attachment_mail(job: AttachmentMailV1, ctx: JobContext<State>) -> Result<(), JobError> {
    let state = &ctx.state;
    let recipient: Option<String> = sqlx::query_scalar(
        "SELECT u.email FROM tasks t JOIN users u ON u.id=t.owner_id \
         JOIN kouga_files f ON f.record_type='tasks' AND f.record_id=t.id \
         WHERE t.id=$1 AND t.owner_id=$2 AND f.id=$3 AND f.owner_id=$2 AND f.state='attached'",
    )
    .bind(job.task_id)
    .bind(job.owner_id)
    .bind(job.file_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| JobError::Retryable("attachment lookup failed"))?;
    let Some(recipient) = recipient else {
        // A deleted attachment is not mailed from a stale queued request.
        return Ok(());
    };
    let storage = storage::open(state.db.clone())
        .map_err(|_| JobError::Retryable("storage configuration unavailable"))?;
    let download = match storage
        .download(CurrentUser { id: job.owner_id }, job.file_id)
        .await
    {
        Ok(download) => download,
        Err(kouga_storage::StorageError::NotFound) => return Ok(()),
        Err(_) => return Err(JobError::Retryable("attachment download failed")),
    };
    let mut bytes = Vec::new();
    let mut stream = download.stream;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| JobError::Retryable("attachment stream failed"))?;
        if bytes.len().saturating_add(chunk.len()) > 10 * 1024 * 1024 {
            return Err(JobError::Permanent("attachment exceeds mail limit"));
        }
        bytes.extend_from_slice(&chunk);
    }
    let mime = download
        .file
        .content_type
        .parse()
        .map_err(|_| JobError::Permanent("invalid attachment content type"))?;
    let mail = MailMessage::new(
        &state.from,
        &recipient,
        "Task attachment",
        "Your task attachment is enclosed.\n",
    )
    .and_then(|mail| mail.attach(download.file.original_name, mime, bytes))
    .map_err(|_| JobError::Permanent("invalid attachment mail"))?;
    mail.deliver(&state.mailer)
        .await
        .map_err(|_| JobError::Retryable("attachment mail delivery failed"))?;
    tracing::info!("taskboard attachment mail delivered");
    Ok(())
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = kouga_telemetry::TelemetryConfig::from_env()?;
    if std::env::var_os("OTEL_SERVICE_NAME").is_none() {
        config.service_name = "taskboard-notice-worker".into();
    }
    let telemetry = kouga_telemetry::Telemetry::init(config)?;
    let db = kouga_model::db::connect(&std::env::var("DATABASE_URL")?, 5, Duration::from_secs(5))
        .await?;
    let host = std::env::var("KOUGA_SMTP_HOST")?;
    let port: u16 = std::env::var("KOUGA_SMTP_PORT")?.parse()?;
    let credentials = match (
        std::env::var("KOUGA_SMTP_USER"),
        std::env::var("KOUGA_SMTP_PASSWORD"),
    ) {
        (Ok(user), Ok(password)) => Some(kouga_mailer::Credentials::new(user, password)),
        (Err(_), Err(_)) => None,
        _ => return Err("both SMTP credentials are required".into()),
    };
    let mailer = if std::env::var("KOUGA_ENV").as_deref() == Ok("test")
        && host == "127.0.0.1"
        && std::env::var("KOUGA_SMTP_LOCAL").as_deref() == Ok("1")
        && credentials.is_none()
    {
        SmtpMailer::insecure_local(port)
    } else {
        SmtpMailer::relay(&host, port, credentials)?
    };
    let state = Arc::new(State {
        db: db.clone(),
        mailer,
        from: std::env::var("KOUGA_MAIL_FROM")?,
    });
    let mut options = WorkerOptions {
        queues: vec!["task-mail".into()],
        ..WorkerOptions::default()
    };
    if std::env::var("KOUGA_ENV").as_deref() == Ok("test") {
        // Keep the local integration test lease comfortably above a complete SMTP
        // exchange, even when the host is busy building container images.
        options.lease_duration = Duration::from_secs(2);
        options.poll_interval = Duration::from_millis(20);
        options.retry_base = Duration::from_millis(20);
        options.retry_max = Duration::from_millis(100);
    }
    let mut worker = Worker::new(db, state, options)?;
    worker.register::<TaskCreatedV1>(old)?;
    worker.register::<TaskCreatedV2>(current)?;
    worker.register::<AttachmentMailV1>(attachment_mail)?;
    let cancellation = tokio_util::sync::CancellationToken::new();
    let signal = cancellation.clone();
    tokio::spawn(async move {
        shutdown().await;
        signal.cancel();
    });
    if std::env::args().any(|arg| arg == "--once") {
        worker
            .run_once(1, Duration::from_secs(5), cancellation)
            .await?;
    } else {
        worker.run_forever(cancellation).await?;
    }
    let _ = telemetry.shutdown(Duration::from_secs(5)).await;
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
