use clap::{Parser, Subcommand};
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

#[derive(Parser)]
#[command(name = "kouga", version, about = "Kouga application CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new HTTP API application.
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
        #[arg(long, default_value = "http")]
        api: String,
    },
    /// List the application's registered HTTP routes.
    Routes,
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
        Commands::New { name, path, api } => {
            check_http(&api)?;
            create(&name, path.as_deref().unwrap_or_else(|| Path::new(&name)))?;
        }
        Commands::Server { api } => {
            check_http(&api)?;
            run_app("server")?;
        }
        Commands::Routes => run_app("routes")?,
    }
    Ok(())
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn check_http(api: &str) -> Result<(), io::Error> {
    if api == "http" {
        Ok(())
    } else {
        Err(invalid("only --api http is implemented"))
    }
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
        "[package]\nname = {name:?}\nversion = \"0.1.0\"\nedition = \"2024\"\nrust-version = \"1.94\"\n\n[package.metadata.kouga]\napi = \"http\"\n\n[dependencies]\nkouga-http = {{ path = {:?} }}\naxum = \"=0.8.9\"\ntokio = {{ version = \"=1.53.1\", features = [\"macros\", \"rt-multi-thread\", \"net\"] }}\n",
        root.join("crates/kouga-http").display().to_string()
    );
    fs::write(destination.join("Cargo.toml"), manifest)?;
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
        destination.join("README.md"),
        format!(
            "# {name}\n\nRun `kouga server` and inspect routes with `kouga routes`.\n\nThis preview uses a local path dependency on the Kouga checkout; keep it available when building.\n"
        ),
    )?;
    println!("Created {}", destination.display());
    Ok(())
}

fn run_app(binary: &str) -> Result<(), Box<dyn Error>> {
    let manifest = fs::read_to_string("Cargo.toml")
        .map_err(|_| invalid("run this command in a generated Kouga application"))?;
    if !manifest.contains("[package.metadata.kouga]") {
        return Err(invalid("not a generated Kouga application").into());
    }
    let status = Command::new("cargo")
        .args(["run", "--quiet", "--bin", binary])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("{binary} exited with status {status}")).into())
    }
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
