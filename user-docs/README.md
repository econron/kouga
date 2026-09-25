# Kouga

## 作りたいAPIから、書き始めよう。

Rustの型で入力を決める。普通の関数で処理を書く。APIドキュメントと、必要なコードだけを含むDockerイメージが、その先につながる。

Kougaは、APIアプリケーションのためのRustフレームワークです。ルーティング、入力検証、DB、認証、ジョブを、一つの使い方に揃えます。

**外部向けのHTTP/JSONも、サービス間のgRPC/Protobufも、一つのプロジェクトで。** 業務処理とmodelを共有し、入口ごとに小さなイメージとして配備できます。

> **ドキュメント・プレビュー** — Kougaは設計中です。このディレクトリのコマンド・コード・出力は、目指す利用体験を検討するための案です。現在実行できる製品や、提供済みの機能を示すものではありません。

**[最初のAPIを作る →](getting-started.md)** · [設計中のこと](preview.md)

### 最初に、動く一周を。

```sh
kouga new taskboard
cd taskboard
kouga generate resource Task title:string completed:bool=false
kouga db create
kouga db migrate
kouga server
```

PostgreSQLの接続先を設定したら、タスクを作成・取得・更新・削除できるAPIが起動します。`/docs`を開けば、そのAPIをブラウザから試せます。生成されたコードは、すべて自分のアプリのコードとして編集できます。

### 入力のルールを、一か所に。

```rust
#[derive(Request)]
pub struct CreateTaskRequest {
    #[validate(length(min = 1, max = 100))]
    pub title: String,
}
```

このルールは、controllerの前で実行されます。空のタイトルなら422。正しい入力だけがcontrollerに届きます。`openapi.yml`にも、同じ長さの制約が反映されます。

独自のルールが必要になったら、普通のRust関数を一つ書いて指定します。

**[入力を検証する →](requests.md)**

### DBの操作を、読める形で。

関連を読むときも、トランザクションを使うときも、共通のmodelを利用します。

```rust
let task = Task::find(&db, task_id).await?;
let tasks = Task::query()
    .filter(Task::completed.eq(false))
    .order_by(Task::id.asc())
    .limit(20)
    .fetch_all(&db)
    .await?;
```

関連を読むときも、トランザクションを使うときも、DBへアクセスする場所がコードに現れます。migrationはSQL。普段の操作をmodelで書き、細かな制御が必要なところではSQLを使えます。

**[データと関連を扱う →](models.md)** · **[DBを変更する →](migrations.md)**

### メールはworkerへ。HTTPは身軽に。

```sh
docker build --target http -t taskboard-http .
docker build --target worker -t taskboard-worker .
```

同じリポジトリから、役割ごとにイメージを作れます。HTTP側が必要とするのはジョブの引数と投入処理。SMTPやメールテンプレートは、送信を担当するworker側に置きます。

**[ジョブとメールを使う →](jobs-and-mail.md)** · **[コンテナとして届ける →](deployment.md)**

### HTTPで始めて、gRPCを足せる。

```sh
kouga add grpc
```

HTTP APIを残したまま、同じプロジェクトへgRPCの入口を追加できます。model、業務ルール、トランザクション、ジョブの投入処理を共有し、それぞれのクライアントへ提供します。

同居するのはプロジェクトと業務コードです。HTTPとgRPCは別の実行ファイル・Dockerイメージにでき、個別に更新・スケールできます。HTTPだけを使う場合、gRPCの依存は入りません。

**[HTTPとgRPCを一緒に使う →](http-and-grpc.md)**

### 覚えることを、少なく。

最初に触るのは、ルート、Request、controller、modelです。

```text
middleware → Requestの検証 → controller → model
```

認証はルートへ追加し、独自middlewareは非同期関数で書きます。ジョブが必要になったところでworkerを追加できます。すべての機能を、最初から自分のアプリへ組み込む必要はありません。

### ガイド

| やりたいこと | 読むページ |
|---|---|
| APIを作り、リクエストを送る | [最初のAPI](getting-started.md) |
| HTTPとgRPCを同じプロジェクトで提供する | [HTTPとgRPCの同居](http-and-grpc.md) |
| 必須項目・独自ルール・PATCHを扱う | [Requestとvalidation](requests.md) |
| CRUD・関連・トランザクションを使う | [Model](models.md) |
| テーブルや列を変更する | [Migration](migrations.md) |
| ログインが必要なAPIを作る | [認証とmiddleware](auth-and-middleware.md) |
| 添付ファイルを保存・認可・清掃する | [ストレージ](storage.md) |
| メールや時間のかかる処理を外へ出す | [ジョブとメール](jobs-and-mail.md) |
| Dockerイメージを作る | [デプロイ](deployment.md) |
| ログを追加し、HTTPからworkerまで追跡する | [ログとOpenTelemetry](observability.md) |
| コマンドやエラーを確認する | [リファレンス](reference.md) |

### どんなアプリに向いている？

フロントエンドやモバイルアプリのバックエンド、業務向けのJSON API、gRPCによるサービス間通信、DBとバックグラウンド処理を使うサービスを想定しています。

初版はPostgreSQLを使います。HTML画面の生成、Railsアプリのそのままの移行、すべてのクラウドでの同一動作は対象にしていません。性能とイメージサイズは、実装後の測定結果を公開する予定です。

まずは[最初のAPI](getting-started.md)を読んで、普段の仕事をこの書き方で進めたいか、確かめてください。
