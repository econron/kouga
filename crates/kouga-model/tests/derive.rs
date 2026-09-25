use kouga_db::{ConstraintKind, DbErrorKind};
use kouga_model::{Model, Uuid, core::Patch};
use sqlx::types::{
    Decimal,
    chrono::{DateTime, NaiveDate, Utc},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "kouga_t12_state", rename_all = "lowercase")]
enum State {
    Draft,
    Done,
}
impl kouga_model::Comparable for State {}

#[derive(Debug, Model)]
#[model(table = "kouga_t12_tasks", module = task_meta)]
struct Task {
    id: Uuid,
    #[model(column = "display_name")]
    title: String,
    #[model(default)]
    note: Option<String>,
    #[model(default)]
    completed: bool,
    state: State,
    priority: i32,
    bigint: i64,
    amount: Decimal,
    run_at: DateTime<Utc>,
    due_on: NaiveDate,
    payload: serde_json::Value,
    blob: Vec<u8>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[allow(dead_code)]
#[derive(Model)]
#[model(table = "kouga_t12_simple")]
struct Simple {
    id: Uuid,
    name: String,
}

#[test]
fn default_snake_case_module_exposes_typed_columns() {
    let _: kouga_model::Column<Simple, String> = simple::columns::name;
}

#[allow(dead_code)]
mod private_crud {
    use super::*;

    #[derive(Model)]
    #[model(table = "kouga_t12_private", crud_visibility = "private")]
    pub struct PrivateTask {
        id: Uuid,
        name: String,
    }

    impl PrivateTask {
        pub async fn create_business<'c, A>(db: A, name: String) -> Result<Self, kouga_db::DbError>
        where
            A: kouga_db::Acquire<'c, Database = kouga_db::Postgres> + Send,
        {
            Self::create(db, NewPrivateTask { name }).await
        }
    }
}

fn attrs(title: &str) -> NewTask {
    NewTask {
        title: title.into(),
        note: None,
        completed: None,
        state: State::Draft,
        priority: 2,
        bigint: 8,
        amount: Decimal::new(1234, 2),
        run_at: DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
        due_on: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        payload: serde_json::json!({"ok":true}),
        blob: vec![1, 2, 3],
    }
}

#[tokio::test]
async fn generated_crud_round_trips_all_supported_types() {
    let Ok(url) = std::env::var("KOUGA_TEST_DATABASE_URL") else {
        return;
    };
    let db = kouga_db::connect(&url, 5, std::time::Duration::from_secs(3))
        .await
        .unwrap();
    sqlx::query("DROP TABLE IF EXISTS kouga_t12_tasks")
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("DROP TYPE IF EXISTS kouga_t12_state")
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("CREATE TYPE kouga_t12_state AS ENUM ('draft', 'done')")
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("CREATE TABLE kouga_t12_tasks (id uuid PRIMARY KEY, display_name text NOT NULL UNIQUE, note text DEFAULT 'db-default', completed boolean NOT NULL DEFAULT false, state kouga_t12_state NOT NULL, priority integer NOT NULL, bigint bigint NOT NULL, amount numeric NOT NULL, run_at timestamptz NOT NULL, due_on date NOT NULL, payload jsonb NOT NULL, blob bytea NOT NULL, created_at timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now())")
        .execute(&db).await.unwrap();

    let task = Task::create(&db, attrs("first")).await.unwrap();
    assert_ne!(task.id, Uuid::nil());
    assert_eq!(task.note.as_deref(), Some("db-default"));
    assert!(!task.completed);
    assert_eq!(task.state, State::Draft);
    assert_eq!(task.bigint, 8);
    assert_eq!(task.priority, 2);
    assert_eq!(task.amount, Decimal::new(1234, 2));
    assert_eq!(task.run_at.timestamp(), 1_700_000_000);
    assert_eq!(task.due_on, NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
    assert_eq!(task.payload, serde_json::json!({"ok":true}));
    assert_eq!(task.blob, vec![1, 2, 3]);
    assert!(task.updated_at >= task.created_at);

    let explicit = Uuid::parse_str("00000000-0000-0000-0000-000000000123").unwrap();
    let mut second = attrs("second");
    second.note = Some(None);
    second.completed = Some(true);
    let second = Task::create_with_id(&db, explicit, second).await.unwrap();
    assert_eq!(second.id, explicit);
    assert_eq!(second.note, None);
    assert!(second.completed);
    let duplicate = Task::create(&db, attrs("first")).await.unwrap_err();
    assert_eq!(
        duplicate.kind,
        DbErrorKind::Constraint(ConstraintKind::Unique)
    );

    let got = Task::find(&db, task.id).await.unwrap().unwrap();
    assert_eq!(got.title, "first");
    assert_eq!(
        Task::query()
            .filter(task_meta::columns::state.eq(State::Draft))
            .count(&db)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        Task::query()
            .filter(task_meta::columns::title.eq("first".into()))
            .count(&db)
            .await
            .unwrap(),
        1
    );
    let updated = Task::update(
        &db,
        task.id,
        UpdateTask {
            note: Patch::Value(None),
            state: Patch::Value(State::Done),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(updated.note, None);
    assert_eq!(updated.state, State::Done);
    assert_eq!(updated.title, "first");
    assert!(updated.updated_at >= task.updated_at);
    assert_eq!(
        Task::update(&db, task.id, UpdateTask::default())
            .await
            .unwrap_err()
            .kind,
        DbErrorKind::InvalidInput
    );
    assert!(Task::delete(&db, explicit).await.unwrap());
    assert!(!Task::delete(&db, explicit).await.unwrap());

    let mut tx = db.begin().await.unwrap();
    let inside = Task::create(&mut tx, attrs("inside")).await.unwrap();
    assert!(Task::find(&mut tx, inside.id).await.unwrap().is_some());
    tx.rollback().await.unwrap();
    assert!(Task::find(&db, inside.id).await.unwrap().is_none());

    sqlx::query("ALTER TYPE kouga_t12_state ADD VALUE 'archived'")
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("UPDATE kouga_t12_tasks SET state = 'archived' WHERE id = $1")
        .bind(task.id)
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        Task::find(&db, task.id).await.unwrap_err().kind,
        DbErrorKind::Decode
    );
}
