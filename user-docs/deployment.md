# 必要なコードを、必要な場所へ

[← ガイドの入口](README.md)

> `kouga dockerfile`はローカルにDockerfileを生成するだけです。クラウドへの配備は行いません。

Kougaの配布単位はDockerイメージです。HTTPとworkerを別々にビルドし、別々の台数・環境で実行できます。

[gRPCを追加](http-and-grpc.md)したプロジェクトでは、gRPCも独立したイメージにできます。業務コードを共有したまま、入口ごとに配備します。

## イメージを作る

```sh
kouga dockerfile
cargo generate-lockfile
docker build --build-context kouga=/path/to/kouga --target http -t taskboard-http .
docker build --build-context kouga=/path/to/kouga --target worker -t taskboard-worker .
```

`kouga dockerfile`は、入口とworkerを追加したあとに実行してください。`http`はHTTP入口、`worker`はジョブ機能、`grpc`はgRPC入口、`admin`はmigration生成時だけ出ます。ジョブworkerと認証メールworkerが両方ある場合は、後者を`mail-worker` targetとして生成します。生成済みDockerfileや`.dockerignore`は上書きしません。

`kouga` named contextには、このアプリを生成したKougaソースcheckoutを指定します。生成アプリのローカルpath依存はビルドステージ内だけで`/kouga`へ置き換えます。Dockerfileと`Cargo.lock`をアプリとともに管理し、ビルド時に対応するKougaソースを渡してください。

| イメージ | 入れるもの |
|---|---|
| HTTP | router、Request、controller、model、ジョブ投入処理 |
| gRPC（追加時） | Protobufの型、gRPC handler、共通の業務処理・model |
| worker | model、ジョブ実行処理、必要なmailer・テンプレート |

ビルドに使ったRustツールチェーンやソース一式は、最終イメージへ含めません。証明書や必要な動的ライブラリは残します。イメージのサイズは、依存とアプリの内容を含めて測定します。

## 実行時の設定を渡す

HTTPを起動する例です。DB接続先などを記載した`production.env`はイメージにもGitにも含めません。

```sh
docker run --rm \
  --read-only --tmpfs /tmp \
  --env-file production.env \
  -e PORT=8080 \
  -p 8080:8080 \
  taskboard-http
```

workerは別コンテナで起動し、同じ外部PostgreSQLを`DATABASE_URL`に指定します。メールworkerには`KOUGA_SMTP_HOST`、`KOUGA_MAIL_FROM`、`KOUGA_RESET_URL`などを実行時に渡します。ワンショット実行は`docker run ... taskboard-worker --once`です。管理targetは`docker run ... taskboard-admin`でmigrationを実行し、HTTP起動時には実行しません。

SMTPのTLS検証は有効のままです。社内CAなどを信頼させる場合は、CAのPEMファイルを読み取り専用でマウントし、メールworkerに`SSL_CERT_FILE`でそのパスを指定します。信頼できない証明書を許容する設定はありません。

イメージの入口がビルド済みバイナリを起動します。本番コンテナ内で`kouga server`やCargoを実行する必要はありません。

コンテナから到達可能なPostgreSQLを指定してください。コンテナ内の`localhost`は、そのコンテナ自身です。

## 配備先に合わせて選ぶ

以下はHTTPとworkerの配備例です。gRPCの配備はHTTP/2などの対応を別途確認し、HTTP向けLambda adapterの対応範囲には含めません。

| 配備先 | 使い方 |
|---|---|
| Cloud Run service | HTTPイメージを起動。サービスが渡すPORTを使用 |
| Cloud Run Jobs | ワンショットタスクや管理処理を実行し、処理後に終了 |
| ECS service | HTTPと常駐workerを別taskとして運用 |
| ECSの単発task | バッチ・migrationなどを一度実行 |
| Lambda | 専用adapterを含むイメージで、HTTPイベントや明示したタスク呼び出しを処理 |

LambdaにはRuntime APIへの対応が必要です。通常のHTTPイメージを、そのまま置くだけの対応とはしません。adapterの具体的な構成は設計中です。長時間のWebSocket接続を同じように扱うことも想定していません。

Cloud Run Jobsは処理を終えて終了する用途です。常駐workerの配置先と混同せず、ワンショットモードを使います。PostgreSQLにジョブを登録しただけでLambdaやCloud Run Jobsが起動するわけではなく、呼び出しやスケジュールは配備先で設定します。

配備先の実行条件は、[Cloud Run](https://docs.cloud.google.com/run/docs/container-contract)、[Lambda](https://docs.aws.amazon.com/lambda/latest/dg/images-create.html)、[ECS](https://docs.aws.amazon.com/AmazonECS/latest/developerguide/task_definitions.html)の公式資料でも確認できます。

## DBとファイルを外に置く

コンテナのローカルファイルが残ることを前提にしません。DBは外部PostgreSQL、永続ファイルはオブジェクトストレージへ保存します。

HTTPとworkerは、それぞれ必要な設定だけを持ちます。SMTP認証情報はmailerを使うworkerに渡し、HTTP側には要求しない構成です。

## アプリの起動とmigrationを分ける

本番では、専用の管理用イメージでmigrationを一度実行してからアプリを更新します。HTTPやworkerが起動するたびにDB構造を変更することはありません。

台数を増やすと、DBの接続数も増えます。各プロセスのpool上限と最大台数を一緒に設定してください。終了時は新規処理の受付を止め、配備先の猶予時間内に終了します。

Kougaが用意するのは、分離されたイメージと、その実行方法です。クラウドアカウントの作成、イメージのpush、サービスの公開は利用者のデプロイ手順で行います。

**次へ：[コマンドとよくある疑問](reference.md)**
