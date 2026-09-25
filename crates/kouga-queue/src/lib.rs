//! Persistent job enqueueing. The worker and SMTP do not appear in this dependency graph.

use chrono::{DateTime, Utc};
use kouga_db::{Acquire, Postgres};
use kouga_job::Job;
use std::{error::Error as StdError, fmt};
use uuid::Uuid;

pub const SCHEMA_SQL: &str = include_str!("schema.sql");

/// W3C trace context is kept separate from business payload and is optional.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TraceMetadata {
    pub traceparent: Option<String>,
    pub tracestate: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct EnqueueOptions {
    pub available_at: Option<DateTime<Utc>>,
    pub trace: TraceMetadata,
}

#[derive(Debug)]
pub enum QueueError {
    InvalidContract(&'static str),
    Serialize(serde_json::Error),
    Database(kouga_db::DbError),
}

impl fmt::Display for QueueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidContract(_) => "invalid job contract",
            Self::Serialize(_) => "job serialization failed",
            Self::Database(_) => "job enqueue failed",
        };
        f.write_str(message)
    }
}

impl StdError for QueueError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::InvalidContract(_) => None,
            Self::Serialize(error) => Some(error),
            Self::Database(error) => Some(error),
        }
    }
}

#[allow(async_fn_in_trait)]
pub trait Enqueue: Job {
    async fn enqueue<'c, A>(&self, db: A) -> Result<Uuid, QueueError>
    where
        A: Acquire<'c, Database = Postgres> + Send,
    {
        self.enqueue_with(db, EnqueueOptions::default()).await
    }

    async fn enqueue_at<'c, A>(&self, db: A, at: DateTime<Utc>) -> Result<Uuid, QueueError>
    where
        A: Acquire<'c, Database = Postgres> + Send,
    {
        self.enqueue_with(
            db,
            EnqueueOptions {
                available_at: Some(at),
                ..Default::default()
            },
        )
        .await
    }

    async fn enqueue_with<'c, A>(&self, db: A, options: EnqueueOptions) -> Result<Uuid, QueueError>
    where
        A: Acquire<'c, Database = Postgres> + Send,
    {
        if Self::NAME.is_empty() || Self::QUEUE.is_empty() || Self::VERSION == 0 {
            return Err(QueueError::InvalidContract(
                "empty name/queue or zero version",
            ));
        }
        let version = i32::try_from(Self::VERSION)
            .map_err(|_| QueueError::InvalidContract("version exceeds PostgreSQL integer"))?;
        let payload = serde_json::to_value(self).map_err(QueueError::Serialize)?;
        let mut connection = db
            .acquire()
            .await
            .map_err(kouga_db::DbError::from)
            .map_err(QueueError::Database)?;
        sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO kouga_jobs (name, version, queue, payload, available_at, traceparent, tracestate) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
        )
        .bind(Self::NAME)
        .bind(version)
        .bind(Self::QUEUE)
        .bind(payload)
        .bind(options.available_at.unwrap_or_else(Utc::now))
        .bind(options.trace.traceparent)
        .bind(options.trace.tracestate)
        .fetch_one(&mut *connection)
        .await
        .map_err(kouga_db::DbError::from)
        .map_err(QueueError::Database)
    }
}

impl<J: Job> Enqueue for J {}
