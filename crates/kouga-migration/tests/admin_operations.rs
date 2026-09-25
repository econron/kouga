use std::{fs, process::Command, time::Duration};

use kouga_migration::{
    AdminTarget, Environment, MigrationSet, MigratorOptions, create_database, dump_schema,
    generate_migration, reset_database, run_seed,
};
use sqlx::{Connection, Executor};

#[test]
fn generation_rejects_names_and_does_not_overwrite() {
    let path = std::env::temp_dir().join(format!("kouga-t06-generator-{}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    assert!(generate_migration(&path, "bad-name").is_err());
    let created = generate_migration(&path, "create_tasks").unwrap();
    assert!(
        created
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .ends_with("_create_tasks.up.sql")
    );
    assert!(generate_migration(&path, "create_tasks").is_err());
    fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn create_reset_seed_and_schema_dump() {
    let Ok(base_url) = std::env::var("KOUGA_TEST_DATABASE_URL") else {
        return;
    };
    let prefix = base_url.rsplit_once('/').unwrap().0;
    let name = format!("kouga_t06_admin_{}", std::process::id());
    let url = format!("{prefix}/{name}");
    let mut target = AdminTarget {
        url: &url,
        expected_database: &name,
        environment: Environment::Test,
        allow_destructive: false,
        allow_production: false,
    };
    create_database(&target).await.unwrap();
    let path =
        std::env::temp_dir().join(format!("kouga-t06-admin-migrations-{}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    fs::write(
        path.join("20260925000100_items.up.sql"),
        "CREATE TABLE kouga_t06_reset_items (id integer PRIMARY KEY);",
    )
    .unwrap();
    let set = MigrationSet::load(&path).unwrap();
    let db = kouga_db::connect(&url, 2, Duration::from_secs(5))
        .await
        .unwrap();
    assert!(
        reset_database(&target, set.clone(), MigratorOptions::default())
            .await
            .is_err()
    );
    target.allow_destructive = true;
    target.environment = Environment::Production;
    assert!(
        reset_database(&target, set.clone(), MigratorOptions::default())
            .await
            .is_err()
    );
    target.environment = Environment::Test;
    db.close().await;
    assert_eq!(
        reset_database(&target, set.clone(), MigratorOptions::default())
            .await
            .unwrap(),
        1
    );
    let db = kouga_db::connect(&url, 2, Duration::from_secs(5))
        .await
        .unwrap();
    run_seed(db.clone(), |db| async move {
        sqlx::query("INSERT INTO kouga_t06_reset_items (id) VALUES (1)")
            .execute(&db)
            .await
            .map(|_| ())
    })
    .await
    .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM kouga_t06_reset_items")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let mut old = sqlx::PgConnection::connect(&url).await.unwrap();
    db.close().await;
    assert_eq!(
        reset_database(&target, set, MigratorOptions::default())
            .await
            .unwrap(),
        1
    );
    assert!(old.execute("SELECT 1").await.is_err());
    let db = kouga_db::connect(&url, 2, Duration::from_secs(5))
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM kouga_t06_reset_items")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 0);

    if Command::new("pg_dump").arg("--version").output().is_ok() {
        let output = path.join("schema.sql");
        dump_schema(&url, &output).unwrap();
        let sql = fs::read_to_string(output).unwrap();
        assert!(sql.contains("kouga_t06_reset_items"));
        assert!(!sql.contains("INSERT INTO"));
        assert!(!sql.contains("OWNER TO"));
        assert!(!sql.contains("GRANT "));
    }
    db.close().await;
    let mut maintenance = sqlx::PgConnection::connect(&base_url).await.unwrap();
    maintenance
        .execute(sqlx::AssertSqlSafe(format!(
            "DROP DATABASE \"{name}\" WITH (FORCE)"
        )))
        .await
        .unwrap();
    fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn failed_reset_restores_database_connections() {
    let Ok(base_url) = std::env::var("KOUGA_TEST_DATABASE_URL") else {
        return;
    };
    let mut maintenance = sqlx::PgConnection::connect(&base_url).await.unwrap();
    let max_prepared: String = sqlx::query_scalar("SHOW max_prepared_transactions")
        .fetch_one(&mut maintenance)
        .await
        .unwrap();
    if max_prepared == "0" {
        return;
    }
    let name = format!("kouga_t06_restore_{}", std::process::id());
    let url = format!("{}/{}", base_url.rsplit_once('/').unwrap().0, name);
    let target = AdminTarget {
        url: &url,
        expected_database: &name,
        environment: Environment::Test,
        allow_destructive: true,
        allow_production: false,
    };
    create_database(&target).await.unwrap();
    let mut conn = sqlx::PgConnection::connect(&url).await.unwrap();
    let gid = format!("kouga_t06_{}", std::process::id());
    conn.execute("BEGIN").await.unwrap();
    conn.execute(sqlx::AssertSqlSafe(format!("PREPARE TRANSACTION '{gid}'")))
        .await
        .unwrap();
    conn.close().await.unwrap();
    let path = std::env::temp_dir().join(format!("kouga-t06-restore-{}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("20260925000100_once.up.sql"), "SELECT 1;").unwrap();
    let set = MigrationSet::load(&path).unwrap();
    assert!(
        reset_database(&target, set, MigratorOptions::default())
            .await
            .is_err()
    );
    let allowed: bool = sqlx::query_scalar("SELECT datallowconn FROM pg_database WHERE datname=$1")
        .bind(&name)
        .fetch_one(&mut maintenance)
        .await
        .unwrap();
    assert!(allowed);
    let mut conn = sqlx::PgConnection::connect(&url).await.unwrap();
    conn.execute(sqlx::AssertSqlSafe(format!("ROLLBACK PREPARED '{gid}'")))
        .await
        .unwrap();
    conn.close().await.unwrap();
    maintenance
        .execute(sqlx::AssertSqlSafe(format!(
            "DROP DATABASE \"{name}\" WITH (FORCE)"
        )))
        .await
        .unwrap();
    fs::remove_dir_all(path).unwrap();
}
