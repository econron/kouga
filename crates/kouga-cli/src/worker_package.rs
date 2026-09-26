use super::invalid;
use std::{fs, io, path::PathBuf};

pub(super) fn root() -> PathBuf {
    if PathBuf::from("../../apps/grpc/Cargo.toml").is_file() {
        PathBuf::from("../..")
    } else {
        PathBuf::from(".")
    }
}

pub(super) fn dir() -> PathBuf {
    root().join("apps/worker")
}

pub(super) fn ensure() -> io::Result<PathBuf> {
    let root = root();
    let worker = root.join("apps/worker");
    if worker.join("Cargo.toml").is_file() {
        return Ok(worker);
    }
    let http_manifest = fs::read_to_string("Cargo.toml")?;
    let name = http_manifest
        .lines()
        .find_map(|line| {
            line.strip_prefix("name = \"")
                .and_then(|s| s.strip_suffix('"'))
        })
        .ok_or_else(|| invalid("missing package name"))?;
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or_else(|| invalid("source checkout unavailable"))?;
    let worker_name = format!("{name}-worker");
    let mut manifest = format!(
        "[package]\nname = {worker_name:?}\nversion = \"0.1.0\"\nedition = \"2024\"\nrust-version = \"1.94\"\n\n[dependencies]\nkouga-worker = {{ path = {:?} }}\nkouga-db = {{ path = {:?} }}\ntokio = {{ version = \"=1.53.1\", features = [\"macros\", \"rt-multi-thread\", \"signal\"] }}\ntokio-util = {{ version = \"=0.7.19\", features = [\"rt\"] }}\n",
        source.join("crates/kouga-worker").display().to_string(),
        source.join("crates/kouga-db").display().to_string(),
    );
    if root.join("crates/contracts/Cargo.toml").is_file() {
        manifest = manifest.replacen("[dependencies]\n", &format!("[dependencies]\napp-contracts = {{ package = {:?}, path = \"../../crates/contracts\" }}\n", format!("{name}-contracts")), 1);
    }
    if http_manifest.contains("kouga-telemetry =") {
        manifest = manifest.replacen(
            "[dependencies]\n",
            &format!(
                "[dependencies]\nkouga-telemetry = {{ path = {:?} }}\n",
                source.join("crates/kouga-telemetry").display().to_string()
            ),
            1,
        );
        manifest = super::otel::worker_dependencies(&manifest)?;
    }
    let root_manifest_path = root.join("Cargo.toml");
    let root_manifest = fs::read_to_string(&root_manifest_path)?;
    let updated = if root_manifest.contains("[workspace]") {
        let line = root_manifest
            .lines()
            .find(|line| line.starts_with("members = ["))
            .ok_or_else(|| invalid("workspace members missing"))?;
        if !line.ends_with(']') {
            return Err(invalid("workspace members were edited"));
        }
        root_manifest.replacen(
            line,
            &format!(
                "{}{}]",
                &line[..line.len() - 1],
                if line == "members = []" {
                    "\"apps/worker\""
                } else {
                    ", \"apps/worker\""
                }
            ),
            1,
        )
    } else {
        format!("{root_manifest}\n[workspace]\nmembers = [\"apps/worker\"]\nresolver = \"3\"\n")
    };
    fs::create_dir_all(worker.join("src/bin"))?;
    fs::write(worker.join("Cargo.toml"), manifest)?;
    fs::write(worker.join("src/lib.rs"), "")?;
    fs::write(root_manifest_path, updated)?;
    Ok(worker)
}

pub(super) fn add_mailer() -> io::Result<()> {
    let worker = ensure()?;
    let path = worker.join("Cargo.toml");
    let old = fs::read_to_string(&path)?;
    if old.contains("kouga-mailer =") {
        return Ok(());
    }
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or_else(|| invalid("source checkout unavailable"))?;
    fs::write(
        path,
        old.replacen(
            "[dependencies]\n",
            &format!(
                "[dependencies]\nkouga-mailer = {{ path = {:?} }}\n",
                source.join("crates/kouga-mailer").display().to_string()
            ),
            1,
        ),
    )
}

pub(super) fn add_auth_mail() -> io::Result<()> {
    add_mailer()?;
    let path = dir().join("Cargo.toml");
    let old = fs::read_to_string(&path)?;
    if old.contains("kouga-model =") {
        return Ok(());
    }
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or_else(|| invalid("source checkout unavailable"))?;
    let additions = format!(
        "kouga-model = {{ path = {:?} }}\nsha2 = \"=0.10.9\"\n\n[dev-dependencies]\nkouga-test = {{ path = {:?} }}\n",
        source.join("crates/kouga-model").display().to_string(),
        source.join("crates/kouga-test").display().to_string()
    );
    fs::write(path, format!("{old}{additions}"))
}
