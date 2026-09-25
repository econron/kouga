use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn new_rejects_bad_names_and_never_overwrites_existing_files() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("kouga-cli-{}-{nonce}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let destination = root.join("my-api");
    let cli = env!("CARGO_BIN_EXE_kouga");
    assert!(
        !Command::new(cli)
            .args(["new", "../bad", "--path"])
            .arg(&destination)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(!destination.exists());
    assert!(
        Command::new(cli)
            .args(["new", "my-api", "--path"])
            .arg(&destination)
            .output()
            .unwrap()
            .status
            .success()
    );
    let manifest = fs::read_to_string(destination.join("Cargo.toml")).unwrap();
    assert!(manifest.contains("[package.metadata.kouga]"));
    assert!(destination.join("src/bin/server.rs").is_file());
    assert!(destination.join("src/bin/openapi.rs").is_file());
    let command = |args: &[&str]| {
        Command::new(cli)
            .current_dir(&destination)
            .env("CARGO_NET_OFFLINE", "true")
            .args(args)
            .output()
            .unwrap()
    };
    let first = command(&["openapi", "generate"]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let document = fs::read(destination.join("openapi.yml")).unwrap();
    assert!(String::from_utf8_lossy(&document).contains("\"openapi\": \"3.1.1\""));
    assert!(command(&["openapi", "check"]).status.success());
    assert!(command(&["openapi", "generate"]).status.success());
    assert_eq!(fs::read(destination.join("openapi.yml")).unwrap(), document);
    fs::write(destination.join("openapi.yml"), "stale").unwrap();
    assert!(!command(&["openapi", "check"]).status.success());
    assert_eq!(
        fs::read_to_string(destination.join("openapi.yml")).unwrap(),
        "stale"
    );
    assert!(command(&["openapi", "generate"]).status.success());
    fs::write(
        destination.join("src/bin/openapi.rs"),
        "compile_error!(\"broken schema\");",
    )
    .unwrap();
    assert!(!command(&["openapi", "generate"]).status.success());
    assert_eq!(fs::read(destination.join("openapi.yml")).unwrap(), document);
    assert!(
        !Command::new(cli)
            .args(["new", "my-api", "--path"])
            .arg(&destination)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(
        fs::read_to_string(destination.join("Cargo.toml")).unwrap(),
        manifest
    );
    assert!(
        !Command::new(cli)
            .args(["new", "grpc-api", "--api", "grpc", "--path"])
            .arg(root.join("grpc-api"))
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(!root.join("grpc-api").exists());
    fs::remove_dir_all(root).unwrap();
}
