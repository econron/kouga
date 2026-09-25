use kouga_db::DbErrorKind;
use kouga_model::{Model, Uuid};

#[derive(Clone, Debug, Model)]
#[model(table = "kouga_t13_projects")]
#[has_many(Task, key = project_id, name = tasks)]
#[has_one(Profile, key = project_id, name = profile)]
#[many_to_many(Tag, through = ProjectTag, key = project_id, target_key = tag_id, name = tags)]
struct Project {
    id: Uuid,
    name: String,
}

#[derive(Clone, Debug, Model)]
#[model(table = "kouga_t13_tasks")]
#[belongs_to(Project, key = project_id, name = project)]
#[belongs_to(User, key = owner_id, name = owner)]
struct Task {
    id: Uuid,
    project_id: Uuid,
    owner_id: Option<Uuid>,
    title: String,
}

#[derive(Clone, Debug, Model)]
#[model(table = "kouga_t13_profiles")]
struct Profile {
    id: Uuid,
    project_id: Uuid,
    bio: String,
}

#[derive(Clone, Debug, Model)]
#[model(table = "kouga_t13_users")]
struct User {
    id: Uuid,
    name: String,
}

#[derive(Clone, Debug, Model)]
#[model(table = "kouga_t13_tags")]
struct Tag {
    id: Uuid,
    name: String,
}

#[derive(Clone, Debug, Model)]
#[model(table = "kouga_t13_project_tags")]
struct ProjectTag {
    id: Uuid,
    project_id: Uuid,
    tag_id: Uuid,
}

#[tokio::test]
async fn explicit_associations_and_preload() {
    let Ok(url) = std::env::var("KOUGA_TEST_DATABASE_URL") else {
        return;
    };
    let db = kouga_db::connect(&url, 2, std::time::Duration::from_secs(3))
        .await
        .unwrap();
    fn assert_send<T: Send>(_: T) {}
    assert_send(
        Task::query()
            .preload(task::relations::project())
            .fetch_all(&db),
    );
    for ddl in [
        "DROP TABLE IF EXISTS kouga_t13_project_tags",
        "DROP TABLE IF EXISTS kouga_t13_tasks",
        "DROP TABLE IF EXISTS kouga_t13_profiles",
        "DROP TABLE IF EXISTS kouga_t13_tags",
        "DROP TABLE IF EXISTS kouga_t13_users",
        "DROP TABLE IF EXISTS kouga_t13_projects",
    ] {
        sqlx::query(ddl).execute(&db).await.unwrap();
    }
    for ddl in [
        "CREATE TABLE kouga_t13_projects (id uuid PRIMARY KEY, name text NOT NULL)",
        "CREATE TABLE kouga_t13_users (id uuid PRIMARY KEY, name text NOT NULL)",
        "CREATE TABLE kouga_t13_tags (id uuid PRIMARY KEY, name text NOT NULL)",
        "CREATE TABLE kouga_t13_tasks (id uuid PRIMARY KEY, project_id uuid NOT NULL, owner_id uuid, title text NOT NULL)",
        "CREATE TABLE kouga_t13_profiles (id uuid PRIMARY KEY, project_id uuid UNIQUE NOT NULL, bio text NOT NULL)",
        "CREATE TABLE kouga_t13_project_tags (id uuid PRIMARY KEY, project_id uuid NOT NULL, tag_id uuid NOT NULL, UNIQUE(project_id, tag_id))",
    ] {
        sqlx::query(ddl).execute(&db).await.unwrap();
    }

    let p1 = Uuid::new_v4();
    let p2 = Uuid::new_v4();
    let user = Uuid::new_v4();
    let tag = Uuid::new_v4();
    let t1 = Uuid::new_v4();
    let t2 = Uuid::new_v4();
    sqlx::query("INSERT INTO kouga_t13_projects VALUES ($1,'first'),($2,'second')")
        .bind(p1)
        .bind(p2)
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO kouga_t13_users VALUES ($1,'Ada')")
        .bind(user)
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO kouga_t13_tags VALUES ($1,'rust')")
        .bind(tag)
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO kouga_t13_tasks VALUES ($1,$2,$3,'one'),($4,$2,NULL,'two')")
        .bind(t1)
        .bind(p1)
        .bind(user)
        .bind(t2)
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO kouga_t13_profiles VALUES ($1,$2,'about')")
        .bind(Uuid::new_v4())
        .bind(p1)
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO kouga_t13_project_tags VALUES ($1,$2,$3)")
        .bind(Uuid::new_v4())
        .bind(p1)
        .bind(tag)
        .execute(&db)
        .await
        .unwrap();

    let tasks = Task::query()
        .order_by(task::columns::title.asc())
        .preload((task::relations::project(), task::relations::owner()))
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].model.title, "one");
    assert_eq!(tasks[0].related.0.name, "first");
    assert_send(tasks[0].model.project(&db));
    assert_eq!(tasks[0].related.1.as_ref().unwrap().name, "Ada");
    assert!(tasks[1].related.1.is_none());
    assert_eq!(tasks[0].model.project(&db).await.unwrap().id, p1);
    assert_eq!(tasks[0].model.owner_query().count(&db).await.unwrap(), 1);
    assert_eq!(tasks[1].model.owner_query().count(&db).await.unwrap(), 0);

    let projects = Project::query()
        .order_by(project::columns::name.asc())
        .preload((
            project::relations::tasks(),
            project::relations::profile(),
            project::relations::tags(),
        ))
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(projects[0].related.0.len(), 2);
    assert_eq!(projects[0].related.1.as_ref().unwrap().bio, "about");
    assert_eq!(projects[0].related.2[0].name, "rust");
    assert!(projects[1].related.0.is_empty());
    assert!(projects[1].related.1.is_none());
    assert!(projects[1].related.2.is_empty());

    let nested = Task::query()
        .filter(task::columns::id.eq(t1))
        .preload(task::relations::project().preload(project::relations::profile()))
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(nested[0].related.model.id, p1);
    assert_eq!(nested[0].related.related.as_ref().unwrap().bio, "about");

    let scoped = Task::query()
        .preload(task::relations::project().filter(project::columns::name.eq("hidden".into())))
        .fetch_all(&db)
        .await;
    assert_eq!(scoped.unwrap_err().kind, DbErrorKind::Integrity);
    let page = Project::query()
        .order_by(project::columns::name.asc())
        .limit(1)
        .preload(project::relations::tasks())
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].related.len(), 2);
    let page = Project::query()
        .order_by(project::columns::name.asc())
        .preload(project::relations::tasks())
        .page(1, 1)
        .fetch(&db)
        .await
        .unwrap();
    assert!(page.has_next);
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].related.len(), 2);

    let mut tx = db.begin().await.unwrap();
    let in_tx = Task::query()
        .filter(task::columns::id.eq(t1))
        .preload(task::relations::project())
        .fetch_all(&mut tx)
        .await
        .unwrap();
    assert_eq!(in_tx[0].related.id, p1);
    tx.rollback().await.unwrap();

    sqlx::query("WITH projects AS (INSERT INTO kouga_t13_projects SELECT gen_random_uuid(), 'bulk-' || n FROM generate_series(1, 1005) n RETURNING id) INSERT INTO kouga_t13_tasks SELECT gen_random_uuid(), id, NULL, 'bulk' FROM projects")
        .execute(&db).await.unwrap();
    let stats = sqlx::query("CREATE EXTENSION IF NOT EXISTS pg_stat_statements")
        .execute(&db)
        .await
        .is_ok();
    let count_sql = "SELECT coalesce(sum(calls), 0)::bigint FROM pg_stat_statements WHERE query LIKE 'SELECT * FROM \"kouga_t13_tasks\"%'";
    let before: Option<i64> = if stats {
        sqlx::query_scalar(count_sql)
            .fetch_optional(&db)
            .await
            .unwrap()
    } else {
        None
    };
    let projects = Project::query()
        .preload(project::relations::tasks())
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(projects.len(), 1_007);
    assert_eq!(
        projects.iter().map(|p| p.related.len()).sum::<usize>(),
        1_007
    );
    if let Some(before) = before {
        let after: i64 = sqlx::query_scalar(count_sql).fetch_one(&db).await.unwrap();
        assert_eq!(
            after - before,
            2,
            "1007 parent IDs must use two related SELECTs"
        );
    }
    // Simulate a legacy orphan; production migrations should enforce a foreign key.
    sqlx::query("UPDATE kouga_t13_tasks SET project_id = $1 WHERE id = $2")
        .bind(Uuid::new_v4())
        .bind(t1)
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        Task::find(&db, t1)
            .await
            .unwrap()
            .unwrap()
            .project(&db)
            .await
            .unwrap_err()
            .kind,
        DbErrorKind::Integrity
    );
}
