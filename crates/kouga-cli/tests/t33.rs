use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn user_journey_generation_keeps_worker_tests_and_routes_lint_clean() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("kouga-t33-{}-{nonce}", std::process::id()));
    let app = root.join("taskboard");
    fs::create_dir(&root).unwrap();
    let cli = env!("CARGO_BIN_EXE_kouga");
    let created = Command::new(cli)
        .args(["new", "taskboard", "--path"])
        .arg(&app)
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let invoke = |args: &[&str]| {
        let result = Command::new(cli)
            .current_dir(&app)
            .args(args)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    };
    invoke(&[
        "generate",
        "resource",
        "Task",
        "title:string",
        "completed:bool=false",
    ]);
    invoke(&["generate", "auth"]);
    invoke(&["add", "grpc"]);
    let controller = fs::read_to_string(app.join("src/controllers/tasks.rs")).unwrap();
    assert!(controller.contains("completed: input.completed,"));
    let lib = fs::read_to_string(app.join("src/lib.rs")).unwrap();
    assert!(lib.contains("greeting.show"));
    assert!(!lib.contains("let router = router"));
    let worker = fs::read_to_string(app.join("apps/worker/src/bin/auth-mail-worker.rs")).unwrap();
    assert!(worker.contains("/../../migrations"));
    assert!(worker.contains("let options = WorkerOptions { queues:"));
    assert!(worker.find("async fn shutdown()").unwrap() < worker.find("#[cfg(test)]").unwrap());
    let later = root.join("later");
    let created = Command::new(cli)
        .args(["new", "later", "--path"])
        .arg(&later)
        .output()
        .unwrap();
    assert!(created.status.success());
    for args in [
        vec!["add", "grpc"],
        vec!["generate", "resource", "Note", "title:string"],
    ] {
        let result = Command::new(cli)
            .current_dir(&later)
            .args(args)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let lib = fs::read_to_string(later.join("src/lib.rs")).unwrap();
    assert!(lib.contains("greeting.show"));
    assert!(lib.contains("controllers::notes::routes(router)"));
    assert!(!lib.contains("let router = router"));
    fs::remove_dir_all(root).unwrap();
}
