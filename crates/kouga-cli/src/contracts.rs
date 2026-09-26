use super::{invalid, worker_package};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub(super) fn dir() -> PathBuf {
    worker_package::root().join("crates/contracts")
}

pub(super) fn ensure() -> io::Result<PathBuf> {
    let contracts = dir();
    if contracts.join("Cargo.toml").is_file() {
        return Ok(contracts);
    }
    let http_manifest = fs::read_to_string("Cargo.toml")?;
    let name = http_manifest
        .lines()
        .find_map(|line| {
            line.strip_prefix("name = \"")
                .and_then(|s| s.strip_suffix('"'))
        })
        .ok_or_else(|| invalid("package name missing"))?;
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or_else(|| invalid("source checkout unavailable"))?;
    let package = format!("{name}-contracts");
    let manifest = format!(
        "[package]\nname = {package:?}\nversion = \"0.1.0\"\nedition = \"2024\"\nrust-version = \"1.94\"\n\n[lib]\nname = \"app_contracts\"\n\n[dependencies]\nkouga-job = {{ path = {:?} }}\nkouga-model = {{ path = {:?} }}\nuuid = {{ version = \"=1.26.1\", features = [\"serde\"] }}\n",
        source.join("crates/kouga-job").display().to_string(),
        source.join("crates/kouga-model").display().to_string()
    );
    let app_path = if worker_package::root() == Path::new(".") {
        "crates/contracts"
    } else {
        "../../crates/contracts"
    };
    let http_manifest = http_manifest.replacen(
        "[dependencies]\n",
        &format!(
            "[dependencies]\napp-contracts = {{ package = {package:?}, path = {app_path:?} }}\n"
        ),
        1,
    );
    let root_path = worker_package::root().join("Cargo.toml");
    let root_manifest = fs::read_to_string(&root_path)?;
    let root_manifest = if root_path == Path::new("./Cargo.toml") {
        http_manifest.clone()
    } else {
        root_manifest
    };
    let root_manifest = if root_manifest.contains("[workspace]") {
        let line = root_manifest
            .lines()
            .find(|line| line.starts_with("members = ["))
            .ok_or_else(|| invalid("workspace members missing"))?;
        if !line.ends_with(']') {
            return Err(invalid("workspace members were edited"));
        }
        root_manifest.replacen(
            line,
            &format!("{}, \"crates/contracts\"]", &line[..line.len() - 1]),
            1,
        )
    } else {
        format!(
            "{root_manifest}\n[workspace]\nmembers = [\"crates/contracts\"]\nresolver = \"3\"\n"
        )
    };
    fs::create_dir_all(contracts.join("src"))?;
    fs::write(contracts.join("Cargo.toml"), manifest)?;
    fs::write(contracts.join("src/lib.rs"), "")?;
    fs::write("Cargo.toml", http_manifest)?;
    fs::write(root_path, root_manifest)?;
    let worker_manifest = worker_package::dir().join("Cargo.toml");
    if worker_manifest.is_file() {
        let old = fs::read_to_string(&worker_manifest)?;
        fs::write(worker_manifest, old.replacen("[dependencies]\n", &format!("[dependencies]\napp-contracts = {{ package = {package:?}, path = \"../../crates/contracts\" }}\n"), 1))?;
    }
    Ok(contracts)
}
