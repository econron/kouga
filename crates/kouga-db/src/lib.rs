//! PostgreSQL access shared by models, migrations, and queues.
//! Use SQLx's `Acquire` directly so a pool or an existing transaction uses one connection.

use std::{error::Error as StdError, fmt, time::Duration};

pub use sqlx::{Acquire, PgConnection, Postgres, QueryBuilder};
pub type Db = sqlx::PgPool;
pub type Transaction<'a> = sqlx::Transaction<'a, Postgres>;

/// Connect only in binaries which need a database; never print the URL.
#[tracing::instrument(name = "kouga.db.query", skip_all, fields(db.operation = "connect"))]
pub async fn connect(
    url: &str,
    max_connections: u32,
    acquire_timeout: Duration,
) -> Result<Db, DbError> {
    if max_connections == 0 || acquire_timeout.is_zero() {
        return Err(DbError::new(DbErrorKind::InvalidInput));
    }
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(acquire_timeout)
        .connect(url)
        .await
        .map_err(Into::into)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsolationLevel {
    ReadCommitted,
    RepeatableRead,
    Serializable,
}

/// SQLx's default `db.begin()` is READ COMMITTED; this makes stronger levels explicit.
#[tracing::instrument(name = "kouga.db.query", skip_all, fields(db.operation = "begin"))]
pub async fn begin_with_isolation(
    db: &Db,
    isolation: IsolationLevel,
) -> Result<Transaction<'static>, DbError> {
    let statement = match isolation {
        IsolationLevel::ReadCommitted => "BEGIN ISOLATION LEVEL READ COMMITTED",
        IsolationLevel::RepeatableRead => "BEGIN ISOLATION LEVEL REPEATABLE READ",
        IsolationLevel::Serializable => "BEGIN ISOLATION LEVEL SERIALIZABLE",
    };
    db.begin_with(statement).await.map_err(Into::into)
}

/// Never retry a failed commit automatically: a lost connection may mean it committed.
#[tracing::instrument(name = "kouga.db.query", skip_all, fields(db.operation = "commit"))]
pub async fn commit_transaction(tx: Transaction<'_>) -> Result<(), DbError> {
    tx.commit().await.map_err(classify_commit_error)
}

fn classify_commit_error(source: sqlx::Error) -> DbError {
    let mut error = DbError::from(source);
    if error.kind == DbErrorKind::Connection {
        error.kind = DbErrorKind::CommitUnknown;
    }
    error
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintKind {
    Unique,
    ForeignKey,
    Check,
    NotNull,
    Exclusion,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbErrorKind {
    Constraint(ConstraintKind),
    Connection,
    PoolTimeout,
    Decode,
    Deadlock,
    Serialization,
    InvalidInput,
    Integrity,
    CommitUnknown,
    Other,
}

/// The source and constraint name are private to the application, never an API response.
pub struct DbError {
    pub kind: DbErrorKind,
    constraint_name: Option<String>,
    source: Option<sqlx::Error>,
}

impl DbError {
    pub fn new(kind: DbErrorKind) -> Self {
        Self {
            kind,
            constraint_name: None,
            source: None,
        }
    }

    /// Application code may match a known DB constraint to a safe domain error.
    pub fn constraint_name(&self) -> Option<&str> {
        self.constraint_name.as_deref()
    }

    pub fn into_core(self) -> kouga_core::Error {
        use kouga_core::{Error, ErrorKind};
        let (kind, code, message) = match self.kind {
            DbErrorKind::Connection | DbErrorKind::PoolTimeout => (
                ErrorKind::Unavailable,
                "db_unavailable",
                "Database unavailable",
            ),
            DbErrorKind::CommitUnknown => (
                ErrorKind::Unavailable,
                "db_commit_unknown",
                "Database commit result unknown",
            ),
            _ => (ErrorKind::Internal, "db_error", "Database error"),
        };
        Error::new(kind, code, message).with_source(self)
    }
}

impl From<sqlx::Error> for DbError {
    fn from(source: sqlx::Error) -> Self {
        use sqlx::Error;
        let (kind, constraint_name) = match &source {
            Error::Database(db) => {
                let code = db.code();
                let code = code.as_deref().unwrap_or("");
                let kind = match code {
                    "23505" => DbErrorKind::Constraint(ConstraintKind::Unique),
                    "23503" => DbErrorKind::Constraint(ConstraintKind::ForeignKey),
                    "23514" => DbErrorKind::Constraint(ConstraintKind::Check),
                    "23502" => DbErrorKind::Constraint(ConstraintKind::NotNull),
                    "23P01" => DbErrorKind::Constraint(ConstraintKind::Exclusion),
                    "40P01" => DbErrorKind::Deadlock,
                    "40001" => DbErrorKind::Serialization,
                    _ if code.starts_with("23") => DbErrorKind::Constraint(ConstraintKind::Other),
                    _ if code.starts_with("08") || matches!(code, "53300" | "57P03") => {
                        DbErrorKind::Connection
                    }
                    _ if code.starts_with("22") => DbErrorKind::InvalidInput,
                    _ => DbErrorKind::Other,
                };
                (kind, db.constraint().map(str::to_owned))
            }
            Error::PoolTimedOut => (DbErrorKind::PoolTimeout, None),
            Error::PoolClosed | Error::Io(_) | Error::Tls(_) | Error::Protocol(_) => {
                (DbErrorKind::Connection, None)
            }
            Error::ColumnDecode { .. }
            | Error::Decode(_)
            | Error::ColumnNotFound(_)
            | Error::ColumnIndexOutOfBounds { .. }
            | Error::TypeNotFound { .. } => (DbErrorKind::Decode, None),
            Error::Configuration(_) | Error::InvalidArgument(_) | Error::Encode(_) => {
                (DbErrorKind::InvalidInput, None)
            }
            _ => (DbErrorKind::Other, None),
        };
        Self {
            kind,
            constraint_name,
            source: Some(source),
        }
    }
}

impl fmt::Debug for DbError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DbError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for DbError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "database error: {:?}", self.kind)
    }
}

impl StdError for DbError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source.as_ref().map(|source| source as _)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_are_classified_without_leaking_sources() {
        let timed_out = DbError::from(sqlx::Error::PoolTimedOut);
        assert_eq!(timed_out.kind, DbErrorKind::PoolTimeout);
        assert_eq!(
            timed_out.into_core().kind,
            kouga_core::ErrorKind::Unavailable
        );

        let secret = "postgres://user:secret@localhost/db";
        let error = DbError::from(sqlx::Error::InvalidArgument(secret.into()));
        assert_eq!(error.kind, DbErrorKind::InvalidInput);
        assert!(!format!("{error:?} {error}").contains("secret"));

        let lost_connection = sqlx::Error::Io(std::io::Error::other("connection lost"));
        assert_eq!(
            classify_commit_error(lost_connection).kind,
            DbErrorKind::CommitUnknown
        );
    }

    #[tokio::test]
    async fn rejects_zero_limits_before_connecting() {
        let error = connect("invalid", 0, Duration::from_secs(1))
            .await
            .unwrap_err();
        assert_eq!(error.kind, DbErrorKind::InvalidInput);
        let error = connect("invalid", 1, Duration::ZERO).await.unwrap_err();
        assert_eq!(error.kind, DbErrorKind::InvalidInput);
    }
}
