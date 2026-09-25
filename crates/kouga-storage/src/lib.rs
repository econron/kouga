//! Private file storage with bounded streaming uploads and database-backed ownership.

use bytes::Bytes;
use futures_util::{Stream, StreamExt, stream::BoxStream};
use http::Method;
use kouga_auth::CurrentUser;
use kouga_db::Db;
use object_store::{
    Attribute, Attributes, ObjectStore, ObjectStoreExt, aws::AmazonS3, buffered::BufWriter,
    local::LocalFileSystem, path::Path, signer::Signer,
};
use sqlx::Row;
use std::{fmt, sync::Arc, time::Duration};
use uuid::Uuid;

pub const SCHEMA_SQL: &str = include_str!("../migrations/20260925000024_create_kouga_files.up.sql");

#[derive(Debug)]
pub enum StorageError {
    Invalid(&'static str),
    NotFound,
    Database(kouga_db::DbError),
    Object(object_store::Error),
    Input(String),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Invalid(_) => "invalid file",
            Self::NotFound => "file not found",
            Self::Database(_) => "database error",
            Self::Object(_) => "storage error",
            Self::Input(_) => "upload stream error",
        };
        f.write_str(message)
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(e) => Some(e),
            Self::Object(e) => Some(e),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for StorageError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(value.into())
    }
}

impl From<object_store::Error> for StorageError {
    fn from(value: object_store::Error) -> Self {
        Self::Object(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Png,
    Jpeg,
    Pdf,
}

impl FileKind {
    pub fn content_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Pdf => "application/pdf",
        }
    }

    fn recognizes(self, first: &[u8]) -> bool {
        match self {
            Self::Png => first.starts_with(b"\x89PNG\r\n\x1a\n"),
            Self::Jpeg => first.starts_with(b"\xff\xd8\xff"),
            Self::Pdf => first.starts_with(b"%PDF-"),
        }
    }
}

pub struct Upload<'a> {
    pub owner: CurrentUser,
    pub filename: &'a str,
    pub declared_content_type: &'a str,
    pub allowed: &'a [FileKind],
    pub max_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct File {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub original_name: String,
    pub content_type: String,
    pub byte_size: i64,
    pub record_type: Option<String>,
    pub record_id: Option<Uuid>,
}

pub struct Download {
    pub file: File,
    pub content_disposition: String,
    pub stream: BoxStream<'static, object_store::Result<Bytes>>,
}

/// Shared API for local files and S3-compatible buckets. Never expose storage keys to callers.
pub struct Storage {
    db: Db,
    store: Arc<dyn ObjectStore>,
    signer: Option<Arc<AmazonS3>>,
}

impl Storage {
    pub fn local(db: Db, root: impl AsRef<std::path::Path>) -> Result<Self, StorageError> {
        let store = LocalFileSystem::new_with_prefix(root).map_err(StorageError::Object)?;
        Ok(Self {
            db,
            store: Arc::new(store),
            signer: None,
        })
    }

    pub fn s3(db: Db, store: AmazonS3) -> Self {
        let store = Arc::new(store);
        Self {
            db,
            store: store.clone(),
            signer: Some(store),
        }
    }

    /// Ephemeral object storage for application tests; metadata still uses the supplied test DB.
    pub fn in_memory(db: Db) -> Self {
        Self {
            db,
            store: Arc::new(object_store::memory::InMemory::new()),
            signer: None,
        }
    }

    pub async fn save<S, E>(&self, upload: Upload<'_>, stream: S) -> Result<File, StorageError>
    where
        S: Stream<Item = Result<Bytes, E>>,
        E: fmt::Display,
    {
        if upload.max_bytes == 0 || upload.max_bytes > i64::MAX as u64 || upload.allowed.is_empty()
        {
            return Err(StorageError::Invalid("invalid upload policy"));
        }
        if upload.filename.is_empty()
            || upload.filename.len() > 255
            || upload
                .filename
                .chars()
                .any(|c| c.is_control() || c == '/' || c == '\\')
        {
            return Err(StorageError::Invalid("invalid filename"));
        }
        let kind = upload
            .allowed
            .iter()
            .copied()
            .find(|kind| kind.content_type() == upload.declared_content_type)
            .ok_or(StorageError::Invalid("content type not allowed"))?;
        let id = Uuid::new_v4();
        let key = format!("files/{}/{}", &id.simple().to_string()[..2], id.simple());
        sqlx::query("INSERT INTO kouga_files (id, owner_id, storage_key, original_name, content_type) VALUES ($1, $2, $3, $4, $5)")
            .bind(id).bind(upload.owner.id).bind(&key).bind(upload.filename).bind(kind.content_type())
            .execute(&self.db).await?;
        let path = Path::from(key.as_str());
        let mut writer =
            BufWriter::with_capacity(self.store.clone(), path.clone(), 5 * 1024 * 1024)
                .with_max_concurrency(2);
        if self.signer.is_some() {
            let mut attributes = Attributes::new();
            attributes.insert(Attribute::ContentType, kind.content_type().into());
            attributes.insert(Attribute::ContentDisposition, "attachment".into());
            attributes.insert(Attribute::CacheControl, "private, no-store".into());
            writer = writer.with_attributes(attributes);
        }
        let mut input = Box::pin(stream);
        let mut prefix = Vec::with_capacity(8);
        let mut size = 0_u64;
        let mut shutdown_started = false;
        let result = async {
            while let Some(chunk) = input.next().await {
                let chunk = chunk.map_err(|e| StorageError::Input(e.to_string()))?;
                size = size
                    .checked_add(chunk.len() as u64)
                    .ok_or(StorageError::Invalid("file too large"))?;
                if size > upload.max_bytes {
                    return Err(StorageError::Invalid("file too large"));
                }
                if prefix.len() < 8 {
                    prefix.extend_from_slice(&chunk[..chunk.len().min(8 - prefix.len())]);
                }
                writer.put(chunk).await?;
            }
            if !kind.recognizes(&prefix) {
                return Err(StorageError::Invalid("file signature mismatch"));
            }
            use tokio::io::AsyncWriteExt;
            shutdown_started = true;
            writer
                .shutdown()
                .await
                .map_err(|e| StorageError::Input(e.to_string()))?;
            Ok::<_, StorageError>(())
        }
        .await;
        if let Err(error) = result {
            // The upload may have partially completed. Keep a DB tombstone if cleanup fails.
            if !shutdown_started {
                let _ = writer.abort().await;
            }
            let _ = sqlx::query("UPDATE kouga_files SET state = 'delete_pending' WHERE id = $1")
                .bind(id)
                .execute(&self.db)
                .await;
            let _ = self.delete_object(id, key).await;
            return Err(error);
        }
        let finalized = sqlx::query("UPDATE kouga_files SET byte_size = $2, state = 'pending' WHERE id = $1 AND state = 'uploading'")
            .bind(id).bind(size as i64).execute(&self.db).await;
        match finalized {
            Ok(result) if result.rows_affected() == 1 => {}
            Ok(_) => return Err(StorageError::Invalid("upload expired")),
            Err(error) => {
                let _ = sqlx::query("UPDATE kouga_files SET state = 'delete_pending' WHERE id = $1 AND state = 'uploading'")
                    .bind(id).execute(&self.db).await;
                return Err(error.into());
            }
        }
        Ok(File {
            id,
            owner_id: upload.owner.id,
            original_name: upload.filename.into(),
            content_type: kind.content_type().into(),
            byte_size: size as i64,
            record_type: None,
            record_id: None,
        })
    }

    /// Links a pending file to an application record. The application must authorize that record separately.
    pub async fn attach(
        &self,
        actor: CurrentUser,
        id: Uuid,
        record_type: &str,
        record_id: Uuid,
    ) -> Result<bool, StorageError> {
        if record_type.is_empty()
            || record_type.len() > 100
            || !record_type
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(StorageError::Invalid("invalid record type"));
        }
        Ok(sqlx::query("UPDATE kouga_files SET state = 'attached', record_type = $3, record_id = $4 WHERE id = $1 AND owner_id = $2 AND state = 'pending'")
            .bind(id).bind(actor.id).bind(record_type).bind(record_id).execute(&self.db).await?.rows_affected() == 1)
    }

    async fn owned(&self, actor: CurrentUser, id: Uuid) -> Result<(File, String), StorageError> {
        let row = sqlx::query("SELECT owner_id, original_name, content_type, byte_size, record_type, record_id, storage_key FROM kouga_files WHERE id = $1 AND owner_id = $2 AND state IN ('pending', 'attached')")
            .bind(id).bind(actor.id).fetch_optional(&self.db).await?.ok_or(StorageError::NotFound)?;
        Ok((
            File {
                id,
                owner_id: row.get("owner_id"),
                original_name: row.get("original_name"),
                content_type: row.get("content_type"),
                byte_size: row.get("byte_size"),
                record_type: row.get("record_type"),
                record_id: row.get("record_id"),
            },
            row.get("storage_key"),
        ))
    }

    /// The caller should set Content-Type, Content-Disposition and X-Content-Type-Options: nosniff.
    pub async fn download(&self, actor: CurrentUser, id: Uuid) -> Result<Download, StorageError> {
        let (file, key) = self.owned(actor, id).await?;
        let result = self.store.get(&Path::from(key.as_str())).await?;
        // Filename is intentionally omitted to avoid header injection and browser path confusion.
        Ok(Download {
            file,
            content_disposition: "attachment".into(),
            stream: result.into_stream(),
        })
    }

    /// Only S3 URLs are signed. Applications must authorize via the actor before issuing one.
    pub async fn signed_download_url(
        &self,
        actor: CurrentUser,
        id: Uuid,
        ttl: Duration,
    ) -> Result<String, StorageError> {
        if ttl.is_zero() || ttl > Duration::from_secs(900) {
            return Err(StorageError::Invalid("invalid signed URL lifetime"));
        }
        let signer = self
            .signer
            .as_ref()
            .ok_or(StorageError::Invalid("signed URL requires S3"))?;
        let (_, key) = self.owned(actor, id).await?;
        Ok(signer
            .signed_url(Method::GET, &Path::from(key.as_str()), ttl)
            .await?
            .to_string())
    }

    /// Hides the file immediately. Cleanup retries any object-store failure.
    pub async fn delete(&self, actor: CurrentUser, id: Uuid) -> Result<(), StorageError> {
        let row = sqlx::query("UPDATE kouga_files SET state = 'delete_pending' WHERE id = $1 AND owner_id = $2 AND state IN ('pending', 'attached') RETURNING storage_key")
            .bind(id).bind(actor.id).fetch_optional(&self.db).await?.ok_or(StorageError::NotFound)?;
        self.delete_object(id, row.get("storage_key")).await
    }

    async fn delete_object(&self, id: Uuid, key: String) -> Result<(), StorageError> {
        match self.store.delete(&Path::from(key.as_str())).await {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => {
                sqlx::query("DELETE FROM kouga_files WHERE id = $1 AND state = 'delete_pending'")
                    .bind(id)
                    .execute(&self.db)
                    .await?;
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Run from `kouga maintenance` or a scheduled one-shot job; bounded by `limit`.
    pub async fn cleanup(&self, older_than: Duration, limit: i64) -> Result<u64, StorageError> {
        let seconds = i64::try_from(older_than.as_secs())
            .map_err(|_| StorageError::Invalid("invalid retention"))?;
        if seconds == 0 || !(1..=1000).contains(&limit) {
            return Err(StorageError::Invalid("invalid cleanup limit"));
        }
        let rows = sqlx::query("WITH stale AS (SELECT id FROM kouga_files WHERE state = 'delete_pending' OR (state IN ('uploading', 'pending') AND created_at < now() - $1 * interval '1 second') ORDER BY created_at LIMIT $2 FOR UPDATE SKIP LOCKED) UPDATE kouga_files AS f SET state = 'delete_pending' FROM stale WHERE f.id = stale.id RETURNING f.id, f.storage_key")
            .bind(seconds).bind(limit).fetch_all(&self.db).await?;
        let mut cleaned = 0;
        for row in rows {
            if self
                .delete_object(row.get("id"), row.get("storage_key"))
                .await
                .is_ok()
            {
                cleaned += 1;
            }
        }
        Ok(cleaned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_are_not_declarations() {
        assert!(FileKind::Png.recognizes(b"\x89PNG\r\n\x1a\ncontent"));
        assert!(!FileKind::Png.recognizes(b"not a png"));
        assert!(FileKind::Jpeg.recognizes(b"\xff\xd8\xffcontent"));
    }
}
