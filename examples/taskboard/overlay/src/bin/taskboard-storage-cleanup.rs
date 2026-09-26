use std::time::Duration;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = kouga_model::db::connect(&std::env::var("DATABASE_URL")?, 5, Duration::from_secs(5))
        .await?;
    let cleaned = taskboard::attachments::cleanup_once(db).await?;
    println!("cleaned {cleaned} attachments");
    Ok(())
}
