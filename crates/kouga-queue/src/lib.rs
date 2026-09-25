//! Persistent job enqueueing. The worker and SMTP do not appear in this dependency graph.

use chrono::{DateTime, Utc};
use kouga_db::{Acquire, Postgres};
use kouga_job::Job;
use std::{error::Error as StdError, fmt};
use uuid::Uuid;

pub const SCHEMA_SQL: &str = include_str!("../migrations/20260925000020_create_kouga_jobs.up.sql");

/// W3C trace context is kept separate from business payload and is optional.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TraceMetadata {
    pub traceparent: Option<String>,
    pub tracestate: Option<String>,
}

impl TraceMetadata {
    fn validate(&self) -> Result<(), QueueError> {
        if let Some(parent) = &self.traceparent {
            let parts: Vec<_> = parent.split('-').collect();
            let valid = parts.len() == 4
                && parts[0] == "00"
                && [2, 32, 16, 2].iter().zip(&parts).all(|(len, part)| {
                    part.len() == *len
                        && part
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                })
                && parts[1].bytes().any(|byte| byte != b'0')
                && parts[2].bytes().any(|byte| byte != b'0');
            if !valid {
                return Err(QueueError::InvalidContract("invalid traceparent"));
            }
        } else if self.tracestate.is_some() {
            return Err(QueueError::InvalidContract(
                "tracestate requires traceparent",
            ));
        }
        if let Some(state) = &self.tracestate
            && (state.len() > 512 || state.bytes().any(|byte| !(0x20..=0x7e).contains(&byte)))
        {
            return Err(QueueError::InvalidContract("invalid tracestate"));
        }
        Ok(())
    }
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

    #[tracing::instrument(name = "kouga.queue.enqueue", skip_all, fields(job.kind = Self::NAME, job.queue = Self::QUEUE))]
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
        #[cfg(feature = "otel")]
        let mut options = options;
        #[cfg(feature = "otel")]
        if options.trace.traceparent.is_none() && options.trace.tracestate.is_none() {
            (options.trace.traceparent, options.trace.tracestate) =
                kouga_telemetry::propagation::capture();
        }
        options.trace.validate()?;
        let payload = serde_json::to_value(self).map_err(QueueError::Serialize)?;
        let mut connection = db
            .acquire()
            .await
            .map_err(kouga_db::DbError::from)
            .map_err(QueueError::Database)?;
        sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO kouga_jobs (name, version, queue, payload, available_at, traceparent, tracestate) \
             VALUES ($1, $2, $3, $4, COALESCE($5, now()), $6, $7) RETURNING id",
        )
        .bind(Self::NAME)
        .bind(version)
        .bind(Self::QUEUE)
        .bind(payload)
        .bind(options.available_at)
        .bind(options.trace.traceparent)
        .bind(options.trace.tracestate)
        .fetch_one(&mut *connection)
        .await
        .map_err(kouga_db::DbError::from)
        .map_err(QueueError::Database)
    }
}

impl<J: Job> Enqueue for J {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_metadata_is_bounded_and_validated() {
        let valid = TraceMetadata {
            traceparent: Some("00-0123456789abcdef0123456789abcdef-0123456789abcdef-01".into()),
            tracestate: Some("vendor=opaque".into()),
        };
        assert!(valid.validate().is_ok());
        let mut bad = valid.clone();
        bad.traceparent = Some("00-00000000000000000000000000000000-0123456789abcdef-01".into());
        assert!(bad.validate().is_err());
        bad = valid;
        bad.tracestate = Some("x".repeat(513));
        assert!(bad.validate().is_err());
    }
}
