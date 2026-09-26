use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn dockerfile_targets_follow_installed_roles_and_worker_is_isolated() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("kouga-t31-{}-{nonce}", std::process::id()));
    let app = root.join("api");
    fs::create_dir(&root).unwrap();
    let cli = env!("CARGO_BIN_EXE_kouga");
    let new = Command::new(cli)
        .args(["new", "api", "--path"])
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
    assert!(
        run(&["generate", "resource", "Task", "title:string"])
            .status
            .success()
    );
    assert!(run(&["generate", "auth"]).status.success());
    assert!(
        run(&["generate", "job", "SendWelcome", "user_id:uuid"])
            .status
            .success()
    );
    assert!(run(&["add", "grpc"]).status.success());
    assert!(run(&["dockerfile"]).status.success());
    let dockerfile = fs::read_to_string(app.join("Dockerfile")).unwrap();
    for target in ["http", "grpc", "worker", "admin"] {
        assert!(dockerfile.contains(&format!("FROM runtime AS {target}")));
    }
    assert!(dockerfile.contains("--build-context kouga="));
    assert!(dockerfile.contains("USER 65532:65532"));
    assert!(!dockerfile.contains("COPY --from=build /app"));
    let http = fs::read_to_string(app.join("Cargo.toml")).unwrap();
    assert!(!http.contains("kouga-worker ="));
    assert!(!http.contains("kouga-mailer ="));
    let worker = fs::read_to_string(app.join("apps/worker/Cargo.toml")).unwrap();
    assert!(worker.contains("kouga-worker ="));
    assert!(worker.contains("kouga-mailer ="));
    assert!(app.join("apps/worker/src/bin/auth-mail-worker.rs").exists());
    let contracts = fs::read_to_string(app.join("crates/contracts/src/lib.rs")).unwrap();
    assert!(contracts.contains("pub mod auth_mail;"));
    assert!(contracts.contains("pub mod jobs;"));
    assert!(!run(&["dockerfile"]).status.success());
    assert_eq!(
        fs::read_to_string(app.join("Dockerfile")).unwrap(),
        dockerfile
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn grpc_first_project_can_add_http_and_generate_role_targets() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("kouga-t31-grpc-{}-{nonce}", std::process::id()));
    let app = root.join("api");
    fs::create_dir(&root).unwrap();
    let cli = env!("CARGO_BIN_EXE_kouga");
    let new = Command::new(cli)
        .args(["new", "api", "--api", "grpc", "--path"])
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
    assert!(run(&["dockerfile"]).status.success());
    let grpc_only = fs::read_to_string(app.join("Dockerfile")).unwrap();
    assert!(grpc_only.contains("FROM runtime AS grpc"));
    assert!(!grpc_only.contains("FROM runtime AS http"));
    fs::remove_file(app.join("Dockerfile")).unwrap();
    fs::remove_file(app.join(".dockerignore")).unwrap();
    assert!(run(&["add", "http"]).status.success());
    assert!(
        run(&["generate", "job", "SendWelcome", "user_id:uuid"])
            .status
            .success()
    );
    assert!(run(&["dockerfile"]).status.success());
    let both = fs::read_to_string(app.join("Dockerfile")).unwrap();
    for target in ["grpc", "http", "worker"] {
        assert!(both.contains(&format!("FROM runtime AS {target}")));
    }
    let http = fs::read_to_string(app.join("apps/http/Cargo.toml")).unwrap();
    assert!(!http.contains("kouga-worker ="));
    let worker = fs::read_to_string(app.join("apps/worker/Cargo.toml")).unwrap();
    assert!(!worker.contains("kouga-http ="));
    fs::remove_dir_all(root).unwrap();
}
