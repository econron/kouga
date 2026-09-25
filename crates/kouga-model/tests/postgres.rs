use kouga_db::{ConstraintKind, DbErrorKind};
use kouga_model::{Column, Db, Field, Model, Query, Uuid, create, delete, find, update};

#[derive(Debug, sqlx::FromRow)]
struct Task {
    id: Uuid,
    name: String,
    note: Option<String>,
    status: i32,
    created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    updated_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
}

impl Model for Task {
    const TABLE: &'static str = "kouga_t11_tasks";
    const COLUMNS: &'static [&'static str] =
        &["id", "name", "note", "status", "created_at", "updated_at"];
    fn id(&self) -> Uuid {
        self.id
    }
}

const NAME: Column<Task, String> = Column::new("name");
const NOTE: Column<Task, Option<String>> = Column::new("note");
const STATUS: Column<Task, i32> = Column::new("status");
const ID: Column<Task, Uuid> = Column::new("id");

fn id(value: u32) -> Uuid {
    Uuid::parse_str(&format!("00000000-0000-0000-0000-{value:012x}")).unwrap()
}

async fn setup(db: &Db) {
    sqlx::query("DROP TABLE IF EXISTS kouga_t11_tasks")
        .execute(db)
        .await
        .unwrap();
    sqlx::query("CREATE TABLE kouga_t11_tasks (id uuid PRIMARY KEY, name text NOT NULL UNIQUE, note text NULL, status integer NOT NULL DEFAULT 7, created_at timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now())")
        .execute(db).await.unwrap();
}

#[tokio::test]
async fn model_operations_against_postgres() {
    let Ok(url) = std::env::var("KOUGA_TEST_DATABASE_URL") else {
        return;
    };
    let db = kouga_db::connect(&url, 5, std::time::Duration::from_secs(3))
        .await
        .unwrap();
    setup(&db).await;

    let first = create::<Task, _>(&db, id(1), vec![Field::new(NAME, "first".to_owned())])
        .await
        .unwrap();
    assert_eq!(
        (first.id, first.name.as_str(), first.status, first.note),
        (id(1), "first", 7, None)
    );
    assert!(first.updated_at >= first.created_at);
    assert!(find::<Task, _>(&db, id(9)).await.unwrap().is_none());

    let changed = update::<Task, _>(&db, id(1), vec![Field::null(NOTE)])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(changed.note, None);
    let changed = update::<Task, _>(&db, id(1), vec![Field::new(NOTE, Some("hello".to_owned()))])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(changed.note.as_deref(), Some("hello"));
    assert_eq!(changed.name, "first");
    assert_eq!(
        update::<Task, _>(&db, id(9), vec![Field::new(STATUS, 2)])
            .await
            .unwrap()
            .map(|t| t.id),
        None
    );
    assert_eq!(
        update::<Task, _>(&db, id(1), vec![])
            .await
            .unwrap_err()
            .kind,
        DbErrorKind::InvalidInput
    );

    create::<Task, _>(&db, id(2), vec![Field::new(NAME, "second".to_owned())])
        .await
        .unwrap();
    create::<Task, _>(&db, id(3), vec![Field::new(NAME, "third".to_owned())])
        .await
        .unwrap();
    let duplicate = create::<Task, _>(&db, id(4), vec![Field::new(NAME, "first".to_owned())])
        .await
        .unwrap_err();
    assert_eq!(
        duplicate.kind,
        DbErrorKind::Constraint(ConstraintKind::Unique)
    );
    assert_eq!(
        duplicate.constraint_name(),
        Some("kouga_t11_tasks_name_key")
    );

    assert_eq!(
        Query::<Task>::new()
            .filter(ID.in_list(vec![]))
            .count(&db)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        Query::<Task>::new()
            .filter(ID.in_list(vec![id(1), id(2)]))
            .count(&db)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        Query::<Task>::new()
            .filter(NOTE.is_null())
            .count(&db)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        Query::<Task>::new()
            .filter(NAME.eq("first' OR true --".to_owned()))
            .count(&db)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        Query::<Task>::new()
            .filter(STATUS.ge(7).and(NOTE.is_not_null()))
            .count(&db)
            .await
            .unwrap(),
        1
    );
    assert!(
        Query::<Task>::new()
            .filter(NAME.eq("first".to_owned()))
            .exists(&db)
            .await
            .unwrap()
    );
    assert_eq!(
        Query::<Task>::new()
            .fetch_optional(&db)
            .await
            .unwrap_err()
            .kind,
        DbErrorKind::Integrity
    );
    let page = Query::<Task>::new()
        .order_by(STATUS.asc())
        .page(1, 2)
        .fetch(&db)
        .await
        .unwrap();
    assert_eq!(
        page.items.iter().map(|t| t.id).collect::<Vec<_>>(),
        vec![id(1), id(2)]
    );
    assert!(page.has_next);
    let page = Query::<Task>::new().page(2, 2).fetch(&db).await.unwrap();
    assert_eq!(page.items.len(), 1);
    assert!(!page.has_next);
    assert_eq!(
        Query::<Task>::new()
            .page(0, 20)
            .fetch(&db)
            .await
            .unwrap_err()
            .kind,
        DbErrorKind::InvalidInput
    );
    assert_eq!(
        Query::<Task>::new()
            .page(1, 101)
            .fetch(&db)
            .await
            .unwrap_err()
            .kind,
        DbErrorKind::InvalidInput
    );

    let invalid = Query::<Task>::new()
        .filter(Column::<Task, String>::new("name;DROP TABLE x").eq("x".into()))
        .fetch_all(&db)
        .await
        .unwrap_err();
    assert_eq!(invalid.kind, DbErrorKind::InvalidInput);
    let invalid = Query::<Task>::new()
        .filter(Column::<Task, String>::new("private").eq("x".into()))
        .fetch_all(&db)
        .await
        .unwrap_err();
    assert_eq!(invalid.kind, DbErrorKind::InvalidInput);

    let mut tx = db.begin().await.unwrap();
    let locked = Query::<Task>::new()
        .filter(ID.eq(id(1)))
        .for_update()
        .fetch_all(&mut tx)
        .await
        .unwrap();
    assert_eq!(locked.len(), 1);
    let wait = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        Query::<Task>::new()
            .filter(ID.eq(id(1)))
            .for_update()
            .fetch_all(&mut db.begin().await.unwrap()),
    )
    .await;
    assert!(
        wait.is_err(),
        "another transaction must wait for the row lock"
    );
    let result =
        sqlx::query("UPDATE kouga_t11_tasks SET status = $1 WHERE id = $2 AND status = $3")
            .bind(8_i32)
            .bind(id(1))
            .bind(7_i32)
            .execute(&mut *tx)
            .await
            .unwrap();
    assert_eq!(result.rows_affected(), 1);
    assert_eq!(
        Query::<Task>::new()
            .filter(STATUS.eq(8))
            .count(&mut tx)
            .await
            .unwrap(),
        1
    );
    tx.rollback().await.unwrap();
    assert_eq!(
        find::<Task, _>(&db, id(1)).await.unwrap().unwrap().status,
        7
    );

    let mut tx = db.begin().await.unwrap();
    let created = create::<Task, _>(&mut tx, id(5), vec![Field::new(NAME, "in-tx".to_owned())])
        .await
        .unwrap();
    assert_eq!(
        find::<Task, _>(&mut tx, created.id)
            .await
            .unwrap()
            .unwrap()
            .name,
        "in-tx"
    );
    assert!(delete::<Task, _>(&mut tx, created.id).await.unwrap());
    tx.commit().await.unwrap();
    assert!(delete::<Task, _>(&db, id(1)).await.unwrap());
    assert!(!delete::<Task, _>(&db, id(1)).await.unwrap());
}
