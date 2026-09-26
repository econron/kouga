use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn extra_binary_target_is_validated_and_does_not_overwrite_files_on_failure() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("kouga-t41-{}-{nonce}", std::process::id()));
    let app = root.join("api");
    fs::create_dir(&root).unwrap();
    let cli = env!("CARGO_BIN_EXE_kouga");
    assert!(
        Command::new(cli)
            .args(["new", "api", "--path"])
            .arg(&app)
            .status()
            .unwrap()
            .success()
    );
    let run = |arg: &str| {
        Command::new(cli)
            .current_dir(&app)
            .args(["dockerfile", "--binary", arg])
            .status()
            .unwrap()
    };
    for bad in [
        "evil=../../tmp:worker",
        "http=.:worker",
        "bad\nRUN=apps/worker:worker",
        "extra=apps/worker:missing",
    ] {
        assert!(!run(bad).success(), "accepted {bad}");
        assert!(!app.join("Dockerfile").exists());
        assert!(!app.join(".dockerignore").exists());
    }
    fs::create_dir_all(app.join("apps/custom/src/bin")).unwrap();
    fs::write(
        app.join("apps/custom/Cargo.toml"),
        "[package]\nname = \"custom-runtime\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(app.join("apps/custom/src/bin/notify.rs"), "fn main() {}\n").unwrap();
    assert!(run("notify=apps/custom:notify").success());
    let dockerfile = fs::read_to_string(app.join("Dockerfile")).unwrap();
    assert!(dockerfile.contains("cargo build --release --locked -p custom-runtime --bin notify"));
    assert!(dockerfile.contains("FROM runtime AS notify"));
    fs::remove_dir_all(root).unwrap();
}
