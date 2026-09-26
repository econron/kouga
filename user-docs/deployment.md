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

`kouga dockerfile`は、入口とworkerを追加したあとに実行してください。`http`はHTTP入口、`worker`はジョブ機能、`grpc`はgRPC入口、`admin`はmigration生成時だけ出ます。ジョブworkerと認証メールworkerが両方ある場合は、後者を`mail-worker` targetとして生成します。`kouga add lambda`を実行した場合だけ`lambda-http` targetも出ます。生成済みDockerfileや`.dockerignore`は上書きしません。後からtargetを追加した場合は、既存Dockerfileをレビューして手動で更新してください。

アプリ独自の実行バイナリは`--binary TARGET=PACKAGE_DIR:BINARY`を繰り返して追加できます。`PACKAGE_DIR`はアプリ内の相対ディレクトリで、`src/bin/BINARY.rs`が必要です。target名・package名・binary名は小文字ASCII・数字・`-`・`_`の識別子に限定し、既存targetとの衝突を拒否します。例えば`kouga dockerfile --binary task-mail=apps/worker:task-notice-worker`です。targetごとにCargo packageを分けると、未使用のHTTP・SMTP・gRPC依存を通常依存treeから外せます。設定変更時は既存Dockerfileと`.dockerignore`をレビューしてから手動で置き換えてください。

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

### Cloud Run service / Jobs

HTTP serviceは`http` targetをレジストリへ置き、`gcloud run deploy SERVICE --image IMAGE --region REGION --max-instances N --no-allow-unauthenticated`で配置する想定です。Kougaは`0.0.0.0:$PORT`でlistenし、SIGTERMで新規受付を止めます。公開の要否・認証・secret注入・DBへのVPC接続は利用者が設定します。レスポンス後の処理継続に依存せず、ジョブは永続queueへ投入してください。DB poolはHTTPプロセスごとに最大5接続なので、`N × 5`とworker分がDB上限を超えないようにします。

Cloud Run Jobsには`worker`または`mail-worker` targetを使い、`gcloud run jobs create JOB --image IMAGE --args=--once --tasks=1 --max-retries=0 --region REGION`、その後`gcloud run jobs execute JOB --region REGION --wait`で実行します。空queueなら0で終了し、処理失敗はretry状態としてDBへ記録されます。管理用migrationは`admin` targetの別Jobにします。Jobのtask timeoutをworkerのワンショット期限より長く取り、終了コードとqueueの状態を監視してください。接続情報はSecret Manager等から実行時に渡し、イメージへ含めません。[Cloud Run serviceのコンテナ契約](https://docs.cloud.google.com/run/docs/container-contract)、[Jobの作成](https://docs.cloud.google.com/run/docs/create-jobs)、[Jobの実行](https://docs.cloud.google.com/run/docs/execute/jobs)を参照してください。

### ECS service / task

`http`と常駐`worker`は別のtask definitionとserviceにします。HTTPは`PORT=8080`と同じcontainer portを設定し、ALBのhealth checkを`/health`に向けます。workerのserviceには公開ポートを設定しません。単発バッチでは同じworkerイメージのcommand overrideを`["--once"]`にした`RunTask`を使い、migrationには`admin`を別taskとして使います。`readonlyRootFilesystem=true`、非root実行、ログ収集、secret参照、必要なCPU/メモリ、stop timeout、外部PostgreSQL/SMTPへの経路をtask definitionで設定します。`essential`コンテナの終了コードとqueueの状態を確認してください。[ECS task definition](https://docs.aws.amazon.com/AmazonECS/latest/developerguide/task_definitions.html)を参照してください。

### Lambda Function URL / API Gateway HTTP API v2

```sh
kouga add lambda
kouga dockerfile
cargo generate-lockfile
docker build --build-context kouga=/path/to/kouga --target lambda-http -t taskboard-lambda .
```

`kouga add lambda`は`apps/lambda`に専用パッケージを追加し、既存のHTTP routerを共有します。通常の`http`・`worker`イメージには`lambda_http`を含めません。`lambda-http`イメージにはAWS Rust Runtime Interface Clientを含め、Lambda Runtime APIでFunction URL/API Gateway HTTP API v2イベントを受けます。DockerfileはDebian系の非root・読み取り専用root対応で、Lambdaでも書き込みは`/tmp`だけを前提とします。Lambda用イメージは単一CPUアーキテクチャで作り、同じリージョンのECRから指定してください。実クラウドへのpush・Function作成はこの手順では行いません。[Lambdaコンテナ要件](https://docs.aws.amazon.com/lambda/latest/dg/images-create.html)を参照してください。

利用者が配備する場合は、ECRへpushしたイメージのdigest、実行role、同じリージョンを確認してから`aws lambda create-function --function-name taskboard-http --package-type Image --code ImageUri=ECR_IMAGE_URI --role ROLE_ARN --architectures x86_64`でFunctionを作り、必要なら`aws lambda create-function-url-config --function-name taskboard-http --auth-type AWS_IAM`でFunction URLを作ります。公開権限を安易に付けず、DB接続用のネットワークと秘密情報も別途設定してください。[create-function](https://docs.aws.amazon.com/cli/latest/reference/lambda/create-function.html)、[Function URL設定](https://docs.aws.amazon.com/cli/latest/reference/lambda/create-function-url-config.html)を参照してください。

バイナリ本文はbase64イベントから復号してHTTP routerへ渡し、応答はバイナリとして渡します。公式runtimeはUTF-8本文をテキスト応答として符号化する場合があります。応答本文の上限は6MiBです。認証ヘッダー、CookieなどのheadersとHTTPエラーstatusはそのまま通します。ただしFunction URL/API Gatewayで設定するIAM認証と、アプリ内の認証は別です。レート制限に使うIPはイベントの`requestContext.http.sourceIp`から取得し、`x-forwarded-for`を無条件には信用しません。Lambda入口でこのsource IPを`trusted_proxies`へ登録すると、転送ヘッダーを再び信頼するため避けてください。直接Invoke権限を与えた主体はイベントを任意に作れるため、実際の接続元として信頼できるのはFunction URL/API Gateway経由に限定されます。v1 REST API、ALB、WebSocketイベント、ネイティブgRPCはこのadapterの対象外です。

Lambda contextの期限より500ms前に新規処理を打ち切り、期限切れはinvocation失敗へ変換します。OTelを使う場合は`kouga add otel`を前後どちらの順序で実行してもLambdaパッケージに反映し、各invocationの戻り前に残り時間内で最大2秒のflushを試みます。強制終了時の配送は保証しません。既存の常駐polling workerをLambdaへ置く構成や、PostgreSQL queueからLambdaの自動起動は提供しません。ワンショットworkerを起動するには別途明示的なイベント入口が必要です。長時間WebSocketも非対応です。

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
