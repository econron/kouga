//! SQL migrations. Application servers never run these implicitly.

use std::{collections::BTreeMap, fmt, fs, path::Path, time::Duration};

use kouga_db::{Acquire, Db, DbError};
use sha2::{Digest, Sha256};
use sqlx::Row;

/// Shared by migrate, rollback, reset, and repair.
pub const LOCK_KEY: i64 = 0x4b4f_5547_415f_4442;
const HISTORY: &str = "CREATE TABLE IF NOT EXISTS _kouga_migrations (version text PRIMARY KEY, name text NOT NULL, up_checksum text NOT NULL, down_checksum text, mode text NOT NULL, state text NOT NULL, direction text NOT NULL, applied_at timestamptz NOT NULL DEFAULT now())";

#[derive(Debug)]
pub enum MigrationError {
    InvalidFile(String),
    HistoryMismatch(String),
    OutOfOrder(String),
    LockTimeout,
    Irreversible(String),
    Dirty(String),
    Database(DbError),
    Unsupported(String),
}

impl fmt::Display for MigrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFile(s)
            | Self::HistoryMismatch(s)
            | Self::OutOfOrder(s)
            | Self::Irreversible(s)
            | Self::Dirty(s)
            | Self::Unsupported(s) => f.write_str(s),
            Self::LockTimeout => f.write_str("migration lock timed out"),
            Self::Database(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for MigrationError {}

impl From<sqlx::Error> for MigrationError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(value.into())
    }
}

#[derive(Debug, Clone)]
pub struct Migration {
    pub version: String,
    pub name: String,
    pub up_checksum: String,
    pub down_checksum: Option<String>,
    pub transactional: bool,
    up: String,
}

#[derive(Debug, Clone)]
pub struct MigrationSet(Vec<Migration>);

type FilePair = (String, Option<Vec<u8>>, Option<Vec<u8>>);

impl MigrationSet {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, MigrationError> {
        let mut files: BTreeMap<String, FilePair> = BTreeMap::new();
        for entry in fs::read_dir(path).map_err(|e| MigrationError::InvalidFile(e.to_string()))? {
            let entry = entry.map_err(|e| MigrationError::InvalidFile(e.to_string()))?;
            if !entry
                .file_type()
                .map_err(|e| MigrationError::InvalidFile(e.to_string()))?
                .is_file()
            {
                continue;
            }
            let filename = entry
                .file_name()
                .into_string()
                .map_err(|_| MigrationError::InvalidFile("non-UTF-8 filename".into()))?;
            if !filename.ends_with(".sql") {
                continue;
            }
            let (stem, direction) = filename
                .strip_suffix(".sql")
                .unwrap()
                .rsplit_once('.')
                .ok_or_else(|| MigrationError::InvalidFile(filename.clone()))?;
            if direction != "up" && direction != "down" {
                return Err(MigrationError::InvalidFile(filename));
            }
            let (version, name) = stem
                .split_once('_')
                .ok_or_else(|| MigrationError::InvalidFile(filename.clone()))?;
            if version.len() != 14
                || !version.bytes().all(|b| b.is_ascii_digit())
                || chrono::NaiveDateTime::parse_from_str(version, "%Y%m%d%H%M%S").is_err()
                || name.is_empty()
                || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            {
                return Err(MigrationError::InvalidFile(filename));
            }
            let bytes =
                fs::read(entry.path()).map_err(|e| MigrationError::InvalidFile(e.to_string()))?;
            let slot = files
                .entry(version.into())
                .or_insert_with(|| (name.into(), None, None));
            if slot.0 != name {
                return Err(MigrationError::InvalidFile(format!(
                    "duplicate version {version}"
                )));
            }
            let target = if direction == "up" {
                &mut slot.1
            } else {
                &mut slot.2
            };
            if target.replace(bytes).is_some() {
                return Err(MigrationError::InvalidFile(format!("duplicate {filename}")));
            }
        }
        let mut migrations = Vec::new();
        for (version, (name, up, down)) in files {
            let up =
                up.ok_or_else(|| MigrationError::InvalidFile(format!("missing up for {version}")))?;
            let up_sql = String::from_utf8(up.clone())
                .map_err(|_| MigrationError::InvalidFile(format!("non-UTF-8 up for {version}")))?;
            if up_sql.trim().is_empty() {
                return Err(MigrationError::InvalidFile(format!(
                    "empty up for {version}"
                )));
            }
            let transactional = up_sql
                .lines()
                .next()
                .is_none_or(|line| line.trim() != "-- kouga: transaction=false");
            let down_checksum = down
                .map(|bytes| {
                    String::from_utf8(bytes.clone()).map_err(|_| {
                        MigrationError::InvalidFile(format!("non-UTF-8 down for {version}"))
                    })?;
                    Ok::<_, MigrationError>(checksum(&bytes))
                })
                .transpose()?;
            migrations.push(Migration {
                version,
                name,
                up_checksum: checksum(&up),
                down_checksum,
                transactional,
                up: up_sql,
            });
        }
        Ok(Self(migrations))
    }

    pub fn migrations(&self) -> &[Migration] {
        &self.0
    }
}

fn checksum(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationState {
    Pending,
    Applied,
}

#[derive(Debug, Clone)]
pub struct MigrationStatus {
    pub version: String,
    pub name: String,
    pub state: MigrationState,
}

#[derive(Debug, Clone, Copy)]
pub struct MigratorOptions {
    pub lock_timeout: Duration,
    pub statement_timeout: Duration,
}

impl Default for MigratorOptions {
    fn default() -> Self {
        Self {
            lock_timeout: Duration::from_secs(10),
            statement_timeout: Duration::from_secs(60),
        }
    }
}

pub struct Migrator {
    db: Db,
    set: MigrationSet,
    options: MigratorOptions,
}

impl Migrator {
    pub fn new(db: Db, set: MigrationSet, options: MigratorOptions) -> Self {
        Self { db, set, options }
    }

    pub async fn status(&self) -> Result<Vec<MigrationStatus>, MigrationError> {
        let rows = history(&self.db).await?;
        check(&self.set, &rows)
    }

    pub async fn migrate(&self) -> Result<usize, MigrationError> {
        // A detached task retains and releases the session lock even if its caller is cancelled.
        let db = self.db.clone();
        let set = self.set.clone();
        let options = self.options;
        tokio::spawn(async move { migrate_locked(db, set, options).await })
            .await
            .map_err(|_| MigrationError::Database(DbError::new(kouga_db::DbErrorKind::Other)))?
    }
}

type HistoryRow = (
    String,
    String,
    String,
    Option<String>,
    String,
    String,
    String,
);

async fn history(db: &Db) -> Result<Vec<HistoryRow>, MigrationError> {
    let exists: Option<String> =
        sqlx::query_scalar("SELECT to_regclass('_kouga_migrations')::text")
            .fetch_one(db)
            .await?;
    if exists.is_none() {
        return Ok(Vec::new());
    }
    read_history(db).await
}

async fn read_history<'e, E>(executor: E) -> Result<Vec<HistoryRow>, MigrationError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let rows = sqlx::query("SELECT version, name, up_checksum, down_checksum, mode, state, direction FROM _kouga_migrations ORDER BY version")
        .fetch_all(executor).await?;
    rows.into_iter()
        .map(|row| {
            Ok((
                row.try_get(0)?,
                row.try_get(1)?,
                row.try_get(2)?,
                row.try_get(3)?,
                row.try_get(4)?,
                row.try_get(5)?,
                row.try_get(6)?,
            ))
        })
        .collect()
}

fn check(set: &MigrationSet, rows: &[HistoryRow]) -> Result<Vec<MigrationStatus>, MigrationError> {
    let max_applied = rows
        .iter()
        .filter(|r| r.5 == "applied")
        .map(|r| r.0.as_str())
        .max();
    for row in rows {
        if row.5 != "applied" {
            return Err(MigrationError::Dirty(row.0.clone()));
        }
        if row.6 != "up" {
            return Err(MigrationError::HistoryMismatch(row.0.clone()));
        }
        let file = set.0.iter().find(|m| m.version == row.0).ok_or_else(|| {
            MigrationError::HistoryMismatch(format!("missing migration {}", row.0))
        })?;
        let mode = if file.transactional {
            "transaction"
        } else {
            "non_transaction"
        };
        if file.name != row.1
            || file.up_checksum != row.2
            || file.down_checksum != row.3
            || mode != row.4
        {
            return Err(MigrationError::HistoryMismatch(format!(
                "changed migration {}",
                row.0
            )));
        }
    }
    set.0
        .iter()
        .map(|file| {
            let applied = rows.iter().any(|r| r.0 == file.version);
            if !applied && max_applied.is_some_and(|max| file.version.as_str() < max) {
                return Err(MigrationError::OutOfOrder(file.version.clone()));
            }
            Ok(MigrationStatus {
                version: file.version.clone(),
                name: file.name.clone(),
                state: if applied {
                    MigrationState::Applied
                } else {
                    MigrationState::Pending
                },
            })
        })
        .collect()
}

async fn migrate_locked(
    db: Db,
    set: MigrationSet,
    options: MigratorOptions,
) -> Result<usize, MigrationError> {
    check(&set, &history(&db).await?)?;
    let mut conn = db.acquire().await?;
    let deadline = tokio::time::Instant::now() + options.lock_timeout;
    loop {
        let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
            .bind(LOCK_KEY)
            .fetch_one(&mut *conn)
            .await?;
        if locked {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(MigrationError::LockTimeout);
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let result = async {
        sqlx::query(HISTORY).execute(&mut *conn).await?;
        let statuses = check(&set, &read_history(&mut *conn).await?)?;
        // Unsupported migrations must not partly apply the set.
        if let Some(m) = set.0.iter().zip(&statuses).find(|(m, s)| s.state == MigrationState::Pending && !m.transactional) {
            return Err(MigrationError::Unsupported(format!("non-transactional migration {} is not supported by T05", m.0.version)));
        }
        let mut applied = 0;
        for (migration, status) in set.0.iter().zip(statuses) {
            if status.state == MigrationState::Applied { continue; }
            let mut tx = conn.begin().await?;
            let timeout_ms = i64::try_from(options.statement_timeout.as_millis()).unwrap_or(i64::MAX);
            sqlx::query("SELECT set_config('statement_timeout', $1, true)").bind(timeout_ms.to_string()).execute(&mut *tx).await?;
            // Migration files are trusted developer-authored SQL, never request input.
            sqlx::raw_sql(sqlx::AssertSqlSafe(migration.up.as_str())).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO _kouga_migrations (version, name, up_checksum, down_checksum, mode, state, direction) VALUES ($1, $2, $3, $4, 'transaction', 'applied', 'up')")
                .bind(&migration.version).bind(&migration.name).bind(&migration.up_checksum).bind(&migration.down_checksum).execute(&mut *tx).await?;
            tx.commit().await?;
            applied += 1;
        }
        Ok(applied)
    }.await;
    let unlocked: Result<bool, sqlx::Error> = sqlx::query_scalar("SELECT pg_advisory_unlock($1)")
        .bind(LOCK_KEY)
        .fetch_one(&mut *conn)
        .await;
    if !matches!(unlocked, Ok(true)) {
        conn.close().await?;
    }
    unlocked.map_err(MigrationError::from).and_then(|ok| {
        if ok {
            Ok(())
        } else {
            Err(MigrationError::LockTimeout)
        }
    })?;
    result
}
