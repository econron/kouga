//! Idempotent development data. Set BOARD_SEED_PASSWORD; never seed production.
use kouga_model::{Uuid, sqlx};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("KOUGA_ENV").as_deref() == Ok("production") {
        return Err("taskboard seed is disabled in production".into());
    }
    let password = std::env::var("BOARD_SEED_PASSWORD")?;
    if password.len() < 12 {
        return Err("BOARD_SEED_PASSWORD must have at least 12 characters".into());
    }
    let db = kouga_model::db::connect(&std::env::var("DATABASE_URL")?, 2, Duration::from_secs(5))
        .await?;
    let user = Uuid::parse_str("11111111-1111-4111-8111-111111111111")?;
    let project = Uuid::parse_str("22222222-2222-4222-8222-222222222222")?;
    let task = Uuid::parse_str("33333333-3333-4333-8333-333333333333")?;
    let hash = kouga_auth::hash_password(&password)
        .map_err(|_| std::io::Error::other("seed password hashing failed"))?;
    let mut tx = db.begin().await?;
    sqlx::query(
        "INSERT INTO users(id,email,password_hash) VALUES($1,$2,$3) ON CONFLICT(id) DO NOTHING",
    )
    .bind(user)
    .bind("seed@example.invalid")
    .bind(hash)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO projects(id,owner_id,slug,name) VALUES($1,$2,$3,$4) ON CONFLICT(id) DO NOTHING")
        .bind(project).bind(user).bind("first-project").bind("First project").execute(&mut *tx).await?;
    sqlx::query("INSERT INTO tasks(id,project_id,owner_id,title) VALUES($1,$2,$3,$4) ON CONFLICT(id) DO NOTHING")
        .bind(task).bind(project).bind(user).bind("First task").execute(&mut *tx).await?;
    tx.commit().await?;
    println!("Seeded Taskboard demo records");
    Ok(())
}
