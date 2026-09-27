use std::time::Duration;

#[path = "../../../../src/storage.rs"]
mod storage;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = kouga_model::db::connect(&std::env::var("DATABASE_URL")?, 5, Duration::from_secs(5))
        .await?;
    let cleaned = storage::open(db)?
        .cleanup(Duration::from_secs(3600), 100)
        .await?;
    println!("cleaned {cleaned} attachments");
    Ok(())
}
