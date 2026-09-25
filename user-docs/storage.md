# 添付ファイルを保存する

[← ガイドの入口](README.md)

> 低レベルの`kouga-storage` APIは実装済みです。添付用のCLI generatorやcontrollerはまだありません。

DBへ`crates/kouga-storage/migrations`のSQLを適用し、`Storage::local(db, root)`または`Storage::s3(db, s3_store)`を作ります。コンテナでは永続ファイルを外部のS3互換ストレージへ置き、ローカル保存は開発用に使います。

HTTPでは`kouga_http::Multipart<T>::next_field()`から受けたfieldを`field.stream()`で`Storage::save`へ渡します。ファイル全体をメモリへ載せずに、個別の上限と許可形式を指定できます。

```rust,ignore
let file = storage.save(
    Upload {
        owner: current_user,
        filename: field.file_name().unwrap_or("upload"),
        declared_content_type: field.content_type().unwrap_or(""),
        allowed: &[FileKind::Png, FileKind::Jpeg, FileKind::Pdf],
        max_bytes: 10 * 1024 * 1024,
    },
    field.stream(),
).await?;
```

許可形式はPNG/JPEG/PDFです。申告されたContent-Typeだけでなく先頭バイトを照合し、保存キーはUUIDから生成します。元ファイル名は保存パスに使いません。保存直後は未関連状態です。対象レコードへの操作をアプリ側で認可した後、`storage.attach(current_user, file.id, "tasks", task_id).await?`で関連付けます。

取得は`storage.download(current_user, id)`を使います。他人のファイルは404相当の`StorageError::NotFound`です。HTTP応答では返された`file.content_type`、`content_disposition`に加え`X-Content-Type-Options: nosniff`を設定し、streamをボディにします。S3の場合は認可後に`storage.signed_download_url(current_user, id, Duration::from_secs(300))`で短期URLも発行できます。URLを知る人は期限内に取得できるので、ログや公開ページへ載せないでください。

削除は直ちにDBから見えなくなります。ストレージ側の削除が失敗しても`delete_pending`が残り、`storage.cleanup(retention, 100).await?`を定期的なワンショットタスクで再実行できます。未関連ファイルと中断アップロードも期限後に対象になります。S3の未完了multipartには別途bucket lifecycleルールを設定してください。テストには`Storage::in_memory(test_db)`を利用できます。
