# ログインが必要なAPIを作る

[← ガイドの入口](README.md)

> ドキュメント・プレビュー。認証用のルート名、登録ヘルパー、コードは公開APIの案です。

標準の認証を生成して、保護したいルートへ付けます。独自middlewareを書くのは、追加の振る舞いが必要になってからで構いません。

## 認証を追加する

```sh
kouga generate auth
kouga db migrate
```

User、トークン保存用のmigration、認証middleware、Request、controller、テストを生成します。メールによるパスワードリセットには、[メール用worker](jobs-and-mail.md)の設定も必要です。

生成時に用意するルート案です。実際に公開するものは`routes.rs`で選びます。

| 操作 | ルート |
|---|---|
| 登録 | `POST /auth/register` |
| ログイン | `POST /auth/login` |
| ログアウト | `POST /auth/logout` |
| 現在のユーザー | `GET /auth/me` |
| パスワードリセット申請 | `POST /auth/password/reset-request` |
| パスワードリセット | `POST /auth/password/reset` |

標準はBearerトークンです。ログインで得たトークンを、`Authorization: Bearer <token>`へ付けます。期限切れ・失効済み・未指定は401です。

## 保護する範囲を、ルートで決める

```rust
router.group("/tasks")
    .middleware(bearer_auth(auth::require_user))
    .resource(tasks::routes());
```

これで、そのグループでは認証がRequest検証より先に実行されます。認証情報はOpenAPIにも反映され、`/docs`でトークンを入力して試せます。

公開APIは、このグループの外へ登録します。

## 「ログインしている」と「このデータを見てよい」は別

Taskにowner_idとその外部キーを追加した場合の、単件取得の抜粋です。

```rust
let task = Task::query()
    .filter(Task::id.eq(task_id))
    .filter(Task::owner_id.eq(current_user.id))
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(Error::not_found)?;
```

認証middlewareが設定した`CurrentUser`をcontrollerの引数で受け取り、そのユーザーの範囲から取得します。この例では、他人のデータも未存在も404にしています。

作成時のowner_idはリクエストに任せず、current_userから設定します。一覧・関連取得・更新・削除にも同じ範囲を適用してください。複数の操作で共有する権限条件はpolicyへまとめられます。

## 独自middlewareは、非同期関数

```rust
pub async fn add_api_version(
    request: HttpRequest,
    next: Next,
) -> Result<Response, Error> {
    let mut response = next.run(request).await?;
    response.headers_mut().insert(
        "x-api-version",
        HeaderValue::from_static("1"),
    );
    Ok(response)
}
```

ルートやグループへ登録します。

```rust
router.group("/tasks")
    .middleware(add_api_version)
    .middleware(bearer_auth(auth::require_user))
    .resource(tasks::routes());
```

登録順に入り、戻る処理は逆順です。`next.run(request)`を呼べば次へ進み、呼ばずにエラーやレスポンスを返せばそこで終了します。上例のヘッダー追加は成功レスポンスが対象で、エラーは共通のエラー処理へ渡します。

## 認証middlewareの中身

標準認証も、考え方は同じです。

```rust
pub async fn require_user(
    mut request: HttpRequest,
    next: Next,
) -> Result<Response, Error> {
    let token = bearer_token(request.headers())?;
    let user = authenticate(request.state().db(), token).await?;
    request.extensions_mut().insert(CurrentUser::from(user));
    next.run(request).await
}
```

ヘッダーからトークンを取り、期限と失効を確認し、型付きのユーザー情報をrequestへ格納します。認証失敗は401、認証DBの障害は503です。認証できない原因を混同しません。

`HttpRequest`はHTTPそのもの、`CreateTaskRequest`などのRequest型は検証する入力です。自分の認証処理に差し替える場合も、`bearer_auth`で登録すれば実行時の認証とOpenAPIの説明を同じ場所に置けます。

**次へ：[ジョブとメール](jobs-and-mail.md)**
