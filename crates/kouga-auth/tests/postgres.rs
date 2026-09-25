use kouga_auth::{
    CurrentUser, authenticate, issue_token, owned_by, revoke_token, revoke_user_tokens,
};
use kouga_model::{Model, Uuid};
use kouga_test::TestDb;
use std::time::Duration;

#[derive(Debug, Model)]
#[model(table = "auth_test_items")]
struct Item {
    id: Uuid,
    owner_id: Uuid,
    name: String,
}

#[tokio::test]
async fn token_lifecycle_and_owner_scope() {
    let Ok(url) = std::env::var("KOUGA_TEST_DATABASE_URL") else {
        return;
    };
    let isolated = TestDb::connect(&url, concat!(env!("CARGO_MANIFEST_DIR"), "/migrations"))
        .await
        .unwrap();
    let db = isolated.db();
    sqlx::query("CREATE TABLE auth_test_items (id uuid PRIMARY KEY, owner_id uuid NOT NULL, name text NOT NULL)")
        .execute(db).await.unwrap();
    let owner = CurrentUser { id: Uuid::new_v4() };
    let other = CurrentUser { id: Uuid::new_v4() };
    sqlx::query("INSERT INTO auth_test_items (id, owner_id, name) VALUES ($1, $2, 'mine'), ($3, $4, 'theirs')")
        .bind(Uuid::new_v4()).bind(owner.id).bind(Uuid::new_v4()).bind(other.id)
        .execute(db).await.unwrap();
    let token = issue_token(db, owner.id, Duration::from_secs(60))
        .await
        .unwrap();
    assert_eq!(authenticate(db, &token).await.unwrap(), Some(owner));
    assert_eq!(authenticate(db, "bad").await.unwrap(), None);
    let saved: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM kouga_auth_tokens LIMIT 1")
        .fetch_one(db)
        .await
        .unwrap();
    assert_eq!(saved.len(), 32);
    assert!(!String::from_utf8_lossy(&saved).contains(&token));
    let mine = owned_by(Item::query(), item::columns::owner_id, owner)
        .fetch_all(db)
        .await
        .unwrap();
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].name, "mine");
    assert_eq!(mine[0].owner_id, owner.id);
    assert_eq!(
        owned_by(
            Item::query().filter(item::columns::id.eq(mine[0].id)),
            item::columns::owner_id,
            other
        )
        .fetch_optional(db)
        .await
        .unwrap()
        .map(|item| item.name),
        None
    );
    let mut tx = db.begin().await.unwrap();
    let target = owned_by(
        Item::query().filter(item::columns::id.eq(mine[0].id)),
        item::columns::owner_id,
        other,
    )
    .for_update()
    .fetch_all(&mut tx)
    .await
    .unwrap();
    assert!(
        target.is_empty(),
        "an update must not select another owner's row"
    );
    tx.rollback().await.unwrap();
    assert!(revoke_token(db, &token).await.unwrap());
    assert_eq!(authenticate(db, &token).await.unwrap(), None);
    let token = issue_token(db, owner.id, Duration::from_secs(60))
        .await
        .unwrap();
    assert_eq!(revoke_user_tokens(db, owner.id).await.unwrap(), 1);
    assert_eq!(authenticate(db, &token).await.unwrap(), None);
    sqlx::query(
        "UPDATE kouga_auth_tokens SET expires_at = now() - interval '1 second' WHERE user_id = $1",
    )
    .bind(owner.id)
    .execute(db)
    .await
    .unwrap();
    let expired = issue_token(db, other.id, Duration::from_secs(60))
        .await
        .unwrap();
    sqlx::query(
        "UPDATE kouga_auth_tokens SET expires_at = now() - interval '1 second' WHERE user_id = $1",
    )
    .bind(other.id)
    .execute(db)
    .await
    .unwrap();
    assert_eq!(authenticate(db, &expired).await.unwrap(), None);
    isolated.close().await.unwrap();
}
