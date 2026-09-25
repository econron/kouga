use std::{fs, path::Path, time::Duration};

use kouga_migration::{
    MigrationError, MigrationSet, MigrationState, Migrator, MigratorOptions, RepairState,
};

fn write(path: &Path, version: &str, name: &str, direction: &str, sql: &str) {
    fs::write(path.join(format!("{version}_{name}.{direction}.sql")), sql).unwrap();
}

#[tokio::test]
async fn rollback_dirty_repair_and_nontransactional_sql() {
    let Ok(url) = std::env::var("KOUGA_TEST_DATABASE_URL") else {
        return;
    };
    let db = kouga_db::connect(&url, 4, Duration::from_secs(5))
        .await
        .unwrap();
    sqlx::query(
        "DROP TABLE IF EXISTS _kouga_migration_repairs, _kouga_migrations, kouga_t06_items CASCADE",
    )
    .execute(&db)
    .await
    .unwrap();
    let path = std::env::temp_dir().join(format!("kouga-t06-{}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    write(
        &path,
        "20260925000100",
        "items",
        "up",
        "CREATE TABLE kouga_t06_items (id integer PRIMARY KEY, code integer NOT NULL); INSERT INTO kouga_t06_items VALUES (1, 1), (2, 1);",
    );
    write(
        &path,
        "20260925000100",
        "items",
        "down",
        "DROP TABLE kouga_t06_items;",
    );
    write(
        &path,
        "20260925000200",
        "unique_code",
        "up",
        "-- kouga: transaction=false\nCREATE UNIQUE INDEX CONCURRENTLY kouga_t06_code_idx ON kouga_t06_items (code);",
    );
    write(
        &path,
        "20260925000200",
        "unique_code",
        "down",
        "-- kouga: transaction=false\nDROP INDEX CONCURRENTLY kouga_t06_code_idx;",
    );
    let set = MigrationSet::load(&path).unwrap();
    let migrator = Migrator::new(db.clone(), set, MigratorOptions::default());
    assert!(matches!(
        migrator.migrate().await,
        Err(MigrationError::Database(_))
    ));
    assert_eq!(
        migrator.status().await.unwrap()[1].state,
        MigrationState::Dirty
    );
    assert!(matches!(
        migrator.migrate().await,
        Err(MigrationError::Dirty(_))
    ));
    assert!(matches!(
        migrator.rollback(1).await,
        Err(MigrationError::Dirty(_))
    ));
    assert!(matches!(
        migrator
            .repair("20260925000100", RepairState::Pending, "wrong")
            .await,
        Err(MigrationError::Dirty(_))
    ));
    sqlx::query("DROP INDEX IF EXISTS kouga_t06_code_idx")
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM kouga_t06_items WHERE id = 2")
        .execute(&db)
        .await
        .unwrap();
    migrator
        .repair(
            "20260925000200",
            RepairState::Pending,
            "removed invalid index and duplicate row",
        )
        .await
        .unwrap();
    assert_eq!(migrator.migrate().await.unwrap(), 1);
    sqlx::query("UPDATE _kouga_migrations SET state = 'dirty' WHERE version = '20260925000200'")
        .execute(&db)
        .await
        .unwrap();
    migrator
        .repair("20260925000200", RepairState::Applied, "index verified")
        .await
        .unwrap();
    let repairs: i64 = sqlx::query_scalar("SELECT count(*) FROM _kouga_migration_repairs")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(repairs, 2);
    assert_eq!(migrator.rollback(1).await.unwrap(), 1);
    let index: Option<String> =
        sqlx::query_scalar("SELECT to_regclass('kouga_t06_code_idx')::text")
            .fetch_one(&db)
            .await
            .unwrap();
    assert!(index.is_none());
    fs::remove_file(path.join("20260925000200_unique_code.up.sql")).unwrap();
    fs::remove_file(path.join("20260925000200_unique_code.down.sql")).unwrap();
    write(&path, "20260925000300", "irreversible", "up", "SELECT 1;");
    let set = MigrationSet::load(&path).unwrap();
    let migrator = Migrator::new(db.clone(), set, MigratorOptions::default());
    assert_eq!(migrator.migrate().await.unwrap(), 1);
    assert!(matches!(
        migrator.rollback(2).await,
        Err(MigrationError::Irreversible(_))
    ));
    assert_eq!(
        migrator
            .status()
            .await
            .unwrap()
            .iter()
            .filter(|s| s.state == MigrationState::Applied)
            .count(),
        2
    );
    fs::remove_dir_all(path).unwrap();
    sqlx::query("DROP TABLE _kouga_migration_repairs, _kouga_migrations, kouga_t06_items CASCADE")
        .execute(&db)
        .await
        .unwrap();
}
