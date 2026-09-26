//! Private Task attachments. Authorization is checked against the Task before storage access.
use axum::{
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use futures_util::stream;
use kouga_auth::CurrentUser;
use kouga_core::{Error as DomainError, ErrorKind};
use kouga_http::{
    ApiOutput, Endpoint, Error, Extension, Multipart, NoContent, Operation, Path, ResponseMeta,
    Router, State, endpoint,
};
use kouga_model::{Db, Uuid, sqlx};
use kouga_storage::{FileKind, Storage, StorageError, Upload};
use kouga_validation::{ApiSchema, SchemaDirection};
use std::{io, time::Duration};

const MAX_BYTES: u64 = 10 * 1024 * 1024;
const ALLOWED: &[FileKind] = &[FileKind::Png, FileKind::Jpeg, FileKind::Pdf];

pub struct AttachmentInput;
#[derive(serde::Deserialize)]
struct AttachmentPath {
    id: String,
    file_id: String,
}
impl ApiSchema for AttachmentInput {
    fn schema(_: &mut schemars::SchemaGenerator, _: SchemaDirection) -> schemars::Schema {
        schemars::Schema::try_from(serde_json::json!({"type":"object","required":["file"],"properties":{"file":{"type":"string","format":"binary"}}})).expect("static schema")
    }
}

#[derive(serde::Serialize)]
pub struct AttachmentOutput {
    pub id: String,
    pub content_type: String,
    pub byte_size: i64,
}
impl schemars::JsonSchema for AttachmentOutput {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "AttachmentOutput".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::Schema::try_from(serde_json::json!({"type":"object","required":["id","content_type","byte_size"],"properties":{"id":{"type":"string","format":"uuid"},"content_type":{"type":"string"},"byte_size":{"type":"integer"}}})).expect("static schema")
    }
}

fn invalid_id() -> Error {
    Error(DomainError::new(
        ErrorKind::BadRequest,
        "invalid_id",
        "Invalid ID",
    ))
}
fn invalid_file() -> Error {
    Error(DomainError::new(
        ErrorKind::BadRequest,
        "invalid_file",
        "Invalid file",
    ))
}
fn not_found() -> Error {
    Error(DomainError::new(
        ErrorKind::NotFound,
        "not_found",
        "Not found",
    ))
}
fn unavailable() -> Error {
    Error(DomainError::new(
        ErrorKind::Unavailable,
        "storage_unavailable",
        "Storage unavailable",
    ))
}
fn parse(raw: &str) -> Result<Uuid, Error> {
    Uuid::parse_str(raw).map_err(|_| invalid_id())
}
fn root() -> Result<String, Error> {
    std::env::var("BOARD_STORAGE_ROOT").map_err(|_| unavailable())
}
fn storage(db: Db) -> Result<Storage, Error> {
    Storage::local(db, root()?).map_err(|e| Error(e.into()))
}
fn http(error: StorageError) -> Error {
    Error(error.into())
}

async fn owned_task(db: &Db, actor: Uuid, task_id: Uuid) -> Result<(), Error> {
    let found: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM tasks WHERE id=$1 AND owner_id=$2)")
            .bind(task_id)
            .bind(actor)
            .fetch_one(db)
            .await
            .map_err(|_| unavailable())?;
    if found { Ok(()) } else { Err(not_found()) }
}

async fn owned_attachment(db: &Db, actor: Uuid, task_id: Uuid, file_id: Uuid) -> Result<(), Error> {
    owned_task(db, actor, task_id).await?;
    let found: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM kouga_files WHERE id=$1 AND owner_id=$2 AND record_type='tasks' AND record_id=$3 AND state='attached')")
        .bind(file_id).bind(actor).bind(task_id).fetch_one(db).await.map_err(|_| unavailable())?;
    if found { Ok(()) } else { Err(not_found()) }
}

#[endpoint(operation_id = "board.attachments.create")]
async fn upload(
    State(db): State<Db>,
    Extension(actor): Extension<CurrentUser>,
    Path(raw): Path<String>,
    mut form: Multipart<AttachmentInput>,
) -> Result<kouga_http::Created<AttachmentOutput>, Error> {
    let task_id = parse(&raw)?;
    owned_task(&db, actor.id, task_id).await?;
    let field = form
        .next_field()
        .await
        .map_err(|_| invalid_file())?
        .ok_or_else(invalid_file)?;
    if field.name() != Some("file") {
        return Err(invalid_file());
    }
    let filename = field.file_name().unwrap_or("").to_owned();
    let content_type = field.content_type().unwrap_or("").to_owned();
    let input = stream::try_unfold(field, |mut field| async move {
        Ok::<_, axum::extract::multipart::MultipartError>(
            field.chunk().await?.map(|chunk| (chunk, field)),
        )
    });
    let storage = storage(db.clone())?;
    let file = storage
        .save(
            Upload {
                owner: actor,
                filename: &filename,
                declared_content_type: &content_type,
                allowed: ALLOWED,
                max_bytes: MAX_BYTES,
            },
            input,
        )
        .await
        .map_err(http)?;
    // A concurrent task deletion cannot make an unattached file visible; cleanup reaps it.
    if let Err(error) = owned_task(&db, actor.id, task_id).await {
        let _ = storage.delete(actor, file.id).await;
        return Err(error);
    }
    match storage.attach(actor, file.id, "tasks", task_id).await {
        Ok(true) => {}
        Ok(false) => {
            let _ = storage.delete(actor, file.id).await;
            return Err(not_found());
        }
        Err(error) => {
            let _ = storage.delete(actor, file.id).await;
            return Err(http(error));
        }
    }
    match form.next_field().await {
        Ok(None) => {}
        Ok(Some(_)) | Err(_) => {
            let _ = storage.delete(actor, file.id).await;
            return Err(invalid_file());
        }
    }
    Ok(kouga_http::Created::new(
        format!("/tasks/{task_id}/attachments/{}", file.id),
        AttachmentOutput {
            id: file.id.to_string(),
            content_type: file.content_type,
            byte_size: file.byte_size,
        },
    ))
}

pub struct FileBody(pub kouga_storage::Download);
impl ApiOutput for FileBody {
    fn metadata() -> ResponseMeta {
        ResponseMeta {
            status: 200,
            content_type: Some("application/octet-stream"),
            data_schema: None,
            paginated: false,
        }
    }
}
impl IntoResponse for FileBody {
    fn into_response(self) -> Response {
        let content_type = self.0.file.content_type.clone();
        let body = Body::from_stream(self.0.stream.map_err(io::Error::other));
        (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, content_type),
                (header::CONTENT_DISPOSITION, self.0.content_disposition),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_owned()),
                (header::CACHE_CONTROL, "private, no-store".to_owned()),
            ],
            body,
        )
            .into_response()
    }
}
use futures_util::TryStreamExt;

async fn download(
    State(db): State<Db>,
    Extension(actor): Extension<CurrentUser>,
    Path(path): Path<AttachmentPath>,
) -> Result<FileBody, Error> {
    let task_id = parse(&path.id)?;
    let file_id = parse(&path.file_id)?;
    owned_attachment(&db, actor.id, task_id, file_id).await?;
    Ok(FileBody(
        storage(db)?.download(actor, file_id).await.map_err(http)?,
    ))
}
fn download_endpoint() -> Endpoint<Db> {
    Endpoint::handler(
        download,
        attachment_path(Operation::new("board.attachments.show").response::<FileBody>()),
    )
}

async fn destroy(
    State(db): State<Db>,
    Extension(actor): Extension<CurrentUser>,
    Path(path): Path<AttachmentPath>,
) -> Result<NoContent, Error> {
    let task_id = parse(&path.id)?;
    let file_id = parse(&path.file_id)?;
    owned_attachment(&db, actor.id, task_id, file_id).await?;
    storage(db)?.delete(actor, file_id).await.map_err(http)?;
    Ok(NoContent)
}
fn destroy_endpoint() -> Endpoint<Db> {
    Endpoint::handler(
        destroy,
        attachment_path(Operation::new("board.attachments.destroy").response::<NoContent>()),
    )
}
fn attachment_path(mut operation: Operation) -> Operation {
    operation.path_schema = Some(
        serde_json::json!({"type":"object","required":["id","file_id"],"properties":{"id":{"type":"string","format":"uuid"},"file_id":{"type":"string","format":"uuid"}}}),
    );
    operation
}

pub async fn schedule_task_cleanup(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE kouga_files SET state='delete_pending' WHERE record_type='tasks' AND record_id=$1 AND state='attached'")
        .bind(task_id).execute(&mut **tx).await?;
    Ok(())
}

pub fn routes(router: Router<Db>) -> Router<Db> {
    let guard = kouga_http::auth::require_bearer(|db: &Db| db);
    router
        .post(
            "/tasks/{id}/attachments",
            upload_endpoint().middleware(guard.clone()),
        )
        .unwrap()
        .get(
            "/tasks/{id}/attachments/{file_id}",
            download_endpoint().middleware(guard.clone()),
        )
        .unwrap()
        .delete(
            "/tasks/{id}/attachments/{file_id}",
            destroy_endpoint().middleware(guard),
        )
        .unwrap()
}

pub async fn cleanup_once(db: Db) -> Result<u64, StorageError> {
    Storage::local(
        db,
        std::env::var("BOARD_STORAGE_ROOT")
            .map_err(|_| StorageError::Invalid("BOARD_STORAGE_ROOT missing"))?,
    )?
    .cleanup(Duration::from_secs(3600), 100)
    .await
}
