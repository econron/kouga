use std::{fs, process::Command, time::SystemTime};

use kouga_migration::{AdminTarget, Environment, create_database};
use sqlx::{Connection, Executor};

fn cli(app: &std::path::Path, url: &str, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_kouga"))
        .current_dir(app)
        .env("DATABASE_URL", url)
        .env("CARGO_NET_OFFLINE", "true")
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn seed_requires_an_explicit_registered_task_and_propagates_failure() {
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let app = std::env::temp_dir().join(format!("kouga-t39-seed-{}-{nonce}", std::process::id()));
    fs::create_dir_all(app.join("src/bin")).unwrap();
    fs::write(
        app.join("Cargo.toml"),
        "[package]\nname = \"t39-seed-test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[package.metadata.kouga]\napi = \"http\"\n",
    )
    .unwrap();
    let absent = cli(&app, "unused", &["db", "seed"]);
    assert!(!absent.status.success());
    fs::write(
        app.join("src/bin/task-seed.rs"),
        "fn main() { println!(\"seed-ran\"); }",
    )
    .unwrap();
    let seeded = cli(&app, "unused", &["db", "seed"]);
    assert!(
        seeded.status.success(),
        "{}",
        String::from_utf8_lossy(&seeded.stderr)
    );
    assert!(String::from_utf8_lossy(&seeded.stdout).contains("seed-ran"));
    fs::write(
        app.join("src/bin/task-seed.rs"),
        "fn main() { std::process::exit(7); }",
    )
    .unwrap();
    assert!(!cli(&app, "unused", &["db", "seed"]).status.success());
    fs::remove_dir_all(app).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn real_database_cli_lifecycle_and_guards() {
    let Ok(base_url) = std::env::var("KOUGA_TEST_DATABASE_URL") else {
        return;
    };
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let name = format!("kouga_t39_{}_{}", std::process::id(), nonce % 1_000_000_000);
    let url = format!("{}/{name}", base_url.rsplit_once('/').unwrap().0);
    let root = std::env::temp_dir().join(&name);
    let app = root.join("app");
    fs::create_dir(&root).unwrap();
    let new = Command::new(env!("CARGO_BIN_EXE_kouga"))
        .args(["new", "t39-app", "--path"])
        .arg(&app)
        .output()
        .unwrap();
    assert!(
        new.status.success(),
        "{}",
        String::from_utf8_lossy(&new.stderr)
    );
    create_database(&AdminTarget {
        url: &url,
        expected_database: &name,
        environment: Environment::Test,
        allow_destructive: false,
        allow_production: false,
    })
    .await
    .unwrap();
    let migrations = app.join("migrations");
    fs::create_dir(&migrations).unwrap();
    let first = migrations.join("20260925000100_create_items.up.sql");
    fs::write(&first, "CREATE TABLE items (id integer PRIMARY KEY);").unwrap();
    fs::write(
        migrations.join("20260925000100_create_items.down.sql"),
        "DROP TABLE items;",
    )
    .unwrap();
    fs::write(
        migrations.join("20260925000200_insert_item.up.sql"),
        "INSERT INTO items(id) VALUES (1);",
    )
    .unwrap();
    fs::write(
        migrations.join("20260925000200_insert_item.down.sql"),
        "DELETE FROM items WHERE id=1;",
    )
    .unwrap();

    let pending = cli(&app, &url, &["db", "status"]);
    assert!(pending.status.success());
    assert!(String::from_utf8_lossy(&pending.stdout).contains("pending"));
    let mut one = Command::new(env!("CARGO_BIN_EXE_kouga"))
        .current_dir(&app)
        .env("DATABASE_URL", &url)
        .args(["db", "migrate"])
        .spawn()
        .unwrap();
    let mut two = Command::new(env!("CARGO_BIN_EXE_kouga"))
        .current_dir(&app)
        .env("DATABASE_URL", &url)
        .args(["db", "migrate"])
        .spawn()
        .unwrap();
    assert!(one.wait().unwrap().success());
    assert!(two.wait().unwrap().success());
    let mut conn = sqlx::PgConnection::connect(&url).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM items")
        .fetch_one(&mut conn)
        .await
        .unwrap();
    assert_eq!(count, 1);

    let schema_file = app.join("db/schema.sql");
    let schema = cli(&app, &url, &["db", "schema"]);
    if Command::new("pg_dump").arg("--version").output().is_ok() {
        assert!(
            schema.status.success(),
            "{}",
            String::from_utf8_lossy(&schema.stderr)
        );
        let sql = fs::read_to_string(&schema_file).unwrap();
        assert!(sql.contains("CREATE TABLE public.items"));
        assert!(!sql.contains("INSERT INTO"));
        assert!(!sql.contains("OWNER TO"));
    } else {
        assert!(!schema.status.success());
        assert!(!schema_file.exists());
    }
    // CI can set this to a disposable PostgreSQL container when pg_dump is not installed
    // on the host. The wrapper preserves the CLI's pg_dump arguments and writes its output
    // to the host path, while credentials stay in environment variables.
    if let Ok(container) = std::env::var("KOUGA_TEST_PG_DUMP_DOCKER") {
        let bin = root.join("bin");
        fs::create_dir(&bin).unwrap();
        let wrapper = bin.join("pg_dump");
        fs::write(
            &wrapper,
            "#!/bin/sh\nout=\nprevious=\nfor arg in \"$@\"; do\n  if [ \"$previous\" = --file ]; then out=$arg; fi\n  previous=$arg\ndone\ntest -n \"$out\" || exit 2\ndocker exec -e PGDATABASE -e PGUSER -e PGPASSWORD -e PGHOST=127.0.0.1 -e PGPORT=5432 \"$KOUGA_TEST_PG_DUMP_DOCKER\" pg_dump --schema-only --no-owner --no-privileges > \"$out\"\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let path = std::env::join_paths(
            std::iter::once(bin.clone())
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        let schema = Command::new(env!("CARGO_BIN_EXE_kouga"))
            .current_dir(&app)
            .env("DATABASE_URL", &url)
            .env("KOUGA_TEST_PG_DUMP_DOCKER", container)
            .env("PATH", path)
            .args(["db", "schema"])
            .output()
            .unwrap();
        assert!(
            schema.status.success(),
            "{}",
            String::from_utf8_lossy(&schema.stderr)
        );
        let sql = fs::read_to_string(&schema_file).unwrap();
        assert!(sql.contains("CREATE TABLE public.items"));
        assert!(!sql.contains("INSERT INTO"));
        assert!(!sql.contains("OWNER TO"));
        assert!(!sql.contains("GRANT "));
    }

    let rolled = cli(&app, &url, &["db", "rollback"]);
    assert!(
        rolled.status.success(),
        "{}",
        String::from_utf8_lossy(&rolled.stderr)
    );
    assert!(String::from_utf8_lossy(&rolled.stdout).contains("Rolled back 1"));
    fs::write(
        &first,
        "CREATE TABLE items (id integer PRIMARY KEY, changed text);",
    )
    .unwrap();
    let changed = cli(&app, &url, &["db", "status"]);
    assert!(!changed.status.success());
    assert!(String::from_utf8_lossy(&changed.stderr).contains("changed migration"));
    assert!(!String::from_utf8_lossy(&changed.stderr).contains(&base_url));
    fs::write(&first, "CREATE TABLE items (id integer PRIMARY KEY);").unwrap();

    let failed_up = migrations.join("20260925000300_failing.up.sql");
    fs::write(&failed_up, "INSERT INTO missing_table(id) VALUES (1);").unwrap();
    assert!(!cli(&app, &url, &["db", "migrate"]).status.success());
    let status = cli(&app, &url, &["db", "status"]);
    assert!(status.status.success());
    assert!(!String::from_utf8_lossy(&status.stdout).contains("dirty"));
    fs::remove_file(failed_up).unwrap();

    let irreversible = migrations.join("20260925000300_irreversible.up.sql");
    let empty_down = migrations.join("20260925000300_irreversible.down.sql");
    fs::write(&irreversible, "SELECT 1;").unwrap();
    fs::write(&empty_down, "-- cannot reverse this\n").unwrap();
    assert!(cli(&app, &url, &["db", "migrate"]).status.success());
    let status = cli(&app, &url, &["db", "status"]);
    assert!(
        String::from_utf8_lossy(&status.stdout)
            .contains("20260925000300\tapplied\tirreversible\tirreversible")
    );
    assert!(
        !cli(&app, &url, &["db", "rollback", "--steps", "2"])
            .status
            .success()
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM items")
        .fetch_one(&mut conn)
        .await
        .unwrap();
    assert_eq!(count, 1);
    fs::remove_file(&irreversible).unwrap();
    fs::remove_file(&empty_down).unwrap();

    assert!(!cli(&app, &url, &["db", "seed"]).status.success());
    assert!(
        !cli(
            &app,
            &url,
            &["db", "reset", "--database", &name, "--environment", "test"]
        )
        .status
        .success()
    );
    assert!(
        !cli(
            &app,
            &url,
            &[
                "db",
                "reset",
                "--database",
                "wrong",
                "--environment",
                "test",
                "--allow-destructive"
            ]
        )
        .status
        .success()
    );
    assert!(
        !cli(
            &app,
            &url,
            &[
                "db",
                "reset",
                "--database",
                &name,
                "--environment",
                "production",
                "--allow-destructive"
            ]
        )
        .status
        .success()
    );
    let mismatch = Command::new(env!("CARGO_BIN_EXE_kouga"))
        .current_dir(&app)
        .env("DATABASE_URL", &url)
        .env("KOUGA_ENV", "production")
        .args([
            "db",
            "reset",
            "--database",
            &name,
            "--environment",
            "test",
            "--allow-destructive",
        ])
        .output()
        .unwrap();
    assert!(!mismatch.status.success());
    conn.close().await.unwrap();
    let reset = cli(
        &app,
        &url,
        &[
            "db",
            "reset",
            "--database",
            &name,
            "--environment",
            "test",
            "--allow-destructive",
        ],
    );
    assert!(
        reset.status.success(),
        "{}",
        String::from_utf8_lossy(&reset.stderr)
    );
    let mut conn = sqlx::PgConnection::connect(&url).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM items")
        .fetch_one(&mut conn)
        .await
        .unwrap();
    assert_eq!(count, 1);

    let dirty = migrations.join("20260925000300_dirty.up.sql");
    fs::write(
        &dirty,
        "-- kouga: transaction=false\nCREATE TABLE partial_items(id integer); SELECT 1/0;",
    )
    .unwrap();
    assert!(!cli(&app, &url, &["db", "migrate"]).status.success());
    let status = cli(&app, &url, &["db", "status"]);
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).contains("dirty"));
    assert!(!cli(&app, &url, &["db", "migrate"]).status.success());
    fs::write(&dirty, "-- kouga: transaction=false\nSELECT 2/0;").unwrap();
    assert!(
        !cli(
            &app,
            &url,
            &[
                "db",
                "repair",
                "--version",
                "20260925000300",
                "--state",
                "pending",
                "--reason",
                "not yet repaired",
            ]
        )
        .status
        .success()
    );
    fs::write(
        &dirty,
        "-- kouga: transaction=false\nCREATE TABLE partial_items(id integer); SELECT 1/0;",
    )
    .unwrap();
    assert!(
        !cli(
            &app,
            &url,
            &[
                "db",
                "repair",
                "--version",
                "20260925000100",
                "--state",
                "applied",
                "--reason",
                "not dirty"
            ]
        )
        .status
        .success()
    );
    conn.execute("DROP TABLE IF EXISTS partial_items")
        .await
        .unwrap();
    let repaired = cli(
        &app,
        &url,
        &[
            "db",
            "repair",
            "--version",
            "20260925000300",
            "--state",
            "pending",
            "--reason",
            "partial table removed",
        ],
    );
    assert!(
        repaired.status.success(),
        "{}",
        String::from_utf8_lossy(&repaired.stderr)
    );
    assert!(
        String::from_utf8_lossy(&cli(&app, &url, &["db", "status"]).stdout).contains("pending")
    );

    conn.close().await.unwrap();
    let mut maintenance = sqlx::PgConnection::connect(&base_url).await.unwrap();
    let drop_sql = format!("DROP DATABASE \"{name}\" WITH (FORCE)");
    maintenance
        .execute(sqlx::AssertSqlSafe(drop_sql))
        .await
        .unwrap();
    fs::remove_dir_all(root).unwrap();
}
