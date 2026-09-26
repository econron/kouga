use kouga_model::{Uuid, sqlx};
use kouga_test::TestDb;

#[tokio::test]
async fn create_task_and_notification_commit_or_rollback_together() {
    if std::env::var("TEST_DATABASE_URL").is_err() {
        return;
    }
    let isolated = TestDb::from_env("migrations").await.unwrap();
    let db = isolated.db();
    let owner = Uuid::new_v4();
    let project = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users(id,email,password_hash) VALUES($1,'queue@example.test','unused')",
    )
    .bind(owner)
    .execute(db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO projects(id,owner_id,slug,name) VALUES($1,$2,'queue','Queue')")
        .bind(project)
        .bind(owner)
        .execute(db)
        .await
        .unwrap();
    let board = taskboard::board::Board::new(db.clone());
    let task = board.create_task(owner, project, "queued").await.unwrap();
    let (job_task, version): (Uuid, i32) = sqlx::query_as(
        "SELECT (payload->>'task_id')::uuid, version FROM kouga_jobs WHERE name='taskboard.task_created'"
    ).fetch_one(db).await.unwrap();
    assert_eq!(job_task, task.id);
    assert_eq!(version, 2);
    // A failed enqueue rolls back the task insert as well.
    sqlx::query("ALTER TABLE kouga_jobs ADD CONSTRAINT reject_task_notice CHECK (name <> 'taskboard.task_created') NOT VALID")
        .execute(db).await.unwrap();
    assert!(
        board
            .create_task(owner, project, "rolled-back")
            .await
            .is_err()
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks WHERE title='rolled-back'")
        .fetch_one(db)
        .await
        .unwrap();
    assert_eq!(count, 0);
    isolated.close().await.unwrap();
}
