# 最初のAPIを作る

[← ガイドの入口](README.md)

> Kouga CLIはまだ配布されていません。以下はローカルcheckoutからCLIをビルドして試せるプレビューです。

リポジトリ内で`cargo build -p kouga-cli`を実行し、`target/debug/kouga`をPATHに入れてください。生成アプリはローカルのKougaソースへのpath依存を持ちます。gRPCの入口追加にはビルド時の`protoc`も必要です。

タスクを登録して、一覧を取得するAPIを作ります。このガイドでは、まずローカルで動く一周を体験します。

## 準備するもの

Rust、PostgreSQL、Kouga CLIを使用します。Rustの対応バージョンとCLIのインストール方法はリリース時に案内します。ここではPostgreSQLが起動しており、開発用DBを作成できるユーザーがある前提です。

## アプリを作る

```sh
kouga new taskboard
cd taskboard
```

接続先を環境変数に設定します。次はローカル開発用の例です。

```sh
export DATABASE_URL='postgresql://app:password@localhost:5432/taskboard_development'
```

続いて、タスクのresourceを生成します。

```sh
kouga generate resource Task title:string completed:bool=false
```

`completed:bool=false`は、完了状態の既定値をfalseにする指定です。コマンドは次の編集場所を用意します。

```text
src/models/task.rs                         保存・取得するデータ
src/requests/tasks.rs                      作成・更新の入力
src/controllers/tasks.rs                   HTTPの処理と公開する出力
src/lib.rs                                 ルートへの登録
migrations/..._create_tasks.up.sql         テーブルの作成
migrations/..._create_tasks.down.sql       テーブルの削除
tests/tasks.rs                             リクエストテスト
```

UUIDのidと作成・更新日時は標準で付きます。ファイルを後から編集して構いません。再生成で編集済みファイルを勝手に上書きすることもありません。

## DBを用意して、起動する

```sh
kouga db create
kouga db migrate
kouga server
```

起動時の案内例です。

```text
Kouga · development
API       http://localhost:3000
Docs      http://localhost:3000/docs
OpenAPI   openapi.yml updated
```

`http://localhost:3000/docs`で、生成されたAPIの入力項目とレスポンスを確認できます。

## タスクを登録する

別のターミナルでリクエストを送ります。

```sh
curl -i http://localhost:3000/tasks \
  -H 'Content-Type: application/json' \
  -d '{"title":"KougaでAPIを作る"}'
```

`201 Created`と、新しいタスクのURLを示す`Location`ヘッダーが返ります。レスポンス例です。

```json
{
  "data": {
    "id": "9ba138ba-7e51-4dc0-9aa0-e1ea87366dd2",
    "title": "KougaでAPIを作る",
    "completed": false
  }
}
```

一覧も取得できます。

```sh
curl 'http://localhost:3000/tasks?page=1&per_page=20'
```

一覧は`data`の配列と、`page`・`per_page`・`has_next`を持つ`meta`を返します。

## タイトルのルールを変える

`src/requests/tasks.rs`の作成用Requestへ、長さの上限を追加します。生成時から空文字は禁止されています。

```rust
#[derive(Request)]
pub struct CreateTaskRequest {
    #[validate(length(min = 1, max = 100))]
    pub title: String,

    pub completed: Option<bool>,
}
```

`completed`を省略した場合、生成controllerはDBの既定値falseを使います。titleの長さはcontrollerの前に検証されます。更新にも同じ上限を適用したい場合は、更新用Requestのtitleにも同じルールを指定します。

サーバーを停止し、`kouga server`で再起動します。変更をビルドし、`openapi.yml`も更新します。

```sh
curl -i http://localhost:3000/tasks \
  -H 'Content-Type: application/json' \
  -d '{"title":""}'
```

生成直後から空文字には`422 Unprocessable Content`が返り、タスクは作成されません。上限を編集した後は、101文字のタイトルでも同じ応答になります。

```json
{
  "error": {
    "code": "validation_failed",
    "message": "Validation failed",
    "details": [{"field": "title", "code": "length"}]
  },
  "request_id": "7b1516c1-94d7-4936-ac8f-a72c38f9ee25"
}
```

## 生成されたAPIを確認する

```sh
kouga routes
```

```text
GET     /tasks        tasks.index
POST    /tasks        tasks.create
GET     /tasks/{id}   tasks.show
PATCH   /tasks/{id}   tasks.update
DELETE  /tasks/{id}   tasks.destroy
```

不要なactionは、`src/controllers/tasks.rs`の`routes`関数から外せます。例えば一覧と詳細だけなら次の2行を残します。

```rust
router
    .get("/tasks", index_endpoint()).expect("generated route")
    .get("/tasks/{id}", show_endpoint()).expect("generated route")
```

## テストする

生成されたリクエストテストは、テスト専用DBを使用します。開発用とは別の接続先を設定して実行します。

```sh
export TEST_DATABASE_URL='postgresql://app:password@localhost:5432/taskboard_test'
cargo test
```

テスト支援が専用DBにmigrationを適用し、テストごとのデータを分離します。ここまでで、登録・一覧・入力エラーというAPIの一周が揃います。

このサンプルのルートはローカル確認用に公開されています。ユーザーごとにデータを分けるアプリは、公開前に[認証と認可](auth-and-middleware.md)を追加してください。

**次へ：[Requestとvalidation](requests.md)**
