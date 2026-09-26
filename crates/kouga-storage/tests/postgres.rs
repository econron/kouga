use bytes::Bytes;
use futures_util::{StreamExt, stream};
use kouga_auth::CurrentUser;
use kouga_storage::{FileKind, Storage, StorageError, Upload};
use kouga_test::TestDb;
use object_store::aws::AmazonS3Builder;
use std::{io, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn local_lifecycle_and_failures() {
    let Ok(url) = std::env::var("KOUGA_TEST_DATABASE_URL") else {
        return;
    };
    let test = TestDb::connect(&url, concat!(env!("CARGO_MANIFEST_DIR"), "/migrations"))
        .await
        .unwrap();
    let root = std::env::temp_dir().join(format!("kouga-storage-test-{}", Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let storage = Storage::local(test.db().clone(), &root).unwrap();
    let owner = CurrentUser { id: Uuid::new_v4() };
    let stranger = CurrentUser { id: Uuid::new_v4() };
    const ALLOWED: &[FileKind] = &[FileKind::Png];
    let policy = || Upload {
        owner,
        filename: "avatar.png",
        declared_content_type: "image/png",
        allowed: ALLOWED,
        max_bytes: 12,
    };
    let png = b"\x89PNG\r\n\x1a\n1234";
    let source = || {
        stream::iter([
            Ok::<_, io::Error>(Bytes::from_static(&png[..4])),
            Ok(Bytes::from_static(&png[4..])),
        ])
    };
    let saved = storage.save(policy(), source()).await.unwrap();
    assert_eq!(saved.byte_size, 12);
    let key: String = sqlx::query_scalar("SELECT storage_key FROM kouga_files WHERE id = $1")
        .bind(saved.id)
        .fetch_one(test.db())
        .await
        .unwrap();
    assert!(!key.contains("avatar.png"));
    assert!(matches!(
        storage.download(stranger, saved.id).await,
        Err(StorageError::NotFound)
    ));
    let download = storage.download(owner, saved.id).await.unwrap();
    assert_eq!(download.content_disposition, "attachment");
    assert_eq!(download.file.content_type, "image/png");
    let chunks: Vec<_> = download.stream.collect().await;
    assert_eq!(
        chunks
            .into_iter()
            .map(Result::unwrap)
            .flat_map(|b| b.to_vec())
            .collect::<Vec<_>>(),
        png
    );
    assert!(
        !storage
            .attach(stranger, saved.id, "users", Uuid::new_v4())
            .await
            .unwrap()
    );
    assert!(
        storage
            .attach(owner, saved.id, "users", Uuid::new_v4())
            .await
            .unwrap()
    );
    assert!(
        !storage
            .attach(owner, saved.id, "users", Uuid::new_v4())
            .await
            .unwrap()
    );
    assert!(matches!(
        storage.delete(stranger, saved.id).await,
        Err(StorageError::NotFound)
    ));
    storage.delete(owner, saved.id).await.unwrap();
    assert!(matches!(
        storage.download(owner, saved.id).await,
        Err(StorageError::NotFound)
    ));

    assert!(matches!(
        storage
            .save(
                Upload {
                    filename: "../escape",
                    ..policy()
                },
                source()
            )
            .await,
        Err(StorageError::Invalid(_))
    ));
    assert!(matches!(
        storage
            .save(
                Upload {
                    max_bytes: 4,
                    ..policy()
                },
                source()
            )
            .await,
        Err(StorageError::TooLarge)
    ));
    assert!(matches!(
        storage
            .save(
                policy(),
                stream::iter([Ok::<_, io::Error>(Bytes::from_static(b"not a png"))])
            )
            .await,
        Err(StorageError::UnsupportedType)
    ));
    let failed = storage
        .save(
            policy(),
            stream::iter([
                Ok(Bytes::from_static(b"\x89PNG")),
                Err::<Bytes, _>(io::Error::other("disconnect")),
            ]),
        )
        .await;
    assert!(matches!(failed, Err(StorageError::Input(_))));
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM kouga_files")
        .fetch_one(test.db())
        .await
        .unwrap();
    assert_eq!(rows, 0, "failed uploads must not leave visible metadata");

    let pending = storage.save(policy(), source()).await.unwrap();
    sqlx::query("UPDATE kouga_files SET created_at = now() - interval '2 days' WHERE id = $1")
        .bind(pending.id)
        .execute(test.db())
        .await
        .unwrap();
    assert_eq!(
        storage
            .cleanup(Duration::from_secs(3600), 10)
            .await
            .unwrap(),
        1
    );
    assert!(matches!(
        storage.download(owner, pending.id).await,
        Err(StorageError::NotFound)
    ));
    let temporary = Storage::in_memory(test.db().clone());
    let memory_file = temporary.save(policy(), source()).await.unwrap();
    temporary.delete(owner, memory_file.id).await.unwrap();
    drop(storage);
    test.close().await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn s3_streaming_and_signed_url() {
    let (Ok(url), Ok(endpoint)) = (
        std::env::var("KOUGA_TEST_DATABASE_URL"),
        std::env::var("KOUGA_TEST_S3_ENDPOINT"),
    ) else {
        return;
    };
    let test = TestDb::connect(&url, concat!(env!("CARGO_MANIFEST_DIR"), "/migrations"))
        .await
        .unwrap();
    let bucket = std::env::var("KOUGA_TEST_S3_BUCKET").unwrap_or_else(|_| "kouga-t24".into());
    let access_key = std::env::var("KOUGA_TEST_S3_ACCESS_KEY_ID").unwrap_or_else(|_| "test".into());
    let secret_key =
        std::env::var("KOUGA_TEST_S3_SECRET_ACCESS_KEY").unwrap_or_else(|_| "test".into());
    let s3 = AmazonS3Builder::new()
        .with_endpoint(&endpoint)
        .with_bucket_name(&bucket)
        .with_region("us-east-1")
        .with_access_key_id(&access_key)
        .with_secret_access_key(&secret_key)
        .with_allow_http(true)
        .build()
        .unwrap();
    let storage = Storage::s3(test.db().clone(), s3);
    let owner = CurrentUser { id: Uuid::new_v4() };
    let stranger = CurrentUser { id: Uuid::new_v4() };
    let mut first = b"\x89PNG\r\n\x1a\n".to_vec();
    first.resize(1024 * 1024, b'x');
    let chunks: Vec<_> = std::iter::once(Bytes::from(first))
        .chain((0..10).map(|_| Bytes::from(vec![b'x'; 1024 * 1024])))
        .map(Ok::<_, io::Error>)
        .collect();
    let saved = storage
        .save(
            Upload {
                owner,
                filename: "large.png",
                declared_content_type: "image/png",
                allowed: &[FileKind::Png],
                max_bytes: 12 * 1024 * 1024,
            },
            stream::iter(chunks),
        )
        .await
        .unwrap();
    assert_eq!(saved.byte_size, 11 * 1024 * 1024);
    assert!(matches!(
        storage
            .signed_download_url(stranger, saved.id, Duration::from_secs(30))
            .await,
        Err(StorageError::NotFound)
    ));
    assert!(matches!(
        storage
            .signed_download_url(owner, saved.id, Duration::from_secs(901))
            .await,
        Err(StorageError::Invalid(_))
    ));
    let url = storage
        .signed_download_url(owner, saved.id, Duration::from_secs(30))
        .await
        .unwrap();
    let response = reqwest::get(url).await.unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/png");
    assert_eq!(response.headers()["content-disposition"], "attachment");
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(response.bytes().await.unwrap().len(), 11 * 1024 * 1024);
    let unavailable = AmazonS3Builder::new()
        .with_endpoint("http://127.0.0.1:1")
        .with_bucket_name(&bucket)
        .with_region("us-east-1")
        .with_access_key_id(&access_key)
        .with_secret_access_key(&secret_key)
        .with_allow_http(true)
        .build()
        .unwrap();
    let broken = Storage::s3(test.db().clone(), unavailable);
    assert!(matches!(
        broken.delete(owner, saved.id).await,
        Err(StorageError::Object(_))
    ));
    assert!(matches!(
        storage.download(owner, saved.id).await,
        Err(StorageError::NotFound)
    ));
    assert_eq!(
        storage
            .cleanup(Duration::from_secs(3600), 10)
            .await
            .unwrap(),
        1
    );
    test.close().await.unwrap();
}
