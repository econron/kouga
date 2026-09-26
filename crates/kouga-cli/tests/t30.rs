use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn feature_generators_preserve_edits_and_inherit_otel() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("kouga-t30-{}-{nonce}", std::process::id()));
    let app = root.join("demo-api");
    fs::create_dir(&root).unwrap();
    let cli = env!("CARGO_BIN_EXE_kouga");
    let new = Command::new(cli)
        .args(["new", "demo-api", "--path"])
        .arg(&app)
        .output()
        .unwrap();
    assert!(
        new.status.success(),
        "{}",
        String::from_utf8_lossy(&new.stderr)
    );
    let invoke = |args: &[&str]| {
        Command::new(cli)
            .current_dir(&app)
            .args(args)
            .output()
            .unwrap()
    };

    let first = invoke(&["generate", "middleware", "Audit"]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(String::from_utf8_lossy(&first.stdout).contains("Register explicitly"));
    let source = fs::read(app.join("src/middlewares/audit.rs")).unwrap();
    assert!(
        !invoke(&["generate", "middleware", "Audit"])
            .status
            .success()
    );
    assert_eq!(
        fs::read(app.join("src/middlewares/audit.rs")).unwrap(),
        source
    );

    let manifest = fs::read(app.join("Cargo.toml")).unwrap();
    let server_path = app.join("src/bin/server.rs");
    let server = fs::read_to_string(&server_path).unwrap();
    fs::write(&server_path, format!("{server}// user edit\n")).unwrap();
    let rejected = invoke(&["add", "otel"]);
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("Required server.rs change"));
    assert_eq!(fs::read(app.join("Cargo.toml")).unwrap(), manifest);
    assert!(
        fs::read_to_string(&server_path)
            .unwrap()
            .ends_with("// user edit\n")
    );
    fs::write(&server_path, server).unwrap();

    assert!(invoke(&["add", "otel"]).status.success());
    let otel_manifest = fs::read_to_string(app.join("Cargo.toml")).unwrap();
    assert!(otel_manifest.contains("kouga-telemetry ="));
    assert!(otel_manifest.contains("features = [\"otel\"]"));
    assert!(!invoke(&["add", "otel"]).status.success());
    assert_eq!(
        fs::read_to_string(app.join("Cargo.toml")).unwrap(),
        otel_manifest
    );

    let job = invoke(&["generate", "job", "SendWelcome", "user_id:uuid"]);
    assert!(
        job.status.success(),
        "{}",
        String::from_utf8_lossy(&job.stderr)
    );
    let worker = fs::read_to_string(app.join("apps/worker/src/bin/job-worker.rs")).unwrap();
    assert!(worker.contains("Telemetry::init"));
    assert!(worker.contains("telemetry.shutdown"));
    let manifest = fs::read_to_string(app.join("apps/worker/Cargo.toml")).unwrap();
    assert!(
        manifest.lines().any(
            |line| line.starts_with("kouga-worker =") && line.contains("features = [\"otel\"]")
        )
    );
    let migrations = fs::read_dir(app.join("migrations"))
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|entry| fs::read_to_string(entry.path()).ok())
        .collect::<String>();
    assert!(migrations.contains("CREATE TABLE kouga_jobs"));
    assert!(migrations.contains("ADD COLUMN failure_reason"));
    assert!(
        !invoke(&["generate", "job", "SendWelcome", "user_id:uuid"])
            .status
            .success()
    );
    assert_eq!(
        fs::read_to_string(app.join("apps/worker/src/bin/job-worker.rs")).unwrap(),
        worker
    );

    let auth = invoke(&["generate", "auth"]);
    assert!(
        auth.status.success(),
        "{}",
        String::from_utf8_lossy(&auth.stderr)
    );
    assert!(
        fs::read_to_string(app.join("apps/worker/src/bin/auth-mail-worker.rs"))
            .unwrap()
            .contains("Telemetry::init")
    );
    let auth_migrations = fs::read_dir(app.join("migrations"))
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|entry| fs::read_to_string(entry.path()).ok())
        .filter(|text| text.contains("CREATE TABLE kouga_password_resets"))
        .collect::<String>();
    assert!(!auth_migrations.contains("CREATE TABLE kouga_jobs"));
    assert!(!auth_migrations.contains("ADD COLUMN failure_reason"));

    assert!(invoke(&["generate", "mailer", "Welcome"]).status.success());
    assert!(invoke(&["generate", "channel", "Events"]).status.success());
    assert!(app.join("src/bin/channel-events.rs").exists());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn console_passes_decoded_credentials_without_echoing_them() {
    use std::os::unix::fs::PermissionsExt;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("kouga-t30-console-{}-{nonce}", std::process::id()));
    let app = root.join("demo-api");
    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let cli = env!("CARGO_BIN_EXE_kouga");
    assert!(
        Command::new(cli)
            .args(["new", "demo-api", "--path"])
            .arg(&app)
            .status()
            .unwrap()
            .success()
    );
    let psql = bin.join("psql");
    fs::write(&psql, "#!/bin/sh\n[ \"$PGUSER\" = 'user:name' ] || exit 11\n[ \"$PGPASSWORD\" = 'p@ss:word' ] || exit 12\n[ \"$PGDATABASE\" = 'demo' ] || exit 13\n[ \"$PGOPTIONS\" = '-csearch_path=custom' ] || exit 14\nprintf 'connected\\n'\n").unwrap();
    fs::set_permissions(&psql, fs::Permissions::from_mode(0o700)).unwrap();
    let output = Command::new(cli).current_dir(&app).arg("console")
        .env("PATH", &bin)
        .env("DATABASE_URL", "postgres://user%3Aname:p%40ss%3Aword@localhost:5432/demo?options=-csearch_path%3Dcustom")
        .output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"connected\n");
    assert!(!String::from_utf8_lossy(&output.stderr).contains("p@ss:word"));
    fs::remove_dir_all(root).unwrap();
}
