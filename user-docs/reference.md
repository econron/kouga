# コマンドとよくある疑問

[← ガイドの入口](README.md)

> 開発プレビュー。以下はローカルcheckoutからビルドしたCLIで利用できます。DB管理のrollback以降、配布済みバイナリ、Docker配備は後続タスクです。

## よく使うコマンド

| したいこと | コマンド |
|---|---|
| アプリを作る | `kouga new taskboard` |
| gRPCでアプリを作る | `kouga new taskboard --api grpc` |
| HTTPと同居するgRPCの入口を追加 | `kouga add grpc` |
| gRPCアプリへHTTPの入口を追加 | `kouga add http` |
| gRPCサーバーを起動 | `kouga server --api grpc` |
| 開発サーバーを起動 | `kouga server` |
| ルートを確認 | `kouga routes` |
| CRUD APIを生成 | `kouga generate resource Task title:string completed:bool=false` |
| modelを生成 | `kouga generate model Project name:string` |
| migrationを生成 | `kouga generate migration add_description_to_tasks` |
| 開発DBを作成 | `kouga db create` |
| migrationを適用 | `kouga db migrate` |
| 適用状況を確認 | `kouga db status` |
| 直前の変更を戻す | `kouga db rollback --steps 1` |
| 認証を追加 | `kouga generate auth` |
| ジョブを追加 | `kouga generate job SendWelcomeEmail user_id:uuid` |
| メールを追加 | `kouga generate mailer Welcome` |
| middlewareを追加 | `kouga generate middleware Audit` |
| WebSocket入口を追加 | `kouga generate channel Events`（認証生成後） |
| 常駐workerを起動 | `kouga worker --queue mail` |
| ワンショットで1件処理 | `kouga worker --queue mail --once` |
| ジョブを一覧・詳細確認 | `kouga jobs list` / `kouga jobs show <UUID>` |
| ジョブを再試行・中止 | `kouga jobs retry <UUID>` / `kouga jobs cancel <UUID>` |
| ジョブを管理者投入 | `printf '%s' '{"user_id":"..."}' \| kouga jobs enqueue send_welcome` |
| 期限切れ行を清掃 | `kouga maintenance` |
| DBのSQLコンソール | `kouga console`（`psql`が必要） |
| 登録済み処理を実行 | `kouga runner <task>` |
| OpenAPIを生成 | `kouga openapi generate` |
| OpenAPIの更新漏れを確認 | `kouga openapi check` |
| OpenTelemetryを追加 | `kouga add otel` |
| テスト | `cargo test` |

`runner <task>`は`src/bin/task-<task>.rs`を実行します。`jobs enqueue`のpayloadは標準入力から読み、引数や一覧・詳細へ表示しません。未登録のジョブ名はworkerで隔離されるため、生成済みの契約名を指定してください。`maintenance`はDBの期限切れcache・token等を清掃します。ストレージ実体は設定済み`Storage::cleanup`をアプリのrunnerから呼びます。生成channelの認可は初期状態ですべて拒否するため、購読を有効にする前に業務用policyを記述してください。

## どこを編集する？

| 変えたいもの | 編集場所 |
|---|---|
| APIのURL・公開する操作 | `apps/http/src/routes.rs` |
| 入力項目・入力ルール | `apps/http/src/requests/` |
| HTTPの処理・出力 | `apps/http/src/controllers/` |
| DB操作・業務ルール | `crates/domain/src/models/` |
| DB構造 | `migrations/` |
| ジョブ名・引数 | `src/jobs/` |
| ジョブの実行処理・登録 | `src/bin/job-worker.rs` |
| メール本文の組み立て | `src/mailers/` |

生成直後にすべてのディレクトリが必要なわけではありません。worker用のファイルは、その機能を追加したときに作ります。

## エラーの読み方

| ステータス | まず確認すること |
|---|---|
| 400 | JSONの形式、型、未知のフィールド、nullの指定 |
| 401 | Bearerトークンの有無・期限・失効 |
| 403 | その操作が利用者に許可されているか |
| 404 | URL・ID・取得可能なデータの範囲 |
| 409 | 現在の状態と操作の競合 |
| 422 | `error.details`にある入力ルール違反 |
| 429 | リクエスト頻度。Retry-Afterも確認 |
| 500 | request IDに対応するサーバーログ |
| 503 | DBなどの依存先、または受付上限 |
| 504 | リクエスト処理のtimeout |

エラーの判定にはmessageの文章ではなくcodeを使います。問い合わせや調査にはrequest IDを添えられます。

## modelにもvalidationを書く？

HTTP入力のルールはRequestへ書きます。workerからも守る業務条件はmodelの操作へ置き、一意性や外部キーはDBでも保証します。すべてのルールを二重に書く設計ではありません。

## validatorのクラスを作る？

必要ありません。基本ルールで足りなければ、普通の関数を`custom`へ指定します。DB参照が必要なときだけ、非同期の`custom_async`を使います。

## OpenAPIを別に書く？

ルート・Request・出力型から生成します。独自検証など自動で分からない情報だけ、実装の近くへ説明を添えます。実行時の検証と生成する仕様で、基本ルールの定義を共有します。

## 全部の機能がHTTPイメージに入る？

入りません。HTTP側がメールジョブを投入するだけなら、SMTPとテンプレートはworker側に置けます。機能の有無を実行時のフラグだけで切り替える構成にはしません。

## カスタムログやOpenTelemetryを使える？

ログは`tracing::info!`、独自の処理時間は`#[tracing::instrument(skip_all)]`で追加する設計です。OTelを任意で組み込み、HTTPとworkerのtrace・metrics・logsを外部へ送れます。導入方法は[ログとOpenTelemetry](observability.md)を参照してください。

## 最初からworkerや追加のサーバーが必要？

最初のCRUD APIはHTTPとPostgreSQLで作れます。ジョブの実行が必要になったらworkerを追加します。queueの標準保存先もPostgreSQLです。

## `kouga console`でRustの式を書ける？

初版のconsoleはアプリの接続設定を使うSQLコンソールです。Rustコードの対話評価は提供しません。アプリの処理を実行する場合は、登録したtaskをrunnerから呼びます。

## キャッシュ、アップロード、WebSocketは？

いずれも初版の計画に含みます。キャッシュはTTL付き、アップロードは非公開を標準とし、WebSocketは購読時にも認可します。これらの利用者向けAPIと詳しいガイドは、今回の叩き台ではまだ定義していません。

## もう使える？

まだ使えません。この文書から使い心地を検討している段階です。インストール先、安定版、性能・サイズの実測値は、実装と検証の後に案内します。

**[最初のAPIへ戻る](getting-started.md)** · **[設計案の確認ポイント](preview.md)**
