//! Run with BOARD_ACTOR_ID and BOARD_TASK_ID in a trusted one-shot task environment.
use kouga_core::Patch;
use kouga_model::Uuid;
use taskboard::board::Board;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = kouga_model::db::connect(
        &std::env::var("DATABASE_URL")?,
        2,
        std::time::Duration::from_secs(5),
    )
    .await?;
    let actor = Uuid::parse_str(&std::env::var("BOARD_ACTOR_ID")?)?;
    let task = Uuid::parse_str(&std::env::var("BOARD_TASK_ID")?)?;
    Board::new(db.clone())
        .update_task(actor, task, Patch::Missing, Patch::Value(true))
        .await?;
    db.close().await;
    Ok(())
}
