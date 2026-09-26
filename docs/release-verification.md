# 初版横断監査（T34）

対象は main `0dedae3`（T33統合直後）、Rust 1.94.0、2026-09-26 のローカル検証。ここでの「確認」は記載した範囲だけを指し、「限定」は仕様の一部を未検証、「未達」は初版の完成条件に必要な実証がないことを指す。タスクの監査・計測完了と**初版リリース可否は別**である。現時点の判定は **初版未達**。第6節で要求する「一つの生成アプリ」での全14シナリオと、業務gRPC・所有者認可付きCRUD・旧payload互換更新などが未実証である。仕様を満たした、あるいはRailsより高速という主張はしない。

Rust 1.94の`cargo fmt --all --check`とworkspace `clippy --all-targets --locked --offline -- -D warnings`は成功。実DB付きworkspace全テストの初回は共有`kouga-t19-postgres`で`pg_stat_statements`が`shared_preload_libraries`にないため`kouga-model/tests/associations.rs`がSQLSTATE 55000で停止した。共有DBは変更せず、T34専用PostgreSQL 17を同拡張の事前ロード付きで起動し、同テスト単独とworkspace全テストを再実行して成功した。`KOUGA_TEST_S3_ENDPOINT`を設定していないためS3試験の本体はスキップされる（テスト結果表示はok）。OTelの`trace_flow.rs`はfeature-gatedなので、workspace標準テストとは別に`-p kouga-worker --features otel --test trace_flow`で実行する。

## 受け入れ条件 → 証拠

仕様の各「受け入れ条件」と「追加の受け入れ条件」を行単位で監査した。証拠はコード/テストと先行タスクの実サービス記録を分けて読むこと。`KOUGA_TEST_DATABASE_URL`などがないと実DBテストはスキップするため、単なるworkspace test成功だけで実サービス合格とはしない。

| 仕様 | 判定 | 実装・試験証拠と残る条件 |
|---|---|---|
| [3.1 実行モデル](specification.md#31-サーバーの実行モデル) | 限定 | `kouga-runtime/src/execution.rs`の1/複数thread、shutdown、`kouga-http/tests/middleware.rs`の上限・deadline。生成アプリの複数process高負荷は未測定。 |
| [3.2–3.3 役割別ビルド・配備](specification.md#32-ビルド単位依存の分離) | 限定 | T31/T32カード、`kouga-cli/tests/t31.rs`・`t32.rs`。HTTP/workerのDB/SMTP連携、read-only/nonroot、PORT/SIGTERM、Lambda Runtime APIモックを実行。実Cloud Run/ECS/Lambdaは未配備。 |
| [4.1 Router](specification.md#41-router) | 確認 | `kouga-http/tests/http.rs::routes_and_responses`で競合・404/405/HEAD/OPTIONS・抽出。 |
| [4.2 Middleware](specification.md#42-middleware) | 限定 | `kouga-http/tests/middleware.rs`で順序、短絡、サイズ、timeout、CORS、rate-limitは`kouga-cache/tests/postgres.rs`。複数serverのrate-limitはDB試験のみで配備負荷は未実測。 |
| [4.3 Auth 基本](specification.md#43-auth認証認可) | 限定 | `kouga-auth/tests/postgres.rs`のtoken失効・所有者scope、T33生成authテスト。単一生成アプリの所有者別CRUD、並行reset token一回性は未実証。 |
| [4.3 Auth追加](specification.md#43-auth認証認可) | 限定 | `kouga-http/tests/auth.rs`の401/503、`middleware.rs`のvalidation前短絡、T18/T30カード。認証変更→OpenAPIの生成アプリ上での差分検査は未実証。 |
| [4.4 Model 基本](specification.md#44-model永続化) | 限定 | `kouga-model/tests/postgres.rs`・`associations.rs`、T33追加テスト`user-docs/examples/advanced.rs`。worker/runnerからの業務不変条件を同じ生成アプリで未実証。 |
| [4.4 Model追加](specification.md#44-model永続化) | 限定 | `kouga-model/tests/derive.rs`・`associations.rs`、`kouga-queue/tests/postgres.rs`にDB/transaction試験。全てのPATCH・未知enum・FK/UNIQUE・preload SQL数・認可・job rollbackの組合せを一つのアプリでは未実証。 |
| [4.5 Migration基本](specification.md#45-dbマイグレーションスキーマ管理) | 確認 | `kouga-migration/tests/migrate.rs`・`admin.rs`・`admin_operations.rs`、T33の空DB生成・適用。 |
| [4.5 Migration追加](specification.md#45-dbマイグレーションスキーマ管理) | 確認 | 同テストで重複/改変/欠落・同時適用・dirty/repair・rollbackを検証。 |
| [4.6 Validation基本](specification.md#46-request-validationcontroller) | 確認 | `kouga-http/tests/http.rs::validation_precedes_controller_and_errors_are_safe`、`middleware.rs::short_circuit_precedes_validation_and_error_has_request_id`、`kouga-validation/tests`。 |
| [4.6 Validation追加](specification.md#46-request-validationcontroller) | 確認 | `kouga-validation/tests/derive.rs`・`validation.rs`でcustom/async/PATCH/Unicode/順序/障害。`kouga-validation/src/lib.rs`の2件のcompile-fail doctestで未検証値の直接構築・可変アクセスを拒否。 |
| [4.7 JSON](specification.md#47-jsonレスポンス) | 確認 | `kouga-http/tests/http.rs`で単件/一覧/204、T33生成CRUD。機密情報はauth側の公開型で分離。 |
| [4.8 Error](specification.md#48-エラーハンドリング) | 確認 | `kouga-http/tests/http.rs`・`auth.rs`・`middleware.rs`で形式/追跡ID/秘匿。 |
| [4.9 Mailer](specification.md#49-mailer) | 限定 | `kouga-mailer/tests/mail.rs`の形式・SMTP/STARTTLS拒否、T30/T31の実SMTP worker。添付付き配送と全失敗種別の再試行は同一アプリで未実証。 |
| [4.10 Queue/Worker](specification.md#410-queueworker) | 限定 | `kouga-worker/tests/postgres.rs`で2 worker、lease再取得、失敗/再投入、graceful、`kouga-queue/tests/postgres.rs`でtransaction。実OS強制終了→別process再取得、旧payload互換更新は未実証。 |
| [4.11 Cache](specification.md#411-キャッシュ) | 限定 | `kouga-cache/tests/postgres.rs`、T23カードでTTL/共有/制限/障害。集計値の更新時無効化を業務アプリで未実証。 |
| [4.12 Storage](specification.md#412-ファイルアップロードストレージ) | 限定 | `kouga-storage/tests/postgres.rs`でローカルと条件付きS3試験、T33でローカル所有者/削除。今回S3 endpoint未提供で実S3は再実行なし。生成HTTP添付routeと削除失敗再試行は未実証。 |
| [4.13 WebSocket](specification.md#413-websocket) | 限定 | `kouga-channel/tests/postgres.rs`で別process配信、ticket、Origin、遅い受信者。生成業務アプリの権限付き購読・password reset後の切断は未実証。 |
| [4.14 Config](specification.md#414-設定シークレット管理) | 確認 | `kouga-runtime/src/config.rs`、T03/T30カードで優先順位/secretファイル/拒否/秘匿。 |
| [4.15 Logs/Health](specification.md#415-ログ計測稼働状態) | 限定 | `kouga-telemetry/tests/health.rs`のDB readiness、T27 trace-flow。生成アプリのDB停止→readiness失敗は未実証。 |
| [4.15.1 OTel追加](specification.md#415-ログ計測稼働状態) | 限定 | `kouga-worker/tests/trace_flow.rs`でHTTP→DB→enqueue→別worker→mail、3 traceのcontext分離/再試行/secret、`kouga-telemetry/tests/{otlp,outage,no_endpoint}.rs`、T33 mock Collector。生成アプリの全経路と負荷中buffer上限は未実証。 |
| [4.16 Test](specification.md#416-テスト支援) | 限定 | `kouga-test/tests/support.rs`とT33生成CRUD/authテスト。認可失敗・DB失敗までの一つの生成CRUD fixtureは未実証。 |
| [4.17 CLI](specification.md#417-cliコード生成コンソール) | 限定 | `kouga-cli/tests/{cli,t30,t33}.rs`、T33 tutorial。`db rollback/repair/schema/seed`等のCLI残項目は未実装（ライブラリAPIとは区別）。 |
| [4.18 OpenAPI](specification.md#418-openapi自動生成) | 限定 | `kouga-openapi/tests/openapi.rs`のschema照合/opt-in UI、CLIの差分試験、T33生成resource。添付API・認証変更・PATCH差分を一つの生成アプリで通す試験は未実証。 |
| [4.19 HTTP/gRPC](specification.md#419-httpとgrpcの同居) | **未達** | `kouga-grpc/tests/server.rs`に認証/認可/DB transaction、T33で同居ビルドとHello RPC。しかしHTTPとgRPCから**同じ業務操作**へ入る生成アプリ、認可とjob投入を通した結合試験がない。 |

## 第5節・第6節の横断判定

第5節のLinux本番・macOS開発・Rust 1.94はT31/T32のLinuxイメージと本Macのビルドで限定確認。HTTP graceful shutdown、有限の本文/pool/queue/WebSocket、複数process共有は各crate試験があるが、単一アプリの複合負荷・実クラウドTLS/権限は未実証。DB停止時の成功扱い防止は各失敗試験、タイムアウト後の副作用非取消しは仕様上の注意として明記済み。公開APIの互換性方針と配布形態は公開前に確定が必要。

第6節は以下のように判定する。`1`（生成/DB/resource/test/server/worker）、`3`（入力検証）、`5`（単独のtransaction/job）、`10`（個別計測）、`11`（OpenAPI/UI）、`12`（イメージ連携）、`13`（OTel）には個別の証拠がある。`2`（所有者別CRUD）、`4`（集計cache無効化）、`6`（実強制終了/冪等更新）、`7`（生成HTTP添付route/削除再試行）、`8`（業務認可WebSocket）、`9`（reset後の接続失効）、`14`（共有業務gRPC）は未達。さらに、個別証拠は**同一生成アプリの全シナリオ通し試験ではない**ため、`1/3/5/10–13`も第6節全体の合格を意味しない。

重点残件は次の通り。

| 観点 | 現在の証拠 | 完成判定を妨げる点 |
|---|---|---|
| 並行更新 | migration同時適用、2 worker claim、DB一意制約の個別試験 | 生成業務CRUDの同時更新/重複、reset token一回性を一つのアプリで未確認 |
| 障害・再起動 | DB/SMTP/Collector拒否、lease期限切れ再取得、graceful、T31 SIGTERM | 強制終了した実worker processの再起動・冪等更新、生成アプリのDB停止中readiness/副作用は未確認 |
| 権限 | auth owner scope、storage owner、channel ticket/policy、gRPC policyの単体/結合試験 | 生成プロジェクト/タスクの他ユーザー拒否、添付route、WebSocket業務policyの通し試験なし |
| context漏れ | `trace_flow.rs`で3 HTTP traceを区別し再試行spanをリンク | 生成アプリの同時HTTP/gRPC/worker負荷における全経路までは未確認 |
| 旧payload互換 | 名前/version不明はquarantine、失敗後の再投入を試験 | 旧HTTPが登録した実payloadを新workerが読む更新試験と互換性方針がない。**必須未達** |
| 依存分離 | T31の`cargo tree`、役割別Docker実行、T32のLambda専用crate | 生成条件を変えた全構成の継続監視は必要。HTTPにSMTP、workerにHTTP/OpenAPI、通常HTTPにLambdaは混入しないという既存確認範囲のみ。 |

## 固定条件のローカル実測

負荷生成器はk6 1.2.3（darwin/arm64）。

MacBook Air arm64（Darwin 24.1.0、8論理CPU、物理RAM 16GiB）、Docker Desktop Engine 29.7.2、PostgreSQL 17コンテナ、Kouga T33生成`taskboard`のLinux/arm64 releaseイメージを使用。HTTP `sha256:e98d9a…`（展開115,670,588 B）、worker `sha256:bd14654…`（104,854,508 B）。Docker Desktop VM/ホストは共有で専有機ではない。両コンテナは`--read-only --tmpfs /tmp --memory 256m --cpus 2`、HTTP `PORT=18084`、`KOUGA_ENV=production`、pool最大5、PostgreSQLは同じMac上の別Dockerコンテナへ`host.docker.internal:54156`で接続。TLS・外部ネットワークはこのベンチマークには含めない。

HTTPはローカルk6（4 VU、15秒、keep-alive、各モード1回、warm-upなし）を[計測スクリプト](../benchmarks/t34-http.js)で順次実行。p値は`http_req_duration`のclient観測値。`json`は`GET /health`、`read`は固定UUIDの`GET /tasks/{id}`、`crud`は各反復でcreate→show→PATCH→deleteを行う。成功率は全チェック100%。同時実行した負荷ではない。

| 操作 | リクエスト/秒 | p50 | p95 | p99 | サンプル数 | HTTPメモリ1点観測 |
|---|---:|---:|---:|---:|---:|---:|
| JSON | 5,931 | 0.523 ms | 1.050 ms | 1.955 ms | 88,963 | 2.113 MiB |
| DB単件取得 | 1,419 | 2.344 ms | 4.401 ms | 8.633 ms | 21,286 | 3.281 MiB |
| CRUD一巡（HTTP request合計） | 1,228 | 2.767 ms | 5.124 ms | 9.120 ms | 18,436 | 3.051 MiB |

CRUDは4 request/反復なので307反復/秒、4,609反復、作成/削除後の残件は固定read用の1件だけ。メモリは`docker stats --no-stream`の**計測中1点**でありピーク・RSS保証ではない。DBコンテナの同時観測はread時71.22 MiB、CRUD時72.72 MiB（他タスクと共有するコンテナ全体）。

ジョブは同じDBに`send_welcome` version 1の無害なpayloadをSQLで一括投入し、別workerコンテナが実行。最初の100件は100件成功、投入時刻から最終更新まで0.418秒、enqueue-to-completion p50/p95/p99 = 0.295/0.407/0.416秒。続く1000件は全件成功、3.284秒 ≒ **305件/秒**、p50/p95/p99 = 1.838/3.145/3.256秒。workerの終了後1点メモリは1.957 MiB。これらはDBタイムスタンプで測る**待ち行列を含む遅延**であり、handler単体の所要時間ではない。サンプルhandlerは`println!`だけで、メール/業務DB更新を含まない。

HTTP起動は停止済み同イメージを`docker run -d`した直前から`/health`初回200までの1回測定で**0.292秒**（CLI/port公開/DB接続を含み、イメージpull・migration・cold cache保証を含まない）。T31カードには同Mac上の別生成アプリのHTTP/gRPC/worker/admin起動と5種の展開/圧縮サイズがある。ここでのHTTP/workerサイズは`docker image inspect --format '{{.Size}}'`で取得した展開サイズで、レジストリ転送量ではない。

再現手順: 専用PostgreSQL DBを作成し、T33生成`taskboard`の`tasks`→`users`→`auth`のup migrationを適用、上記イメージを同条件で起動、`POST /tasks`で固定read用UUIDを取得する。次に`MODE=json`、`MODE=read TASK_ID=<UUID>`、`MODE=crud`を各々 `k6 run --vus 4 --duration 15s --summary-trend-stats 'avg,min,med,max,p(90),p(95),p(99)' benchmarks/t34-http.js`で実行する。ジョブは`INSERT INTO kouga_jobs (name,version,queue,payload,available_at) SELECT 'send_welcome',1,'default',jsonb_build_object('user_id',gen_random_uuid()),now() FROM generate_series(1,1000)`で投入し、`status='succeeded'`件数と`updated_at-created_at`の`percentile_cont`を集計する。計測用DB `kouga_t34_bench`は他DBと分離したまま残し、T34専用HTTP/workerコンテナは停止済み。ローカルk6 summary原本は`/private/tmp/kouga-t34-{json,read,crud}-summary.json`（一時ファイル、Git管理外）。

この1環境1回の観測は容量設計の根拠にできない。再現時はDBサイズ、CPU割当、VUs、温度/電源、Docker Desktop共有負荷、計測回数を増やし、同一アプリに業務認可・mail・gRPC等を載せた値を別途測る。
