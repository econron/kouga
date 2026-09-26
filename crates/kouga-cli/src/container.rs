use super::{check_app, invalid};
use std::{error::Error, fs, io, path::Path};

fn package(path: &Path) -> io::Result<String> {
    fs::read_to_string(path)?
        .lines()
        .find_map(|line| {
            line.strip_prefix("name = \"")
                .and_then(|s| s.strip_suffix('"'))
        })
        .map(str::to_owned)
        .ok_or_else(|| invalid("package name missing"))
}

pub(super) fn generate(binaries: &[String]) -> Result<(), Box<dyn Error>> {
    check_app()?;
    if Path::new("Dockerfile").exists() || Path::new(".dockerignore").exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Dockerfile or .dockerignore already exists; preserve local edits",
        )
        .into());
    }
    let mut dockerfile = String::from(
        "# syntax=docker/dockerfile:1.7\n\
         # Build with: docker build --build-context kouga=/path/to/kouga --target http -t app-http .\n\
         FROM rust:1.94-bookworm AS build\n\
         RUN apt-get update && apt-get install -y --no-install-recommends protobuf-compiler ca-certificates && rm -rf /var/lib/apt/lists/*\n\
         COPY --from=kouga /Cargo.toml /kouga/Cargo.toml\n\
         COPY --from=kouga /Cargo.lock /kouga/Cargo.lock\n\
         COPY --from=kouga /crates /kouga/crates\n\
         WORKDIR /app\n\
         COPY . /app\n\
         RUN find /app -name Cargo.toml -type f -exec sed -i -E 's|path = \"[^\"]*/crates/(kouga-[^\"]+)\"|path = \"/kouga/crates/\\1\"|g' {} +\n\
         ENV CARGO_PROFILE_RELEASE_STRIP=symbols\n\
         FROM debian:bookworm-slim AS runtime\n\
         COPY --from=build /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt\n\
         ENV SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt KOUGA_ENV=production\n\
         WORKDIR /app\n\
         USER 65532:65532\n\
         STOPSIGNAL SIGTERM\n",
    );
    let http_manifest = if Path::new("src/bin/server.rs").is_file() {
        Some(Path::new("Cargo.toml"))
    } else if Path::new("apps/http/src/bin/server.rs").is_file() {
        Some(Path::new("apps/http/Cargo.toml"))
    } else {
        None
    };
    if let Some(manifest) = http_manifest {
        let name = package(manifest)?;
        target(
            &mut dockerfile,
            "http",
            &name,
            "server",
            "ENV PORT=8080\nEXPOSE 8080\n",
        );
        let admin_bin = if Path::new("src/bin/db-migrate.rs").is_file()
            || Path::new("apps/http/src/bin/db-migrate.rs").is_file()
        {
            Some("db-migrate")
        } else {
            None
        };
        if let Some(bin) = admin_bin {
            let source = if manifest == Path::new("Cargo.toml") {
                "/app/migrations"
            } else {
                "/app/apps/http/migrations"
            };
            target(
                &mut dockerfile,
                "admin",
                &name,
                bin,
                &format!("COPY --from=build --chown=65532:65532 {source} /app/migrations\n"),
            );
        }
    }
    if Path::new("apps/grpc/src/main.rs").is_file() {
        let name = package(Path::new("apps/grpc/Cargo.toml"))?;
        target(
            &mut dockerfile,
            "grpc",
            &name,
            "server-grpc",
            "ENV KOUGA_GRPC_BIND=0.0.0.0:50051\nEXPOSE 50051\n",
        );
    }
    if Path::new("apps/worker/Cargo.toml").is_file() {
        let name = package(Path::new("apps/worker/Cargo.toml"))?;
        let job = Path::new("apps/worker/src/bin/job-worker.rs").is_file();
        let mail = Path::new("apps/worker/src/bin/auth-mail-worker.rs").is_file();
        if job {
            target(&mut dockerfile, "worker", &name, "job-worker", "");
        }
        if mail {
            target(
                &mut dockerfile,
                if job { "mail-worker" } else { "worker" },
                &name,
                "auth-mail-worker",
                "",
            );
        }
    }
    if Path::new("apps/lambda/src/main.rs").is_file() {
        let name = package(Path::new("apps/lambda/Cargo.toml"))?;
        target(&mut dockerfile, "lambda-http", &name, &name, "");
    }
    for binary in binaries {
        let (target_name, source) = binary
            .split_once('=')
            .ok_or_else(|| invalid("--binary must be TARGET=PACKAGE_DIR:BINARY"))?;
        let (directory, binary_name) = source
            .split_once(':')
            .ok_or_else(|| invalid("--binary must be TARGET=PACKAGE_DIR:BINARY"))?;
        if !valid_identifier(target_name)
            || !valid_identifier(binary_name)
            || matches!(target_name, "build" | "runtime")
            || target_name.starts_with("build-")
            || !directory.split('/').all(valid_identifier)
            || dockerfile.contains(&format!("FROM runtime AS {target_name}\n"))
        {
            return Err(invalid(
                "invalid or duplicate Docker target, package directory, or binary",
            )
            .into());
        }
        let root = fs::canonicalize(".")?;
        if !fs::canonicalize(directory)?.starts_with(&root) {
            return Err(invalid("--binary package directory must stay inside the app").into());
        }
        let manifest = Path::new(directory).join("Cargo.toml");
        let name = package(&manifest)?;
        if !valid_identifier(&name)
            || !Path::new(directory)
                .join("src/bin")
                .join(format!("{binary_name}.rs"))
                .is_file()
        {
            return Err(invalid("--binary must reference a package src/bin/*.rs binary").into());
        }
        target(&mut dockerfile, target_name, &name, binary_name, "");
    }
    fs::write("Dockerfile", dockerfile)?;
    fs::write(
        ".dockerignore",
        "target\n**/target\n.git\n.env\n.env.*\n*.env\n.worktrees\n.DS_Store\n",
    )?;
    println!("Created Dockerfile and .dockerignore");
    Ok(())
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
}

fn target(out: &mut String, target: &str, package: &str, binary: &str, settings: &str) {
    out.push_str(&format!(
        r#"
FROM build AS build-{target}
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --locked -p {package} --bin {binary} && \
    mkdir -p /out && cp /app/target/release/{binary} /out/{binary}
FROM runtime AS {target}
COPY --from=build-{target} --chmod=0555 /out/{binary} /app/{binary}
{settings}ENTRYPOINT ["/app/{binary}"]
"#
    ));
}
