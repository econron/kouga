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
    let grpc = root.join("grpc-api");
    assert!(
        Command::new(cli)
            .args(["new", "grpc-api", "--api", "grpc", "--path"])
            .arg(&grpc)
            .status()
            .unwrap()
            .success()
    );
    assert!(grpc.join("apps/grpc/src/main.rs").is_file());
    assert!(grpc.join("crates/rpc/build.rs").is_file());
    assert!(
        fs::read_to_string(grpc.join("apps/grpc/Cargo.toml"))
            .unwrap()
            .contains("package = \"grpc-api-domain\"")
    );
    assert!(!grpc.join("apps/http").exists());
    assert!(
        Command::new(cli)
            .current_dir(&grpc)
            .args(["add", "http"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        !Command::new(cli)
            .current_dir(&grpc)
            .args(["add", "http"])
            .status()
            .unwrap()
            .success()
    );
    assert!(grpc.join("apps/http/src/lib.rs").is_file());
    assert!(
        fs::read_to_string(grpc.join("apps/http/Cargo.toml"))
            .unwrap()
            .contains("package = \"grpc-api-domain\"")
    );
    let later = root.join("http-first");
    assert!(
        Command::new(cli)
            .args(["new", "http-first", "--path"])
            .arg(&later)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new(cli)
            .current_dir(&later)
            .args(["add", "grpc"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new(cli)
            .current_dir(&later)
            .args(["generate", "resource", "Task", "title:string"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        fs::read_to_string(later.join("src/lib.rs"))
            .unwrap()
            .contains("greeting.show")
    );
    let protected = root.join("protected");
    assert!(
        Command::new(cli)
            .args(["new", "protected", "--path"])
            .arg(&protected)
            .status()
            .unwrap()
            .success()
    );
    fs::create_dir(protected.join("proto")).unwrap();
    fs::write(protected.join("proto/greeting.proto"), "user contract").unwrap();
    assert!(
        !Command::new(cli)
            .current_dir(&protected)
            .args(["add", "grpc"])
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(
        fs::read_to_string(protected.join("proto/greeting.proto")).unwrap(),
        "user contract"
    );
    assert!(!protected.join("crates/domain").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn resource_generator_registers_routes_and_preserves_existing_files() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("kouga-resource-{}-{nonce}", std::process::id()));
    let app = root.join("taskboard");
    fs::create_dir(&root).unwrap();
    let cli = env!("CARGO_BIN_EXE_kouga");
    assert!(
        Command::new(cli)
            .args(["new", "taskboard", "--path"])
            .arg(&app)
            .status()
            .unwrap()
            .success()
    );
    let generate = |fields: &[&str]| {
        Command::new(cli)
            .current_dir(&app)
            .args(["generate", "resource", "Task"])
            .args(fields)
            .output()
            .unwrap()
    };
    assert!(
        Command::new(cli)
            .current_dir(&app)
            .args(["generate", "request", "Note", "body:string"])
            .status()
            .unwrap()
            .success()
    );
    assert!(app.join("src/requests/notes.rs").exists());
    assert!(!generate(&["id:string"]).status.success());
    assert!(!app.join("migrations").exists());
    let lib = app.join("src/lib.rs");
    let original = fs::read_to_string(&lib).unwrap();
    fs::write(&lib, format!("// edited\n{original}")).unwrap();
    assert!(!generate(&["title:string"]).status.success());
    assert!(!app.join("migrations").exists());
    assert!(fs::read_to_string(&lib).unwrap().starts_with("// edited"));
    fs::write(&lib, original).unwrap();
    assert!(
        generate(&["title:string", "completed:bool=false"])
            .status
            .success()
    );
    let controller = app.join("src/controllers/tasks.rs");
    let before = fs::read(&controller).unwrap();
    assert!(
        !generate(&["title:string", "completed:bool=false"])
            .status
            .success()
    );
    assert_eq!(fs::read(&controller).unwrap(), before);
    assert!(
        Command::new(cli)
            .current_dir(&app)
            .args(["generate", "resource", "Tag", "label:string"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        fs::read_to_string(app.join("src/lib.rs"))
            .unwrap()
            .contains("controllers::tasks::routes(router)")
    );
    assert!(
        fs::read_to_string(app.join("src/lib.rs"))
            .unwrap()
            .contains("controllers::tags::routes(router)")
    );
    assert_eq!(fs::read_dir(app.join("migrations")).unwrap().count(), 4);
    assert!(app.join("tests/tasks.rs").exists());
    assert!(
        Command::new(cli)
            .current_dir(&app)
            .args(["add", "grpc"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        !Command::new(cli)
            .current_dir(&app)
            .args(["add", "grpc"])
            .status()
            .unwrap()
            .success()
    );
    let augmented = fs::read_to_string(app.join("src/lib.rs")).unwrap();
    assert!(augmented.contains("controllers::tasks::routes(router)"));
    assert!(augmented.contains("greeting.show"));
    assert_eq!(fs::read(&controller).unwrap(), before);
    fs::remove_dir_all(root).unwrap();
}
