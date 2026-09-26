use super::{check_app, invalid, valid_name};
use std::{error::Error, fs, io, path::Path, process::Command};

const GREET_ROUTE: &str = r#"    let router = router
        .get(
            "/greet/{name}",
            kouga_http::Endpoint::handler(
                |kouga_http::Path(name): kouga_http::Path<String>| async move {
                    kouga_http::Json(app_domain::greet(&name))
                },
                kouga_http::Operation::new("greeting.show")
                    .path_input::<String>()
                    .response::<kouga_http::Json<String>>(),
            ),
        )
        .expect("generated route is valid");
"#;
const GREET_BEGIN: &str = "    // kouga: greeting begin\n";
const GREET_END: &str = "    // kouga: greeting end\n";

pub(super) fn with_greeting(lib: &str) -> Result<String, io::Error> {
    if lib.contains("greeting.show") {
        return Err(invalid("greeting route already exists"));
    }
    let baseline = include_str!("../templates/lib.rs.txt");
    let lib = if let Some(prefix) = lib.strip_suffix(baseline) {
        format!("{prefix}{}", expanded_baseline())
    } else {
        lib.to_owned()
    };
    let at = "    router\n}";
    if !lib.contains(at) {
        return Err(invalid(
            "lib.rs was edited; register gRPC greeting route manually",
        ));
    }
    Ok(lib.replacen(
        at,
        &format!("{GREET_BEGIN}{GREET_ROUTE}{GREET_END}    router\n}}"),
        1,
    ))
}

pub(super) fn without_greeting(lib: &str) -> Option<String> {
    let begin = lib.find(GREET_BEGIN)?;
    let end = begin + lib[begin..].find(GREET_END)? + GREET_END.len();
    let old = format!("{}{}", &lib[..begin], &lib[end..]);
    let baseline = include_str!("../templates/lib.rs.txt");
    let expanded = expanded_baseline();
    if let Some(prefix) = old.strip_suffix(expanded.as_str()) {
        Some(format!("{prefix}{baseline}"))
    } else {
        Some(old)
    }
}

fn expanded_baseline() -> String {
    include_str!("../templates/lib.rs.txt")
        .replacen("    Router::new()", "    let router = Router::new()", 1)
        .replacen(
            ".expect(\"generated route is valid\")\n}",
            ".expect(\"generated route is valid\");\n    router\n}",
            1,
        )
}

pub(super) fn greeting_snippet(lib: &str) -> Option<&str> {
    let begin = lib.find(GREET_BEGIN)?;
    let end = begin + lib[begin..].find(GREET_END)? + GREET_END.len();
    Some(&lib[begin..end])
}

pub(super) fn create_grpc(name: &str, destination: &Path) -> Result<(), Box<dyn Error>> {
    if !valid_name(name) {
        return Err(invalid("name must be lowercase kebab-case").into());
    }
    if fs::symlink_metadata(destination).is_ok() {
        return Err(
            io::Error::new(io::ErrorKind::AlreadyExists, "destination already exists").into(),
        );
    }
    let source = kouga_source()?;
    fs::create_dir(destination)?;
    let manifest = format!(
        "[workspace]\nmembers = [\"crates/domain\", \"crates/rpc\", \"apps/grpc\"]\nresolver = \"3\"\n\n[workspace.metadata.kouga]\napi = \"grpc\"\nname = {name:?}\n"
    );
    fs::write(destination.join("Cargo.toml"), manifest)?;
    write_domain(destination, name)?;
    write_rpc(destination, name)?;
    write_grpc(destination, name, &source)?;
    fs::write(
        destination.join("README.md"),
        format!(
            "# {name}\n\nStart with `kouga server`; add HTTP later with `kouga add http`.\nThe shared operation is in `crates/domain`, the Protobuf contract in `proto/greeting.proto`, generated types in `crates/rpc`, and the gRPC binary in `apps/grpc`.\nUse `cargo build -p {name}-grpc --bin server-grpc` for an independent gRPC build. `protoc` is needed at build time.\nGenerated packages use local path dependencies on the Kouga source checkout.\n"
        ),
    )?;
    println!("Created {}", destination.display());
    Ok(())
}

pub(super) fn add(api: &str) -> Result<(), Box<dyn Error>> {
    check_app()?;
    match api {
        "grpc" => add_grpc(),
        "http" => add_http(),
        _ => Err(invalid("add expects http or grpc").into()),
    }
}

fn add_grpc() -> Result<(), Box<dyn Error>> {
    if !Path::new("src/lib.rs").is_file() || !Path::new("src/bin/server.rs").is_file() {
        return Err(invalid("gRPC is already present or HTTP application is missing").into());
    }
    for path in ["crates/domain", "crates/rpc", "apps/grpc", "proto"] {
        if fs::symlink_metadata(path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{path} already exists"),
            )
            .into());
        }
    }
    let source = kouga_source()?;
    let manifest = fs::read_to_string("Cargo.toml")?;
    if manifest.contains("[workspace]")
        || manifest.contains("app-domain =")
        || !manifest.contains("[dependencies]\n")
    {
        return Err(invalid("Cargo.toml already declares a workspace or domain dependency").into());
    }
    let name = package_name(&manifest)?;
    let lib = fs::read_to_string("src/lib.rs")?;
    let new_lib = with_greeting(&lib)?;
    let domain = format!("{name}-domain");
    let new_manifest = manifest.replacen(
        "[dependencies]\n",
        &format!(
            "[dependencies]\napp-domain = {{ package = {domain:?}, path = \"crates/domain\" }}\n"
        ),
        1,
    ) + "\n[workspace]\nmembers = [\"crates/domain\", \"crates/rpc\", \"apps/grpc\"]\nresolver = \"3\"\n";
    write_domain(Path::new("."), &name)?;
    write_rpc(Path::new("."), &name)?;
    write_grpc(Path::new("."), &name, &source)?;
    fs::write("src/lib.rs", new_lib)?;
    fs::write("Cargo.toml", new_manifest)?;
    println!("Added gRPC to {name}");
    Ok(())
}

fn add_http() -> Result<(), Box<dyn Error>> {
    if !Path::new("apps/grpc/Cargo.toml").is_file() {
        return Err(invalid("gRPC application is missing").into());
    }
    if Path::new("apps/http").exists() || Path::new("src/bin/server.rs").exists() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, "HTTP is already present").into());
    }
    let source = kouga_source()?;
    let manifest = fs::read_to_string("Cargo.toml")?;
    let expected = "members = [\"crates/domain\", \"crates/rpc\", \"apps/grpc\"]";
    if !manifest.contains(expected) {
        return Err(invalid("workspace was edited; add HTTP package manually").into());
    }
    let name = manifest
        .lines()
        .find_map(|line| {
            line.strip_prefix("name = \"")
                .and_then(|s| s.strip_suffix('"'))
        })
        .ok_or_else(|| invalid("missing Kouga application name"))?;
    let domain = format!("{name}-domain");
    let http = Path::new("apps/http");
    fs::create_dir_all(http.join("src/bin"))?;
    fs::write(
        http.join("Cargo.toml"),
        format!(
            "[package]\nname = {name:?}\nversion = \"0.1.0\"\nedition = \"2024\"\nrust-version = \"1.94\"\n\n[package.metadata.kouga]\napi = \"http\"\n\n[dependencies]\napp-domain = {{ path = \"../../crates/domain\" }}\nkouga-http = {{ path = {:?} }}\nkouga-openapi = {{ path = {:?} }}\naxum = \"=0.8.9\"\ntokio = {{ version = \"=1.53.1\", features = [\"macros\", \"rt-multi-thread\", \"net\"] }}\n",
            source.join("crates/kouga-http").display().to_string(),
            source.join("crates/kouga-openapi").display().to_string()
        ).replace("app-domain = { path =", &format!("app-domain = {{ package = {domain:?}, path =")),
    )?;
    let lib = with_greeting(include_str!("../templates/lib.rs.txt"))?;
    fs::write(http.join("src/lib.rs"), lib)?;
    for (file, template) in [
        ("server.rs", include_str!("../templates/server.rs.txt")),
        ("routes.rs", include_str!("../templates/routes.rs.txt")),
        ("openapi.rs", include_str!("../templates/openapi.rs.txt")),
    ] {
        fs::write(
            http.join("src/bin").join(file),
            template.replace("APP_CRATE", &name.replace('-', "_")),
        )?;
    }
    fs::write(
        "Cargo.toml",
        manifest.replacen(
            expected,
            "members = [\"crates/domain\", \"crates/rpc\", \"apps/grpc\", \"apps/http\"]",
            1,
        ),
    )?;
    println!("Added HTTP to {name}");
    Ok(())
}

pub(super) fn run_server(api: &str) -> Result<(), Box<dyn Error>> {
    check_app()?;
    let (package, binary) = match api {
        "http" if Path::new("src/bin/server.rs").is_file() => (None, "server"),
        "http" if Path::new("apps/http/src/bin/server.rs").is_file() => {
            (Some(http_package()?), "server")
        }
        "grpc" if Path::new("apps/grpc/src/main.rs").is_file() => {
            (Some(grpc_package()?), "server-grpc")
        }
        "http" | "grpc" => return Err(invalid("requested API entrance is not installed").into()),
        _ => return Err(invalid("--api must be http or grpc").into()),
    };
    let mut command = Command::new("cargo");
    command.args(["run", "--quiet"]);
    if let Some(package) = package {
        command.args(["-p", &package]);
    }
    let status = command.args(["--bin", binary]).status()?;
    if !status.success() {
        return Err(io::Error::other(format!("{binary} exited with status {status}")).into());
    }
    Ok(())
}

fn package_name(manifest: &str) -> Result<String, io::Error> {
    manifest
        .lines()
        .find_map(|line| {
            line.strip_prefix("name = \"")
                .and_then(|s| s.strip_suffix('"'))
        })
        .map(str::to_owned)
        .ok_or_else(|| invalid("missing package name"))
}

pub(super) fn http_package() -> Result<String, io::Error> {
    package_name(&fs::read_to_string("apps/http/Cargo.toml")?)
}

fn grpc_package() -> Result<String, io::Error> {
    package_name(&fs::read_to_string("apps/grpc/Cargo.toml")?)
}

fn kouga_source() -> Result<std::path::PathBuf, Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| invalid("Kouga source checkout unavailable"))?;
    Ok(fs::canonicalize(root)?)
}

fn write_domain(destination: &Path, name: &str) -> io::Result<()> {
    let domain = destination.join("crates/domain");
    fs::create_dir_all(domain.join("src"))?;
    fs::write(
        domain.join("Cargo.toml"),
        format!(
            "[package]\nname = {:?}\nversion = \"0.1.0\"\nedition = \"2024\"\nrust-version = \"1.94\"\n",
            format!("{name}-domain")
        ),
    )?;
    fs::write(
        domain.join("src/lib.rs"),
        include_str!("../templates/domain.rs.txt"),
    )
}

fn write_rpc(destination: &Path, name: &str) -> io::Result<()> {
    let rpc = destination.join("crates/rpc");
    fs::create_dir_all(rpc.join("src"))?;
    fs::create_dir_all(destination.join("proto"))?;
    fs::write(
        destination.join("proto/greeting.proto"),
        include_str!("../templates/greeting.proto.txt"),
    )?;
    fs::write(
        rpc.join("Cargo.toml"),
        format!(
            "[package]\nname = {:?}\nversion = \"0.1.0\"\nedition = \"2024\"\nrust-version = \"1.94\"\n\n[lib]\nname = \"app_rpc\"\n\n[dependencies]\nprost = \"=0.14.4\"\ntonic = \"=0.14.6\"\ntonic-prost = \"=0.14.6\"\n\n[build-dependencies]\ntonic-prost-build = \"=0.14.6\"\n",
            format!("{name}-rpc")
        ),
    )?;
    fs::write(
        rpc.join("build.rs"),
        "fn main() {\n    tonic_prost_build::compile_protos(\"../../proto/greeting.proto\")\n        .expect(\"compile greeting.proto\");\n}\n",
    )?;
    fs::write(
        rpc.join("src/lib.rs"),
        "pub mod rpc {\n    tonic::include_proto!(\"kouga\");\n}\n",
    )?;
    Ok(())
}

fn write_grpc(destination: &Path, name: &str, source: &Path) -> io::Result<()> {
    let grpc = destination.join("apps/grpc");
    let domain = format!("{name}-domain");
    let rpc = format!("{name}-rpc");
    fs::create_dir_all(grpc.join("src"))?;
    fs::write(
        grpc.join("Cargo.toml"),
        format!(
            "[package]\nname = {:?}\nversion = \"0.1.0\"\nedition = \"2024\"\nrust-version = \"1.94\"\n\n[lib]\nname = \"app_grpc\"\n\n[[bin]]\nname = \"server-grpc\"\npath = \"src/main.rs\"\n\n[[bin]]\nname = \"grpc-client\"\npath = \"src/client.rs\"\n\n[dependencies]\napp-domain = {{ package = {domain:?}, path = \"../../crates/domain\" }}\napp-rpc = {{ package = {rpc:?}, path = \"../../crates/rpc\" }}\nkouga-grpc = {{ path = {:?} }}\ntonic = {{ version = \"=0.14.6\", features = [\"transport\"] }}\ntokio = {{ version = \"=1.53.1\", features = [\"macros\", \"rt-multi-thread\", \"net\", \"time\"] }}\ntower = {{ version = \"=0.5.3\", features = [\"util\"] }}\n",
            format!("{name}-grpc"),
            source.join("crates/kouga-grpc").display().to_string()
        ),
    )?;
    fs::write(
        grpc.join("src/main.rs"),
        include_str!("../templates/grpc-main.rs.txt"),
    )?;
    fs::write(
        grpc.join("src/lib.rs"),
        include_str!("../templates/grpc-lib.rs.txt"),
    )?;
    fs::write(
        grpc.join("src/client.rs"),
        include_str!("../templates/grpc-client.rs.txt"),
    )?;
    Ok(())
}
