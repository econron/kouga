//! Shared storage configuration for the HTTP and cleanup processes.
use kouga_model::Db;
use kouga_storage::{AmazonS3Builder, Storage, StorageError};

pub fn open(db: Db) -> Result<Storage, StorageError> {
    match std::env::var("BOARD_STORAGE_BACKEND").as_deref() {
        Ok("s3") => {
            let bucket = required("BOARD_S3_BUCKET")?;
            let region = required("BOARD_S3_REGION")?;
            let mut builder = AmazonS3Builder::from_env()
                .with_bucket_name(bucket)
                .with_region(region);
            if let Ok(endpoint) = std::env::var("BOARD_S3_ENDPOINT") {
                builder = builder.with_endpoint(endpoint);
            }
            if std::env::var("BOARD_S3_ALLOW_HTTP").as_deref() == Ok("1") {
                if std::env::var("KOUGA_ENV").as_deref() == Ok("production") {
                    return Err(StorageError::Invalid("insecure S3 endpoint in production"));
                }
                builder = builder.with_allow_http(true);
            }
            if std::env::var("KOUGA_ENV").as_deref() == Ok("production") {
                // Also override AWS_ALLOW_HTTP read by `from_env`.
                builder = builder.with_allow_http(false);
            }
            let store = builder
                .build()
                .map_err(|_| StorageError::Invalid("invalid S3 configuration"))?;
            Ok(Storage::s3(db, store))
        }
        Ok("local") | Err(_) => Storage::local(db, required("BOARD_STORAGE_ROOT")?),
        Ok(_) => Err(StorageError::Invalid("unknown storage backend")),
    }
}

fn required(key: &'static str) -> Result<String, StorageError> {
    std::env::var(key)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or(StorageError::Invalid(key))
}
