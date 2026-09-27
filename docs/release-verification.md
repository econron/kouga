# 初版再監査（T44・公開判定保留）

T44の作業ブランチは`task/T44-final-hardening`。以下はT43統合mainからの再監査であり、初版公開可能の判定ではない。GitHub/registryへのpush、実クラウドdeploy、公開はしていない。検証環境はmacOS arm64上のDocker Desktop、Rust 1.94、専用PostgreSQL 17（新規生成物用`kouga_t44_final`）、SeaweedFS S3互換、ローカルSMTP sink、ローカルOTLP受信器。資格情報は既存テストコンテナから試験実行時だけ取得し、文書へ値を出さない。HTTP/workerは`KOUGA_ENV=test`と`BOARD_S3_ALLOW_HTTP=1`でローカルHTTP S3 endpointを使う。これは本番TLS/クラウドIAMの試験ではない。

### 直接実証した範囲

T44 source CLIを共有`CARGO_TARGET_DIR`で再ビルドして新規Taskboardを生成。旧共有CLIはT43 worktreeの絶対pathを埋め込んでおり、新生成時にT43/T44の同名crateがlockfile上で衝突したため使わなかった。最終clean commit `320082b861416cf04a6e02bdd4f82d15adfe5e48`から生成した`/private/tmp/kouga-t44-taskboard-final`でfmt、全target Clippy `-D warnings`、専用実DBの生成workspace全テストが成功。S3専用`storage_s3`は全testへS3 envを付けずに分離して実SeaweedFSで成功。初回に全testへS3 envを混在させた失敗（local backend試験のファイル不存在）は環境設定ミスで、分離後に通過した。OpenAPIのRequest/認証差分検出→再生成→check成功。`package-source.sh`で同commitを`vendor/kouga`へ固定し、Cargo manifestに絶対pathがないことを確認した。検証は共有`CARGO_TARGET_DIR`と`CARGO_INCREMENTAL=0`を使用した。

このsnapshotと`Cargo.lock`から`docker build --build-context kouga=./vendor/kouga --target TARGET`で7 targetすべてを個別ビルド。全imageはLinux/arm64、`USER 65532:65532`。展開サイズはadmin 101,381,817 B、HTTP 121,707,932 B、gRPC 107,090,668 B、認証worker 108,334,940 B、通知worker 110,760,540 B、Channel 103,604,164 B、清掃 107,216,988 B。全役割を`--read-only --tmpfs /tmp --memory 256m --cpus 2`で起動した。adminは**新DB**へ5 migration、同じ生成物で再実行すると適用0件。HTTP `/health`と`/ready`は200、別gRPCとChannelは稼働、認証workerは空queueの`--once`でexit 0、清掃は実S3設定のワンショットでexit 0。HTTP発行tokenを別gRPC imageへ渡したProject/Task作成が成功し、別通知worker imageがjobを処理して独立SMTP sinkに1件受理された。HTTP imageから所有者認証付き実S3 PNG添付を保存し、`attachment_mail` jobを別通知worker imageがS3から取得してSMTP sinkに配送した。2 jobの`status=succeeded`をDBで確認した。

最終image IDを`docker image inspect`で記録した。HTTP・両worker・Channel・清掃は先行故障注入時と**同じimage ID**と照合し、admin/gRPCは最終版で直接起動・連携再試験した。先行故障結果の継承は同一IDの5役割に限定する。

| 役割 | 最終image ID |
| --- | --- |
| admin | `sha256:b8cb31bf3f24848b5dbf8e36df4e3fc87b1882a59de8e81b333a6239d44f246c` |
| HTTP | `sha256:bb32dc97521327f86540805fc358571a774a392cbf6b593fb5bb55a3d5349e24` |
| gRPC | `sha256:e55af61fba644112c41992ba52045bfc5f0c030321ab623a71238f3c75b9d708` |
| 認証worker | `sha256:612a087e6df50ed4dd569927ee6890d61139c6df0558407bd5ba9d834de2b078` |
| 通知worker | `sha256:ba6fc74d4d8831184d145685ac5702bce5203c2a5e3ff2e5baf70665177ff570` |
| Channel | `sha256:7865a907f8739dfffb6ca3511b8048f825f4c6c13893daa22a94f5acfed9d2f9` |
| 清掃 | `sha256:3371dd3b7dbb4019ed8bdd8e35ce17dfaa880aca55155129f1d48b2a1ff3d65e` |

ソース再生成ごとに認証migrationのtimestampが変わり得るため、新生成アプリを旧生成アプリの既存DBへ向けると`HistoryMismatch`になる。配布・更新では同じ生成アプリのソース、snapshot、migration履歴を一組として扱い、別生成アプリには新DBを使う。

故障注入では専用DB停止中にHTTP `/ready=503`、`/health=200`、復旧後`/ready=200`。S3停止中の認証付きuploadは503、DBには`delete_pending`が1件残り、S3復旧後に別清掃imageが1件再処理し、正常な`attached`添付は保持された。通知workerを永続効果1件・job `running`の直後にコンテナ強制終了し、lease失効後に別read-only workerがattempt 2で再取得、DB効果は合計1回、SMTP sink受理は1回増、jobは`succeeded`となった。最初の10秒pause試行は停止窓を逃して先にjobが成功したため故障実証に数えず、30秒pauseの再試験を根拠とする。

生成アプリの実DB/S3統合テストへ**debug build限定**の1秒`HttpOptions.timeout`と既存のenqueue後2秒pauseを組み合わせ、`POST .../attachments/{id}/email`のサーバー応答504を直接確認した。DBの同種job件数は2から3へ増え、504はcommit取消しを意味しない。生成gRPCサービスにはdebug buildかつ`KOUGA_ENV=test`限定のcommit後pauseを追加し、100 ms client deadlineで`DeadlineExceeded`を受けた後にもTaskと通知jobがcommitされたことを専用DBで確認した。両者とも**release imageでの故障注入ではない**。実S3 uploadは10 MiBちょうど201、10 MiB+1 Bは413。HTTP JSON本文は11 MiBちょうどサイズ制限を通過して不正JSONの400、11 MiB+1 Bは413。queue/接続/WebSocket/gRPC上限の同時境界は未実測。

最終snapshotのRust依存候補は[配布方針](distribution-compatibility.md)の役割別CycloneDX生成スクリプトで再計測し、HTTP/admin各296、gRPC246、両worker各245、Channel186、清掃192件で先行snapshotと一致。Trivy 0.66のオフラインlicense scanは各imageのDebian 12.15 OS package 88件＋OS componentを検出したが静的Rust依存は検出しないため両台帳が必要。`libcrypt1`、`libgcc-s1`、`libstdc++6`はlicense未分類でも同梱copyrightファイル有。法的notice義務、権利者、CA bundle/Swagger UI、現行脆弱性DBを使ったCVE監査は未完了。Kougaのライセンス本文/著作権者、公開後サポート期間/EOLはユーザー判断待ち。

### 未完了の横断条件

最終imageをローカルOTLP受信器と同じネットワーク名前空間で起動し、異なるHTTP/gRPC認証付きTask作成を同時発行した。DBに保存された2つの異なるjob trace IDは通知worker実行後も保持され、両jobが`succeeded`、SMTP sinkが2通受理。OTLP protobufでは各IDがそれぞれの入口serviceと通知workerのpayloadに現れ、互いのpayloadへ混入しなかった。外部から指定した`traceparent`は取り込まれなかったが、HTTP生成アプリは`trusted_trace_peers`が既定の空集合、gRPC生成BoardServiceは信頼peer用`kouga_grpc::trace_request`を配線していない。前者は意図した信頼境界、後者の上流trace継承は要手動配線であり、信頼peer構成での継承は未試験。Collector停止・復旧での欠落/回復も未試験。

最終snapshotの`TEST_DATABASE_URL`をホストloopbackへ向けて`cargo +1.94.0 test --test attachment_channel --locked --offline owner_attachment_cross_server_events_and_reset -- --exact`を再実行し、password reset後の旧token拒否、旧ticketでの接続拒否、既存WebSocketの2秒以内closeを確認した。初回はDocker内向け`host.docker.internal` URLをホスト試験へ誤用してDB接続失敗、修正後に成功。Docker release Channel imageでのreset後socket試験は未実施。

性能は最終HTTP＋OTLP受信器で`k6 run --vus 2 --duration 15s benchmarks/t40-taskboard-http.js`を使用。`/health`は5523件、失敗0、p95 18.73 ms。認証付きTask readは既定rate 120/分で429が混入したためその試行を性能値に使わず、`BOARD_RATE_LIMIT_PER_MINUTE=100000`と明示して再起動後396件、失敗0、p95 216.56 ms。macOS Docker Desktop共有環境で各1回のみ、CPU/DB/Collectorの競合を含む。性能優位、容量、回帰判定には使わない。

未完了はSMTP受理直後のack故障と重複境界、全役割同時停止/復旧、実外部TLS relayとクラウドIAM/署名、キュー/接続/gRPC/WS上限の同時境界、法的third-party notices・CVE・サポート期間。T43以前の個別試験証拠をT44の同時複合試験と混同しない。**第6節14件を単一配布物で全て合格とは判定しない。**

# 初版再監査（T43）

T43の作業ブランチは`task/T43-release-hardening`。この節はT41/T42統合後の同じ再生成可能なTaskboardに対する追加監査であり、T40以前の節は履歴である。判定は**公開前の残件あり**。今回の直接実証とT35〜T42の引継ぎ証拠を分けて記す。実クラウドdeploy、registry push、GitHub公開はしていない。

### T43で直接確認した範囲

公式[SeaweedFS](https://github.com/seaweedfs/seaweedfs)のS3互換コンテナ（取得時digest `sha256:ce9e796f1fe6f06968f4c04bdaf8f678dad9c8acdfef3d244133d71bfa6bf882`）をloopbackで起動し、専用PostgreSQL 17と接続。`kouga-storage/tests/postgres.rs::s3_streaming_and_signed_url`で11 MiB分割upload、署名URL/所有者拒否、object削除障害→清掃を実行した。最新CLIから再生成したTaskboardの`tests/storage_s3.rs`では、S3 backendでHTTP upload→別router instanceからの認証付きdownload、別owner 404、危険なファイル名拒否、S3 endpoint停止相当の削除503→`delete_pending`非公開→復旧後清掃を実行した。`BOARD_S3_ALLOW_HTTP=1`はテスト専用で、本番は明示拒否する。ローカルストレージと実S3の双方を同じ`Storage`操作で利用する。

再現は専用PostgreSQL 17を`shared_preload_libraries=pg_stat_statements`で起動し、SeaweedFSの`chrislusf/seaweedfs`をloopback `8333`へ公開、検証専用bucketを`S3_BUCKET`で作る。`KOUGA_TEST_DATABASE_URL`、`TEST_DATABASE_URL`と`KOUGA_TEST_S3_ENDPOINT/BUCKET/ACCESS_KEY_ID/SECRET_ACCESS_KEY`を与えて`cargo +1.94.0 test --workspace --locked --offline -- --test-threads=1`を実行する。生成アプリは`TEST_DATABASE_URL`、`BOARD_S3_ENDPOINT/BUCKET/REGION`、`BOARD_S3_ALLOW_HTTP=1`、`AWS_ACCESS_KEY_ID/SECRET_ACCESS_KEY`を与えて`cargo +1.94.0 test --test storage_s3 --locked --offline`を実行する。両方で専用DB schemaをテストごとに作成・破棄する。sandboxでloopbackが拒否される場合はネットワーク権限が要る。T43で使った検証用コンテナ`kouga-t43-s3`と`kouga-t43-postgres`は試験後停止済み、削除せずローカルに残置した。

`kouga-mailer/tests/mail.rs::smtp_transfers_text_html_and_attachment`はloopback SMTP sinkでMIMEのtext/HTML/添付を実配送し、HTML escapeと添付bytesを確認した。さらに再生成Taskboard Dの`apps/worker/tests/attachment_mail.rs`で実S3に保存した添付を独立worker processが取得し、MIME添付として実SMTP sinkへ配送した。偽造owner・削除済みファイルを直接queueに積んでも送らないこと、HTTP側はowner照合後の202とjob登録、通常依存treeにSMTP/workerを含まないことを確認した。外部TLS relay、チェックから送信までの競合的な所有者変更、SMTP受理直後のworker crashは未試験。

同じTaskboard Dの実DB＋S3 HTTP試験で、`KOUGA_ENV=test`のdebug専用フックによりattachment mail jobがDBへ確定した直後から応答を2秒保持した。2件目jobの確定をDBで観測してからクライアントに100 ms deadlineを適用し、応答なしでHTTP futureを破棄してもjobが2件残ることを確認した。これは**クライアントdeadline後の確定副作用**の実証であり、サーバー側504やSMTP受理後の重複配送の実証ではない。利用者向けREADMEに、無条件再試行による重複enqueueの可能性を明記した。

生成Taskboard Dの全workspace testとOpenAPI generate/checkはtimeout hook追加前に成功した。hook追加後はRust 1.94の生成fixture `fmt --all --check`、`clippy --workspace --all-targets --locked --offline -- -D warnings`、専用実DB＋S3 `test --test storage_s3`を再実行して成功。hook後の全workspace testは未再実行。OpenAPI checkの再実行は内部コンパイルで一時的に空き3.8 GiBとなり中断したため、先の成功と差分がhookのみであることを根拠とし、最新状態のOpenAPI check成功とは書かない。独立fixtureの一時target 1.3 GiBはCargo停止時に限定清掃した。

移動可能なアプリは[ソース配布・互換性方針](distribution-compatibility.md)の`package-source.sh`で、commit固定の追跡済みKouga sourceをアプリ内へsnapshot化し、相対path依存と`Cargo.lock`で検証する。clean commit `16bdd2d`から新規生成した`/private/tmp/kouga-t43-taskboard-c`でscriptを実行し、snapshot 1.2 MiB、manifestにローカル絶対pathなし、`.git`/`target`/`.env`混入なし、別pathから共有`CARGO_TARGET_DIR`と`CARGO_INCREMENTAL=0`で`cargo +1.94.0 check --workspace --locked --offline`成功、同snapshotの実DB＋S3 HTTP試験成功を確認した。二度目のscript実行は既存snapshot保護のため終了2。Cargoが入れ子のvendored workspaceを外側へ誤包含しないよう、アプリworkspaceに`exclude = ["vendor/kouga"]`を追加する。Docker named contextを`./vendor/kouga`に向けた**新規release image buildは未実施**で、T41の旧snapshot image証拠と区別する。crates.io公開やGitHub releaseはまだない。法務監査ではregistry由来404 packageの`license`/`license_file`欠落0件を確認したが、Kouga自身のMIT/Apacheライセンス本文、第三者notice/SBOM、公開後のEOL期間は未整備である。

### 仕様第5節・第6節の14シナリオ

「直接」はT43の同じ生成Taskboardで再試験、「引継ぎ」は過去タスクの同一fixture系に記録済み、「限定」は重要な連結/環境を未実証、「未達」は要求そのものが未実装である。過去試験はT43の**単一起動セッションで14件全て同時実行した証拠ではない**。

| §6 | 判定 | 証拠と境界 |
|---:|---|---|
| 1 | 引継ぎ確認 | T42の生成・5 migration・seed二重実行・本番拒否、T41のadmin image。T43では再生成まで直接実施。 |
| 2 | 引継ぎ確認 | T35/T38の所有者別Project/Task CRUD、HTTP/gRPC。T43ではS3添付のowner拒否を直接確認。 |
| 3 | 引継ぎ確認 | T35/T38の未知属性・並行重複とDB制約。T43の危険なファイル名拒否。多process同時更新の新規試験は未実施。 |
| 4 | 引継ぎ確認 | T35のpreload・ページング・transaction内cache無効化。 |
| 5 | 直接＋引継ぎ | T36/T38のTaskと通知job同時commit、worker→SMTP。T43は別worker processによる実S3添付のSMTP配送を確認。 |
| 6 | 直接＋引継ぎ | T43再生成Taskboardの全テストでT36由来の実worker kill→lease失効→別worker再取得、DB効果1回と旧payloadを再確認。SMTP受理後の重複は既知のat-least-once境界で未故障注入。 |
| 7 | 直接確認 | T43の実S3とHTTP owner/失敗/清掃。T37のlocal backend。S3を使ったrelease image一式は未起動。 |
| 8 | 引継ぎ確認 | T37/T41の別Channel process、HTTP変更通知と他owner拒否。 |
| 9 | 引継ぎ確認 | T37のreset後旧token/ticket拒否、既存socketの所定間隔での切断。 |
| 10 | 限定 | T42の2 HTTP process共有rate-limit・DB停止readiness 503・SIGTERM/metrics。全役割を同時に停止/再起動する複合障害は未実施。 |
| 11 | 直接＋引継ぎ | T42のCRUD/auth/添付schema、開発UI/本番404、Request/認証差分check。T43で添付メール202 schemaのgenerate/checkを確認。 |
| 12 | 限定 | T41の7独立Linux image、非root/read-onlyと連携、旧v1 payload。T43のS3変更版imageの再build/全役割再実起動は未実施。 |
| 13 | 限定 | T43再生成Taskboardの全テストでT42由来のHTTP/worker/gRPC別service.name、Collector受信/停止時継続を再確認。認証付きgRPC業務RPC→workerの同時context分離は未実施。 |
| 14 | 引継ぎ確認 | T38の共通BoardでHTTP/gRPC認証・認可・入力・DB/job。T41の別image起動。 |

第5節の有限上限はHTTP本文11 MiB、添付10 MiBをTaskboardの実S3試験で上限超過413まで確認した。gRPC 4 MiB/128 in-flight、DB pool/queue/WebSocket上限は既存試験と設定による確認であり、全境界値の同時負荷試験は未実施。HTTPのクライアントtimeout後のDB job残存は上記の通り直接確認したが、サーバー504後・gRPC deadline後・SMTP受理後の副作用は未実測。性能の既存固定条件はT40/T42の表を参照し、T43のS3/worker/Collector同時負荷の新数値はない。Linux imageの本番起動、外部TLS、実クラウドIAM/署名/ネットワーク、ライセンスnotice完成が公開前の残件である。**初版完成や性能優位を宣言しない。**

## 初版再監査（T40・履歴）

対象は main `3dee5fc`（T36〜T39統合後）、Rust 1.94.0、2026-09-26。判定は**初版未達**である。T35〜T38により所有者別CRUD、通知job、添付、別process WebSocket、業務gRPCが同一の再生成可能なTaskboardに接続された。T39でDB管理CLIも追加された。ただし「コードにある」「実DBテストに成功」と「仕様第6節を一つの配布可能なアプリとして満たす」は区別する。以下が現行判定であり、後半の「T34時点」は比較用の履歴である。

### T40の再現条件と証拠の強さ

`examples/taskboard/generate.sh`を現在のKouga CLIで実行し、`/private/tmp/kouga-t40-taskboard-current`を新規生成した。`TEST_DATABASE_URL=postgres://postgres:…@127.0.0.1:54156/kouga_t19`（専用schemaをテストごとに作成）、`CARGO_TARGET_DIR=<Kouga checkout>/target`、`cargo +1.94.0 test --workspace --locked --offline -- --test-threads=1`が成功。PostgreSQL 17は既存の検証専用コンテナであり、SMTPはテスト内のloopback sink。実S3・実クラウド・外部SMTP/TLSは今回の試験ではない。生成元CLIが古いと別worktreeへの絶対path依存が混じりCargo.lockのpackage collisionとなるため、生成前に現在のcheckoutで`cargo +1.94.0 build -p kouga-cli --locked --offline`を実施した。これはfixtureの欠陥ではなく、現行のローカルpath配布方式の制約である。

### 受け入れ条件の再判定

「確認」は当該条件に対する試験があること、「限定」は一部だけ、「未達」は必須の証拠または実装がないことを示す。T34の22基本条件と追加条件を省略せず再判定した。個別crateの過去の実証を単一Taskboardの結果に読み替えない。

| 仕様 | T40判定 | 新たな証拠と残件 |
|---|---|---|
| 3.1 実行モデル | 限定 | Tokio 1/複数thread、制限・停止はcrate試験。Taskboard複合負荷時の公平性・終了は未実測。 |
| 3.2–3.3 役割別ビルド・配備 | **未達** | 通常依存treeでHTTPにSMTP/worker/tonic/prost、gRPCにHTTP/OpenAPI、workerにHTTP/OpenAPIは入らない。生成DockerfileはHTTP/gRPC/auth-mail-worker/adminのみ。業務通知worker、Channel、添付清掃のtargetがない。実クラウド未配備。 |
| 4.1 Router | 確認 | T34のルート競合・404/405/HEAD/OPTIONS試験を維持。 |
| 4.2 Middleware | 限定 | 順序・短絡・サイズ・timeoutはcrate試験。Taskboardのレート超過と複数HTTP processでの共有制限は未実証。 |
| 4.3 Auth基本 | 確認 | 生成fixtureの`auth_lifecycle_and_single_use_reset`、`taskboard`、`attachment_channel`試験で登録、所有者拒否、旧token/ticket失効と既存socket切断。 |
| 4.3 Auth追加 | 限定 | 401短絡/503はcrate試験。Taskboardの認証登録変更→OpenAPI差分試験は未実施。 |
| 4.4 Model基本 | 確認 | Taskboardの所有者別CRUD、preload、runnerの他owner拒否・完了不可逆を実DBで試験。 |
| 4.4 Model追加 | 限定 | Taskboardで並行重複、FK、cache/job同一transactionを試験。省略/null/未知enum等の全組合せと同時更新競合は個別crateのみ。 |
| 4.5 Migration基本・追加 | 確認 | T39の実DB CLI、従来のmigration/admin試験で適用、rollback、dirty/repair、改変、同時実行。 |
| 4.6 Validation基本・追加 | 確認 | Taskboardで未知属性・不正値の書込防止。custom/async/PATCH三態等はvalidation crate試験。 |
| 4.7 JSON / 4.8 Error | 確認 | 生成fixtureの公開型とエラーを含むHTTP試験、従来の形式/秘匿試験。 |
| 4.9 Mailer | 限定 | 実loopback SMTPで通知送信・一時失敗→再試行。添付付きメールと外部TLS relayはこのfixtureで未実証。 |
| 4.10 Queue/Worker | 確認 | `task_notice`でTask+job同時commit/rollback、実worker kill→lease再取得・attempt 2・DB効果1回、旧v1 payloadを新workerが処理。SMTP受理後の重複はat-least-onceの既知制約。 |
| 4.11 Cache | 確認 | Taskboardのproject件数をPostgreSQL cacheで共有し、Task変更transaction内で無効化。TTL/容量/障害はcrate試験。 |
| 4.12 Storage | 限定 | 生成HTTPで所有者限定upload/download/delete、削除失敗→`delete_pending`→清掃再試行。実S3は未実証。 |
| 4.13 WebSocket | 確認 | 別Channel processへ通知、他owner購読拒否、reset後の旧token/ticket拒否と既存接続切断を生成fixtureで試験。 |
| 4.14 Config | 確認 | 設定優先順位・秘密ファイル・欠落/競合・秘匿は従来のcrate/生成試験。 |
| 4.15 Logs/Health | 限定 | readinessのDB失敗はcrate試験。TaskboardのDB停止・rate超過・終了時ログ/メトリクスは未実証。 |
| 4.15.1 OTel追加 | 限定 | 別のtrace-flow/Collector試験にHTTP→DB→job→worker→mail、context分離・停止時継続がある。Taskboard自体はOTel exporter/独自span・metricsを設定しておらず第6節13未達。 |
| 4.16 Test | 確認 | 生成Taskboardで認証・認可・入力・DB制約/transaction失敗を独立した実DB schemaで再現。 |
| 4.17 CLI | 限定 | T39で`db status/rollback/repair/reset/schema/seed`を実DB試験。Taskboardには登録済みseedがなく、第6節1の同一アプリseed実行は未達。 |
| 4.18 OpenAPI | 限定 | ルートから仕様生成・差分検出/開発UI/本番非公開はcrate/CLI試験。TaskboardのCRUD・認証・添付全体と制約変更後のcheck失敗→再生成成功の通し試験は未実証。 |
| 4.19 HTTP/gRPC | 確認 | `apps/grpc/tests/board.rs`で同じBoardを両入口から呼び、認証前検証・他owner拒否・不正入力・DB一意制約・jobを検査。`grpc_task_notice`で別worker/SMTPも接続。gRPCイメージのT40実ビルドは別途判定。 |

### 第5節と第6節の再判定

Linux/arm64の役割別コンテナはT31で原型を検証したが、統合Taskboardは独自binaryを含むイメージ一式を作れない。Rust 1.94/macOS開発は確認。DB障害、終了処理、rate-limit、OTel Collector停止を**同じTaskboardで**試す横断証拠は不足。公開APIと生成コードの互換性/配布方針も未確定である。初版完成を宣言しない。

| §6 | 判定 | 同一生成Taskboardの根拠または不足 |
|---:|---|---|
| 1 | 限定 | 生成、DB migration、テスト、HTTP/worker起動の証拠はある。登録済みseedの実行がない。 |
| 2 | 確認 | 所有者別Project/Task CRUDと他owner拒否。 |
| 3 | 確認 | validation短絡、未知属性、並行重複とDB制約、JSON error。 |
| 4 | 確認 | 関連preload、ページング、project件数cacheと変更時無効化。 |
| 5 | 確認 | Task+job同時transaction、別workerの実SMTP sink送信。 |
| 6 | 確認 | 実worker kill→lease再取得、DB効果一回、旧payload。SMTP配送自体は重複可能。 |
| 7 | 確認 | 所有者限定の添付HTTP、削除失敗後の清掃再試行。S3は別の未実証条件。 |
| 8 | 確認 | 別process Channelへ変更通知、他owner購読拒否。 |
| 9 | 確認 | reset後の旧token/ticket拒否と所定間隔での既存socket切断。 |
| 10 | **未達** | Taskboard全経路のログ/metrics、DB停止/readiness、rate超過、終了処理の通し試験がない。 |
| 11 | 限定 | 自動OpenAPI/UIの個別証拠。Taskboard制約変更と認証・添付を含む差分検査がない。 |
| 12 | **未達** | 通常依存分離と旧payloadは確認。生成Dockerfileに通知worker/Channel/清掃targetがなく、全役割の独立イメージと連携を確認できない。 |
| 13 | **未達** | TaskboardにOTelを追加したservice別Collector、独自span/log/metrics、停止時継続の試験がない。 |
| 14 | 確認 | 共通Boardを使う両入口と実DB/jobを確認。独立gRPC release imageを起動し、HTTP発行tokenで業務RPC成功、無認証RPCは`Unauthenticated`。 |

重点観点の変化: 並行重複書込・reset token一回性、実worker kill/再起動、他owner拒否、旧v1 payload互換は単一fixtureで実証された。OTelの並行HTTP/gRPC/worker context混入は別crate試験のみで、Taskboardには未接続。SMTP受理後の停止でのメール重複は仕様上残る。通常依存treeは`cargo tree --locked --offline -e normal -p <package>`で確認し、dev-dependencyを含むtreeとは区別した。現行fixtureのローカルpath依存はcheckout移動/古いCLIで壊れ得るため、公開配布の互換性方針が必要である。

### T40の統合版イメージと固定条件実測

Linux/arm64の統合Taskboard `http`（展開114,820,156 B）、`admin`（101,381,817 B）、`grpc`（103,676,924 B）、`worker`（auth-mail-worker、102,293,092 B）を`docker build --build-context kouga=<T40 checkout> --target <target>`で個別release buildした。adminは`--read-only --tmpfs /tmp`の非root UID 65532で専用DBに5 migrationを適用。HTTPも非root/read-onlyで起動し`GET /health`が200。gRPCは別ポート/別コンテナで起動し、HTTPで発行したtokenを用いた`Board.CreateProject`成功と無認証`Board.GetProjectCount`の`Unauthenticated`を`grpcurl`で確認。HTTP/gRPCはSIGTERM後exit 0、OOMなし。auth-mail-workerは非root/read-onlyで空の`mail` queueを`--once`処理してexit 0。イメージはツールチェーンを含まない。今回Docker内の実SMTP送信やgRPC起点jobの別worker配送までは試しておらず、生成fixtureの実DB/SMTPテストを根拠とする。DockerfileにはTaskboard独自の通知worker/Channel/清掃targetが存在せず、全役割のイメージ連携は未達（T41）。

MacBook Air arm64（Darwin 24.1.0、8論理CPU、RAM 16GiB）、Docker Desktop Engine 29.7.2、k6 1.2.3、PostgreSQL 17。HTTPは`--memory 256m --cpus 2 --read-only --tmpfs /tmp`、`PORT=8080`をホスト`127.0.0.1:18086`へ公開、DBは同Macの別Dockerコンテナ`host.docker.internal:54156/kouga_t40_bench`、poolは生成アプリ既定値。測定前に単一ownerを登録しProjectと固定read用Taskを作成。`4 VU×15秒`、各モード1回、keep-alive、warm-upなし、モード順はJSON→read→CRUD。TLS/外部ネットワーク、同時worker、Channel、OTelは含まない。CRUDはTask作成→取得→完了→削除の4 HTTP requestで、作成時に通知jobを同一transactionで登録するが、この測定中はworkerを動かしていない。`[再現スクリプト](../benchmarks/t40-taskboard.sh)`が独立DB上でユーザーを用意して3モードを走らせる。summary原本は`/private/tmp/kouga-t40-{json,read,crud}-summary.json`（Git管理外）。

| 操作 | request/s | p50 | p95 | p99 | HTTP数 | check |
|---|---:|---:|---:|---:|---:|---:|
| JSON `/health` | 2,216.6 | 0.750 ms | 3.799 ms | 13.483 ms | 33,874 | 100% |
| 認可付きTask単件 | 163.6 | 15.66 ms | 51.36 ms | 167.06 ms | 2,456 | 100% |
| Task CRUD（4 request/巡） | 74.6 | 29.78 ms | 110.05 ms | 801.91 ms | 1,124 | 100% |

CRUDは281巡/15秒（18.65巡/s）。測定**後**のHTTPメモリ1点観測は3.051 MiB/256 MiB、共有DBコンテナ全体は97.07 MiBで、ピークではない。T34の素のTask resourceではHTTP認証・Board・関連取得・cache・通知job・添付/Channelルートを持たず、DBとDocker Desktopも他作業と共有する。したがってT34の数値との差をフレームワーク性能回帰と判定しない。この1環境・各1回・低VU数から容量、p99上限、Rails比較は主張できない。繰り返し測定、DB/worker/OTelを同時に動かす負荷、冷間起動、CPU/IO計測は後続に残る。T40のHTTP/gRPC検証コンテナは停止済み、4イメージと専用DB`kouga_t40_bench`はローカル再確認用に残した。共有PostgreSQLコンテナは停止・変更していない。

## T34時点の監査（比較用・現行判定ではない）

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
