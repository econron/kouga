use super::{JobsCommand, check_app, invalid};
use std::{error::Error, fs, io, path::Path, process::Command, time::Duration};

fn database() -> Result<(tokio::runtime::Runtime, kouga_db::Db), Box<dyn Error>> {
    let url = std::env::var("DATABASE_URL")?;
    let runtime = tokio::runtime::Runtime::new()?;
    let db = runtime.block_on(kouga_db::connect(&url, 3, Duration::from_secs(5)))?;
    Ok((runtime, db))
}

fn run_db<T>(
    runtime: &tokio::runtime::Runtime,
    future: impl std::future::Future<Output = Result<T, sqlx::Error>>,
) -> Result<T, Box<dyn Error>> {
    runtime
        .block_on(future)
        .map_err(|_| io::Error::other("database operation failed").into())
}

fn job_id(value: &str) -> Result<uuid::Uuid, io::Error> {
    uuid::Uuid::parse_str(value).map_err(|_| invalid("job ID must be a UUID"))
}

fn safe_job_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

pub(super) fn jobs(command: JobsCommand) -> Result<(), Box<dyn Error>> {
    check_app()?;
    let (runtime, db) = database()?;
    match command {
        JobsCommand::List { limit } => {
            let rows: Vec<(uuid::Uuid, String, String, String, i32)> = run_db(&runtime,
                sqlx::query_as("SELECT id,name,queue,status,attempt FROM kouga_jobs ORDER BY created_at DESC,id DESC LIMIT $1")
                    .bind(i64::from(limit)).fetch_all(&db)
            )?;
            for (id, name, queue, status, attempt) in rows {
                println!("{id}\t{name}\t{queue}\t{status}\t{attempt}");
            }
        }
        JobsCommand::Show { id } => {
            let row: Option<(uuid::Uuid, String, i32, String, String, i32)> = run_db(
                &runtime,
                sqlx::query_as(
                    "SELECT id,name,version,queue,status,attempt FROM kouga_jobs WHERE id=$1",
                )
                .bind(job_id(&id)?)
                .fetch_optional(&db),
            )?;
            let (id, name, version, queue, status, attempt) =
                row.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "job not found"))?;
            println!(
                "id: {id}\nname: {name}\nversion: {version}\nqueue: {queue}\nstatus: {status}\nattempt: {attempt}"
            );
        }
        JobsCommand::Retry { id } => {
            let changed = run_db(&runtime, sqlx::query("UPDATE kouga_jobs SET status='pending',attempt=0,available_at=now(),failure_reason=NULL,lease_token=NULL,lease_until=NULL,updated_at=now() WHERE id=$1 AND status IN ('dead','quarantined')")
                .bind(job_id(&id)?).execute(&db))?.rows_affected();
            if changed == 0 {
                return Err(invalid("job is not dead or quarantined").into());
            }
            println!("Retried {id}");
        }
        JobsCommand::Cancel { id } => {
            let changed = run_db(&runtime, sqlx::query("UPDATE kouga_jobs SET status='cancelled',updated_at=now() WHERE id=$1 AND status='pending'")
                .bind(job_id(&id)?).execute(&db))?.rows_affected();
            if changed == 0 {
                return Err(invalid("job is not pending").into());
            }
            println!("Cancelled {id}");
        }
        JobsCommand::Enqueue {
            name,
            queue,
            version,
        } => {
            if !safe_job_name(&name) || !safe_job_name(&queue) || version < 1 {
                return Err(invalid("invalid job name, queue, or version").into());
            }
            let mut payload = String::new();
            use io::Read;
            io::stdin()
                .take(1024 * 1024 + 1)
                .read_to_string(&mut payload)?;
            if payload.len() > 1024 * 1024 {
                return Err(invalid("job payload exceeds 1 MiB").into());
            }
            let payload: serde_json::Value = serde_json::from_str(&payload)
                .map_err(|_| invalid("job payload must be valid JSON"))?;
            if !payload.is_object() {
                return Err(invalid("job payload must be a JSON object").into());
            }
            let id: uuid::Uuid = run_db(&runtime, sqlx::query_scalar("INSERT INTO kouga_jobs (name,version,queue,payload,available_at) VALUES ($1,$2,$3,$4,now()) RETURNING id")
                .bind(name).bind(version).bind(queue).bind(payload).fetch_one(&db))?;
            println!("{id}");
        }
    }
    Ok(())
}

pub(super) fn maintenance() -> Result<(), Box<dyn Error>> {
    check_app()?;
    let (runtime, db) = database()?;
    for (table, query) in [
        (
            "kouga_cache",
            "DELETE FROM kouga_cache WHERE expires_at <= now()",
        ),
        (
            "kouga_auth_tokens",
            "DELETE FROM kouga_auth_tokens WHERE expires_at <= now() OR revoked_at < now() - interval '30 days'",
        ),
        (
            "kouga_password_resets",
            "DELETE FROM kouga_password_resets WHERE expires_at <= now() OR consumed_at < now() - interval '30 days'",
        ),
        (
            "kouga_channel_tickets",
            "DELETE FROM kouga_channel_tickets WHERE expires_at <= now()",
        ),
    ] {
        let exists: bool = run_db(
            &runtime,
            sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
                .bind(table)
                .fetch_one(&db),
        )?;
        if exists {
            let count = run_db(&runtime, sqlx::query(query).execute(&db))?.rows_affected();
            println!("{table}: {count}");
        }
    }
    // Storage objects need a backend-specific delete; never drop metadata alone.
    Ok(())
}

pub(super) fn console() -> Result<(), Box<dyn Error>> {
    check_app()?;
    let raw = std::env::var("DATABASE_URL")?;
    let url = url::Url::parse(&raw).map_err(|_| invalid("invalid DATABASE_URL"))?;
    if !matches!(url.scheme(), "postgres" | "postgresql") || url.host_str().is_none() {
        return Err(invalid("DATABASE_URL must be a PostgreSQL URL").into());
    }
    let decode = |value: &str| {
        percent_encoding::percent_decode_str(value)
            .decode_utf8()
            .map(|value| value.into_owned())
            .map_err(|_| invalid("invalid DATABASE_URL encoding"))
    };
    let mut command = Command::new("psql");
    for key in [
        "PGHOST",
        "PGPORT",
        "PGUSER",
        "PGPASSWORD",
        "PGDATABASE",
        "PGSSLMODE",
        "PGOPTIONS",
    ] {
        command.env_remove(key);
    }
    command.env("PGHOST", url.host_str().unwrap());
    if let Some(port) = url.port() {
        command.env("PGPORT", port.to_string());
    }
    if !url.username().is_empty() {
        command.env("PGUSER", decode(url.username())?);
    }
    if let Some(password) = url.password() {
        command.env("PGPASSWORD", decode(password)?);
    }
    command.env("PGDATABASE", decode(url.path().trim_start_matches('/'))?);
    for (key, value) in url.query_pairs() {
        if key == "sslmode" {
            command.env("PGSSLMODE", value.as_ref());
        } else if key == "options" {
            command.env("PGOPTIONS", value.as_ref());
        }
    }
    let status = command.status().map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            io::Error::new(
                io::ErrorKind::NotFound,
                "psql is required; install the PostgreSQL client",
            )
        } else {
            error
        }
    })?;
    if !status.success() {
        return Err(io::Error::other("psql exited unsuccessfully").into());
    }
    Ok(())
}

pub(super) fn runner(task: &str) -> Result<(), Box<dyn Error>> {
    check_app()?;
    if !safe_job_name(task) || task.contains(['.', '-']) {
        return Err(invalid("task must be snake_case").into());
    }
    let binary = format!("task-{task}");
    let path = format!("src/bin/{binary}.rs");
    if !Path::new(&path).is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("registered task {task} not found"),
        )
        .into());
    }
    let status = Command::new("cargo")
        .args(["run", "--quiet", "--bin", &binary])
        .status()?;
    if !status.success() {
        return Err(io::Error::other(format!("task {task} exited unsuccessfully")).into());
    }
    Ok(())
}

pub(super) fn worker(queue: Option<&str>, once: bool) -> Result<(), Box<dyn Error>> {
    check_app()?;
    if Path::new("apps/http/Cargo.toml").is_file() && !Path::new("src/bin/server.rs").is_file() {
        std::env::set_current_dir("apps/http")?;
    }
    let worker = super::worker_package::dir();
    let binary = match queue {
        Some("mail") if worker.join("src/bin/auth-mail-worker.rs").is_file() => "auth-mail-worker",
        None | Some("default") if worker.join("src/bin/job-worker.rs").is_file() => "job-worker",
        None if worker.join("src/bin/auth-mail-worker.rs").is_file() => "auth-mail-worker",
        _ => return Err(invalid("no generated worker for the requested queue").into()),
    };
    let package = fs::read_to_string(worker.join("Cargo.toml"))?
        .lines()
        .find_map(|line| {
            line.strip_prefix("name = \"")
                .and_then(|s| s.strip_suffix('"'))
        })
        .ok_or_else(|| invalid("worker package name missing"))?
        .to_owned();
    let mut command = Command::new("cargo");
    command.args(["run", "--quiet", "-p", &package, "--bin", binary]);
    if once {
        command.args(["--", "--once"]);
    }
    let status = command.status()?;
    if !status.success() {
        return Err(io::Error::other(format!("{binary} exited unsuccessfully")).into());
    }
    Ok(())
}

pub(super) fn add_otel() -> Result<(), Box<dyn Error>> {
    super::otel::add()
}
