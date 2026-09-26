# 入力のルールを、Requestに書く

[← ガイドの入口](README.md)

> `Request` derive、`Validated<T>`、`ValidatedQuery<T>`、`Patch<T>`、三つの基本ルールとcustom検証は実装済みです。以下は関心部分の抜粋で、完全に動く生成コードは`kouga generate resource`後の`src/requests/tasks.rs`と`src/controllers/tasks.rs`を参照してください。

Requestは、「この操作で何を受け取り、どの値なら処理してよいか」を表します。controllerは検証済みのRequestを受け取ります。

## 型と、三つの基本ルール

```rust
#[derive(Request)]
pub struct CreateTaskRequest {
    #[validate(length(min = 1, max = 100))]
    pub title: String,

    #[validate(range(min = 1, max = 5))]
    pub priority: Option<i32>,
}
```

`String`は必須の文字列、`Option<i32>`は任意の整数です。型で表せる条件を、別のルールとして書き直す必要はありません。

組み込みの検証は、まず三つです。

| ルール | 使う場面 |
|---|---|
| `length(min, max)` | 文字列の長さ、配列の件数 |
| `range(min, max)` | 数値の範囲。境界を含む |
| `email` | メールアドレスの形式 |

文字列の長さはUnicodeスカラー値で数えます。空文字と空白のみの文字列は別です。勝手にtrimや小文字化はしません。

## 自分のルールは、関数を一つ

空白だけのタイトルも拒否したくなったら、関数を追加します。

```rust
fn not_blank(value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        return Err(ValidationError::new("blank"));
    }
    Ok(())
}
```

フィールドから、その関数を指定します。

```rust
#[validate(length(min = 1, max = 100), custom = not_blank)]
#[schema(description = "1〜100文字。空白だけのタイトルは使用できません")]
pub title: String,
```

クラスや独自traitを実装する必要はありません。最初はRequestと同じファイルへ置き、複数箇所で使うようになったら共通モジュールへ移せます。

組み込みルールはOpenAPIへ自動反映されます。独自関数の中身までは自動変換しないため、`schema(description)`で利用者へ伝える条件を補足します。

## controllerでは、処理を書く

```rust
#[kouga_http::endpoint(operation_id = "tasks.create")]
pub async fn create(
    State(db): State<Db>,
    input: Validated<CreateTaskRequest>,
) -> Result<Created<TaskOutput>, Error> {
    let task = Task::create(&db, NewTask {
        title: input.title.clone(),
        completed: false,
    }).await.map_err(|error| Error(error.into_core()))?;

    Ok(Created::new(
        format!("/tasks/{}", task.id),
        TaskOutput::from(task),
    ))
}
```

ルート登録時に同じ定義から実行handlerとschemaメタデータを保持します。

```rust
let router = Router::<AppState>::new()
    .post("/tasks", create_endpoint())?;
// router.routes() は state なしで登録済み Operation を返す。
// 配信時に router.with_state(state) を呼ぶ。
```

この抜粋ではtitleだけを保存し、completedはfalseに固定しています。`NewTask`はmodel側の作成属性、`TaskOutput`は公開する出力型です。生成された実コードは`completed`を省略時にDB既定値へ任せます。`Created`は201とLocation、`data`形式の本文を返します。

`Validated<T>`を引数に指定すると、検証は呼び出し前に実行されます。controllerに`validate()`や検証エラー用の分岐は書きません。`Validated<T>`は検証済みの値を読み取るための型で、内側を変更することはできません。

クエリも`#[derive(Request)]`した型を`ValidatedQuery<T>`で受けます。型変換失敗は400、検証ルール違反は422となり、controllerは呼ばれません。`Query<T>`を直接使うendpointは登録対象外です。パス変数は`/tasks/{id}`と書き、`Path<T>`と個数・名前を合わせます。無効なパスは登録時にエラーになります。

## 更新は「省略」と「消す」を区別する

```rust
#[derive(Request)]
pub struct UpdateTaskRequest {
    #[validate(length(min = 1, max = 100))]
    pub title: Patch<String>,
    pub description: Patch<Option<String>>,
}
```

descriptionがnullableな列の場合、次の違いをそのまま扱えます。

| JSON | 意味 |
|---|---|
| `{}` | 値を変更しない。変更項目が全くなければ422 |
| `{"description": null}` | descriptionを消す |
| `{"description": "明日まで"}` | descriptionを書き換える |
| `{"title": null}` | titleはnullableではないので400 |

`Patch`は更新でだけ使います。省略された項目の長さ検証などは実行しません。

## 項目をまたぐ条件

開始日と終了日のような条件は、Request全体に関数を指定します。

```rust
#[derive(Request)]
#[validate(custom = valid_period)]
pub struct SearchTasksRequest {
    pub from: Date,
    pub to: Date,
}

fn valid_period(input: &SearchTasksRequest) -> Result<(), ValidationError> {
    if input.to < input.from {
        return Err(ValidationError::new("before_start").field("to"));
    }
    Ok(())
}
```

DBを確認する必要がある検証には`custom_async`を使います。非同期関数は検証contextからDBと認証ユーザーを参照でき、値の不正とDB障害を別のエラーとして返します。通常の型・同期ルールが通った後に実行されます。

一意性や参照整合性は、最後にDB制約でも守ります。Requestの検証後に別の書き込みが入る可能性があるためです。所有者の権限確認は[認可](auth-and-middleware.md)の責務です。

## エラーとAPIドキュメントまで、同じ定義で

JSONの型が不正なら400、検証ルール違反なら422、DBが利用不能なら503です。エラーにはフィールド名と安定したcodeが入り、controllerは実行されません。

```sh
kouga openapi generate
kouga openapi check
```

`generate`で仕様を更新し、`check`で更新漏れを検出できます。フロントエンドへ渡すために、同じ入力項目をYAMLで書き直す必要はありません。

**次へ：[Model](models.md)**
