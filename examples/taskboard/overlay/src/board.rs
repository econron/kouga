//! Taskboard domain and HTTP entry. The same Board methods can be called by a runner or worker.
#[cfg(feature = "http")]
use kouga_auth::CurrentUser;
use kouga_cache::PgCache;
use kouga_core::{Error as DomainError, ErrorKind, Patch};
#[cfg(feature = "http")]
use kouga_http::{
    Created, Error, Extension, Json, NoContent, Page, Path, Router, State, Validated,
    ValidatedQuery, endpoint,
};
use kouga_model::{Db, Model, Uuid, sqlx};
use kouga_queue::Enqueue;
#[cfg(feature = "http")]
use kouga_validation::{Request, ValidationError};
use std::time::Duration;

#[derive(Clone, Debug, Model)]
#[model(table = "projects", crud_visibility = "private")]
#[has_many(Task, key = project_id, name = tasks)]
pub struct Project {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub slug: String,
    pub name: String,
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    pub updated_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
}

#[derive(Debug, Model)]
#[model(table = "tasks", crud_visibility = "private")]
#[belongs_to(Project, key = project_id, name = project)]
pub struct Task {
    pub id: Uuid,
    pub project_id: Uuid,
    pub owner_id: Uuid,
    pub title: String,
    pub completed: bool,
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    pub updated_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
}

fn failure(kind: ErrorKind, code: &'static str, message: &'static str) -> DomainError {
    DomainError::new(kind, code, message)
}
fn not_found() -> DomainError {
    failure(ErrorKind::NotFound, "not_found", "Not found")
}
fn invalid() -> DomainError {
    failure(
        ErrorKind::Validation,
        "invalid_state",
        "Invalid taskboard state",
    )
}
fn db(error: sqlx::Error) -> DomainError {
    if let Some(database) = error.as_database_error() {
        match database.code().as_deref() {
            Some("23505") => return failure(ErrorKind::Conflict, "duplicate", "Already exists"),
            Some("23503") => return not_found(),
            Some("23514") => return invalid(),
            _ => {}
        }
    }
    kouga_model::db::DbError::from(error).into_core()
}
#[cfg(feature = "http")]
fn slug(value: &str) -> Result<(), ValidationError> {
    if value.is_empty()
        || value.len() > 80
        || !value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(ValidationError::new("slug"));
    }
    Ok(())
}
#[cfg(feature = "http")]
fn non_blank(value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        Err(ValidationError::new("blank"))
    } else {
        Ok(())
    }
}
#[cfg(feature = "http")]
fn uuid(value: &str) -> Result<(), ValidationError> {
    Uuid::parse_str(value)
        .map(|_| ())
        .map_err(|_| ValidationError::new("uuid"))
}

#[cfg(feature = "http")]
#[derive(Request)]
pub struct CreateProject {
    #[validate(length(min = 1, max = 80), custom = slug)]
    pub slug: String,
    #[validate(length(min = 1, max = 100), custom = non_blank)]
    pub name: String,
}
#[cfg(feature = "http")]
#[derive(Request)]
pub struct ProjectPatchInput {
    #[validate(length(min = 1, max = 100), custom = non_blank)]
    pub name: Patch<String>,
}
#[cfg(feature = "http")]
#[derive(Request)]
pub struct CreateTask {
    #[validate(custom = uuid)]
    pub project_id: String,
    #[validate(length(min = 1, max = 200), custom = non_blank)]
    pub title: String,
}
#[cfg(feature = "http")]
#[derive(Request)]
pub struct TaskPatchInput {
    #[validate(length(min = 1, max = 200), custom = non_blank)]
    pub title: Patch<String>,
    pub completed: Patch<bool>,
}
#[cfg(feature = "http")]
#[derive(Request)]
pub struct ListQuery {
    #[validate(range(min = 1, max = 1000000))]
    pub page: Option<usize>,
    #[validate(range(min = 1, max = 100))]
    pub per_page: Option<usize>,
}

#[cfg(feature = "http")]
#[derive(serde::Serialize)]
pub struct ProjectOutput {
    pub id: String,
    pub slug: String,
    pub name: String,
}
#[cfg(feature = "http")]
impl From<Project> for ProjectOutput {
    fn from(value: Project) -> Self {
        Self {
            id: value.id.to_string(),
            slug: value.slug,
            name: value.name,
        }
    }
}
#[cfg(feature = "http")]
#[derive(serde::Serialize)]
pub struct TaskOutput {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub completed: bool,
    pub project_name: String,
}
#[cfg(feature = "http")]
fn task_output(value: Task, project_name: String) -> TaskOutput {
    TaskOutput {
        id: value.id.to_string(),
        project_id: value.project_id.to_string(),
        title: value.title,
        completed: value.completed,
        project_name,
    }
}
#[derive(serde::Serialize)]
pub struct CountOutput {
    pub total: i64,
    pub completed: i64,
}
#[cfg(feature = "http")]
macro_rules! output_schema {
    ($name:ident, $schema:expr) => {
        impl schemars::JsonSchema for $name {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                stringify!($name).into()
            }
            fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
                schemars::Schema::try_from($schema).expect("static output schema")
            }
        }
    };
}
#[cfg(feature = "http")]
output_schema!(
    ProjectOutput,
    serde_json::json!({"type":"object","required":["id","slug","name"],"properties":{"id":{"type":"string","format":"uuid"},"slug":{"type":"string"},"name":{"type":"string"}}})
);
#[cfg(feature = "http")]
output_schema!(
    TaskOutput,
    serde_json::json!({"type":"object","required":["id","project_id","title","completed","project_name"],"properties":{"id":{"type":"string","format":"uuid"},"project_id":{"type":"string","format":"uuid"},"title":{"type":"string"},"completed":{"type":"boolean"},"project_name":{"type":"string"}}})
);
#[cfg(feature = "http")]
output_schema!(
    CountOutput,
    serde_json::json!({"type":"object","required":["total","completed"],"properties":{"total":{"type":"integer"},"completed":{"type":"integer"}}})
);

/// Domain operations are owner-scoped regardless of their caller's transport.
#[derive(Clone)]
pub struct Board {
    db: Db,
}
impl Board {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    #[tracing::instrument(skip_all, name = "taskboard.project.create")]
    pub async fn create_project(
        &self,
        actor: Uuid,
        slug: &str,
        name: &str,
    ) -> Result<Project, DomainError> {
        if slug.is_empty()
            || slug.len() > 80
            || !slug
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || name.trim().is_empty()
            || name.chars().count() > 100
        {
            return Err(invalid());
        }
        sqlx::query_as(
            "INSERT INTO projects (id, owner_id, slug, name) VALUES ($1,$2,$3,$4) RETURNING *",
        )
        .bind(Uuid::new_v4())
        .bind(actor)
        .bind(slug)
        .bind(name)
        .fetch_one(&self.db)
        .await
        .map_err(db)
    }
    pub async fn project(&self, actor: Uuid, id: Uuid) -> Result<Project, DomainError> {
        Project::query()
            .filter(project::columns::owner_id.eq(actor))
            .filter(project::columns::id.eq(id))
            .fetch_optional(&self.db)
            .await
            .map_err(|e| e.into_core())?
            .ok_or_else(not_found)
    }
    pub async fn update_project(
        &self,
        actor: Uuid,
        id: Uuid,
        name: &str,
    ) -> Result<Project, DomainError> {
        if name.trim().is_empty() || name.chars().count() > 100 {
            return Err(invalid());
        }
        sqlx::query_as(
            "UPDATE projects SET name=$1, updated_at=now() WHERE id=$2 AND owner_id=$3 RETURNING *",
        )
        .bind(name)
        .bind(id)
        .bind(actor)
        .fetch_optional(&self.db)
        .await
        .map_err(db)?
        .ok_or_else(not_found)
    }
    pub async fn delete_project(&self, actor: Uuid, id: Uuid) -> Result<(), DomainError> {
        let mut tx = self.db.begin().await.map_err(db)?;
        sqlx::query("UPDATE kouga_files SET state='delete_pending' WHERE record_type='tasks' AND state='attached' AND record_id IN (SELECT id FROM tasks WHERE project_id=$1 AND owner_id=$2)")
            .bind(id).bind(actor).execute(&mut *tx).await.map_err(db)?;
        // Explicitly remove children; FK defaults to RESTRICT for accidental direct deletes.
        let deleted_tasks: Vec<Uuid> = sqlx::query_scalar(
            "DELETE FROM tasks WHERE project_id=$1 AND owner_id=$2 RETURNING id",
        )
        .bind(id)
        .bind(actor)
        .fetch_all(&mut *tx)
        .await
        .map_err(db)?;
        for task_id in deleted_tasks {
            crate::realtime::task_changed(&mut tx, actor, task_id, "deleted")
                .await
                .map_err(|_| {
                    failure(
                        ErrorKind::Unavailable,
                        "channel_unavailable",
                        "Channel unavailable",
                    )
                })?;
        }
        let deleted = sqlx::query("DELETE FROM projects WHERE id=$1 AND owner_id=$2")
            .bind(id)
            .bind(actor)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        if deleted.rows_affected() == 0 {
            return Err(not_found());
        }
        Self::invalidate_in(&mut tx, id).await?;
        tx.commit().await.map_err(db)
    }
    #[tracing::instrument(skip_all, name = "taskboard.task.create")]
    pub async fn create_task(
        &self,
        actor: Uuid,
        project_id: Uuid,
        title: &str,
    ) -> Result<Task, DomainError> {
        if title.trim().is_empty() || title.chars().count() > 200 {
            return Err(invalid());
        }
        let mut tx = self.db.begin().await.map_err(db)?;
        let value: Task = sqlx::query_as(
            "INSERT INTO tasks (id, project_id, owner_id, title) VALUES ($1,$2,$3,$4) RETURNING *",
        )
        .bind(Uuid::new_v4())
        .bind(project_id)
        .bind(actor)
        .bind(title)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
        app_contracts::task_notice::TaskCreatedV2 {
            task_id: value.id,
            owner_id: actor,
        }
        .enqueue(&mut *tx)
        .await
        .map_err(|_| {
            failure(
                ErrorKind::Internal,
                "job_enqueue_failed",
                "Task notification unavailable",
            )
        })?;
        Self::invalidate_in(&mut tx, project_id).await?;
        crate::realtime::task_changed(&mut tx, actor, value.id, "created")
            .await
            .map_err(|_| {
                failure(
                    ErrorKind::Unavailable,
                    "channel_unavailable",
                    "Channel unavailable",
                )
            })?;
        tx.commit().await.map_err(db)?;
        Ok(value)
    }
    pub async fn task(&self, actor: Uuid, id: Uuid) -> Result<Task, DomainError> {
        Task::query()
            .filter(task::columns::owner_id.eq(actor))
            .filter(task::columns::id.eq(id))
            .fetch_optional(&self.db)
            .await
            .map_err(|e| e.into_core())?
            .ok_or_else(not_found)
    }
    pub async fn update_task(
        &self,
        actor: Uuid,
        id: Uuid,
        title: Patch<String>,
        completed: Patch<bool>,
    ) -> Result<Task, DomainError> {
        if title.is_missing() && completed.is_missing() {
            return Err(invalid());
        }
        if let Patch::Value(ref value) = title
            && (value.trim().is_empty() || value.chars().count() > 200)
        {
            return Err(invalid());
        }
        let current = self.task(actor, id).await?;
        let next_title = match title {
            Patch::Missing => current.title.clone(),
            Patch::Value(value) => value,
        };
        let next_completed = match completed {
            Patch::Missing => current.completed,
            Patch::Value(value) => value,
        };
        if current.completed && !next_completed {
            return Err(invalid());
        }
        let mut tx = self.db.begin().await.map_err(db)?;
        let value: Task = sqlx::query_as("UPDATE tasks SET title=$1, completed=$2, updated_at=now() WHERE id=$3 AND owner_id=$4 AND (completed=false OR $2=true) RETURNING *")
            .bind(next_title).bind(next_completed).bind(id).bind(actor).fetch_optional(&mut *tx).await.map_err(db)?.ok_or_else(invalid)?;
        Self::invalidate_in(&mut tx, value.project_id).await?;
        crate::realtime::task_changed(&mut tx, actor, value.id, "updated")
            .await
            .map_err(|_| {
                failure(
                    ErrorKind::Unavailable,
                    "channel_unavailable",
                    "Channel unavailable",
                )
            })?;
        tx.commit().await.map_err(db)?;
        Ok(value)
    }
    pub async fn delete_task(&self, actor: Uuid, id: Uuid) -> Result<(), DomainError> {
        let mut tx = self.db.begin().await.map_err(db)?;
        sqlx::query("UPDATE kouga_files SET state='delete_pending' WHERE record_type='tasks' AND record_id=$1 AND state='attached'")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let project_id: Option<Uuid> = sqlx::query_scalar(
            "DELETE FROM tasks WHERE id=$1 AND owner_id=$2 RETURNING project_id",
        )
        .bind(id)
        .bind(actor)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?;
        let project_id = project_id.ok_or_else(not_found)?;
        Self::invalidate_in(&mut tx, project_id).await?;
        crate::realtime::task_changed(&mut tx, actor, id, "deleted")
            .await
            .map_err(|_| {
                failure(
                    ErrorKind::Unavailable,
                    "channel_unavailable",
                    "Channel unavailable",
                )
            })?;
        tx.commit().await.map_err(db)
    }
    async fn invalidate_in(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        project_id: Uuid,
    ) -> Result<(), DomainError> {
        sqlx::query("DELETE FROM kouga_cache WHERE namespace='taskboard-count' AND key=$1")
            .bind(project_id.to_string())
            .execute(&mut **tx)
            .await
            .map_err(db)?;
        Ok(())
    }
    pub async fn count(&self, actor: Uuid, project_id: Uuid) -> Result<CountOutput, DomainError> {
        self.project(actor, project_id).await?;
        let cache = PgCache::new(self.db.clone());
        let key = project_id.to_string();
        match cache.get("taskboard-count", &key).await {
            Ok(Some(value)) => {
                if let (Some(total), Some(completed)) =
                    (value["total"].as_i64(), value["completed"].as_i64())
                {
                    return Ok(CountOutput { total, completed });
                }
            }
            Err(error) => tracing::warn!(?error, "taskboard count cache unavailable"),
            Ok(None) => {}
        }
        let (total, completed): (i64, i64) = sqlx::query_as("SELECT count(*), count(*) FILTER (WHERE completed) FROM tasks WHERE project_id=$1 AND owner_id=$2")
            .bind(project_id).bind(actor).fetch_one(&self.db).await.map_err(db)?;
        if let Err(error) = cache
            .set(
                "taskboard-count",
                &key,
                &serde_json::json!({"total":total,"completed":completed}),
                Duration::from_secs(30),
            )
            .await
        {
            tracing::warn!(?error, "taskboard count cache unavailable");
        }
        Ok(CountOutput { total, completed })
    }
}

#[cfg(feature = "http")]
fn parse_id(raw: &str) -> Result<Uuid, Error> {
    Uuid::parse_str(raw)
        .map_err(|_| Error(failure(ErrorKind::BadRequest, "invalid_id", "Invalid ID")))
}
#[cfg(feature = "http")]
fn service(db: Db) -> Board {
    Board::new(db)
}
#[cfg(feature = "http")]
fn http(error: DomainError) -> Error {
    Error(error)
}

#[cfg(feature = "http")]
#[endpoint(operation_id = "board.projects.index")]
async fn projects(
    State(db): State<Db>,
    Extension(actor): Extension<CurrentUser>,
    query: ValidatedQuery<ListQuery>,
) -> Result<Page<ProjectOutput>, Error> {
    let page = query.page.unwrap_or(1);
    let per_page = query.per_page.unwrap_or(20);
    let rows = Project::query()
        .filter(project::columns::owner_id.eq(actor.id))
        .order_by(project::columns::created_at.desc())
        .order_by(project::columns::id.asc())
        .page(page as i64, per_page as i64)
        .fetch(&db)
        .await
        .map_err(|e| http(e.into_core()))?;
    Ok(Page {
        data: rows.items.into_iter().map(Into::into).collect(),
        page,
        per_page,
        has_next: rows.has_next,
    })
}
#[cfg(feature = "http")]
#[endpoint(operation_id = "board.projects.create")]
async fn project_create(
    State(db): State<Db>,
    Extension(actor): Extension<CurrentUser>,
    input: Validated<CreateProject>,
) -> Result<Created<ProjectOutput>, Error> {
    let value = service(db)
        .create_project(actor.id, &input.slug, &input.name)
        .await
        .map_err(http)?;
    Ok(Created::new(
        format!("/projects/{}", value.id),
        value.into(),
    ))
}
#[cfg(feature = "http")]
#[endpoint(operation_id = "board.projects.show")]
async fn project_show(
    State(db): State<Db>,
    Extension(actor): Extension<CurrentUser>,
    Path(raw): Path<String>,
) -> Result<Json<ProjectOutput>, Error> {
    Ok(Json(
        service(db)
            .project(actor.id, parse_id(&raw)?)
            .await
            .map_err(http)?
            .into(),
    ))
}
#[cfg(feature = "http")]
#[endpoint(operation_id = "board.projects.update")]
async fn project_update(
    State(db): State<Db>,
    Extension(actor): Extension<CurrentUser>,
    Path(raw): Path<String>,
    input: Validated<ProjectPatchInput>,
) -> Result<Json<ProjectOutput>, Error> {
    let Patch::Value(ref name) = input.name else {
        return Err(http(invalid()));
    };
    Ok(Json(
        service(db)
            .update_project(actor.id, parse_id(&raw)?, name)
            .await
            .map_err(http)?
            .into(),
    ))
}
#[cfg(feature = "http")]
#[endpoint(operation_id = "board.projects.destroy")]
async fn project_destroy(
    State(db): State<Db>,
    Extension(actor): Extension<CurrentUser>,
    Path(raw): Path<String>,
) -> Result<NoContent, Error> {
    service(db)
        .delete_project(actor.id, parse_id(&raw)?)
        .await
        .map_err(http)?;
    Ok(NoContent)
}
#[cfg(feature = "http")]
#[endpoint(operation_id = "board.tasks.index")]
async fn tasks(
    State(db): State<Db>,
    Extension(actor): Extension<CurrentUser>,
    query: ValidatedQuery<ListQuery>,
) -> Result<Page<TaskOutput>, Error> {
    let page = query.page.unwrap_or(1);
    let per_page = query.per_page.unwrap_or(20);
    let rows = Task::query()
        .filter(task::columns::owner_id.eq(actor.id))
        .order_by(task::columns::created_at.desc())
        .order_by(task::columns::id.asc())
        .preload(task::relations::project())
        .page(page as i64, per_page as i64)
        .fetch(&db)
        .await
        .map_err(|e| http(e.into_core()))?;
    Ok(Page {
        data: rows
            .items
            .into_iter()
            .map(|row| task_output(row.model, row.related.name))
            .collect(),
        page,
        per_page,
        has_next: rows.has_next,
    })
}
#[cfg(feature = "http")]
#[endpoint(operation_id = "board.tasks.create")]
async fn task_create(
    State(db): State<Db>,
    Extension(actor): Extension<CurrentUser>,
    input: Validated<CreateTask>,
) -> Result<Created<TaskOutput>, Error> {
    let board = service(db);
    let value = board
        .create_task(actor.id, parse_id(&input.project_id)?, &input.title)
        .await
        .map_err(http)?;
    let project = board
        .project(actor.id, value.project_id)
        .await
        .map_err(http)?;
    Ok(Created::new(
        format!("/tasks/{}", value.id),
        task_output(value, project.name),
    ))
}
#[cfg(feature = "http")]
#[endpoint(operation_id = "board.tasks.show")]
async fn task_show(
    State(db): State<Db>,
    Extension(actor): Extension<CurrentUser>,
    Path(raw): Path<String>,
) -> Result<Json<TaskOutput>, Error> {
    let board = service(db);
    let value = board.task(actor.id, parse_id(&raw)?).await.map_err(http)?;
    let project = board
        .project(actor.id, value.project_id)
        .await
        .map_err(http)?;
    Ok(Json(task_output(value, project.name)))
}
#[cfg(feature = "http")]
#[endpoint(operation_id = "board.tasks.update")]
async fn task_update(
    State(db): State<Db>,
    Extension(actor): Extension<CurrentUser>,
    Path(raw): Path<String>,
    input: Validated<TaskPatchInput>,
) -> Result<Json<TaskOutput>, Error> {
    let board = service(db);
    let value = board
        .update_task(
            actor.id,
            parse_id(&raw)?,
            input.title.clone(),
            input.completed.clone(),
        )
        .await
        .map_err(http)?;
    let project = board
        .project(actor.id, value.project_id)
        .await
        .map_err(http)?;
    Ok(Json(task_output(value, project.name)))
}
#[cfg(feature = "http")]
#[endpoint(operation_id = "board.tasks.destroy")]
async fn task_destroy(
    State(db): State<Db>,
    Extension(actor): Extension<CurrentUser>,
    Path(raw): Path<String>,
) -> Result<NoContent, Error> {
    service(db)
        .delete_task(actor.id, parse_id(&raw)?)
        .await
        .map_err(http)?;
    Ok(NoContent)
}
#[cfg(feature = "http")]
#[endpoint(operation_id = "board.projects.count")]
async fn project_count(
    State(db): State<Db>,
    Extension(actor): Extension<CurrentUser>,
    Path(raw): Path<String>,
) -> Result<Json<CountOutput>, Error> {
    Ok(Json(
        service(db)
            .count(actor.id, parse_id(&raw)?)
            .await
            .map_err(http)?,
    ))
}

#[cfg(feature = "http")]
pub fn routes(router: Router<Db>) -> Router<Db> {
    let guard = kouga_http::auth::require_bearer(|db: &Db| db);
    router
        .get("/projects", projects_endpoint().middleware(guard.clone()))
        .unwrap()
        .post(
            "/projects",
            project_create_endpoint().middleware(guard.clone()),
        )
        .unwrap()
        .get(
            "/projects/{id}",
            project_show_endpoint().middleware(guard.clone()),
        )
        .unwrap()
        .patch(
            "/projects/{id}",
            project_update_endpoint().middleware(guard.clone()),
        )
        .unwrap()
        .delete(
            "/projects/{id}",
            project_destroy_endpoint().middleware(guard.clone()),
        )
        .unwrap()
        .get(
            "/projects/{id}/count",
            project_count_endpoint().middleware(guard.clone()),
        )
        .unwrap()
        .get("/tasks", tasks_endpoint().middleware(guard.clone()))
        .unwrap()
        .post("/tasks", task_create_endpoint().middleware(guard.clone()))
        .unwrap()
        .get(
            "/tasks/{id}",
            task_show_endpoint().middleware(guard.clone()),
        )
        .unwrap()
        .patch(
            "/tasks/{id}",
            task_update_endpoint().middleware(guard.clone()),
        )
        .unwrap()
        .delete("/tasks/{id}", task_destroy_endpoint().middleware(guard))
        .unwrap()
}
