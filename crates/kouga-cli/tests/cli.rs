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
