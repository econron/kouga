use super::{check_app, invalid};
use std::{error::Error, fs, io, path::Path};

pub(super) fn add() -> Result<(), Box<dyn Error>> {
    check_app()?;
    let root = super::worker_package::root();
    let http_manifest_path = if root.join("src/bin/server.rs").is_file() {
        root.join("Cargo.toml")
    } else if root.join("apps/http/src/bin/server.rs").is_file() {
        root.join("apps/http/Cargo.toml")
    } else {
        return Err(invalid("HTTP entrance is required before adding Lambda").into());
    };
    let lambda = root.join("apps/lambda");
    if lambda.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Lambda package already exists",
        )
        .into());
    }
    let http_manifest = fs::read_to_string(&http_manifest_path)?;
    let name = http_manifest
        .lines()
        .find_map(|line| {
            line.strip_prefix("name = \"")
                .and_then(|s| s.strip_suffix('"'))
        })
        .ok_or_else(|| invalid("HTTP package name missing"))?;
    let workspace = fs::read_to_string(root.join("Cargo.toml"))?;
    let updated = if let Some(line) = workspace
        .lines()
        .find(|line| line.starts_with("members = ["))
    {
        if !line.ends_with(']') {
            return Err(invalid("workspace members were edited").into());
        }
        workspace.replacen(
            line,
            &format!(
                "{}{}]",
                &line[..line.len() - 1],
                if line == "members = []" {
                    "\"apps/lambda\""
                } else {
                    ", \"apps/lambda\""
                }
            ),
            1,
        )
    } else if workspace.contains("[workspace]") {
        return Err(invalid("workspace members missing").into());
    } else {
        format!("{workspace}\n[workspace]\nmembers = [\"apps/lambda\"]\nresolver = \"3\"\n")
    };
    let http_path = if http_manifest_path == root.join("Cargo.toml") {
        "../.."
    } else {
        "../http"
    };
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| invalid("source checkout unavailable"))?;
    let db = http_manifest.contains("kouga-model =");
    let otel = http_manifest.contains("kouga-telemetry =");
    let manifest = format!(
        "[package]\nname = {lambda_name:?}\nversion = \"0.1.0\"\nedition = \"2024\"\nrust-version = \"1.94\"\n\n[dependencies]\napp-http = {{ package = {name:?}, path = {http_path:?} }}\naxum = \"=0.8.9\"\nkouga-openapi = {{ path = {:?} }}\nlambda_http = {{ version = \"=1.3.1\", default-features = false, features = [\"apigw_http\"] }}\ntokio = {{ version = \"=1.53.1\", features = [\"macros\", \"rt-multi-thread\", \"time\", \"sync\"] }}\ntower = \"=0.5.3\"\n{}{}",
        source.join("crates/kouga-openapi").display().to_string(),
        if db {
            format!(
                "kouga-model = {{ path = {:?} }}\n",
                source.join("crates/kouga-model").display().to_string()
            )
        } else {
            String::new()
        },
        if otel {
            format!(
                "kouga-telemetry = {{ path = {:?} }}\n",
                source.join("crates/kouga-telemetry").display().to_string()
            )
        } else {
            String::new()
        },
        lambda_name = format!("{name}-lambda"),
    );
    let setup = telemetry_setup(name);
    let code = include_str!("../templates/lambda.rs.txt")
        .replace("APP_CRATE", &name.replace('-', "_"))
        .replace("// DB_SETUP", if db { "let url = std::env::var(\"DATABASE_URL\")?;\n    let state = kouga_model::db::connect(&url, 5, std::time::Duration::from_secs(5)).await?;" } else { "let state = ();" })
        .replace("// OTEL_SETUP", if otel { &setup } else { "// OTEL_SETUP" })
        .replace("// OTEL_BORROW", if otel { "let telemetry = telemetry.clone();" } else { "// OTEL_BORROW" })
        .replace("// OTEL_FLUSH", if otel { telemetry_flush() } else { "// OTEL_FLUSH" });
    fs::create_dir_all(lambda.join("src"))?;
    fs::create_dir_all(lambda.join("tests"))?;
    fs::write(lambda.join("Cargo.toml"), manifest)?;
    fs::write(lambda.join("src/main.rs"), code)?;
    fs::write(
        lambda.join("tests/runtime_api.rs"),
        include_str!("../templates/lambda-runtime-test.rs.txt")
            .replace("APP_LAMBDA_BIN", &format!("{name}-lambda")),
    )?;
    fs::write(root.join("Cargo.toml"), updated)?;
    println!("Added Lambda HTTP target at {}", lambda.display());
    Ok(())
}

pub(super) fn telemetry_setup(name: &str) -> String {
    format!(
        "let mut config = kouga_telemetry::TelemetryConfig::from_env()?;\n    if std::env::var_os(\"OTEL_SERVICE_NAME\").is_none() {{ config.service_name = {:?}.into(); }}\n    let telemetry = std::sync::Arc::new(tokio::sync::Mutex::new(kouga_telemetry::Telemetry::init(config)?));",
        format!("{name}-lambda")
    )
}

pub(super) fn telemetry_flush() -> &'static str {
    "if let Ok(budget) = remaining_time(deadline)
                && let Err(error) = telemetry.lock().await.flush(budget.min(std::time::Duration::from_secs(2))).await
            {
                eprintln!(\"Kouga telemetry flush: {error}\");
            }"
}
