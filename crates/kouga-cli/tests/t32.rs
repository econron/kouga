use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn lambda_package_is_separate_and_docker_target_is_opt_in() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("kouga-t32-{}-{nonce}", std::process::id()));
    let app = root.join("demo");
    fs::create_dir(&root).unwrap();
    let cli = env!("CARGO_BIN_EXE_kouga");
    let new = Command::new(cli)
        .args(["new", "demo", "--path"])
        .arg(&app)
        .output()
        .unwrap();
    assert!(
        new.status.success(),
        "{}",
        String::from_utf8_lossy(&new.stderr)
    );
    let run = |args: &[&str]| {
        Command::new(cli)
            .current_dir(&app)
            .args(args)
            .output()
            .unwrap()
    };
    assert!(run(&["add", "lambda"]).status.success());
    assert!(!run(&["add", "lambda"]).status.success());
    assert!(run(&["dockerfile"]).status.success());
    let dockerfile = fs::read_to_string(app.join("Dockerfile")).unwrap();
    assert!(dockerfile.contains("FROM runtime AS lambda-http"));
    assert!(dockerfile.contains("FROM runtime AS http"));
    let http = fs::read_to_string(app.join("Cargo.toml")).unwrap();
    assert!(!http.contains("lambda_http ="));
    let lambda = fs::read_to_string(app.join("apps/lambda/Cargo.toml")).unwrap();
    assert!(lambda.contains("lambda_http ="));
    assert!(http.contains("\"apps/lambda\""));
    let code = fs::read_to_string(app.join("apps/lambda/src/main.rs")).unwrap();
    assert!(code.contains("source_ip"));
    assert!(code.contains("remaining_time"));
    assert!(run(&["add", "otel"]).status.success());
    let lambda = fs::read_to_string(app.join("apps/lambda/Cargo.toml")).unwrap();
    assert!(lambda.contains("kouga-telemetry ="));
    let code = fs::read_to_string(app.join("apps/lambda/src/main.rs")).unwrap();
    assert!(code.contains("telemetry.lock().await.flush(budget.min"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn grpc_first_requires_http_before_lambda() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("kouga-t32-grpc-{}-{nonce}", std::process::id()));
    let app = root.join("demo");
    fs::create_dir(&root).unwrap();
    let cli = env!("CARGO_BIN_EXE_kouga");
    let new = Command::new(cli)
        .args(["new", "demo", "--api", "grpc", "--path"])
        .arg(&app)
        .output()
        .unwrap();
    assert!(
        new.status.success(),
        "{}",
        String::from_utf8_lossy(&new.stderr)
    );
    let run = |args: &[&str]| {
        Command::new(cli)
            .current_dir(&app)
            .args(args)
            .output()
            .unwrap()
    };
    assert!(!run(&["add", "lambda"]).status.success());
    assert!(run(&["add", "http"]).status.success());
    assert!(run(&["add", "otel"]).status.success());
    assert!(
        Command::new(cli)
            .current_dir(app.join("apps/http"))
            .args(["add", "lambda"])
            .status()
            .unwrap()
            .success()
    );
    let manifest = fs::read_to_string(app.join("apps/lambda/Cargo.toml")).unwrap();
    assert!(manifest.contains("path = \"../http\""));
    assert!(manifest.contains("kouga-telemetry ="));
    fs::remove_dir_all(root).unwrap();
}
