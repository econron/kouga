//! PostgreSQL-backed, at-least-once job execution. Apply the queue migrations before running.

use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use kouga_db::Db;
use kouga_job::Job;
use serde_json::Value;
use sqlx::FromRow;
use tokio::{task::JoinSet, time::Instant};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

type JobFuture = Pin<Box<dyn Future<Output = Result<(), JobError>> + Send>>;
type ExecutionFuture = Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send>>;
type Handler<S> = Arc<dyn Fn(Value, JobContext<S>) -> ExecutionFuture + Send + Sync>;

enum ExecutionError {
    Decode,
    Handler(JobError),
}

pub trait JobHandler<J, S>: Send + Sync + 'static {
    fn call(&self, job: J, context: JobContext<S>) -> JobFuture;
}

impl<J, S, F, Fut> JobHandler<J, S> for F
where
    F: Fn(J, JobContext<S>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<(), JobError>> + Send + 'static,
{
    fn call(&self, job: J, context: JobContext<S>) -> JobFuture {
        Box::pin(self(job, context))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobError {
    Retryable(&'static str),
    Permanent(&'static str),
}

#[derive(Debug)]
pub enum WorkerError {
    InvalidConfig(&'static str),
    DuplicateHandler,
    Database(kouga_db::DbError),
    Task(tokio::task::JoinError),
    ShutdownTimeout,
}

impl std::fmt::Display for WorkerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfig(_) => f.write_str("invalid worker configuration"),
            Self::DuplicateHandler => f.write_str("duplicate job handler"),
            Self::Database(_) => f.write_str("worker database error"),
            Self::Task(_) => f.write_str("worker task failed"),
            Self::ShutdownTimeout => f.write_str("worker shutdown timed out"),
        }
    }
}

impl std::error::Error for WorkerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(e) => Some(e),
            Self::Task(e) => Some(e),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for WorkerError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(value.into())
    }
}

impl From<tokio::task::JoinError> for WorkerError {
    fn from(value: tokio::task::JoinError) -> Self {
        Self::Task(value)
    }
}

#[derive(Clone)]
pub struct JobContext<S> {
    pub state: Arc<S>,
    pub job_id: Uuid,
    pub attempt: i32,
    pub cancellation: CancellationToken,
}

#[derive(Clone)]
pub struct WorkerOptions {
    pub queues: Vec<String>,
    pub concurrency: usize,
    pub poll_interval: Duration,
    pub lease_duration: Duration,
    pub job_timeout: Duration,
    pub shutdown_grace: Duration,
    pub max_attempts: i32,
    pub retry_base: Duration,
    pub retry_max: Duration,
}

impl Default for WorkerOptions {
    fn default() -> Self {
        Self {
            queues: vec!["default".into()],
            concurrency: 4,
            poll_interval: Duration::from_millis(250),
            lease_duration: Duration::from_secs(30),
            job_timeout: Duration::from_secs(300),
            shutdown_grace: Duration::from_secs(30),
            max_attempts: 5,
            retry_base: Duration::from_secs(1),
            retry_max: Duration::from_secs(300),
        }
    }
}

impl WorkerOptions {
    fn validate(&self) -> Result<(), WorkerError> {
        if self.queues.is_empty()
            || self.queues.iter().any(String::is_empty)
            || self.concurrency == 0
            || self.poll_interval.is_zero()
            || self.lease_duration < Duration::from_millis(3)
            || self.job_timeout.is_zero()
            || self.shutdown_grace.is_zero()
            || self.max_attempts < 1
            || self.retry_base < Duration::from_millis(1)
            || self.retry_max < self.retry_base
            || i64::try_from(self.lease_duration.as_millis()).is_err()
            || i64::try_from(self.retry_max.as_millis()).is_err()
        {
            return Err(WorkerError::InvalidConfig(
                "invalid queue, duration, or limit",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RunReport {
    pub claimed: usize,
    pub succeeded: usize,
    pub retried: usize,
    pub dead: usize,
    pub quarantined: usize,
}

#[derive(Debug, Clone, FromRow)]
pub struct FailedJob {
    pub id: Uuid,
    pub name: String,
    pub version: i32,
    pub queue: String,
    pub attempt: i32,
    pub status: String,
    pub failure_reason: Option<String>,
    pub available_at: DateTime<Utc>,
}

#[derive(FromRow)]
struct Claimed {
    id: Uuid,
    name: String,
    version: i32,
    payload: Value,
    attempt: i32,
    lease_token: Uuid,
}

pub struct Worker<S> {
    db: Db,
    state: Arc<S>,
    handlers: HashMap<(String, i32), Handler<S>>,
    options: WorkerOptions,
}

impl<S: Send + Sync + 'static> Worker<S> {
    pub fn new(db: Db, state: Arc<S>, options: WorkerOptions) -> Result<Self, WorkerError> {
        options.validate()?;
        Ok(Self {
            db,
            state,
            handlers: HashMap::new(),
            options,
        })
    }

    pub fn register<J>(&mut self, handler: impl JobHandler<J, S>) -> Result<(), WorkerError>
    where
        J: Job,
    {
        let version =
            i32::try_from(J::VERSION).map_err(|_| WorkerError::InvalidConfig("job version"))?;
        if J::NAME.is_empty() || version == 0 || J::QUEUE.is_empty() {
            return Err(WorkerError::InvalidConfig("job contract"));
        }
        let key = (J::NAME.to_owned(), version);
        if self.handlers.contains_key(&key) {
            return Err(WorkerError::DuplicateHandler);
        }
        self.handlers.insert(
            key,
            Arc::new(
                move |payload, ctx| match serde_json::from_value::<J>(payload) {
                    Ok(job) => {
                        let future = handler.call(job, ctx);
                        Box::pin(async move { future.await.map_err(ExecutionError::Handler) })
                    }
                    Err(_) => Box::pin(async { Err(ExecutionError::Decode) }),
                },
            ),
        );
        Ok(())
    }

    /// Polls until cancelled. Active jobs get `shutdown_grace` to finish.
    pub async fn run_forever(
        &self,
        cancellation: CancellationToken,
    ) -> Result<RunReport, WorkerError> {
        self.run(None, cancellation).await
    }

    /// Stops on count, duration, empty queue, or cancellation; count is claims, not successes.
    pub async fn run_once(
        &self,
        max_jobs: usize,
        max_duration: Duration,
        cancellation: CancellationToken,
    ) -> Result<RunReport, WorkerError> {
        if max_jobs == 0 || max_duration.is_zero() {
            return Err(WorkerError::InvalidConfig("one-shot limits"));
        }
        self.run(
            Some((max_jobs, Instant::now() + max_duration)),
            cancellation,
        )
        .await
    }

    async fn run(
        &self,
        once: Option<(usize, Instant)>,
        cancellation: CancellationToken,
    ) -> Result<RunReport, WorkerError> {
        let handlers = Arc::new(self.handlers.clone());
        let mut tasks = JoinSet::new();
        let mut report = RunReport::default();
        let mut stopping = false;
        loop {
            if cancellation.is_cancelled()
                || once.is_some_and(|(max, deadline)| {
                    report.claimed >= max || Instant::now() >= deadline
                })
            {
                stopping = true;
            }
            if !stopping && tasks.len() < self.options.concurrency {
                let claim = tokio::select! {
                    _ = cancellation.cancelled() => { stopping = true; None }
                    result = claim(&self.db, &self.options) => result?,
                };
                if let Some(job) = claim {
                    report.claimed += 1;
                    let db = self.db.clone();
                    let state = self.state.clone();
                    let options = self.options.clone();
                    let handlers = handlers.clone();
                    tasks.spawn(async move { process(db, state, handlers, options, job).await });
                    continue;
                }
                if once.is_some() {
                    stopping = true;
                }
            }
            if stopping && tasks.is_empty() {
                return Ok(report);
            }
            if stopping {
                let drain = async {
                    while let Some(result) = tasks.join_next().await {
                        report.add(result??);
                    }
                    Ok::<_, WorkerError>(report)
                };
                return match tokio::time::timeout(self.options.shutdown_grace, drain).await {
                    Ok(result) => result,
                    Err(_) => {
                        tasks.abort_all();
                        Err(WorkerError::ShutdownTimeout)
                    }
                };
            }
            tokio::select! {
                _ = cancellation.cancelled() => stopping = true,
                result = tasks.join_next(), if !tasks.is_empty() => { if let Some(result) = result { report.add(result??); } },
                _ = tokio::time::sleep(self.options.poll_interval) => {},
            }
        }
    }

    pub async fn cancel_waiting(&self, id: Uuid) -> Result<bool, WorkerError> {
        let changed = sqlx::query("UPDATE kouga_jobs SET status='cancelled', updated_at=now() WHERE id=$1 AND status='pending'")
            .bind(id).execute(&self.db).await?.rows_affected();
        Ok(changed == 1)
    }

    pub async fn failed(&self, limit: i64) -> Result<Vec<FailedJob>, WorkerError> {
        if !(1..=1000).contains(&limit) {
            return Err(WorkerError::InvalidConfig("failed list limit"));
        }
        Ok(sqlx::query_as("SELECT id,name,version,queue,attempt,status,failure_reason,available_at FROM kouga_jobs WHERE status IN ('dead','quarantined') ORDER BY updated_at DESC LIMIT $1")
            .bind(limit).fetch_all(&self.db).await?)
    }

    pub async fn retry_failed(&self, id: Uuid) -> Result<bool, WorkerError> {
        let changed = sqlx::query("UPDATE kouga_jobs SET status='pending', attempt=0, available_at=now(), failure_reason=NULL, lease_token=NULL, lease_until=NULL, updated_at=now() WHERE id=$1 AND status IN ('dead','quarantined')")
            .bind(id).execute(&self.db).await?.rows_affected();
        Ok(changed == 1)
    }
}

impl RunReport {
    fn add(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Succeeded => self.succeeded += 1,
            Outcome::Retried => self.retried += 1,
            Outcome::Dead => self.dead += 1,
            Outcome::Quarantined => self.quarantined += 1,
            Outcome::LostLease => {}
        }
    }
}

#[derive(Clone, Copy)]
enum Outcome {
    Succeeded,
    Retried,
    Dead,
    Quarantined,
    LostLease,
}

async fn claim(db: &Db, options: &WorkerOptions) -> Result<Option<Claimed>, WorkerError> {
    let lease_ms = i64::try_from(options.lease_duration.as_millis())
        .map_err(|_| WorkerError::InvalidConfig("lease duration"))?;
    sqlx::query("UPDATE kouga_jobs SET status='dead', failure_reason='lease expired after maximum attempts', lease_token=NULL, lease_until=NULL, updated_at=now() WHERE queue=ANY($1) AND status='running' AND lease_until<=now() AND attempt >= $2")
        .bind(&options.queues).bind(options.max_attempts).execute(db).await?;
    Ok(sqlx::query_as("WITH candidate AS (SELECT id FROM kouga_jobs WHERE queue = ANY($1) AND ((status='pending' AND available_at<=now()) OR (status='running' AND lease_until<=now())) ORDER BY available_at,id FOR UPDATE SKIP LOCKED LIMIT 1) UPDATE kouga_jobs j SET status='running', attempt=j.attempt+1, lease_token=gen_random_uuid(), lease_until=now()+($2::bigint * interval '1 millisecond'), updated_at=now() FROM candidate WHERE j.id=candidate.id RETURNING j.id,j.name,j.version,j.payload,j.attempt,j.lease_token")
        .bind(&options.queues).bind(lease_ms).fetch_optional(db).await?)
}

async fn process<S: Send + Sync + 'static>(
    db: Db,
    state: Arc<S>,
    handlers: Arc<HashMap<(String, i32), Handler<S>>>,
    options: WorkerOptions,
    job: Claimed,
) -> Result<Outcome, WorkerError> {
    let Some(handler) = handlers.get(&(job.name.clone(), job.version)) else {
        return finish(&db, &job, "quarantined", "unknown job kind", None).await;
    };
    let cancellation = CancellationToken::new();
    let ctx = JobContext {
        state,
        job_id: job.id,
        attempt: job.attempt,
        cancellation: cancellation.clone(),
    };
    let run = tokio::time::timeout(options.job_timeout, handler(job.payload.clone(), ctx));
    tokio::pin!(run);
    let every = options.lease_duration / 3;
    let mut ticks = tokio::time::interval_at(Instant::now() + every, every);
    let result = loop {
        tokio::select! {
            result = &mut run => break result,
            _ = ticks.tick() => {
                let lease_ms = i64::try_from(options.lease_duration.as_millis()).map_err(|_| WorkerError::InvalidConfig("lease duration"))?;
                let changed = sqlx::query("UPDATE kouga_jobs SET lease_until=now()+($3::bigint * interval '1 millisecond'), updated_at=now() WHERE id=$1 AND lease_token=$2 AND status='running' AND lease_until>now()")
                    .bind(job.id).bind(job.lease_token).bind(lease_ms).execute(&db).await?;
                if changed.rows_affected() == 0 { cancellation.cancel(); return Ok(Outcome::LostLease); }
            }
        }
    };
    cancellation.cancel();
    let (status, reason, delay) = match result {
        Ok(Ok(())) => ("succeeded", "", None),
        Ok(Err(ExecutionError::Decode)) => ("quarantined", "invalid payload", None),
        Ok(Err(ExecutionError::Handler(JobError::Permanent(reason)))) => ("dead", reason, None),
        Ok(Err(ExecutionError::Handler(JobError::Retryable(reason))))
            if job.attempt < options.max_attempts =>
        {
            (
                "pending",
                reason,
                Some(retry_delay(&options, job.id, job.attempt)),
            )
        }
        Ok(Err(ExecutionError::Handler(JobError::Retryable(reason)))) => ("dead", reason, None),
        Err(_) if job.attempt < options.max_attempts => (
            "pending",
            "job timed out",
            Some(retry_delay(&options, job.id, job.attempt)),
        ),
        Err(_) => ("dead", "job timed out", None),
    };
    finish(&db, &job, status, reason, delay).await
}

fn retry_delay(options: &WorkerOptions, id: Uuid, attempt: i32) -> i64 {
    let power = u32::try_from(attempt.saturating_sub(1))
        .unwrap_or(0)
        .min(30);
    let base = options
        .retry_base
        .as_millis()
        .saturating_mul(1u128 << power)
        .min(options.retry_max.as_millis());
    let spread = (base / 4).max(1);
    let jitter = u128::from(id.as_u128() as u64).wrapping_add(attempt as u128) % (spread + 1);
    i64::try_from(base.saturating_sub(spread) + jitter).unwrap_or(i64::MAX)
}

async fn finish(
    db: &Db,
    job: &Claimed,
    status: &str,
    reason: &str,
    delay_ms: Option<i64>,
) -> Result<Outcome, WorkerError> {
    let changed = sqlx::query("UPDATE kouga_jobs SET status=$3, failure_reason=NULLIF($4,''), available_at=CASE WHEN $5::bigint IS NULL THEN available_at ELSE now()+($5::bigint * interval '1 millisecond') END, lease_token=NULL, lease_until=NULL, updated_at=now() WHERE id=$1 AND lease_token=$2 AND status='running' AND lease_until>now()")
        .bind(job.id).bind(job.lease_token).bind(status).bind(reason).bind(delay_ms).execute(db).await?.rows_affected();
    if changed == 0 {
        return Ok(Outcome::LostLease);
    }
    Ok(match status {
        "succeeded" => Outcome::Succeeded,
        "pending" => Outcome::Retried,
        "quarantined" => Outcome::Quarantined,
        _ => Outcome::Dead,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capped_retry_still_has_jitter() {
        let options = WorkerOptions {
            retry_base: Duration::from_millis(100),
            retry_max: Duration::from_millis(100),
            ..Default::default()
        };
        let a = retry_delay(&options, Uuid::from_u128(1), 1);
        let b = retry_delay(&options, Uuid::from_u128(2), 1);
        assert_ne!(a, b);
        assert!((75..=100).contains(&a));
    }
}
