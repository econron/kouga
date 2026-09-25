use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use kouga_migration::{
    LOCK_KEY, MigrationError, MigrationSet, MigrationState, Migrator, MigratorOptions,
};

fn files() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "kouga-migration-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn write(path: &Path, version: &str, name: &str, sql: &str) {
    fs::write(path.join(format!("{version}_{name}.up.sql")), sql).unwrap();
}

#[test]
fn validates_files_and_checksums() {
    let path = files();
    write(&path, "20260925000100", "first", "SELECT 1;\n");
    let set = MigrationSet::load(&path).unwrap();
    assert_eq!(set.migrations().len(), 1);
    assert_eq!(
        set.migrations()[0].up_checksum,
        "b4e0497804e46e0a0b0b8c31975b062152d551bac49c3c2e80932567b4085dcd"
    );
    write(&path, "20260925000100", "second", "SELECT 2;");
    assert!(matches!(
        MigrationSet::load(&path),
        Err(MigrationError::InvalidFile(_))
    ));
    fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn postgres_migration_contract() {
    let Ok(url) = std::env::var("KOUGA_TEST_DATABASE_URL") else {
        return;
    };
    let db = kouga_db::connect(&url, 5, Duration::from_secs(5))
        .await
        .unwrap();
    sqlx::query("DROP TABLE IF EXISTS _kouga_migrations, kouga_t05_first, kouga_t05_second, kouga_t05_third CASCADE").execute(&db).await.unwrap();
    let path = files();
    write(
        &path,
        "20260925000100",
        "first",
        "CREATE TABLE kouga_t05_first (id integer PRIMARY KEY);",
    );
    write(
        &path,
        "20260925000200",
        "second",
        "CREATE TABLE kouga_t05_second (id integer PRIMARY KEY);",
    );
    let set = MigrationSet::load(&path).unwrap();
    let options = MigratorOptions::default();
    let m1 = Migrator::new(db.clone(), set.clone(), options);
    let m2 = Migrator::new(db.clone(), set, options);
    let (a, b) = tokio::join!(m1.migrate(), m2.migrate());
    assert_eq!(a.unwrap() + b.unwrap(), 2);
    assert_eq!(
        m1.status()
            .await
            .unwrap()
            .iter()
            .filter(|s| s.state == MigrationState::Applied)
            .count(),
        2
    );
    assert_eq!(m1.migrate().await.unwrap(), 0);

    write(
        &path,
        "20260925000300",
        "third",
        "CREATE TABLE kouga_t05_third (id integer PRIMARY KEY); SELECT 1/0;",
    );
    let broken = Migrator::new(db.clone(), MigrationSet::load(&path).unwrap(), options);
    assert!(matches!(
        broken.migrate().await,
        Err(MigrationError::Database(_))
    ));
    let exists: Option<String> = sqlx::query_scalar("SELECT to_regclass('kouga_t05_third')::text")
        .fetch_one(&db)
        .await
        .unwrap();
    assert!(exists.is_none());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM _kouga_migrations")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 2);
    write(
        &path,
        "20260925000300",
        "third",
        "CREATE TABLE kouga_t05_third (id integer PRIMARY KEY);",
    );
    let mut blocker = db.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(LOCK_KEY)
        .execute(&mut *blocker)
        .await
        .unwrap();
    let short = MigratorOptions {
        lock_timeout: Duration::from_millis(50),
        ..options
    };
    assert!(matches!(
        Migrator::new(db.clone(), MigrationSet::load(&path).unwrap(), short)
            .migrate()
            .await,
        Err(MigrationError::LockTimeout)
    ));
    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(LOCK_KEY)
        .execute(&mut *blocker)
        .await
        .unwrap();
    drop(blocker);
    assert_eq!(
        Migrator::new(db.clone(), MigrationSet::load(&path).unwrap(), options)
            .migrate()
            .await
            .unwrap(),
        1
    );

    write(&path, "20260925000100", "first", "SELECT 42;");
    assert!(matches!(
        Migrator::new(db.clone(), MigrationSet::load(&path).unwrap(), options)
            .migrate()
            .await,
        Err(MigrationError::HistoryMismatch(_))
    ));
    write(
        &path,
        "20260925000100",
        "first",
        "CREATE TABLE kouga_t05_first (id integer PRIMARY KEY);",
    );
    fs::remove_file(path.join("20260925000100_first.up.sql")).unwrap();
    assert!(matches!(
        Migrator::new(db.clone(), MigrationSet::load(&path).unwrap(), options)
            .migrate()
            .await,
        Err(MigrationError::HistoryMismatch(_))
    ));
    write(
        &path,
        "20260925000100",
        "first",
        "CREATE TABLE kouga_t05_first (id integer PRIMARY KEY);",
    );
    write(&path, "20260925000150", "late", "SELECT 1;");
    assert!(matches!(
        Migrator::new(db.clone(), MigrationSet::load(&path).unwrap(), options)
            .migrate()
            .await,
        Err(MigrationError::OutOfOrder(_))
    ));
    fs::remove_dir_all(path).unwrap();
    sqlx::query(
        "DROP TABLE _kouga_migrations, kouga_t05_first, kouga_t05_second, kouga_t05_third CASCADE",
    )
    .execute(&db)
    .await
    .unwrap();
}
