use super::{check_app, invalid};
use std::{
    error::Error,
    fs, io,
    path::{Path, PathBuf},
};

fn feature(manifest: &str, dependency: &str) -> Result<String, io::Error> {
    let prefix = format!("{dependency} = {{ path = ");
    let Some(line) = manifest.lines().find(|line| line.starts_with(&prefix)) else {
        return Ok(manifest.to_owned());
    };
    if line.contains("features = [\"otel\"]") {
        return Ok(manifest.to_owned());
    }
    if line.contains("features =") || !line.ends_with(" }") {
        return Err(invalid(
            "dependency was edited; add the otel feature manually",
        ));
    }
    Ok(manifest.replacen(
        line,
        &line.replacen(" }", ", features = [\"otel\"] }", 1),
        1,
    ))
}

fn enabled_manifest(old: &str) -> Result<String, io::Error> {
    if old.contains("kouga-telemetry =") {
        return Err(invalid("OTel is already installed"));
    }
    if !old.contains("[dependencies]\n") {
        return Err(invalid("Cargo.toml has no dependencies section"));
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| invalid("source checkout unavailable"))?;
    let mut manifest = old.replacen(
        "[dependencies]\n",
        &format!(
            "[dependencies]\nkouga-telemetry = {{ path = {:?} }}\n",
            root.join("crates/kouga-telemetry").display().to_string()
        ),
        1,
    );
    for dependency in ["kouga-http", "kouga-queue", "kouga-worker"] {
        manifest = feature(&manifest, dependency)?;
    }
    let tokio_line = manifest
        .lines()
        .find(|line| line.starts_with("tokio = "))
        .ok_or_else(|| invalid("tokio dependency not found"))?
        .to_owned();
    if !tokio_line.contains("\"signal\"") {
        if !tokio_line.ends_with("] }") {
            return Err(invalid("tokio features were edited; add signal manually"));
        }
        manifest = manifest.replacen(
            &tokio_line,
            &tokio_line.replacen("] }", ", \"signal\"] }", 1),
            1,
        );
    }
    Ok(manifest)
}

pub(super) fn service_name() -> Result<String, io::Error> {
    fs::read_to_string("Cargo.toml")?
        .lines()
        .find_map(|line| {
            line.strip_prefix("name = \"")
                .and_then(|name| name.strip_suffix('"'))
        })
        .map(str::to_owned)
        .ok_or_else(|| invalid("missing package name"))
}

pub(super) fn instrument(code: &str, service: &str, server: bool) -> Result<String, io::Error> {
    if code.contains("kouga: otel begin") {
        return Err(invalid("OTel is already installed in binary"));
    }
    let anchor = "async fn main() -> Result<(), Box<dyn std::error::Error>> {\n";
    if !code.contains(anchor) || !code.contains("    Ok(())\n}") {
        return Err(invalid(
            "binary was edited; initialize and flush telemetry manually",
        ));
    }
    let init = format!(
        "    // kouga: otel begin\n    let mut config = kouga_telemetry::TelemetryConfig::from_env()?;\n    if std::env::var_os(\"OTEL_SERVICE_NAME\").is_none() {{ config.service_name = {service:?}.into(); }}\n    let telemetry = kouga_telemetry::Telemetry::init(config)?;\n    // kouga: otel end\n"
    );
    let mut result = code.replacen(anchor, &format!("{anchor}{init}"), 1);
    if server {
        let line = result
            .lines()
            .find(|line| line.contains("axum::serve(listener,") && line.ends_with(".await?;"))
            .ok_or_else(|| invalid("server.rs was edited; add graceful shutdown manually"))?
            .to_owned();
        if !line.contains(".with_graceful_shutdown(") {
            result = result.replacen(
                &line,
                &line.replacen(
                    ".await?;",
                    ".with_graceful_shutdown(async { let _ = tokio::signal::ctrl_c().await; }).await?;",
                    1,
                ),
                1,
            );
        }
    } else {
        let anchor = "    let cancellation = tokio_util::sync::CancellationToken::new();\n";
        let stop = "    let stop = tokio_util::sync::CancellationToken::new();\n";
        let (anchor, variable) = if result.contains(anchor) {
            (anchor, "cancellation")
        } else if result.contains(stop) {
            (stop, "stop")
        } else {
            return Err(invalid("worker was edited; add shutdown signal manually"));
        };
        if !result.contains("let signal =") {
            result = result.replacen(anchor, &format!("{anchor}    let signal = {variable}.clone();\n    tokio::spawn(async move {{ let _ = tokio::signal::ctrl_c().await; signal.cancel(); }});\n"), 1);
        }
    }
    Ok(result.replacen(
        "    Ok(())\n}",
        "    telemetry.shutdown(std::time::Duration::from_secs(5)).await?;\n    Ok(())\n}",
        1,
    ))
}

pub(super) fn worker_code(code: &str, name: &str) -> Result<String, io::Error> {
    instrument(code, &format!("{name}-worker"), false)
}

pub(super) fn worker_dependencies(manifest: &str) -> Result<String, io::Error> {
    if !manifest.contains("kouga-telemetry =") {
        return Ok(manifest.to_owned());
    }
    let manifest = feature(manifest, "kouga-queue")?;
    feature(&manifest, "kouga-worker")
}

pub(super) fn add() -> Result<(), Box<dyn Error>> {
    check_app()?;
    if Path::new("apps/http/Cargo.toml").is_file() && !Path::new("src/bin/server.rs").is_file() {
        std::env::set_current_dir("apps/http")?;
    }
    let old = fs::read_to_string("Cargo.toml")?;
    let manifest = enabled_manifest(&old)?;
    let name = service_name()?;
    let server_path = PathBuf::from("src/bin/server.rs");
    let server = fs::read_to_string(&server_path)?;
    let app = name.replace('-', "_");
    let plain = include_str!("../templates/server.rs.txt").replace("APP_CRATE", &app);
    let database = include_str!("../templates/server-db.rs.txt").replace("APP_CRATE", &app);
    let auth = database.replacen(
        "axum::serve(listener, app)",
        "axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>())",
        1,
    );
    if server != plain && server != database && server != auth {
        eprintln!(
            "Required server.rs change: initialize Telemetry::init(TelemetryConfig::from_env()?) at startup, add Ctrl-C graceful shutdown, then call telemetry.shutdown(...).await before returning."
        );
        return Err(invalid("server.rs was edited; add OTel manually").into());
    }
    let server = instrument(&server, &format!("{name}-http"), true).inspect_err(|_| {
        eprintln!("Required server.rs change: initialize Telemetry::init(TelemetryConfig::from_env()?) at startup, add Ctrl-C graceful shutdown, then call telemetry.shutdown(...).await before returning.");
    })?;
    let mut updates = vec![
        (PathBuf::from("Cargo.toml"), manifest),
        (server_path, server),
    ];
    let worker_dir = super::worker_package::dir();
    let worker_manifest = worker_dir.join("Cargo.toml");
    if worker_manifest.is_file() {
        let old = fs::read_to_string(&worker_manifest)?;
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| invalid("source checkout unavailable"))?;
        let with_telemetry = old.replacen(
            "[dependencies]\n",
            &format!(
                "[dependencies]\nkouga-telemetry = {{ path = {:?} }}\n",
                root.join("crates/kouga-telemetry").display().to_string()
            ),
            1,
        );
        updates.push((worker_manifest, worker_dependencies(&with_telemetry)?));
    }
    for filename in ["job-worker.rs", "auth-mail-worker.rs"] {
        let path = worker_dir.join("src/bin").join(filename);
        if path.is_file() {
            let old = fs::read_to_string(&path)?;
            let updated = worker_code(&old, &name).inspect_err(|_| {
                eprintln!("Required {filename} change: initialize telemetry at startup, cancel worker on Ctrl-C, flush before exit.");
            })?;
            updates.push((path, updated));
        }
    }
    let lambda_dir = super::worker_package::root().join("apps/lambda");
    let lambda_manifest = lambda_dir.join("Cargo.toml");
    if lambda_manifest.is_file() {
        let old = fs::read_to_string(&lambda_manifest)?;
        if old.contains("kouga-telemetry =") {
            return Err(invalid("Lambda telemetry already installed").into());
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| invalid("source checkout unavailable"))?;
        let updated = old.replacen(
            "[dependencies]\n",
            &format!(
                "[dependencies]\nkouga-telemetry = {{ path = {:?} }}\n",
                root.join("crates/kouga-telemetry").display().to_string()
            ),
            1,
        );
        let path = lambda_dir.join("src/main.rs");
        let code = fs::read_to_string(&path)?;
        if !code.contains("// OTEL_SETUP")
            || !code.contains("// OTEL_BORROW")
            || !code.contains("// OTEL_FLUSH")
        {
            eprintln!(
                "Required Lambda change: initialize telemetry once and flush within the invocation deadline before returning."
            );
            return Err(invalid("Lambda adapter was edited; add OTel manually").into());
        }
        let code = code
            .replacen("// OTEL_SETUP", &super::lambda::telemetry_setup(&name), 1)
            .replacen("// OTEL_BORROW", "let telemetry = telemetry.clone();", 1)
            .replacen("// OTEL_FLUSH", super::lambda::telemetry_flush(), 1);
        updates.push((lambda_manifest, updated));
        updates.push((path, code));
    }
    for (path, content) in &updates {
        let old = fs::read_to_string(path)?;
        println!("--- {}\n+++ {}", path.display(), path.display());
        for line in content.lines().filter(|line| !old.contains(line)) {
            println!("+{line}");
        }
    }
    for (path, content) in updates {
        fs::write(path, content)?;
    }
    println!(
        "Enabled OTel for {name}; set OTEL_EXPORTER_OTLP_ENDPOINT to export. Existing workers inherit the integration."
    );
    Ok(())
}
