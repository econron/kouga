# ログインが必要なAPIを作る

[← ガイドの入口](README.md)

`kouga generate auth`は、User model・migration・Request・controller・認証ルート・メール用worker・テストを生成します。Kouga checkoutからビルドしたCLIで利用できます。

## 認証を追加する

```sh
kouga generate auth
kouga db migrate
```

生成された`src/auth.rs`は、次のルートを`src/lib.rs`へ登録します。

| 操作 | ルート |
|---|---|
| 登録 | `POST /auth/register` |
| ログイン | `POST /auth/login` |
| ログアウト | `POST /auth/logout` |
| 現在のユーザー | `GET /auth/me` |
| リセット申請 | `POST /auth/password/reset-request` |
| パスワードリセット | `POST /auth/password/reset` |

登録・ログインの成功時は`data.token`と`data.user`を返します。Bearer tokenは`Authorization: Bearer <token>`で送ります。`/auth/me`と`/auth/logout`には認証middlewareが付いています。未指定・期限切れ・失効済みは401、認証DBの障害は503です。ログアウトはそのユーザーの全セッションを失効させます。tokenはレスポンス例やログへ貼らず、秘密情報として扱ってください。

[新規生成アプリの通し例](tutorial.md#2-認証とmodel)では登録と実DBテストを確認できます。生成されたTask CRUDは、認証を追加しても公開のままです。

## 自分のルートを保護する

生成された`src/auth.rs`の実際の登録方法は次の形です。

```rust
let protected = kouga_http::auth::require_bearer(|db: &Db| db);
router
    .get("/auth/me", me_endpoint().middleware(protected))
    .expect("auth route")
```

Taskを保護する場合は`src/controllers/tasks.rs`の`routes`関数を編集し、一覧・作成・詳細・更新・削除のすべてのendpointへ`.middleware(protected.clone())`を付けます。ひとつだけ保護しても、ほかのルートは公開されたままです。認証情報は`Extension<CurrentUser>`でcontrollerへ受け取れます。

「ログイン済み」と「この行を見てよい」は別です。所有者別の取得・一覧には`kouga_auth::owned_by`等でscopeをかけ、作成時のowner_idはRequestからではなく認証主体から設定します。更新・削除は同一transaction内で所有者scope付きの`for_update`取得を行ってから操作してください。関連queryにも同じ認可条件が必要です。

## 独自middleware

```sh
kouga generate middleware Audit
```

これにより`src/middlewares/audit.rs`へ次の最小関数ができます。CLIは`src/lib.rs`へmoduleを追加しますが、ルートへの適用は利用者が明示します。

```rust
use kouga_http::{Error, HttpRequest, Next};
use axum::response::Response;

pub async fn audit<S: Clone + Send + Sync + 'static>(
    request: HttpRequest<S>,
    next: Next<S>,
) -> Result<Response, Error> {
    next.run(request).await
}
```

`router.middleware(middlewares::audit::audit)`は、その後に登録するルートへ適用します。個別のendpointなら`endpoint.middleware(...)`を使います。`next.run(request)`を呼ばずにエラーを返せばそこで終了します。middlewareはRequestの検証より前に実行されます。

## メールによるパスワードリセット

リセット申請は、存在するメールアドレスにも存在しないメールアドレスにも同じ応答を返します。HTTP側は送信ジョブを登録し、平文tokenはメールにだけ載せます。DBにはハッシュを保存します。

別プロセスのメールworkerへ`DATABASE_URL`、`KOUGA_SMTP_HOST`、`KOUGA_MAIL_FROM`、`KOUGA_RESET_URL`を渡します。SMTP認証には`KOUGA_SMTP_USER`と`KOUGA_SMTP_PASSWORD`を両方渡してください。

```sh
kouga worker --queue mail --once
# または cargo run -p taskboard-worker --bin auth-mail-worker -- --once
```

開発用のローカルSMTPシンクに限り、`KOUGA_ENV=test`、`KOUGA_SMTP_HOST=127.0.0.1`、`KOUGA_SMTP_LOCAL=1`と`KOUGA_SMTP_PORT`を指定できます。本番ではTLS証明書検証を有効にし、SMTP秘密情報をHTTPコンテナへ渡さないでください。生成されたworkerの実SMTPシンクテストは`TEST_DATABASE_URL`を指定した`cargo test --workspace`で実行されます。

## レート制限

生成された認証ルートはPostgreSQL共有の`kouga-cache` rate limiterを使い、IPとメールアドレス由来のキーに制限をかけます。上限超過は429と`Retry-After`、DB障害時は503で、認証を無条件に通過させません。転送ヘッダーの送信元IPを使う場合は、`HttpOptions::trusted_proxies`へ信頼できる直近プロキシを明示してください。

**次へ：[ジョブとメール](jobs-and-mail.md)**
