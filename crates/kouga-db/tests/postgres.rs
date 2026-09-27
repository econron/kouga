//! Set KOUGA_TEST_DATABASE_URL to run against a disposable PostgreSQL database.

use kouga_db::{
    Acquire, ConstraintKind, DbError, DbErrorKind, IsolationLevel, Postgres, begin_with_isolation,
    commit_transaction, connect,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[tokio::test]
async fn pool_one_connection_admits_one_and_times_out_the_next() {
    let Ok(url) = std::env::var("KOUGA_TEST_DATABASE_URL") else {
        return;
    };
    let db = connect(&url, 1, Duration::from_secs(1)).await.unwrap();
    let held = db.acquire().await.unwrap();
    let second = db.acquire().await;
    assert!(matches!(second, Err(sqlx::Error::PoolTimedOut)));
    drop(held);
    let recovered = db.acquire().await.unwrap();
    drop(recovered);
}

// Only this test's generated table name (ASCII digits and underscores) is interpolated.
fn safe_test_sql(sql: String) -> sqlx::AssertSqlSafe<String> {
    sqlx::AssertSqlSafe(sql)
}

async fn count_on_connection<'c, A>(db: A, table: &str) -> Result<i64, sqlx::Error>
where
    A: Acquire<'c, Database = Postgres> + Send,
{
    let mut conn = db.acquire().await?;
    sqlx::query_scalar(safe_test_sql(format!("SELECT count(*) FROM {table}")))
        .fetch_one(&mut *conn)
        .await
}

#[tokio::test]
async fn postgres_transactions_and_errors() {
    let Ok(url) = std::env::var("KOUGA_TEST_DATABASE_URL") else {
        eprintln!("skipping PostgreSQL integration test: KOUGA_TEST_DATABASE_URL is unset");
        return;
    };
    let db = connect(&url, 2, Duration::from_millis(200)).await.unwrap();
    let table = format!(
        "kouga_t04_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    sqlx::query(safe_test_sql(format!(
        "CREATE TABLE {table} (id integer PRIMARY KEY, value integer NOT NULL UNIQUE CHECK (value > 0), parent_id integer REFERENCES {table}(id))"
    )))
    .execute(&db)
    .await
    .unwrap();

    let mut tx = db.begin().await.unwrap();
    sqlx::query(safe_test_sql(format!(
        "INSERT INTO {table} (id, value) VALUES (1, 1)"
    )))
    .execute(&mut *tx)
    .await
    .unwrap();
    assert_eq!(count_on_connection(&mut tx, &table).await.unwrap(), 1);
    assert_eq!(count_on_connection(&db, &table).await.unwrap(), 0);
    tx.rollback().await.unwrap();
    let count: i64 = sqlx::query_scalar(safe_test_sql(format!("SELECT count(*) FROM {table}")))
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 0);

    let mut tx = begin_with_isolation(&db, IsolationLevel::Serializable)
        .await
        .unwrap();
    let level: String = sqlx::query_scalar("SHOW transaction_isolation")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(level, "serializable");
    sqlx::query(safe_test_sql(format!(
        "INSERT INTO {table} (id, value) VALUES (1, 1)"
    )))
    .execute(&mut *tx)
    .await
    .unwrap();
    commit_transaction(tx).await.unwrap();

    let unique = sqlx::query(safe_test_sql(format!(
        "INSERT INTO {table} (id, value) VALUES (2, 1)"
    )))
    .execute(&db)
    .await
    .unwrap_err();
    let unique = DbError::from(unique);
    assert_eq!(unique.kind, DbErrorKind::Constraint(ConstraintKind::Unique));
    assert!(unique.constraint_name().is_some());
    let fk = sqlx::query(safe_test_sql(format!(
        "INSERT INTO {table} (id, value, parent_id) VALUES (2, 2, 99)"
    )))
    .execute(&db)
    .await
    .unwrap_err();
    assert_eq!(
        DbError::from(fk).kind,
        DbErrorKind::Constraint(ConstraintKind::ForeignKey)
    );
    let check = sqlx::query(safe_test_sql(format!(
        "INSERT INTO {table} (id, value) VALUES (2, 0)"
    )))
    .execute(&db)
    .await
    .unwrap_err();
    assert_eq!(
        DbError::from(check).kind,
        DbErrorKind::Constraint(ConstraintKind::Check)
    );

    let mut locked = db.begin().await.unwrap();
    sqlx::query(safe_test_sql(format!(
        "SELECT id FROM {table} WHERE id = 1 FOR UPDATE"
    )))
    .execute(&mut *locked)
    .await
    .unwrap();
    let blocked = tokio::time::timeout(
        Duration::from_millis(100),
        sqlx::query(safe_test_sql(format!(
            "SELECT id FROM {table} WHERE id = 1 FOR UPDATE"
        )))
        .execute(&db),
    )
    .await;
    assert!(blocked.is_err(), "row lock must block a second connection");
    locked.rollback().await.unwrap();

    let held = db.acquire().await.unwrap();
    let other = db.acquire().await.unwrap();
    let timeout = db.acquire().await.unwrap_err();
    assert_eq!(DbError::from(timeout).kind, DbErrorKind::PoolTimeout);
    drop(held);
    drop(other);

    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let db_for_task = db.clone();
    let insert_sql = format!("INSERT INTO {table} (id, value) VALUES (2, 2)");
    let task = tokio::spawn(async move {
        let mut tx = db_for_task.begin().await.unwrap();
        sqlx::query(safe_test_sql(insert_sql))
            .execute(&mut *tx)
            .await
            .unwrap();
        ready_tx.send(()).unwrap();
        tokio::time::sleep(Duration::from_secs(60)).await;
        commit_transaction(tx).await.unwrap();
    });
    ready_rx.await.unwrap();
    task.abort();
    task.await.unwrap_err();
    let count: i64 = sqlx::query_scalar(safe_test_sql(format!("SELECT count(*) FROM {table}")))
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 1, "cancelled uncommitted transaction must roll back");

    sqlx::query(safe_test_sql(format!(
        "INSERT INTO {table} (id, value) VALUES (2, 2)"
    )))
    .execute(&db)
    .await
    .unwrap();

    let mut first = begin_with_isolation(&db, IsolationLevel::Serializable)
        .await
        .unwrap();
    let mut second = begin_with_isolation(&db, IsolationLevel::Serializable)
        .await
        .unwrap();
    let _: i32 = sqlx::query_scalar(safe_test_sql(format!(
        "SELECT value FROM {table} WHERE id = 1"
    )))
    .fetch_one(&mut *first)
    .await
    .unwrap();
    let _: i32 = sqlx::query_scalar(safe_test_sql(format!(
        "SELECT value FROM {table} WHERE id = 1"
    )))
    .fetch_one(&mut *second)
    .await
    .unwrap();
    sqlx::query(safe_test_sql(format!(
        "UPDATE {table} SET value = 3 WHERE id = 1"
    )))
    .execute(&mut *first)
    .await
    .unwrap();
    commit_transaction(first).await.unwrap();
    let serialization = sqlx::query(safe_test_sql(format!(
        "UPDATE {table} SET value = 4 WHERE id = 1"
    )))
    .execute(&mut *second)
    .await
    .unwrap_err();
    assert_eq!(
        DbError::from(serialization).kind,
        DbErrorKind::Serialization
    );
    second.rollback().await.unwrap();

    let mut first = db.begin().await.unwrap();
    let mut second = db.begin().await.unwrap();
    sqlx::query(safe_test_sql(format!(
        "SELECT id FROM {table} WHERE id = 1 FOR UPDATE"
    )))
    .execute(&mut *first)
    .await
    .unwrap();
    sqlx::query(safe_test_sql(format!(
        "SELECT id FROM {table} WHERE id = 2 FOR UPDATE"
    )))
    .execute(&mut *second)
    .await
    .unwrap();
    let (a, b) = tokio::join!(
        sqlx::query(safe_test_sql(format!(
            "SELECT id FROM {table} WHERE id = 2 FOR UPDATE"
        )))
        .execute(&mut *first),
        sqlx::query(safe_test_sql(format!(
            "SELECT id FROM {table} WHERE id = 1 FOR UPDATE"
        )))
        .execute(&mut *second),
    );
    assert!(
        a.err()
            .into_iter()
            .chain(b.err())
            .any(|error| DbError::from(error).kind == DbErrorKind::Deadlock),
        "one transaction must report a deadlock"
    );
    first.rollback().await.unwrap();
    second.rollback().await.unwrap();

    sqlx::query(safe_test_sql(format!("DROP TABLE {table}")))
        .execute(&db)
        .await
        .unwrap();
}
