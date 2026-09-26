use clap::{Parser, Subcommand};
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

mod api;
mod auth;
mod container;
mod contracts;
mod db;
mod features;
mod lambda;
mod operations;
mod otel;
mod resource;
mod worker_package;

#[derive(Parser)]
#[command(name = "kouga", version, about = "Kouga application CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new HTTP or gRPC API application.
    New {
        name: String,
        /// Destination directory (defaults to ./<name>).
        #[arg(long)]
        path: Option<PathBuf>,
        #[arg(long, default_value = "http")]
        api: String,
    },
    /// Run the development HTTP server.
    Server {
        #[arg(long)]
        api: Option<String>,
    },
    /// Add another API entrance without replacing existing code.
    Add { api: String },
    /// Generate role-specific Docker build targets for the current application.
    Dockerfile,
    /// List the application's registered HTTP routes.
    Routes,
    /// Generate or verify openapi.yml from registered routes.
    Openapi {
        #[command(subcommand)]
        command: OpenapiCommand,
    },
    /// Generate a DB-backed HTTP resource.
    Generate {
        #[command(subcommand)]
        command: GenerateCommand,
    },
    /// Manage the generated application's database.
    Db {
        #[command(subcommand)]
        command: DbCommand,
    },
    /// Inspect and manage queued jobs.
    Jobs {
        #[command(subcommand)]
        command: JobsCommand,
    },
    /// Delete expired cache and authentication records.
    Maintenance,
    /// Open psql using DATABASE_URL without exposing it in process arguments.
    Console,
    /// Run a registered src/bin/task-<name>.rs binary.
    Runner { task: String },
    /// Run a generated worker binary.
    Worker {
        #[arg(long)]
        queue: Option<String>,
        #[arg(long)]
        once: bool,
    },
}

#[derive(Subcommand)]
enum GenerateCommand {
    Auth,
    Resource { name: String, fields: Vec<String> },
    Model { name: String, fields: Vec<String> },
    Request { name: String, fields: Vec<String> },
    Migration { name: String },
    Middleware { name: String },
    Mailer { name: String },
    Job { name: String, fields: Vec<String> },
    Channel { name: String },
}

#[derive(Subcommand)]
enum JobsCommand {
    List {
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=1000))]
        limit: u32,
    },
    Show {
        id: String,
    },
    Retry {
        id: String,
    },
    Cancel {
        id: String,
    },
    /// Read a JSON object from stdin; never pass payload/secrets in shell arguments.
    Enqueue {
        name: String,
        #[arg(long, default_value = "default")]
        queue: String,
        #[arg(long, default_value_t = 1)]
        version: i32,
    },
}

#[derive(Subcommand)]
enum DbCommand {
    Create,
    Migrate,
    Status,
    Rollback {
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        steps: u32,
    },
    Schema {
        #[arg(long, default_value = "db/schema.sql")]
        output: PathBuf,
    },
    /// Run the registered src/bin/task-seed.rs application task.
    Seed,
    Repair {
        #[arg(long)]
        version: String,
        #[arg(long, value_enum)]
        state: db::RepairTarget,
        #[arg(long)]
        reason: String,
    },
    Reset {
        #[arg(long)]
        database: String,
        #[arg(long, value_enum)]
        environment: db::TargetEnvironment,
        #[arg(long)]
        allow_destructive: bool,
        #[arg(long)]
        allow_production: bool,
        #[arg(long)]
        seed: bool,
    },
}

#[derive(Subcommand)]
enum OpenapiCommand {
    Generate {
        #[arg(long, default_value = "openapi.yml")]
        output: PathBuf,
    },
    Check {
        #[arg(long, default_value = "openapi.yml")]
        output: PathBuf,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("kouga: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn Error>> {
    match cli.command {
        Commands::New { name, path, api } => match api.as_str() {
            "http" => create(&name, path.as_deref().unwrap_or_else(|| Path::new(&name)))?,
            "grpc" => api::create_grpc(&name, path.as_deref().unwrap_or_else(|| Path::new(&name)))?,
            _ => return Err(invalid("--api must be http or grpc").into()),
        },
        Commands::Server { api } => {
            let api = api.unwrap_or_else(|| {
                if Path::new("src/bin/server.rs").exists() || Path::new("apps/http").exists() {
                    "http"
                } else {
                    "grpc"
                }
                .into()
            });
            if api == "http" && docs_enabled() {
                openapi(OpenapiCommand::Generate {
                    output: PathBuf::from("openapi.yml"),
                })?;
            }
            api::run_server(&api)?;
        }
        Commands::Add { api } => match api.as_str() {
            "otel" => operations::add_otel()?,
            "lambda" => lambda::add()?,
            _ => api::add(&api)?,
        },
        Commands::Dockerfile => container::generate()?,
        Commands::Routes => run_app("routes")?,
        Commands::Openapi { command } => openapi(command)?,
        Commands::Generate { command } => resource::generate(command)?,
        Commands::Db { command } => db::run(command)?,
        Commands::Jobs { command } => operations::jobs(command)?,
        Commands::Maintenance => operations::maintenance()?,
        Commands::Console => operations::console()?,
        Commands::Runner { task } => operations::runner(&task)?,
        Commands::Worker { queue, once } => operations::worker(queue.as_deref(), once)?,
    }
    Ok(())
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn docs_enabled() -> bool {
    matches!(
        std::env::var("KOUGA_ENV").as_deref(),
        Err(std::env::VarError::NotPresent) | Ok("development" | "test")
    )
}

fn valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes[0].is_ascii_lowercase()
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
        && !name.contains("--")
}

fn create(name: &str, destination: &Path) -> Result<(), Box<dyn Error>> {
    if !valid_name(name) {
        return Err(invalid("name must be lowercase kebab-case (for example, my-api)").into());
    }
    if fs::symlink_metadata(destination).is_ok() {
        return Err(
            io::Error::new(io::ErrorKind::AlreadyExists, "destination already exists").into(),
        );
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| invalid("source workspace not found"))?;
    let http = root.join("crates/kouga-http");
    if !http.is_dir() {
        return Err(invalid("Kouga source checkout is unavailable").into());
    }
    let root = fs::canonicalize(root)?;
    fs::create_dir(destination)?;
    fs::create_dir(destination.join("src"))?;
    fs::create_dir(destination.join("src/bin"))?;
    let manifest = format!(
        "[package]\nname = {name:?}\nversion = \"0.1.0\"\nedition = \"2024\"\nrust-version = \"1.94\"\n\n[package.metadata.kouga]\napi = \"http\"\n\n[dependencies]\nkouga-http = {{ path = {:?} }}\nkouga-openapi = {{ path = {:?} }}\naxum = \"=0.8.9\"\ntokio = {{ version = \"=1.53.1\", features = [\"macros\", \"rt-multi-thread\", \"net\"] }}\n",
        root.join("crates/kouga-http").display().to_string(),
        root.join("crates/kouga-openapi").display().to_string()
    );
    fs::write(
        destination.join("Cargo.toml"),
        manifest.replace("\"net\"]", "\"net\", \"signal\"]"),
    )?;
    fs::write(
        destination.join("src/lib.rs"),
        include_str!("../templates/lib.rs.txt"),
    )?;
    fs::write(
        destination.join("src/bin/server.rs"),
        include_str!("../templates/server.rs.txt").replace("APP_CRATE", &name.replace('-', "_")),
    )?;
    fs::write(
        destination.join("src/bin/routes.rs"),
        include_str!("../templates/routes.rs.txt").replace("APP_CRATE", &name.replace('-', "_")),
    )?;
    fs::write(
        destination.join("src/bin/openapi.rs"),
        include_str!("../templates/openapi.rs.txt").replace("APP_CRATE", &name.replace('-', "_")),
    )?;
    fs::write(
        destination.join("README.md"),
        format!(
            "# {name}\n\nRun `kouga server`, inspect routes with `kouga routes`, and open `/docs` in development.\nGenerate `openapi.yml` with `kouga openapi generate`; verify it in CI with `kouga openapi check`.\nFor a database-backed API, run `kouga generate resource Task title:string completed:bool=false`, set `DATABASE_URL`, then run `kouga db create` and `kouga db migrate` before `kouga server`.\nSet `TEST_DATABASE_URL` to a separate database when running generated tests.\nSet `KOUGA_ENV=production` to disable Docs and automatic file updates.\n\nThis preview uses a local path dependency on the Kouga checkout; keep it available when building.\n"
        ),
    )?;
    println!("Created {}", destination.display());
    Ok(())
}

fn run_app(binary: &str) -> Result<(), Box<dyn Error>> {
    check_app()?;
    let mut command = Command::new("cargo");
    command.args(["run", "--quiet"]);
    if Path::new("apps/http/Cargo.toml").is_file() && !Path::new("src/bin/server.rs").is_file() {
        command.args(["-p", &api::http_package()?]);
    }
    let status = command.args(["--bin", binary]).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("{binary} exited with status {status}")).into())
    }
}

fn check_app() -> Result<(), Box<dyn Error>> {
    let manifest = fs::read_to_string("Cargo.toml")
        .map_err(|_| invalid("run this command in a generated Kouga application"))?;
    if !manifest.contains("[package.metadata.kouga]")
        && !manifest.contains("[workspace.metadata.kouga]")
    {
        return Err(invalid("not a generated Kouga application").into());
    }
    Ok(())
}

fn openapi(command: OpenapiCommand) -> Result<(), Box<dyn Error>> {
    check_app()?;
    let mut command_runner = Command::new("cargo");
    command_runner.args(["run", "--quiet"]);
    if Path::new("apps/http/Cargo.toml").is_file() && !Path::new("src/bin/server.rs").is_file() {
        command_runner.args(["-p", &api::http_package()?]);
    }
    let generated = command_runner.args(["--bin", "openapi"]).output()?;
    if !generated.status.success() {
        return Err(
            io::Error::other(String::from_utf8_lossy(&generated.stderr).into_owned()).into(),
        );
    }
    let content = generated.stdout;
    match command {
        OpenapiCommand::Check { output } => {
            if fs::read(&output).ok().as_deref() != Some(content.as_slice()) {
                return Err(
                    io::Error::other(format!("{} is missing or stale", output.display())).into(),
                );
            }
        }
        OpenapiCommand::Generate { output } => {
            if fs::read(&output).ok().as_deref() == Some(content.as_slice()) {
                return Ok(());
            }
            let parent = output
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            let mut temporary = None;
            for nonce in 0..100 {
                let candidate = parent.join(format!(".openapi-{}-{nonce}.tmp", std::process::id()));
                match fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&candidate)
                {
                    Ok(mut file) => {
                        use std::io::Write;
                        let result = file.write_all(&content).and_then(|()| file.sync_all());
                        if let Err(error) = result {
                            let _ = fs::remove_file(&candidate);
                            return Err(error.into());
                        }
                        temporary = Some(candidate);
                        break;
                    }
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => return Err(error.into()),
                }
            }
            let temporary =
                temporary.ok_or_else(|| io::Error::other("no temporary file available"))?;
            if let Err(error) = fs::rename(&temporary, &output) {
                let _ = fs::remove_file(&temporary);
                return Err(error.into());
            }
            println!("Updated {}", output.display());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::valid_name;

    #[test]
    fn names_are_safe_package_names() {
        assert!(valid_name("my-api2"));
        for name in ["", "../escape", "Upper", "-bad", "bad-", "bad--name", "a_b"] {
            assert!(!valid_name(name));
        }
    }
}
