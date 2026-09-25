use std::{
    fs::OpenOptions,
    future::Future,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
    time::Duration,
};

use chrono::Utc;
use kouga_db::Db;
use sqlx::{Connection, Executor, postgres::PgConnectOptions};

use crate::{LOCK_KEY, MigrationError, MigrationSet, Migrator, MigratorOptions};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    Development,
    Test,
    Production,
}

/// The caller must show this name and environment before destructive work.
pub struct AdminTarget<'a> {
    pub url: &'a str,
    pub expected_database: &'a str,
    pub environment: Environment,
    pub allow_destructive: bool,
    pub allow_production: bool,
}

fn target_options(
    target: &AdminTarget<'_>,
    destructive: bool,
) -> Result<PgConnectOptions, MigrationError> {
    let options = PgConnectOptions::from_str(target.url)
        .map_err(|_| MigrationError::InvalidFile("invalid database URL".into()))?;
    let name = options
        .get_database()
        .ok_or_else(|| MigrationError::InvalidFile("database name is required".into()))?;
    if name != target.expected_database
        || name.is_empty()
        || name.len() > 63
        || name.chars().any(char::is_control)
        || matches!(name, "postgres" | "template0" | "template1")
    {
        return Err(MigrationError::InvalidFile(
            "database confirmation does not match a safe target".into(),
        ));
    }
    if destructive
        && (!target.allow_destructive
            || (target.environment == Environment::Production && !target.allow_production))
    {
        return Err(MigrationError::InvalidFile(
            "destructive database operation requires explicit permission".into(),
        ));
    }
    Ok(options)
}

fn quoted_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Creates only a named, non-system database; an existing database is an error.
pub async fn create_database(target: &AdminTarget<'_>) -> Result<(), MigrationError> {
    let options = target_options(target, false)?;
    let mut maintenance = sqlx::PgConnection::connect_with(&options.database("postgres")).await?;
    let name = quoted_identifier(target.expected_database);
    let sql = sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}"));
    maintenance.execute(sql).await?;
    Ok(())
}

/// Drops and recreates a development/test DB, then applies all migrations.
/// Production also requires a separate `allow_production` permit.
pub async fn reset_database(
    target: &AdminTarget<'_>,
    set: MigrationSet,
    options: MigratorOptions,
) -> Result<usize, MigrationError> {
    target_options(target, true)?;
    let url = target.url.to_owned();
    let expected_database = target.expected_database.to_owned();
    let environment = target.environment;
    let allow_destructive = target.allow_destructive;
    let allow_production = target.allow_production;
    // Caller cancellation must not leave ALLOW_CONNECTIONS disabled halfway through reset.
    tokio::spawn(async move {
        let target = AdminTarget {
            url: &url,
            expected_database: &expected_database,
            environment,
            allow_destructive,
            allow_production,
        };
        reset_database_inner(&target, set, options).await
    })
    .await
    .map_err(|_| {
        MigrationError::InvalidFile(
            "reset task terminated; inspect database access before retrying".into(),
        )
    })?
}

async fn restore_connections(options: &PgConnectOptions, name: &str) -> Result<(), MigrationError> {
    let mut maintenance =
        sqlx::PgConnection::connect_with(&options.clone().database("postgres")).await?;
    maintenance
        .execute(sqlx::AssertSqlSafe(format!(
            "ALTER DATABASE {name} WITH ALLOW_CONNECTIONS true"
        )))
        .await?;
    Ok(())
}

async fn reset_database_inner(
    target: &AdminTarget<'_>,
    set: MigrationSet,
    options: MigratorOptions,
) -> Result<usize, MigrationError> {
    let connect_options = target_options(target, true)?;
    let name = quoted_identifier(target.expected_database);
    let mut target_conn = sqlx::PgConnection::connect_with(&connect_options).await?;
    let mut maintenance =
        sqlx::PgConnection::connect_with(&connect_options.clone().database("postgres")).await?;
    let deadline = tokio::time::Instant::now() + options.lock_timeout;
    loop {
        let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
            .bind(LOCK_KEY)
            .fetch_one(&mut target_conn)
            .await?;
        if locked {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(MigrationError::LockTimeout);
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let locked_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut target_conn)
        .await?;
    // Once connections are disabled, no new migrator can acquire the target DB lock.
    if let Err(error) = maintenance
        .execute(sqlx::AssertSqlSafe(format!(
            "ALTER DATABASE {name} WITH ALLOW_CONNECTIONS false"
        )))
        .await
    {
        if restore_connections(&connect_options, &name).await.is_err() {
            return Err(MigrationError::InvalidFile(format!(
                "reset interrupted; manually restore connections with ALTER DATABASE {name} WITH ALLOW_CONNECTIONS true"
            )));
        }
        return Err(error.into());
    }
    let dropped = async {
        sqlx::query(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname=$1 AND pid<>$2",
        )
        .bind(target.expected_database)
        .bind(locked_pid)
        .execute(&mut maintenance)
        .await?;
        target_conn.close().await?;
        maintenance
            .execute(sqlx::AssertSqlSafe(format!("DROP DATABASE {name}")))
            .await?;
        maintenance
            .execute(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
            .await?;
        Ok::<_, MigrationError>(())
    }
    .await;
    if let Err(error) = dropped {
        // If the DB still exists, restore access after an unsuccessful drop.
        if restore_connections(&connect_options, &name).await.is_err() {
            return Err(MigrationError::InvalidFile(format!(
                "reset failed; manually restore connections with ALTER DATABASE {name} WITH ALLOW_CONNECTIONS true"
            )));
        }
        return Err(error);
    }
    let db = kouga_db::connect(target.url, 1, options.lock_timeout)
        .await
        .map_err(MigrationError::Database)?;
    Migrator::new(db, set, options).migrate().await
}

/// Uses the system `pg_dump`; no data, owners, or grants are written.
pub fn dump_schema(url: &str, path: impl AsRef<Path>) -> Result<(), MigrationError> {
    let parsed = url::Url::parse(url)
        .map_err(|_| MigrationError::InvalidFile("invalid database URL".into()))?;
    if !matches!(parsed.scheme(), "postgres" | "postgresql") {
        return Err(MigrationError::InvalidFile(
            "PostgreSQL URL required".into(),
        ));
    }
    let decoded = |value: &str| {
        percent_encoding::percent_decode_str(value)
            .decode_utf8()
            .map(|v| v.into_owned())
            .map_err(|_| MigrationError::InvalidFile("invalid URL encoding".into()))
    };
    let database = decoded(parsed.path().trim_start_matches('/'))?;
    if database.is_empty() {
        return Err(MigrationError::InvalidFile(
            "database name is required".into(),
        ));
    }
    let path = path.as_ref();
    let parent = path.parent().ok_or_else(|| {
        MigrationError::InvalidFile("schema output needs a parent directory".into())
    })?;
    std::fs::create_dir_all(parent).map_err(|e| MigrationError::InvalidFile(e.to_string()))?;
    let temp = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut command = Command::new("pg_dump");
    command.env("PGDATABASE", database);
    if !parsed.username().is_empty() {
        command.env("PGUSER", decoded(parsed.username())?);
    }
    if let Some(host) = parsed.host_str() {
        command.env("PGHOST", host);
    }
    if let Some(port) = parsed.port() {
        command.env("PGPORT", port.to_string());
    }
    if let Some(password) = parsed.password() {
        command.env("PGPASSWORD", decoded(password)?);
    }
    for (key, value) in parsed.query_pairs() {
        let variable = match key.as_ref() {
            "sslmode" => "PGSSLMODE",
            "sslrootcert" => "PGSSLROOTCERT",
            "sslcert" => "PGSSLCERT",
            "sslkey" => "PGSSLKEY",
            _ => {
                return Err(MigrationError::InvalidFile(format!(
                    "unsupported pg_dump URL option: {key}"
                )));
            }
        };
        command.env(variable, value.as_ref());
    }
    let result = command
        .arg("--schema-only")
        .arg("--no-owner")
        .arg("--no-privileges")
        .arg("--file")
        .arg(&temp)
        .status()
        .map_err(|e| MigrationError::InvalidFile(format!("pg_dump unavailable: {e}")))?;
    if !result.success() {
        let _ = std::fs::remove_file(&temp);
        return Err(MigrationError::InvalidFile("pg_dump failed".into()));
    }
    std::fs::rename(&temp, path).map_err(|e| MigrationError::InvalidFile(e.to_string()))?;
    Ok(())
}

/// Creates an up template only: no empty down file that falsely promises reversibility.
pub fn generate_migration(path: impl AsRef<Path>, name: &str) -> Result<PathBuf, MigrationError> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err(MigrationError::InvalidFile(
            "migration name must be snake_case".into(),
        ));
    }
    std::fs::create_dir_all(&path).map_err(|e| MigrationError::InvalidFile(e.to_string()))?;
    let filename = format!("{}_{}.up.sql", Utc::now().format("%Y%m%d%H%M%S"), name);
    let path = path.as_ref().join(filename);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| MigrationError::InvalidFile(e.to_string()))?;
    file.write_all(b"-- Write PostgreSQL SQL here. Add a matching .down.sql only if reversible.\n")
        .map_err(|e| MigrationError::InvalidFile(e.to_string()))?;
    Ok(path)
}

/// Runs application-registered seed code explicitly; server startup never calls it.
pub async fn run_seed<F, Fut, E>(db: Db, seed: F) -> Result<(), E>
where
    F: FnOnce(Db) -> Fut,
    Fut: Future<Output = Result<(), E>>,
{
    seed(db).await
}
