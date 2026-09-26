# 新規ディレクトリから一周する

[← ガイドの入口](README.md)

この手順はローカルcheckoutからビルドした開発版CLIを使います。2026年9月26日にRust 1.94.0、PostgreSQL、生成アプリの実コードで確認した範囲を示します。各機能の「生成」と「業務アプリとして完成」は別です。後半の添付・WebSocketは低レベルAPIまでの案内で、実際のHTTP画面/ルートを自動生成しません。

## 0. 準備

Rust 1.94.0、PostgreSQL、gRPC用の`protoc`、Dockerイメージを試す場合はBuildKit対応Dockerを用意します。Kouga checkoutでCLIをビルドし、絶対pathをPATHへ追加します。

```sh
cargo +1.94.0 build -p kouga-cli --locked
export KOUGA_SOURCE="$(pwd)"
export PATH="$(pwd)/target/debug:$PATH"
```

以下はKouga checkoutの外に新規ディレクトリを作る例です。生成アプリはCLIビルド元のKouga checkoutを絶対pathで参照します。checkoutを移動するとCargo.tomlのpath依存を更新する必要があります。PostgreSQLの接続情報は自分の環境に置き換えてください。

```sh
cd /tmp
kouga new taskboard
cd taskboard
export DATABASE_URL='postgresql://app:password@localhost:5432/taskboard_development'
export TEST_DATABASE_URL='postgresql://app:password@localhost:5432/taskboard_test'
```

DBユーザーには開発・テストDBを作成する権限が必要です。秘密情報をGitへcommitしないでください。

## 1. CRUDと入力検証

```sh
kouga generate resource Task title:string completed:bool=false
kouga db create
kouga db migrate
DATABASE_URL="$TEST_DATABASE_URL" kouga db create
kouga server
```

別ターミナルから確認します。

```sh
curl -i -X POST http://127.0.0.1:3000/tasks \
  -H 'Content-Type: application/json' \
  -d '{"title":"KougaでAPIを作る"}'
curl -i 'http://127.0.0.1:3000/tasks?page=1&per_page=20'
curl -i -X POST http://127.0.0.1:3000/tasks \
  -H 'Content-Type: application/json' -d '{"title":""}'
```

作成は201と`Location: /tasks/<UUID>`、空タイトルは422と`validation_failed`を返しました。生成された`tests/tasks.rs`は作成・詳細・更新・一覧・削除と検証失敗を一周します。`TEST_DATABASE_URL`なしではDBを使うテストがスキップされるため、必ず専用DBを指定して実行してください。

```sh
cargo +1.94.0 test --test tasks
kouga routes
kouga openapi check
```

開発モードでは`kouga server`が`openapi.yml`を再生成し、`/docs`と`/openapi.yml`を公開します。`KOUGA_ENV=production`では公開しません。

## 2. 認証とmodel

サーバーを止めてから追加します。

```sh
kouga generate auth
kouga generate model Project name:string
kouga db migrate
kouga server
```

`POST /auth/register`へ`{"email":"reader@example.test","password":"long-password-123"}`を送ると、200で`data.user`とBearer tokenが返ることを確認しました。トークンは秘密情報です。`Authorization: Bearer <token>`で`GET /auth/me`を試し、使い終えたら`POST /auth/logout`で失効させます。生成されたTask APIは認証追加後も公開のままです。保護したいrouteには[認証middleware](auth-and-middleware.md)を明示的に付け、一覧・更新・削除まで所有者scopeを実装してください。

`generate model`は`src/models/project.rs`とmigrationを作りますが、HTTP routeは作りません。TaskとProjectを関連づけるにはTask側の`project_id`、外部キーmigration、`#[belongs_to(Project, key = project_id, name = project)]`、controllerでの`task.project(&db).await?`等を手で追加します。宣言だけでDB列や認可は生えません。後述の[追加テスト](#追加テスト関連添付websocket)は同じ新規生成アプリ内に専用の関連テーブルを作り、derive/preloadを実行します。

## 3. queue、メール、worker

```sh
kouga generate job SendWelcome user_id:uuid
kouga generate mailer Welcome
kouga db migrate
```

`crates/contracts/src/jobs/send_welcome.rs`には`send_welcome`という契約、`apps/worker/src/bin/job-worker.rs`には`default` queueのhandlerができます。生成直後のhandlerはjob IDを表示して成功にするだけです。`apps/worker/src/mailers/welcome.rs`の`build(to, from)`も、handlerへ自動では接続されません。実メールを送るにはユーザーをDBから読み、build・SMTP送信と失敗時の再試行/永続失敗判定をworker側に実装してください。HTTP側へSMTP資格情報を置く必要はありません。

生成処理そのものは次で確認できます。`<UUID>`は実在するユーザーIDに置き換えます。管理投入のpayloadは標準入力から渡し、シェル引数やログに秘密情報を含めないでください。

```sh
printf '%s' '{"user_id":"<UUID>"}' | kouga jobs enqueue send_welcome
kouga jobs list
kouga worker --once
```

認証のパスワードリセットは別経路です。`POST /auth/password/reset-request`はmail queueへ登録し、`kouga worker --queue mail --once`が`auth-mail-worker`を起動します。workerの実行環境へ`KOUGA_SMTP_HOST`、`KOUGA_MAIL_FROM`、`KOUGA_RESET_URL`を設定してください。認証付きSMTPなら`KOUGA_SMTP_USER`と`KOUGA_SMTP_PASSWORD`も両方必要です。実SMTPとの結合は生成テストで確認していますが、送信先を持たない読者の環境で自動送信する手順ではありません。ローカルSMTPシンク専用の非TLS設定は[認証ガイド](auth-and-middleware.md)を参照してください。

## 4. 添付とWebSocket

添付は`kouga-storage` crateの`Storage::save`/`attach`/`download`/`cleanup`を利用します。`kouga generate storage`や添付用HTTP routeはありません。ユーザー認可、multipart route、ストレージmigration、S3または開発用ローカル保存先を自分で配線します。[ストレージガイド](storage.md)に所有者・サイズ・形式・削除後清掃の契約があります。後述の追加テストでは、同じ新規生成アプリからローカルストレージと実DBを使い、保存・関連付け・他人の拒否・取得・削除を再現します。無編集の生成アプリに`curl`アップロード routeはありません。

WebSocketは認証生成後に次を実行できます。

```sh
kouga generate channel Events
kouga db migrate
KOUGA_CHANNEL_ORIGIN=http://localhost:3000 cargo run --bin channel-events
```

`src/bin/channel-events.rs`に別入口を生成します。生成policyはすべての購読/配信を拒否します。業務用policyを記述してから、認証済みHTTPでticketを取得し、`/_kouga/ws`へ接続してください。通知はbest-effortで、履歴はHTTPから再取得します。ticket取得と購読の例は[WebSocketガイド](websocket.md)にあります。後述の追加テストでは許可したactorだけの購読と別インスタンスからの配信を実行します。

## 追加テスト：関連・添付・WebSocket

Kouga checkoutに含まれる[追加テスト](examples/advanced.rs)を、上で生成したアプリの`tests/advanced.rs`へコピーします。これは設計スケッチではなく、生成アプリ内でコンパイルして実DB・ローカルファイル・実WebSocket接続を使うコードです。`KOUGA_SOURCE`は準備段階で設定したKouga checkoutの絶対pathです。

```sh
cargo add --dev --path "$KOUGA_SOURCE/crates/kouga-storage"
cargo add --dev bytes@1 futures-util@0.3 tokio-tungstenite@0.29
cp "$KOUGA_SOURCE/user-docs/examples/advanced.rs" tests/advanced.rs
cargo +1.94.0 test --test advanced -- --test-threads=1
```

`TEST_DATABASE_URL`が必要です。`TestDb`はテストごとに別schemaを作り、生成済みmigrationを適用します。関連テストは専用の外部キー付きProject/Taskテーブルを作り、`belongs_to`とpreloadを確認します。添付はPNGの署名・所有者・削除を確認します。WebSocketはticketとOriginを使って接続し、別の`Channel`インスタンスからPostgreSQL経由で通知します。生成された業務APIへこれらを自動登録するものではありません。

## 5. HTTPとgRPC、OTel

```sh
kouga add grpc
kouga add otel
cargo +1.94.0 check --workspace
```

別々のターミナルでHTTPとgRPCを起動できます。

```sh
kouga server --api http
kouga server --api grpc
cargo run -p taskboard-grpc --bin grpc-client -- Kouga
```

最後のクライアントは`Hello, Kouga!`を返します。HTTP側の`GET /greet/Kouga`とgRPC `Greeting.Greet`は`crates/domain`の同じ関数を呼びます。これは生成されたGreetingのunaryサンプルであり、Task CRUDをgRPCへ自動公開しません。gRPC独自バイナリへOTelを入れる場合も手動統合です。

`kouga add otel`は標準HTTP/workerの起動・終了処理に追加します。Collectorを使う場合だけ`OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318`等を実行時に設定します。送信先がない場合、外部送信はしません。traces/metricsは既定で有効、logsのOTLP送信は`OTEL_LOGS_EXPORTER=otlp`で明示します。独自metricを一つ記録するなら、`cargo add opentelemetry@0.33.0`後、生成済み`src/bin/server.rs`の`Telemetry::init(config)?`直後へ次を加えます。受信先に実データが送られるかはCollector側で確認してください。

```rust
opentelemetry::global::meter("taskboard")
    .u64_counter("taskboard.startups")
    .build()
    .add(1, &[]);
```

`SIGTERM`で正常終了したローカルHTTPアプリから、OTLP/HTTPの`/v1/traces`・`/v1/metrics`・`/v1/logs`がmock Collectorへ届くことを確認しました。独自gRPCバイナリは手動統合です。[OTelガイド](observability.md)も参照してください。

## 6. イメージにする

すべての入口を追加した後で実行します。

```sh
kouga dockerfile
cargo +1.94.0 generate-lockfile
docker build --build-context kouga=/path/to/kouga --target http -t taskboard-http .
docker build --build-context kouga=/path/to/kouga --target grpc -t taskboard-grpc .
docker build --build-context kouga=/path/to/kouga --target worker -t taskboard-worker .
docker build --build-context kouga=/path/to/kouga --target mail-worker -t taskboard-mail-worker .
docker build --build-context kouga=/path/to/kouga --target admin -t taskboard-admin .
```

`/path/to/kouga`はアプリを生成したKouga checkoutへ置き換えます。HTTP/gRPC/ジョブworker/メールworker/管理処理は別の最終イメージです。生成済みDockerfileを後から自動上書きしないため、新しい入口を追加した場合は差分を確認して手動で更新します。実DB/SMTPに到達できるか、PORT・終了猶予・証明書を含む配備条件は[デプロイガイド](deployment.md)にあります。ここではクラウドへのpush/deployはしません。

## 検証の境界

この通し例で実際に新規生成・型検査・実DB CRUD/認証・追加テストで確認したものと、既存の結合テストで確認したものを区別しています。関連の既存Task APIへの配線、添付HTTP route、生成channelの業務policy、独自ジョブのSMTP送信、gRPCの業務RPC、実クラウド配備は読者のアプリ固有の実装で、無編集の生成アプリに含まれません。性能・イメージサイズの横断評価は[T34](../docs/development-tasks.md)で扱います。
