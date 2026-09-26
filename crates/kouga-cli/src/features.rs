use super::{invalid, resource};
use std::{
    error::Error,
    fs, io,
    path::{Path, PathBuf},
};

fn name(input: &str) -> Result<String, io::Error> {
    if !input.as_bytes().first().is_some_and(u8::is_ascii_uppercase) {
        return Err(invalid("name must be PascalCase"));
    }
    let snake = resource::snake(input).ok_or_else(|| invalid("name must be ASCII PascalCase"))?;
    if !resource::identifier(&snake) {
        return Err(invalid("invalid generated name"));
    }
    Ok(snake)
}

fn dependency(manifest: &str, dependency: &str) -> Result<String, io::Error> {
    if manifest.contains(&format!("{dependency} =")) {
        return Ok(manifest.to_owned());
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| invalid("source checkout unavailable"))?;
    let old = "[dependencies]\n";
    if !manifest.contains(old) {
        return Err(invalid("Cargo.toml has no dependencies section"));
    }
    Ok(manifest.replacen(
        old,
        &format!(
            "{old}{dependency} = {{ path = {:?} }}\n",
            root.join("crates").join(dependency).display().to_string()
        ),
        1,
    ))
}

fn module(lib: &str, module: &str) -> Result<String, io::Error> {
    if lib.contains(&format!("pub mod {module};")) {
        return Ok(lib.to_owned());
    }
    if !lib.contains("pub fn router()") {
        return Err(invalid("lib.rs was edited; add module manually"));
    }
    Ok(format!("pub mod {module};\n{lib}"))
}

fn emit(
    new_files: Vec<(PathBuf, String)>,
    updates: Vec<(PathBuf, String)>,
) -> Result<(), Box<dyn Error>> {
    for (path, _) in &new_files {
        if fs::symlink_metadata(path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} already exists", path.display()),
            )
            .into());
        }
    }
    for (path, _) in &updates {
        if fs::symlink_metadata(path)?.file_type().is_symlink() {
            return Err(invalid("refusing to edit a symlink").into());
        }
    }
    for (path, _) in &new_files {
        println!("+ {}", path.display());
    }
    for (path, content) in &updates {
        let old = fs::read_to_string(path)?;
        if &old != content {
            println!("--- {}\n+++ {}", path.display(), path.display());
            for line in content.lines().filter(|line| !old.contains(line)) {
                println!("+{line}");
            }
        }
    }
    for (path, content) in new_files {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        use io::Write;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?
            .write_all(content.as_bytes())?;
    }
    for (path, content) in updates {
        fs::write(path, content)?;
    }
    Ok(())
}

fn manifest_and_lib(
    module_name: &str,
    dependencies: &[&str],
) -> Result<(String, String), Box<dyn Error>> {
    let mut manifest = fs::read_to_string("Cargo.toml")?;
    for dependency_name in dependencies {
        manifest = dependency(&manifest, dependency_name)?;
    }
    let lib = module(&fs::read_to_string("src/lib.rs")?, module_name)?;
    Ok((manifest, lib))
}

pub(super) fn middleware(input: &str) -> Result<(), Box<dyn Error>> {
    let snake = name(input)?;
    let path = PathBuf::from(format!("src/middlewares/{snake}.rs"));
    let first = !Path::new("src/middlewares/mod.rs").exists();
    let (manifest, lib) = manifest_and_lib("middlewares", &[])?;
    let mut new_files = vec![(
        path,
        format!(
            "use kouga_http::{{Error, HttpRequest, Next}};\nuse axum::response::Response;\n\npub async fn {snake}<S: Clone + Send + Sync + 'static>(request: HttpRequest<S>, next: Next<S>) -> Result<Response, Error> {{\n    next.run(request).await\n}}\n"
        ),
    )];
    let mut updates = vec![(PathBuf::from("src/lib.rs"), lib)];
    let mod_path = PathBuf::from("src/middlewares/mod.rs");
    if first {
        new_files.push((mod_path, format!("pub mod {snake};\n")));
    } else {
        let mod_file = fs::read_to_string(&mod_path)?;
        updates.push((mod_path, format!("{mod_file}pub mod {snake};\n")));
    }
    if manifest != fs::read_to_string("Cargo.toml")? {
        updates.push((PathBuf::from("Cargo.toml"), manifest));
    }
    emit(new_files, updates)?;
    println!("Register explicitly in router(): .middleware(middlewares::{snake}::{snake})");
    Ok(())
}

pub(super) fn mailer(input: &str) -> Result<(), Box<dyn Error>> {
    let snake = name(input)?;
    super::worker_package::add_mailer()?;
    let worker = super::worker_package::dir();
    let first = !worker.join("src/mailers/mod.rs").exists();
    let mut new_files = vec![(
        worker.join(format!("src/mailers/{snake}.rs")),
        format!(
            "use kouga_mailer::{{MailError, MailMessage}};\n\npub fn build(to: &str, from: &str) -> Result<MailMessage, MailError> {{\n    MailMessage::new(from, to, \"{input}\", \"Hello from {input}\")\n}}\n"
        ),
    )];
    let mut updates = Vec::new();
    let mod_path = worker.join("src/mailers/mod.rs");
    if first {
        new_files.push((mod_path, format!("pub mod {snake};\n")));
        updates.push((worker.join("src/lib.rs"), "pub mod mailers;\n".into()));
    } else {
        updates.push((
            mod_path.clone(),
            format!("{}pub mod {snake};\n", fs::read_to_string(&mod_path)?),
        ));
    }
    emit(new_files, updates)?;
    println!(
        "Use mailers::{snake}::build in a worker; SMTP credentials stay in that worker's environment."
    );
    Ok(())
}

pub(super) fn job(input: &str, fields: &[String]) -> Result<(), Box<dyn Error>> {
    let snake = name(input)?;
    if fields.is_empty() {
        return Err(invalid("job needs at least one field").into());
    }
    let mut declarations = String::new();
    for field in fields {
        let (field_name, ty) = field
            .split_once(':')
            .ok_or_else(|| invalid("field must be name:type"))?;
        if !resource::identifier(field_name) || declarations.contains(&format!("pub {field_name}:"))
        {
            return Err(invalid("invalid or duplicate job field").into());
        }
        let ty = match ty {
            "uuid" => "uuid::Uuid",
            "string" => "String",
            "bool" => "bool",
            "integer" | "int" => "i32",
            "bigint" => "i64",
            _ => return Err(invalid("job field type must be uuid/string/bool/int/bigint").into()),
        };
        declarations.push_str(&format!("    pub {field_name}: {ty},\n"));
    }
    let contracts = super::contracts::ensure()?;
    super::worker_package::ensure()?;
    let (manifest, lib) = manifest_and_lib("jobs", &["kouga-queue"])?;
    let lib = lib.replacen("pub mod jobs;", "pub use app_contracts::jobs;", 1);
    let mod_path = contracts.join("src/jobs/mod.rs");
    let first = !mod_path.exists();
    let mut new_files = vec![(
        contracts.join(format!("src/jobs/{snake}.rs")),
        format!(
            "#[kouga_job::job(name = \"{snake}\", version = 1, queue = \"default\")]\npub struct {input} {{\n{declarations}}}\n"
        ),
    )];
    let otel_enabled = manifest.contains("kouga-telemetry =");
    let mut updates = vec![
        (PathBuf::from("Cargo.toml"), manifest),
        (PathBuf::from("src/lib.rs"), lib),
    ];
    if first {
        new_files.push((mod_path, format!("pub mod {snake};\n")));
        let contracts_lib = contracts.join("src/lib.rs");
        updates.push((
            contracts_lib.clone(),
            format!("{}pub mod jobs;\n", fs::read_to_string(&contracts_lib)?),
        ));
    } else {
        updates.push((
            mod_path.clone(),
            format!("{}pub mod {snake};\n", fs::read_to_string(&mod_path)?),
        ));
    }
    let worker_path = super::worker_package::dir().join("src/bin/job-worker.rs");
    let package = fs::read_to_string("Cargo.toml")?
        .lines()
        .find_map(|line| {
            line.strip_prefix("name = \"")
                .and_then(|s| s.strip_suffix('"'))
        })
        .ok_or_else(|| invalid("missing package name"))?
        .to_owned();
    let app = "app_contracts";
    if worker_path.exists() {
        let old = fs::read_to_string(&worker_path)?;
        if !old.contains("    // kouga: job registrations") {
            return Err(invalid("job-worker.rs was edited; register the job manually").into());
        }
        let before = format!(
            "    worker.register::<{app}::jobs::{snake}::{input}>(|_job, ctx: kouga_worker::JobContext<kouga_db::Db>| async move {{ println!(\"processed job {{}}\", ctx.job_id); Ok(()) }})?;\n"
        );
        updates.push((
            worker_path,
            old.replacen(
                "    // kouga: job registrations",
                &format!("{before}    // kouga: job registrations"),
                1,
            ),
        ));
    } else {
        let worker = format!(
            "use std::{{sync::Arc, time::Duration}};\nuse kouga_worker::{{Worker, WorkerOptions}};\n\n#[tokio::main(flavor = \"multi_thread\")]\nasync fn main() -> Result<(), Box<dyn std::error::Error>> {{\n    let db = kouga_db::connect(&std::env::var(\"DATABASE_URL\")?, 5, Duration::from_secs(5)).await?;\n    let mut worker = Worker::new(db.clone(), Arc::new(db), WorkerOptions::default())?;\n    worker.register::<{app}::jobs::{snake}::{input}>(|_job, ctx: kouga_worker::JobContext<kouga_db::Db>| async move {{ println!(\"processed job {{}}\", ctx.job_id); Ok(()) }})?;\n    // kouga: job registrations\n    let stop = tokio_util::sync::CancellationToken::new();\n    if std::env::args().any(|arg| arg == \"--once\") {{\n        worker.run_once(1, Duration::from_secs(30), stop).await?;\n    }} else {{ worker.run_forever(stop).await?; }}\n    Ok(())\n}}\n"
        );
        let worker = worker.replacen(
            "    let stop = tokio_util::sync::CancellationToken::new();\n",
            "    let stop = tokio_util::sync::CancellationToken::new();\n    let signal = stop.clone();\n    tokio::spawn(async move { shutdown().await; signal.cancel(); });\n",
            1,
        );
        let worker = format!("{worker}\n{}", include_str!("../templates/shutdown.rs.txt"));
        let worker = if otel_enabled {
            super::otel::worker_code(&worker, &package)?
        } else {
            worker
        };
        new_files.push((worker_path, worker));
    }
    let migrations: Vec<_> = fs::read_dir("migrations")
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| fs::read_to_string(entry.path()).ok())
        .collect();
    let has_queue_migration = migrations
        .iter()
        .any(|text| text.contains("CREATE TABLE kouga_jobs"));
    let has_worker_migration = migrations
        .iter()
        .any(|text| text.contains("ADD COLUMN failure_reason"));
    if !has_queue_migration || !has_worker_migration {
        let version = resource::timestamp()?;
        let up = format!(
            "{}{}",
            if has_queue_migration {
                ""
            } else {
                include_str!("../../kouga-queue/migrations/20260925000020_create_kouga_jobs.up.sql")
            },
            if has_worker_migration {
                ""
            } else {
                include_str!("../../kouga-worker/migrations/20260925000021_add_job_failure.up.sql")
            }
        );
        let down = if has_queue_migration {
            "DROP INDEX kouga_jobs_expired;\nALTER TABLE kouga_jobs DROP COLUMN failure_reason;\n"
        } else {
            "DROP TABLE kouga_jobs;\n"
        };
        new_files.push((
            PathBuf::from(format!("migrations/{version}_create_kouga_jobs.up.sql")),
            up,
        ));
        new_files.push((
            PathBuf::from(format!("migrations/{version}_create_kouga_jobs.down.sql")),
            down.into(),
        ));
    }
    emit(new_files, updates)?;
    println!(
        "Build worker: cargo build -p {package}-worker --bin job-worker; run once: kouga worker --once"
    );
    Ok(())
}

pub(super) fn channel(input: &str) -> Result<(), Box<dyn Error>> {
    let snake = name(input)?;
    if !Path::new("src/auth.rs").exists() {
        return Err(invalid("generate auth and migrate before adding a channel").into());
    }
    let manifest = dependency(&fs::read_to_string("Cargo.toml")?, "kouga-channel")?;
    let binary = format!("src/bin/channel-{snake}.rs");
    let code = format!(
        "use kouga_channel::{{Channel, Options}};\nuse std::time::Duration;\n\n#[tokio::main(flavor = \"multi_thread\")]\nasync fn main() -> Result<(), Box<dyn std::error::Error>> {{\n    let db = kouga_model::db::connect(&std::env::var(\"DATABASE_URL\")?, 5, Duration::from_secs(5)).await?;\n    let origin = std::env::var(\"KOUGA_CHANNEL_ORIGIN\")?;\n    let channel = Channel::start(db.clone(), Options {{ allowed_origins: vec![origin], ..Options::default() }}, |_actor, _action, _name| {{\n        // Define authorization for {snake} before accepting subscriptions.\n        false\n    }}).await?;\n    let address = std::env::var(\"KOUGA_CHANNEL_BIND\").unwrap_or_else(|_| \"127.0.0.1:3001\".into());\n    let listener = tokio::net::TcpListener::bind(address).await?;\n    axum::serve(listener, channel.router()).await?;\n    Ok(())\n}}\n"
    );
    let mut new_files = vec![(PathBuf::from(binary), code)];
    let has_channel_migration = fs::read_dir("migrations")
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| fs::read_to_string(entry.path()).ok())
        .any(|text| text.contains("CREATE TABLE kouga_channel_tickets"));
    if !has_channel_migration {
        let version = resource::timestamp()?;
        new_files.push((
            PathBuf::from(format!(
                "migrations/{version}_create_channel_tickets.up.sql"
            )),
            include_str!(
                "../../kouga-channel/migrations/20260925000025_create_channel_tickets.up.sql"
            )
            .to_owned(),
        ));
        new_files.push((
            PathBuf::from(format!(
                "migrations/{version}_create_channel_tickets.down.sql"
            )),
            include_str!(
                "../../kouga-channel/migrations/20260925000025_create_channel_tickets.down.sql"
            )
            .to_owned(),
        ));
    }
    emit(new_files, vec![(PathBuf::from("Cargo.toml"), manifest)])?;
    println!(
        "Set KOUGA_CHANNEL_ORIGIN and run the channel binary. Change the policy before production."
    );
    Ok(())
}
