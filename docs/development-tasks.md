# Kouga — worktree単位の開発タスク

作成日: 2026-09-25  
状態: T00〜T43統合済み。T44は進行中。T34/T40/T43監査で初版未達と判定し、公開前の残件を追跡
対象: 初版の全機能（T00〜T34の当初計画と、監査後の残件）

[仕様書](specification.md)と[利用者向けドキュメント](../user-docs/README.md)を実装するための作業単位です。本書の作成は、各タスクの実行・Git初期化・worktree作成を意味しません。

## 1. 着手順序と運用

- 作成時点ではRustコードとGitリポジトリがないため、T00 → T01 → T02を先に統合する。T00だけは既存worktreeで作業し、それ以降は依存タスクを統合したmainから分岐する。
- 一つのタスクを一つのworktree・ブランチ・レビュー単位にする。ID順ではなく、依存欄の全タスクが統合済みであることを着手条件にする。T16はT17の完了を待つ。
- ブランチは`task/Txx-short-name`、worktreeの推奨配置は`.worktrees/Txx-short-name`。配置先はGit追跡から除外する。T00の初期コミット以降、実際の作業ディレクトリとブランチを各カードへ記録する。
- 各カードの担当領域は論理的な所有範囲。T01で実際のcrate/ファイル配置を確定する。derive・runtime・CLIテンプレートを分け、異なる機能の担当が同じ巨大ファイルを同時編集しない。
- workspace共通設定、lockfile、CI、共通契約の最終調整は統合担当が持つ。担当者は必要な変更を引き継ぎに明記し、統合は一件ずつ行う。
- 共通契約を変える必要が出たら、その変更を先に合意・統合してから依存作業へ反映する。未完了の依存を独自の仮APIで置き換えない。
- 主担当外のコードへの接続変更は必要最小限にする。競合する場合は先行変更を統合してから更新し、他担当の変更を上書きしない。
- 各機能のテストはそのタスクで完成させる。T17はテスト支援、T34は横断確認であり、テストの後回しを認めるタスクではない。
- GitHubへのリポジトリ作成・push、GitHub Pages公開、クラウドへのデプロイは本書の実行範囲外。自動mergeも行わない。

### 並列化の目安

| 合流点 | その後に分けられる作業 |
|---|---|
| T02完了 | T03 設定/起動 と T07 validation |
| T04完了 | T05 migration、T11 model、T20 queue投入 |
| T09完了 | T10 middleware、T15 CLI、T17 テスト支援（各依存の完了が前提） |
| T10完了 | T14 OpenAPI、T23 cache/制限、T18 auth（T12/T17も必要） |
| T18完了 | T25 WebSocket、T28 gRPC、T24 storage（T21も必要） |
| 個別機能統合後 | generator統合 → 個別イメージ → 配備先対応 → 通しの利用体験 → 完成判定 |

並列数は固定しない。着手可能なタスクから、編集対象が重ならない組み合わせを選ぶ。

T03後のT04と、T07後のT08も並行可能。T04完了時にはT26（計測基盤）も着手条件を満たす。先に作ったT03/T07 worktreeは、T02までmainへ統合した後、未commit変更を確認して各task branchへmainをmergeしてから開始する。自動mergeは行わない。

## 2. タスク一覧

状態は`未着手 → 作業中 → レビュー待ち → 完了`。阻害要因がある場合は`ブロック中`と理由を記載する。本書の進捗集約は統合担当が行い、各worktreeで全カードの状態を同時編集しない。

| ID | タスク | 依存 | 状態 |
|---|---|---|---|
| T00 | ローカルGit管理の開始 | なし | 完了 |
| T01 | 利用者向けAPIと共通契約の確定 | T00 | 完了 |
| T02 | Cargo workspace・最小CI | T01 | 完了 |
| T03 | 設定・起動・通常ログ | T02 | 完了 |
| T04 | DB接続・トランザクション・DBエラー | T03 | 完了 |
| T05 | migrationの生成・適用・履歴 | T04 | 完了 |
| T06 | migrationの巻き戻し・復旧・管理操作 | T05 | 完了（CLI接続は未了） |
| T07 | validationの基本型と実行 | T02 | 完了 |
| T08 | Requestのderiveと検証メタデータ | T07 | 完了 |
| T09 | HTTP router・controller・レスポンス | T03、T08 | 完了 |
| T10 | middleware基盤と標準middleware | T09 | 完了 |
| T11 | modelのCRUD・query実行 | T04 | 完了 |
| T12 | modelのderive・属性型生成 | T11 | 完了 |
| T13 | association・preload | T12 | 完了 |
| T14 | OpenAPI生成と開発用Docs | T08、T09、T10 | 完了 |
| T15 | CLI基盤と新規アプリ生成 | T03、T09 | 完了 |
| T16 | model・resource・Requestのgenerator | T06、T12、T14、T15、T17 | 完了 |
| T17 | テスト支援基盤 | T04、T05、T09 | 完了（生成テスト接続はT16） |
| T18 | 認証の共通処理・policy | T10、T12、T17 | 完了 |
| T19 | 認証API・リセット・auth生成 | T18、T21、T22、T23、T16 | 完了 |
| T20 | ジョブ契約・queue投入 | T04 | 完了 |
| T21 | worker・retry・ワンショット | T20、T03 | 完了 |
| T22 | mailer・SMTP・メールテスト支援 | T20、T03 | 完了 |
| T23 | キャッシュ・共有レート制限 | T04、T10 | 完了 |
| T24 | アップロード・ストレージ | T10、T18、T21 | 完了 |
| T25 | WebSocket・複数サーバー配信 | T10、T18 | 完了 |
| T26 | 計測基盤・OTel exporter | T03、T04 | 完了 |
| T27 | 処理間のtrace連携 | T26、T10、T21、T22、T28 | 完了 |
| T28 | gRPC入口・Protobuf・handler | T03、T04、T07、T18 | 完了 |
| T29 | HTTP/gRPC同居と追加generator | T28、T15、T16 | 完了 |
| T30 | 補助CLI・機能追加generator | T06、T19、T21、T22、T24、T25、T26、T29 | 完了 |
| T31 | 役割別Dockerイメージ | T29、T30 | 統合済み |
| T32 | 配備先への実行対応 | T31、T27 | 統合済み |
| T33 | 利用者ガイドと通しのサンプル | T13、T14、T19、T24、T25、T27、T32 | 統合済み |
| T34 | 初版の横断検証・計測 | T33 | 統合済み（初版未達） |
| T35 | 認可付き業務サンプル基盤 | T34 | 統合済み |
| T36 | queue再起動・旧payload互換 | T35 | 統合済み |
| T37 | 添付・WebSocket業務連携 | T35 | 統合済み |
| T38 | 共通業務処理へのgRPC入口 | T35 | 統合済み |
| T39 | DB管理CLIの残項目 | T34 | 統合済み |
| T40 | 初版の残件再監査 | T36、T37、T38、T39 | 統合済み |
| T41 | Taskboard全役割の独立イメージ | T40 | 統合済み |
| T42 | 単一アプリの観測・OpenAPI・運用経路 | T40 | 統合済み |
| T43 | 実ストレージ・境界/障害横断・配布方針 | T41、T42 | 統合済み（限定事項はT44） |
| T44 | 公開前の複合障害・配布・ライセンス仕上げ | T43 | レビュー待ち（公開判定保留） |

## 3. 共通の完了条件と引き継ぎ

- 対応する仕様節とカードの完了条件を満たす。後続機能が必要な結合検証は引き継ぎ先を示し、そのタスクの完了時に実施する。
- 正常系と、その機能に重要な不正入力・失敗・競合を検証する。DB/SMTP/storageは必要な実サービスとの結合テストも行う。
- Rust変更は関連するfmt/clippy/testを通す。型保証はコンパイル失敗テスト、generatorは一時ディレクトリで生成・ビルド・実行、イメージは依存グラフと実行で確認する。
- 新しい公開APIと使い方の変更を文書化する。T01後の契約変更は後続タスクへ周知し、未実装の提案と実装済み機能を区別する。
- 必須機能が未完了・必要な検証が失敗している状態で完了にしない。クラウド実環境など未検証の範囲は明記する。
- gRPCの初版必須範囲はunary。HTTP/gRPCは同一プロジェクトで併用し、実行ファイル・イメージは分離する。同一プロセス/ポートへの統合は必須にしない。
- 機能固有のSQL migrationはその機能タスクが所有し、T05/T06の実行基盤を使う。番号衝突は統合前に解消する。

各タスクの完了報告には、変更内容、公開API、検証コマンドと結果、制約/未検証事項、後続タスクへの引き継ぎを記載する。結果はレビュー時に本書の該当カードへ集約する。

## 4. タスク詳細

### T00 — ローカルGit管理の開始

- 状態: 完了
- 担当者: ユーザー
- ブランチ: `main`（初期化作業）
- worktree: 現在の作業ディレクトリ
- 依存: なし
- 対応仕様: 全体の着手準備
- 主担当領域: Git管理・ignore設定

**実装すること**: 既存文書を保持してGitを初期化し、mainの初期コミットを作成する。作業時点で既存リポジトリになっていれば履歴を引き継ぐ。

**完了条件**

- [x] 仕様書と利用者文書が追跡され、秘密情報・ビルド成果物・worktree配置先が除外される。
- [x] mainを分岐元としてworktreeを作成可能。Gitの作成者情報を捏造せず、未設定なら必要な設定を確認する。

**今回含めないこと**: リモート作成、push、worktreeの一括作成。

**検証結果・後続への引き継ぎ**: mainの`69a9a89`、追跡済みの仕様/利用者文書、.gitignore、T01/T03/T07 worktreeを確認。ユーザーが作成済みの履歴を継続し、remote操作は行っていない。

### T01 — 利用者向けAPIと共通契約の確定

- 状態: 完了
- 担当者: Codex
- ブランチ: `task/T01-api-contracts`
- worktree: `.worktrees/T01-api-contracts`
- 依存: T00
- 対応仕様: 第2・3・8節、user-docs全体
- 主担当領域: 共通契約・依存構成の設計

**実装すること**: 利用者向けの提案構文をレビューし、後続実装が共有する契約をdocs/api-contracts.mdへ固定する。採用crate・バージョン・Rust最低対応版・ライセンスも確認する。

**完了条件**

- [x] Request/Validated/Patch、model/query/関連取得結果、DB executor、middleware、エラー、ジョブ契約、設定・起動・計測の接続口を記載する。
- [x] HTTP/gRPCの入出力と共通業務型を分離し、生成コード例・schemaメタデータ・crate所有範囲を確定する。未決の共通型がある機能は並列実装へ渡さない。

**今回含めないこと**: フレームワーク本体の実装、将来用途だけの抽象化。

**検証結果・後続への引き継ぎ**: [共通API契約](api-contracts.md)を追加。crate metadata・SQLx Acquire/raw_sql・axum Nextを確認し、関連descriptorとJobContextの例を同期した。文書のリンク・コードフェンス・差分を検査。MSRVと公開APIのcompile fixtureはT02、実DB/HTTPの振る舞いは各実装タスクで検証する。未実装コードのコンパイル成功は主張しない。レビュー・mainへの統合後にT02へ進み、T03/T07はT02統合を待つ。

### T02 — Cargo workspace・最小CI

- 状態: 完了
- 担当者: Codex
- ブランチ: `task/T02-workspace`
- worktree: `.worktrees/T02-workspace`
- 依存: T01
- 対応仕様: 第3節、4.16、第5節
- 主担当領域: workspace・共通ビルド設定・CI

**実装すること**: T01で決めたcrate境界と公開契約の最小骨格を用意し、ローカルとCIの検証入口を揃える。

**完了条件**

- [x] cargo fmt --check、clippy、testが実行でき、最低対応Rustでもビルドできる。
- [x] runtime/derive/CLIテンプレートの所有範囲と、HTTP/gRPC/workerの依存方向が確認できる。空の将来機能を大量に生成しない。

**今回含めないこと**: クラウドへの公開、すべての機能のダミー実装。

**検証結果・後続への引き継ぎ**: Rust 1.94.0と1.95.0で`cargo fmt --all --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`を通した。Cargo.lockを追跡し、CIも同じコマンドを1.94.0/stableで実行する。`kouga-core`にPatch/Error、`kouga-validation`にRequest/Validatedの公開型、`kouga-runtime`にT03用のcrate境界を用意。PATCHの3状態とValidatedの未検証構築拒否、Requestのtrait構文を試験。現在の依存はruntime→core、validation→coreのみ。deriveとCLI templatesは[共通API契約](api-contracts.md)の担当タスクで追加し、生成アプリの入口はそこで分離する。T03/T07はこのタスクをmainへ統合してから開始する。

### T03 — 設定・起動・通常ログ

- 状態: 完了（mainへ統合済み）
- 担当者: subagent
- ブランチ: `task/T03-runtime-config`
- worktree: `.worktrees/T03-runtime-config`
- 依存: T02
- 対応仕様: 3.1、4.14、4.15、第5節
- 主担当領域: 共通runtime・設定・通常ログ

**実装すること**: Tokio起動、型付き設定、秘密情報の取得、通常のtracingログ、終了処理を提供する。

**完了条件**

- [ ] 設定の優先順位、欠落・競合・不正値の拒否、秘密情報の非表示を検証する。
- [ ] 上限付きの重い処理の分離、終了シグナル、終了猶予、起動時の一度だけの初期化を検証する。

**今回含めないこと**: OTel SDK/exporter、HTTP専用のmiddleware。

**検証結果・後続への引き継ぎ**: `kouga-runtime`に型付き設定、環境別上書き、秘密値参照、Tokio起動、上限付きblocking実行、終了猶予、JSON形式の通常ログを追加。Rust 1.94.0で`cargo fmt --all --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`を通過（runtime 6テスト）。`git diff --check`も通過。SIGTERMの実シグナルと実サービス結合は未検証。OTelとHTTP専用middlewareは含めない。`Cargo.lock`の変更はT07と統合時に調整する。

### T04 — DB接続・トランザクション・DBエラー

- 状態: 完了（mainへ統合済み）
- 担当者: subagent
- ブランチ: `task/T04-database`
- worktree: `.worktrees/T04-database`
- 依存: T03
- 対応仕様: 4.4.1、4.4.4
- 主担当領域: DBアクセス基盤

**実装すること**: PostgreSQL poolとトランザクションを、model・migration・queueから共通利用できる形で実装する。

**完了条件**

- [ ] 接続/pool timeout、commit/rollback、行ロック、分離レベル、DB制約・deadlockなどの分類を実DBで確認する。
- [ ] キャンセルした未確定トランザクションの接続を安全に扱い、commit結果不明を盲目的に再試行しない。

**今回含めないこと**: modelのCRUD生成、独自DBドライバ。

**検証結果・後続への引き継ぎ**: `kouga-db`を追加し、SQLxのpool/transaction/Acquire、分離レベル、commit helper、DBエラー分類を公開。PostgreSQL 17でcommit/rollback、行ロック、pool timeout、キャンセル時rollback、UNIQUE/FK/CHECK、serialization failure、deadlockを検証した。Rust 1.94.0でfmt、clippy、全workspaceテストを通過。実通信断時のcommit結果不明は未再現で、分類の単体テストのみ。テスト用コンテナは停止・自動削除済み。`Cargo.lock`はT08との統合時に再生成した。

### T05 — migrationの生成・適用・履歴

- 状態: 完了（main統合済み）
- 担当者: subagent
- ブランチ: `task/T05-migration-core`
- worktree: `.worktrees/T05-migration-core`
- 依存: T04
- 対応仕様: 4.5.1、4.5.2
- 主担当領域: migrationの適用runtime

**実装すること**: SQLファイルの検出・履歴・checksum・順序・排他を実装し、通常migrationを適用可能にする。CLI接続は後続へ公開する。

**完了条件**

- [x] 空DBから適用でき、重複version・改変・欠落・順序不整合を実行前に拒否する。
- [x] 同時適用で二重実行せず、DDLと履歴が同時commitされ、失敗したmigrationだけrollbackする。

**今回含めないこと**: dirty修復・rollbackなどの管理操作、モデル差分による自動schema変更。

**検証結果・後続への引き継ぎ**: `a79ae10`、`2016baa`。`kouga-migration`でSQLファイル検出、14桁UTC version、SHA-256 checksum、履歴照合、session advisory lock、再照合、migrationごとのDDL/履歴同一tx、statusを追加。PostgreSQL 17で同時適用、二重防止、失敗時rollback、履歴改変/欠落/順序、lock timeoutを検証。Rust 1.94でfmt/clippy/workspace全テスト通過。非tx・rollback・repairはT06へ。テスト用コンテナは停止・自動削除済み。

### T06 — migrationの巻き戻し・復旧・管理操作

- 状態: 完了（main統合済み。CLI接続は後続）
- 担当者: Codex
- ブランチ: `task/T06-migration-admin`
- worktree: `.worktrees/T06-migration-admin`
- 依存: T05
- 対応仕様: 4.5.2、4.5.3
- 主担当領域: migration管理API・管理用SQL実行

**実装すること**: rollback、非トランザクション適用とdirty/repair、DB作成/reset、schema出力、seed実行を追加する。

**完了条件**

- [x] 不可逆な対象を含むrollbackは全件未変更で拒否し、非トランザクション中断から手動修復後に継続できる。
- [x] 破壊操作の明示許可、schema出力、seedを確認する。reset時の接続終了と排他も検証し、通常起動でmigrationしない。

**今回含めないこと**: クラウドのDB作成、バックアップ復元、自動的な破壊変更。

**検証結果・後続への引き継ぎ**: rollback、非tx適用/down、dirty表示、理由必須のrepair監査に加え、`AdminTarget`による対象DB・環境・破壊許可の確認、DB作成/reset、`pg_dump`によるschema出力、seed callback、up SQLひな形生成を実装。PostgreSQL 17で通常reset、既存接続の終了、prepared transactionによるDROP失敗後の`ALLOW_CONNECTIONS`復旧、非tx失敗とrepairを検証。`pg_dump` 18.3からPostgreSQL 17へのschema-only出力で所有者・権限・データを含まないことを確認。Rust 1.94.0のfmt、clippy、実DB込みの全workspaceテストを通過。CLI自体はT15の`kouga-cli`へ統合後に`kouga db ...`へ配線する必要があり、このブランチでは未実装。実プロセス強制終了・DB接続断の途中復旧は未検証で、アクセス不可になった場合は管理DBから`ALTER DATABASE <name> WITH ALLOW_CONNECTIONS true`を行う。通常起動からmigrationは呼ばない。

### T07 — validationの基本型と実行

- 状態: 完了（mainへ統合済み）
- 担当者: subagent
- ブランチ: `task/T07-validation-core`
- worktree: `.worktrees/T07-validation-core`
- 依存: T02
- 対応仕様: 4.6、4.6.1
- 主担当領域: validation runtime・公開型

**実装すること**: 少数の基本ルール、同期/非同期custom、検証context、エラー、Validated/Patchを実装する。DB接続の具体実装には依存させない。

**完了条件**

- [ ] 省略/null、Unicodeの長さ、エラー順序・上限、同期失敗後の非同期スキップを確認する。
- [ ] Validatedの不正構築・変更をコンパイル失敗テストで拒否し、検証失敗と基盤障害を区別できる。

**今回含めないこと**: HTTPの入力抽出、DBへの自動書き込み、独自validatorクラス体系。

**検証結果・後続への引き継ぎ**: `kouga-validation`に基本`Rule`、最大100件の検証エラー、同期成功後だけ非同期を実行する`validate`、`Validated`の不正構築・変更を拒否するcompile-failテストを追加。`Patch`の3状態はT02のcore実装を利用。Rust 1.94.0で`cargo fmt --all --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`を通過。`git diff --check`も通過。T08ではderiveによるOption/Patchのルール省略、ネスト深度32、順序・上限での打切り、schemaへのRule接続が必要。`Cargo.lock`の変更はT03と統合時に調整する。

### T08 — Requestのderiveと検証メタデータ

- 状態: 完了（mainへ統合済み）
- 担当者: subagent
- ブランチ: `task/T08-request-derive`
- worktree: `.worktrees/T08-request-derive`
- 依存: T07
- 対応仕様: 4.6.1、4.18
- 主担当領域: Request用proc macro・schemaメタデータ

**実装すること**: Request宣言から検証処理とOpenAPI用の共通メタデータを生成する。

**完了条件**

- [ ] 型・基本属性・custom/custom_async・ネスト・PATCHを含む例がコンパイルして検証できる。
- [ ] 不正な属性を分かる位置でコンパイルエラーにし、schemaに必須/null/制約が一致する。

**今回含めないこと**: OpenAPI文書全体の生成、model derive。

**検証結果・後続への引き継ぎ**: `kouga-request-derive`とvalidation側の接続を追加。名前付きstruct、単位・名前付きvariantのenumでDeserialize、検証、schemaを生成し、unknown field、rename、custom/custom_async、nested、PATCH、深さ32をテストした。Rust 1.94.0でfmt、clippy、全workspaceテストを通過。tuple variantは未対応で、明示的なコンパイルエラーとした。型付きdecodeの深さguardはRequest由来の再帰を対象とするため、HTTP入口での生JSON全体の深さ制限はT09で確認する。`Cargo.lock`はT04との統合時に再生成した。

### T09 — HTTP router・controller・レスポンス

- 状態: 完了（main統合済み）
- 担当者: subagent
- ブランチ: `task/T09-http-core`
- worktree: `.worktrees/T09-http-core`
- 依存: T03、T08
- 対応仕様: 4.1、4.6、4.7、4.8
- 主担当領域: HTTP runtime・router・抽出・応答

**実装すること**: 型付き入力からcontrollerを実行し、JSONと共通エラーを返すHTTP経路を実装する。

**完了条件**

- [x] ルート衝突・優先順位・404/405/HEAD/OPTIONSとパラメータ抽出を検証する。
- [x] 不正入力でcontrollerを実行せず、201/Location・204・一覧形式と情報を漏らさないエラー変換を確認する。

**今回含めないこと**: 業務controllerの自動生成、認証実装。

**検証結果・後続への引き継ぎ**: `c41b9fb`、`3883d2d`、`ab54a14`。axum経路、Validated JSON/Query、raw JSON深さ32、安全エラー、201/204/Page、handlerと同じ宣言のroute metadata生成を追加。独立レビューで検出したQuery検証抜けと無効path登録時panicを修正し、Path schema不一致も登録時に拒否。Rust 1.94でfmt/clippy/workspaceテスト通過。手動の低レベル`Endpoint::handler`ではhandlerと任意metadataの完全一致は保証せず、通常は`#[endpoint]`を使う。request ID/middlewareはT10、OpenAPI文書化はT14へ。

### T10 — middleware基盤と標準middleware

- 状態: 完了（main統合済み）
- 担当者: subagent
- ブランチ: `task/T10-middleware`
- worktree: `.worktrees/T10-middleware`
- 依存: T09
- 対応仕様: 4.2、4.2.1、3.1
- 主担当領域: HTTP middleware

**実装すること**: 通常の非同期関数と消費するNextを使う登録API、標準middlewareを提供する。

**完了条件**

- [x] 順序・途中終了・extensions、CORS/preflight、request ID・アクセスログ・共通エラーを検証する。
- [x] timeout・サイズ制限・信頼プロキシ・同時処理上限を検証し、streamingを壊さない。共有レート制限を後付けできる。

**今回含めないこと**: 認証ストア、共有レート制限ストア。

**公開API**: `Router::middleware`、`Router::group(...).middleware(...).finish()`、`Endpoint::middleware`、`bearer_auth`、`HttpOptions`/`Router::configure`、`HttpRequest<S>`/`Next<S>`、`RequestId`/`ClientIp` extensions。共有rate-limit storeは対象外だが通常middlewareで429等を実装可能。

**検証結果・後続への引き継ぎ**: HTTP結合テスト6件でglobal/group/route順、短絡とvalidation順、extensions、CORS、共通エラー/request ID、timeout、Content-Length有無の413、trusted proxy、ストリーム中の同時実行枠を検証。追補で止まったbody streamの絶対期限・枠解放、および複数行`X-Forwarded-For`の信頼境界を再現テストで確認。アクセスログは`tracing::info!`でrequest ID・method・path・status・duration・client IPのみ出力し、query/ヘッダーを含めない（出力捕捉の自動テストは未実施）。Rust 1.94のfmt/clippy/workspace testを実行。ローカルポートが必要な既存OTLPテストだけsandboxで失敗したため、workspace testを許可済み環境で再実行して全件通過。実TCP/ブラウザ越しのCORS確認および共有rate-limit storeは未検証・対象外。

### T11 — modelのCRUD・query実行

- 状態: 完了（main統合済み）
- 担当者: subagent
- ブランチ: `task/T11-model-query`
- worktree: `.worktrees/T11-model-query`
- 依存: T04
- 対応仕様: 4.4.1、4.4.2、4.4.4
- 主担当領域: model/query runtime

**実装すること**: DB操作の実行基盤と型付きqueryを実装し、通常のpoolとtxの両方から使用可能にする。

**完了条件**

- [x] 作成・部分更新・未存在・削除・NULL・空IN・ページング・DB既定値を実DBで検証する。
- [x] bind、動的列の制限、制約エラー、行ロック・条件付き更新を検証する。部分取得と完全modelを混同しない。

**今回含めないこと**: derive、association、自動saveやdirty tracking。

**検証結果・後続への引き継ぎ**: `cf182e8`、`17318b2`。`kouga-model`へModel/Column/Predicate/Query/LockedQuery/PageQueryとCRUDを追加。SQL識別子を列allowlist、値をbindし、空IN、NULL、部分更新、DB既定値、制約、行ロック、条件付き更新、ページングをPostgreSQL 17で検証。Rust 1.94でfmt/clippy/workspaceテスト通過。UUID自動生成・属性deriveはT12へ。テスト用コンテナは停止済み。

### T12 — modelのderive・属性型生成

- 状態: 完了（main統合済み）
- 担当者: Codex
- ブランチ: `task/T12-model-derive`
- worktree: `.worktrees/T12-model-derive`
- 依存: T11
- 対応仕様: 4.4.1、4.4.2、4.4.4
- 主担当領域: model用proc macro

**実装すること**: model宣言から型付き列、New/Update属性とCRUD接続を生成する。

**完了条件**

- [x] 型マッピング・NULL・enum・既定値・日時列と、生成コードからのCRUDを検証する。
- [x] CRUDを非公開にして業務メソッドへ集約できる。不正なmodel宣言をコンパイル時に拒否する。

**今回含めないこと**: Requestへの自動変換、HTTP出力へのmodel全属性の自動公開。

**検証結果・後続への引き継ぎ**: `#[derive(kouga_model::Model)]`と`#[model(table = "...", module = task_meta, crud_visibility = "pub(crate)")]`を追加。`#[model(column = "...")]`と`#[model(default)]`から`NewX`/`UpdateX`、型付き`columns`、SQLx `FromRow`、pool/tx共通のCRUDを生成する。UUID `id`は自動生成または`create_with_id`で指定できる。日時列は更新属性から除外し、`updated_at`はDB時刻で更新する。Rust 1.94.0のfmt/clippy/workspace test、compile-fail doctest、PostgreSQL 17で型・enum/未知値・NULL/DB default・制約違反・CRUD・rollbackを確認。実DBテストは`KOUGA_TEST_DATABASE_URL`未設定時スキップ。associationはT13、Request変換は対象外。短縮列定数は生成せず`<model_module>::columns`を正本とする。

### T13 — association・preload

- 状態: 完了（main統合済み）
- 担当者: Codex
- ブランチ: `task/T13-associations`
- worktree: `.worktrees/T13-associations`
- 依存: T12
- 対応仕様: 4.4.3
- 主担当領域: 関連宣言・関連query・関連取得結果

**実装すること**: 四種類の関連を明示的に読み、一括取得できるAPIを追加する。必要なmacro変更も本タスクが所有する。

**完了条件**

- [x] belongs_to/has_one/has_many/多対多、空関連・未存在・未取得の区別を検証する。
- [x] preloadのSQL件数、ID分割、ページ境界、関連の認可条件と明示したネストだけの取得を確認する。

**今回含めないこと**: 暗黙のlazy loading、関連の自動保存、polymorphic関連。

**検証結果・後続への引き継ぎ**: `#[belongs_to]`、`#[has_one]`、`#[has_many]`、`#[many_to_many]`から単件メソッド、`*_query()`、`<model>::relations::*()`を生成。`Loaded`と`Query::preload`は親順・件数を維持し、tuple 1〜4件・必須belongs_toネスト・関連filter/順序・ページを提供。1,000一意IDずつSQLを発行する。PostgreSQL 17で四種、空・任意/必須、認可scope、明示ネスト、ページ境界、トランザクション内preloadと1007親→関連SELECT 2回（pg_stat_statements）を検証。Rust 1.94 fmt/clippy/workspaceテスト実施。手書きModelには`id()`追加が必要。共有参照先ModelはCloneが必要。ネストは必須belongs_to起点のみ、多対多の対象が1000件を超える並び順指定はInvalidInputとする。外部キー/UNIQUEはmigrationで明示する。

### T14 — OpenAPI生成と開発用Docs

- 状態: 完了（main統合済み）
- 担当者: subagent
- ブランチ: `task/T14-openapi`
- worktree: `.worktrees/T14-openapi`
- 依存: T08、T09、T10
- 対応仕様: 4.18
- 主担当領域: OpenAPI生成・開発Docs

**実装すること**: 共通メタデータからOpenAPI 3.1.1のYAMLと開発用閲覧UIを提供する。

**完了条件**

- [x] 型・検証・認証・エラー・multipart・本文なし応答が仕様に一致し、未解決参照や重複operation IDを拒否する。
- [x] 外部サービスなしの決定的生成、差分check、失敗時の元ファイル保持、本番での標準非公開を検証する。

**今回含めないこと**: gRPCからOpenAPIへの自動変換、独自UIの開発。

**検証結果・後続への引き継ぎ**: `kouga-openapi::generate`は登録済み`Operation`からOpenAPI 3.1.1をYAML 1.2互換JSONとして出力し、`serve`は明示有効時だけvendored Swagger UIと仕様を公開する。`kouga openapi generate|check [--output]`と開発`server`自動更新を追加。応答のdata/meta、201 Location、204、標準エラー、path/query、Bearerを反映する。`Multipart<T>`でaxumのストリーミングfieldを受け、同じ`T`からmultipart schemaを生成。実HTTPでファイルfieldのchunk読取・200本文・media type不一致の415を確認。生成アプリで連続生成・差分check・生成コンパイル失敗時の元ファイル保持を確認。固定した公式OpenAPI 3.1 schema、OpenAPI型のparse、未解決参照、重複ID、Docsの公開切替とvendored asset、RequestのPATCH省略/nullと長さ制約、実HTTPの422/200と200本文schema一致をテスト。Rust 1.94のfmt、workspace clippy、権限付きworkspaceテストは通過（sandbox内のSMTPソケット試験は権限制約で失敗）。multipartのfield値検証・個別上限・保存はT24へ。

### T15 — CLI基盤と新規アプリ生成

- 状態: 完了（main統合済み）
- 担当者: subagent
- ブランチ: `task/T15-cli`
- worktree: `.worktrees/T15-cli`
- 依存: T03、T09
- 対応仕様: 4.17、3.2
- 主担当領域: CLIの共通処理・new/server/routes

**実装すること**: 新規アプリの作成、開発起動、ルート表示と安全なファイル生成を実装する。

**完了条件**

- [x] 新しい一時ディレクトリから生成アプリがビルド・起動できる。
- [x] 名称・生成先・衝突検査、既存ファイル保護、失敗終了コード、秘密情報の非表示を確認する。

**今回含めないこと**: resourceなどの機能別テンプレート、クラウドの公開操作。

**公開API**: `kouga new <name> [--path <destination>]`、`kouga server [--api http]`、`kouga routes`。生成アプリの入口は`src/bin/server.rs`と`src/bin/routes.rs`に分離。`--api grpc`は未対応として非ゼロ終了する。

**検証結果・後続への引き継ぎ**: 新しい一時ディレクトリで生成アプリをビルドし、`kouga routes`で`GET /health health.check`、`kouga server`で起動して`GET /health`の200/JSONを確認。既存生成先・不正名称・未実装APIを非ゼロ終了で拒否し、ファイルを保護。生成時にsecretをログ出力しない。Rust 1.94 fmt/clippy/workspace testを実行。未公開crateのため生成アプリはローカルcheckoutへのpath依存であり、checkout移動後の可搬性は未対応。T06のDB/migration CLI、T29のgRPC入口、機能別generatorは含めない。

### T16 — model・resource・Requestのgenerator

- 状態: 完了（main統合済み）
- 担当者: Codex
- ブランチ: `task/T16-resource-generator`
- worktree: `.worktrees/T16-resource-generator`
- 依存: T06、T12、T14、T15、T17
- 対応仕様: 4.17、4.18、利用者ガイド最初のAPI
- 主担当領域: CRUD関連CLIサブコマンド・テンプレート

**実装すること**: model/migration/Request/controller/出力/route/テストを一緒に生成し、OpenAPIも連携する。

**完了条件**

- [x] 利用者ガイドの最初のAPIを新規生成から実行でき、入力不正・PATCH・作成と更新を検証する。
- [x] 生成コードが通常のRustとして編集でき、ルートの安全な自動登録または差分提示を行う。

**今回含めないこと**: auth/jobなどの後続generator、編集済みコードの強制上書き。

**検証結果・後続への引き継ぎ**: Resource生成はTask例で全target型チェックを通過。PostgreSQL 17の使い捨てDBで`kouga db create`→`db migrate`→`db status`を実行し、別の`TEST_DATABASE_URL`を使った生成HTTPテストで不正入力422、作成201/DB既定値、GET、PATCH、一覧、DELETEを確認。`kouga openapi generate/check`は5操作、文字列最小長、UUID形式、201/204を出力して通過。Rust 1.94のworkspace fmt、clippy、全テストを通過（SMTPテストのみsandbox内のソケット拒否のため権限付きで再実行）。既存ファイル衝突・編集済みlibの保護、単独Request生成と連続Resource生成をCLIテストで確認。試験用PostgreSQLコンテナは停止・自動削除済み。初版field型はstring/bool/integer/bigint、boolean既定値のみ。HTTP/gRPC併用のgeneratorはT29、他機能generatorはT19/T30へ。

### T17 — テスト支援基盤

- 状態: 完了（main統合済み。生成テスト接続はT16）
- 担当者: main agent
- ブランチ: `task/T17-test-support`
- worktree: `.worktrees/T17-test-support`
- 依存: T04、T05、T09
- 対応仕様: 4.16
- 主担当領域: HTTP/DBテスト支援

**実装すること**: ポート不要のリクエストテスト、専用DB構築、データ分離と認証主体注入の接続口を提供する。

**完了条件**

- [ ] 生成テストが`cargo test`で動作し、並列テストと開発DBのデータを分離する。支援APIと分離は検証済み。生成テストへの組込はT15/T16待ち。
- [x] 正常/不正入力/DB障害を再現でき、認証主体の注入が認可を省略しない。

**今回含めないこと**: 全機能のテストの集中実装。メール・ストレージ固有支援は各機能担当が持つ。

**検証結果・後続への引き継ぎ**: `kouga-test`でポート不要の`TestClient::send`と、`TEST_DATABASE_URL`からテストごとに専用schemaを作成してmigrationを適用する`TestDb`を追加。認証は通常ヘッダー経由で通し、認可を省略する特別経路は用意しない。PostgreSQL 17で並列schema分離・migration・閉鎖poolによるDB障害を確認。Rust 1.94のfmt/clippy/workspace全テストと、実DB結合テストを通過。生成テストへの組込はT15/T16、mailer/storage固有支援は各担当へ。異常終了時はschemaが残り得るため、通常テストは`close()`を必ず呼ぶ。

### T18 — 認証の共通処理・policy

- 状態: 完了（main統合済み）
- 担当者: Codex
- ブランチ: `task/T18-auth-core`
- worktree: `.worktrees/T18-auth-core`
- 依存: T10、T12、T17
- 対応仕様: 4.3、4.2.1
- 主担当領域: 認証runtime・policy・認証middleware

**実装すること**: パスワード処理、token保存・期限・失効、CurrentUser、policyと所有者範囲を提供する。

**完了条件**

- [x] 平文・tokenの非保存、失効・期限・未認証の拒否、DB障害と認証失敗の区別を確認する。
- [x] 所有者以外の取得/更新/一覧への混入を拒否し、照合・policyをgRPCからも再利用できる。

**今回含めないこと**: メールリセットのAPI、OAuth/OIDC/MFA。

**検証結果・後続への引き継ぎ**: `kouga-auth`にArgon2id、ハッシュのみを保存する期限付きBearer token、失効、`CurrentUser`、明示許可policy、所有者scopeを実装。HTTPの`require_bearer`は既存middleware/OpenAPI security metadataへ接続し、401/503を区別する。専用PostgreSQL 17で発行・失効・期限・scopeを、HTTPテストで未認証/DB障害を確認。Rust 1.94 fmt/clippy/workspaceテストを実施。ユーザー表・ログイン/リセットAPI・rate limitはT19/T23、gRPC adapterはT28。T19はユーザー削除時のtoken失効またはFK cascadeも組み込む。パスワードハッシュは同期処理のためT19のHTTP loginではT03の制限付きblocking実行へ移す。実SMTP/本番高負荷環境は対象外。

### T19 — 認証API・リセット・auth生成

- 状態: 完了（main統合済み）
- 担当者: Codex
- ブランチ: `task/T19-auth-api`
- worktree: `.worktrees/T19-auth-api`
- 依存: T18、T21、T22、T23、T16
- 対応仕様: 4.3、4.17
- 主担当領域: 認証HTTP API・authテンプレート

**実装すること**: 登録・ログイン・ログアウト・現在ユーザー・パスワードリセットと、そのgeneratorを実装する。

**完了条件**

- [x] 生成アプリでtoken発行から失効・メールリセットまで実行できる。
- [x] 並行リセットで一度しかtokenを使えず、試行制限・存在を漏らさない応答・認証OpenAPIを確認する。

**今回含めないこと**: 外部認証サービスとの連携。

**検証結果・後続への引き継ぎ**: `kouga generate auth`で登録・ログイン・ログアウト・現在ユーザー・リセット要求/確定のHTTP API、型付きqueueジョブ、別バイナリのSMTP mail worker、migration・認証OpenAPIを生成する。生成時は既存ファイルを上書きしない。PostgreSQL 17で発行・失効・リセットの単回消費（並行実行を含む）・試行制限・メールアドレス存在を漏らさない応答・ユーザー削除時のFK cascadeを確認。実SMTPサーバーへのworker送信を確認。生成アプリの`openapi generate/check`で認証経路とBearer security schemeを確認。Rust 1.94で生成アプリ全target check、CLIテスト、workspace fmt/clippy/全テストを実施。外部認証サービスは対象外。

### T20 — ジョブ契約・queue投入

- 状態: 完了（main統合済み）
- 担当者: main agent
- ブランチ: `task/T20-queue-producer`
- worktree: `.worktrees/T20-queue-producer`
- 依存: T04
- 対応仕様: 4.10、3.2
- 主担当領域: ジョブ契約・queue投入crate・queue schema

**実装すること**: handlerから独立した型付き引数、名前・version・queue・日時の指定とDBへの投入を実装する。

**完了条件**

- [x] HTTP側がworker・SMTPに依存せず投入でき、payload/versionを読み戻せる。
- [x] 業務更新とジョブ登録が同一txでcommit/rollbackされる。trace metadataの保存用接続口を用意する。

**今回含めないこと**: workerの実行・retry、メモリタスクによる永続queueの代替。

**検証結果・後続への引き継ぎ**: `1d0a675`、`b7eeef5`、`68beed3`、`68840ba`、`50b6217`。Job契約/属性macro、DB投入、遅延投入、trace metadata、queue SQL migrationを追加。即時時刻はDB基準、trace値は形式・サイズを検証。PostgreSQL 17でpayload/versionの読み戻しと業務行＋ジョブの同一tx commit/rollbackを確認し、同一DBで結合テストを2回連続通過。Rust 1.94でfmt/clippy/workspace全テスト通過。HTTPはworker/SMTPに依存しない。worker実行はT21、migrationのアプリへの自動組込は後続generatorへ。テスト用コンテナは停止・自動削除済み。

### T21 — worker・retry・ワンショット

- 状態: 完了（main統合済み）
- 担当者: Codex
- ブランチ: `task/T21-queue-worker`
- worktree: `.worktrees/T21-queue-worker`
- 依存: T20、T03
- 対応仕様: 4.10、3.3
- 主担当領域: queue実行crate・worker runtime

**実装すること**: handler登録、実行権、retry、管理操作、常駐/ワンショットと終了を実装する。

**完了条件**

- [x] 複数worker・強制終了・lease再取得・旧workerの完了拒否・retry/dead/未知payloadを検証する。
- [x] 件数/時間/空queueの終了、待機中キャンセル、失敗表示・再投入、graceful shutdownを確認する。

**今回含めないこと**: SMTP処理、クラウドの起動スケジューラー。

**検証結果・後続への引き継ぎ**: `kouga-worker`を追加。`Worker::new/register/run_forever/run_once/cancel_waiting/failed/retry_failed`、`JobContext`、`WorkerOptions`、`JobError`を公開。T20 schemaに追加する失敗理由列・期限切れlease indexのSQL migrationを同梱した。PostgreSQL 17で複数worker、retry/dead、未知・不正payload隔離、強制終了後再取得、旧lease拒否、待機中取消、手動再投入、終了猶予を検証。追加レビューで、one-shot期限中の遅いDB claimを中止し、claim直後も期限を確認して未実行jobのleaseを返すよう修正。shutdown tokenをhandlerへ伝播し、heartbeat DB障害・タイムアウトではhandlerに取消を通知して短く待ち、他jobも終了猶予内でdrainしてからエラーを返す。Tokioの`child_token()`は親の取消を子へ伝播する仕様を確認し、handler開始後に親を取消して`JobContext.cancellation`の通知を受ける実DBテストで証明した。独立したPostgreSQL 17でテーブルロック中の期限切れ、2件実行中のDB停止も再現。Rust 1.94.0のfmt、clippy、workspace全テスト、T21実DBテスト通過。追加修正前には実DBを使うworkspace全テストも通過したが、最終修正後の同テストは既存T04の200ms接続制限で2回失敗したため、T21実DBテストとDB環境変数なしのworkspaceテストで個別に確認した。CLI、SMTP、クラウド起動は未実装。SIGTERM実信号と長期高負荷は未検証。

### T22 — mailer・SMTP・メールテスト支援

- 状態: 完了（main統合済み）
- 担当者: Codex
- ブランチ: `task/T22-mailer`
- worktree: `.worktrees/T22-mailer`
- 依存: T20、T03
- 対応仕様: 4.9、4.16、3.2
- 主担当領域: mailer runtime・テンプレート・メール検査

**実装すること**: テキスト/HTML/添付、同期送信とジョブ投入、開発用記録を提供する。

**完了条件**

- [x] SMTPへの送信、TLS検証、HTML escape、ヘッダー注入拒否、失敗時の結果を検証する。
- [x] HTTP側からSMTP実装を除外でき、worker handlerへ組み込める。外部送信なしでメールを検査できる。

**今回含めないこと**: SMTPサーバー運用、メール送信のexactly-once保証。

**検証結果・後続への引き継ぎ**: `kouga-mailer`を追加。`MailMessage::new/html/attach/deliver`、`render_html`、`SmtpMailer::relay/starttls/insecure_local`、`MemoryMailer::recorded`を公開。lettreのTLS必須設定とMiniJinjaのHTML autoescapeを利用し、平文SMTPはloopback専用の明示APIだけに限定。ローカルSMTPで受理・拒否・STARTTLS非対応時の送信拒否を確認し、メール内容・MIME添付・ヘッダー改行拒否・Sendなworker handler futureをテスト。`cargo +1.94.0 fmt --all --check`、`clippy --workspace --all-targets --locked -- -D warnings`、`test --workspace --locked`を実行。実SMTPサーバーの公開CA証明書検証およびT21の実worker登録は未検証。HTTP側はジョブ契約とqueueのみに依存させ、送信はworker handlerから`deliver`を呼ぶ。queueによる再試行はat-least-onceで重複送信し得る。

### T23 — キャッシュ・共有レート制限

- 状態: 完了（main統合済み）
- 担当者: Codex
- ブランチ: `task/T23-cache-limit`
- worktree: `.worktrees/T23-cache-limit`
- 依存: T04、T10
- 対応仕様: 4.11、4.2
- 主担当領域: cache runtime・レート制限の追加middleware

**実装すること**: メモリ/DBキャッシュと、複数プロセスで共有するレート制限を実装する。

**完了条件**

- [x] TTL・容量上限・名前空間・清掃と、キャッシュ障害時の元データ取得を検証する。
- [x] 同時要求でも上限を守り、429/Retry-Afterを返す。認可に関わる制限をキャッシュ同様にfail-openしない。

**今回含めないこと**: stampede完全防止、キャッシュを使った業務処理の一度限り保証。

**検証結果・後続への引き継ぎ**: `kouga-cache`の`MemoryCache`・`PgCache`・`RateLimiter`、`rate_limit` middlewareを追加。SQL migrationを適用してから使う。PostgreSQL 17で複数同時要求の上限、窓リセット、HTTP 429/Retry-After、DB障害時503、キャッシュ障害時の元データ取得を検証。Rust 1.94のfmt/clippy/workspace testを実行。厳密なstampede防止は行わない。固定窓による境界バーストは許容し、高負荷の実計測はT34へ。

### T24 — アップロード・ストレージ

- 状態: 完了（main統合済み）
- 担当者: Codex
- ブランチ: `task/T24-storage`
- worktree: `.worktrees/T24-storage`
- 依存: T10、T18、T21
- 対応仕様: 4.12、4.16
- 主担当領域: storage runtime・関連metadata・清掃ジョブ

**実装すること**: ローカル/S3互換のstreaming保存、認可付き取得、削除と失敗時清掃を実装する。

**完了条件**

- [x] 両backendで保存/取得/削除し、サイズ・種別・パス・無認可アクセスを検証する。
- [x] 途中失敗・未関連ファイル・削除再試行と署名付きURLを確認し、一時ストレージのテスト支援を提供する。

**今回含めないこと**: 画像変換、ウイルススキャン、ブラウザ直接アップロード。

**検証結果・後続への引き継ぎ**: `kouga-storage`を追加。`Storage::local/s3/in_memory`、`save`、`attach`、`download`、`signed_download_url`、`delete`、`cleanup`を公開。`save`は5MiBバッファ・2並列まででstreamを保存し、PNG/JPEG/PDFの先頭バイト・個別上限を検証。PostgreSQL 17と実ローカルFS、およびMoto 5.1.15のS3互換HTTPで11MiB multipart、保存・取得・削除・署名付きGET、所有者外の不可視化、途中失敗、削除失敗からの再試行を検証。Rust 1.94 fmt/clippy/workspace全テストを通過（全テストは省容量profile・ローカルソケット許可下で実行）。S3の未完了multipartはbucket lifecycleで清掃する必要がある。HTTP multipartのfieldを渡すcontrollerと`kouga maintenance` CLIの呼び出しは後続タスク。

### T25 — WebSocket・複数サーバー配信

- 状態: 完了（main統合済み）
- 担当者: Codex
- ブランチ: `task/T25-websocket`
- worktree: `.worktrees/T25-websocket`
- 依存: T10、T18
- 対応仕様: 4.13
- 主担当領域: channel runtime・配信・接続ticket

**実装すること**: 認証・購読認可・DBによるサーバー間通知・接続管理を実装する。

**完了条件**

- [x] 一度限りのticket、Origin、購読/操作、期限/失効、接続切断を検証する。
- [x] 別プロセスへの配信、サイズ上限、遅い受信者、heartbeatと再接続後のHTTP取得を確認する。

**今回含めないこと**: 永続配信・切断中の履歴再送。

**検証結果・後続への引き継ぎ**: PostgreSQL 17で実WebSocketと別OSプロセスへの配信、一度限りticket、Origin拒否、購読/操作拒否、token失効時切断、通知サイズ上限、1件buffer溢れでの遅い受信者切断、heartbeat後の継続配信、再接続後のテスト用HTTP routeからの状態取得を確認。Rust 1.94 fmt/clippy/workspace testも通過。`kouga-channel`のルートは既存HTTP Routerの`with_state`結果へmergeする。チケットはURLでなく`Sec-WebSocket-Protocol`で渡す。認証migrationの後にchannel migrationを適用する。

### T26 — 計測基盤・OTel exporter

- 状態: 完了（main統合済み）
- 担当者: main agent
- ブランチ: `task/T26-telemetry`
- worktree: `.worktrees/T26-telemetry`
- 依存: T03、T04
- 対応仕様: 4.15、4.15.1
- 主担当領域: メトリクス・稼働状態・任意OTel統合

**実装すること**: 通常ログの上に任意OTel依存とOTLP送信、custom計測の拡張口を実装する。

**完了条件**

- [x] trace/metrics/logs送信、各設定、redaction、sampling、標準出力との分離を検証する。
- [x] 未搭載の依存グラフ、readiness/liveness、収集先停止・上限・再取り込み防止・終了flushを確認する。

**今回含めないこと**: 可視化backendの内蔵、サービス横断の接続実装全体。

**検証結果・後続への引き継ぎ**: `b7d825c`、`7b810a5`。独立crateにOTel SDK 0.33/OTLP HTTP protobufを閉じ込め、traces/metrics/logs、設定・sampling、通常JSONログとの併用、endpointなし、終了時flushと独自providerの終了hookを追加。模擬Collectorで3 signal送信とCollector停止時の業務継続を確認。`kouga-runtime::Health`でliveness/readinessを分離し、PostgreSQL停止時にreadiness 503を実DBで確認。SDK内部のqueue上限・初回drop警告・終了時drop件数警告を有効化した。`kouga-runtime`の依存グラフにOTelがないこと、Rust 1.94のfmt/clippy/workspace全テスト通過を確認。SDKのdrop数は非公開で稼働中の件数APIは提供しない。HTTP health経路と標準HTTP/DB/ジョブ計測の接続はT10/T27へ。

### T27 — 処理間のtrace連携

- 状態: 完了（main統合済み）
- 担当者: Codex
- ブランチ: `task/T27-trace-propagation`
- worktree: `.worktrees/T27-trace-propagation`
- 依存: T26、T10、T21、T22、T28
- 対応仕様: 4.15.1、4.19
- 主担当領域: OTel context伝播・各機能の接続

**実装すること**: HTTP/gRPCからDB・queue・別worker・mailerまでを関連付ける。

**完了条件**

- [x] テストCollectorで標準/custom spanとjobのlink、試行番号、別service名を確認する。
- [x] 並行context混入、信頼しない入力、秘密情報、contextなしの既存ジョブ、ワンショットflushを検証する。

**今回含めないこと**: 業務payloadへのtrace情報の混入、監査ログの配送保証。

**検証結果・後続への引き継ぎ**: `48f1e6e`。HTTP/gRPC入口、model DB、queue metadata、worker試行spanと投入spanへのlink、mail送信を`tracing`で接続。HTTP/gRPC/queue/workerのOTelは`otel` featureで任意。HTTPの外部親は指定した直接TCP peerのみ許可。`KOUGA_TEST_DATABASE_URL=... cargo test -p kouga-worker --features otel --test trace_flow`をPostgreSQL 17で実行し、別子プロセスのHTTP/workerから模擬Collectorへ送ったOTLP protobufを復号して別`service.name`、HTTP→model DB→queue投入、retry試行1/2の別spanとlink、worker→mailの子span、並行requestのcontext分離、無効/信頼外親の拒否、機密文字列のtrace/log/stdout非記録、trace contextがない既存ジョブ、ワンショット終了時flushを確認。SQLx `Acquire`のSend推論制約に対し公開model/queueの汎用DB操作を明示的な`impl Future + Send`へ変更し、同じHTTP handlerでmodel作成とqueue投入を実行できることを確認。Rust 1.94のfmt、all-features workspace clippy、workspace全テストも通過。HTTP/queue/workerの既定依存グラフにOTelがないことを確認。生SQLと任意の外向きHTTPクライアントは自動計測しない。実SMTP配送とgRPC→workerの通し試験は別タスクで扱う。

### T28 — gRPC入口・Protobuf・handler

- 状態: 完了（main統合済み）
- 担当者: Codex
- ブランチ: `task/T28-grpc-core`
- worktree: `.worktrees/T28-grpc-core`
- 依存: T03、T04、T07、T18
- 対応仕様: 4.19
- 主担当領域: gRPC runtime・生成型接続・検証/認証adapter

**実装すること**: unary RPCの生成・登録・実行を提供し、HTTP非依存の業務コードを呼べるようにする。

**完了条件**

- [x] metadata認証・入力検証・policy・status変換・deadline・サイズ/負荷制限を検証する。
- [x] presence/既定値を考慮し、共通modelとtxを利用できる。OTelを後付けできる計測点を用意する。

**今回含めないこと**: 同一ポート多重化、grpc-web、streamingの必須対応。

**検証結果・後続への引き継ぎ**: `kouga-grpc`で共通Error→Status、非同期metadata認証、共通Request検証、global処理枠、handler期限、tonicのサイズ超過応答の変換を提供。`.proto`→`tonic-prost-build`のunary serviceを結合テストで生成し、実PostgreSQL 17とHTTP/2のgRPC通信で認証・policy・検証・モデル参照・tx・サイズ/過負荷・期限・DB停止を確認。`optional`/`oneof`→Patch三状態を確認。認証・検証spanを追加した。生成に必要な`protoc`はCIビルド環境のみへ追加し、runtimeイメージには含めない。Rust 1.94のfmt/clippy/workspace testと実DB結合テストは通過。tonic/prost系のMSRVは1.85〜1.88、ライセンスはMIT/Apache-2.0を配布Cargo.tomlで確認。tonicクライアントが応答前に返すローカル`grpc-timeout`はサーバーで変換できないため、`normalize_client_timeout`を提供し、実通信で`DEADLINE_EXCEEDED`を検証した。T29の生成クライアント入口でこの関数を適用する。CLI/アプリ生成はT29へ。現行fixtureはテスト専用で本番の業務型を生成しない。

### T29 — HTTP/gRPC同居と追加generator

- 状態: 完了（main統合済み）
- 担当者: T29担当
- ブランチ: `task/T29-grpc-coexistence`
- worktree: `.worktrees/T29-grpc-coexistence`
- 依存: T28、T15、T16
- 対応仕様: 4.17、4.19、利用者ガイドHTTPとgRPC
- 主担当領域: 入口追加CLI・gRPCテンプレート

**実装すること**: gRPC単独作成、add grpc/add http、入口別起動と生成型のビルドを実装する。

**完了条件**

- [x] HTTP追加前後とgRPC追加前後で既存コード・ルートを維持し、両入口から同じ業務操作を呼べる。
- [x] 起動対象の既定値・重複追加・既存ファイル保護と、入口ごとの独立ビルドを検証する。

**今回含めないこと**: 同一実行ファイルでの統合起動、OpenAPIと.protoの相互変換。

**検証結果・後続への引き継ぎ**: HTTP-firstでgRPC追加後、HTTPとgRPCを独立ビルドし、`/greet/Kouga`と`Greeting.Greet`が同じdomain関数から`Hello, Kouga!`を返す実通信を確認。gRPC-first単独時は`kouga server`がgRPC、後付けHTTP後はHTTPを既定起動し、両入口で同じ応答と既存`/health`を確認。`cargo tree`でgRPCから`kouga-http`/`kouga-openapi`、HTTPから`kouga-grpc`/Protobuf build依存が入らないことを確認。CLI統合テストでは重複追加、既存`.proto`保護、gRPC追加後のResource生成を検証し、整形後もResource生成と既存route保持を手動確認。ResourceとgRPCを併用する生成アプリの両packageも型チェック通過。Rust 1.94のworkspace fmt/clippy/test通過（SMTPテストのみsandboxのソケット制限により権限付きで再実行）。gRPCサンプルは公開Greeting操作のみ。業務固有の認証・DB・queue連携は利用者が実装し、Docker targetはT31へ。

### T30 — 補助CLI・機能追加generator

- 状態: 完了（main統合済み）
- 担当者: Codex
- ブランチ: `task/T30-cli-features`
- worktree: `.worktrees/T30-cli-features`
- 依存: T06、T19、T21、T22、T24、T25、T26、T29
- 対応仕様: 4.17、4.15.1
- 主担当領域: CLIの残項目・機能別テンプレート

**実装すること**: jobs/maintenance/console/runner、middleware/mailer/job/channel生成、add otelを統合する。

**完了条件**

- [x] 各コマンドの正常/異常終了を確認し、生成された各機能をビルドして動作させる。
- [x] 編集済み箇所の保護・差分提示、後付けworkerへの設定継承、秘密情報の非表示を検証する。

**今回含めないこと**: Rust REPL、内蔵cron、デプロイCLI。

**検証結果・後続への引き継ぎ**: `kouga jobs list/show/retry/cancel/enqueue`、`maintenance`、`console`、`runner`、`worker`とmiddleware/mailer/job/channel生成、`add otel`を追加。生成アプリの全targetをOTel追加後も型チェックし、実PostgreSQLでqueue migration・投入→worker完了・cancel・retry・期限切れcacheだけの清掃、channel起動と未認証401を確認。生成auth-mail-workerのPostgreSQL＋ローカルSMTP、生成mailerのMemoryMailer配送、runner・middleware登録も確認。編集済みserverの拒否、差分提示、新規ファイル衝突拒否、OTel先行後のjob/auth worker継承、`console`接続情報の復号と秘匿を回帰テスト化。Rust 1.94 `fmt --check`、workspace `clippy -D warnings`、workspace testは成功。生成job handlerは動作例なので業務処理へ置換が必要。ストレージ実体の清掃はアプリ側の`Storage::cleanup`をrunner等から実行する。HTTP/gRPC同居アプリでの`add otel`は`apps/http`と既存workerのみ自動変更し、gRPCバイナリのOTel初期化は手動統合とする。誤削除・独自起動コードの上書きを避けるための制約。

### T31 — 役割別Dockerイメージ

- 状態: 統合済み
- 担当者: Codex
- ブランチ: `task/T31-docker-images`
- worktree: `.worktrees/T31-docker-images`
- 依存: T29、T30
- 対応仕様: 3.2、3.3
- 主担当領域: Dockerテンプレート・ビルド検証

**実装すること**: HTTP/gRPC/worker/管理用の独立targetを提供し、必要なバイナリと素材のみを含める。

**完了条件**

- [x] 対象ごとの依存グラフと最終イメージを確認し、ソース・toolchain・未使用runtimeが入らない。
- [x] 非root・read-only root、CA/TLS・DNS、PORT、終了シグナル、外部DB接続を実行確認する。

**今回含めないこと**: レジストリへのpush、根拠のないサイズ目標。

**検証結果・後続への引き継ぎ**: `kouga dockerfile`でBuildKit named contextを用いるmulti-stage Dockerfileを生成し、HTTP/gRPC/job worker/auth mail worker/adminの最終targetを実ビルドした。生成アプリの`apps/worker`と`crates/contracts`を独立packageに分離し、`cargo tree`でHTTPにworker/SMTP/lettreがなく、workerにHTTP/OpenAPIがないことを確認。HTTP/gRPC/worker/adminを別バイナリで起動し、外部PostgreSQLの専用schemaへadminでmigration、HTTPで認証登録とqueue投入、worker `--once`でジョブ処理、mail workerでSMTP配送を確認。gRPCは実RPCで`Hello, Kouga!`を返した。HTTPは`PORT=18081`で`/health`が200、HTTP/gRPCともSIGTERM終了コード0。HTTP/workerは`--read-only --tmpfs /tmp`とUID 65532で動作し、最終イメージにソース・Cargo・rustcがないことを確認。mail-workerで`host.docker.internal`のDNS解決、CA証明書の存在を確認。SMTPは信頼済みテストCAを`SSL_CERT_FILE`で渡すと成功し、CA未指定ではTLS `unknown ca`として拒否され、ジョブは再試行待ちになる。TLS検証無効化はしていない。

同一Mac/Docker Desktop環境での実測値（バイト、圧縮列は`docker save | gzip -1 | wc -c`でありレジストリ転送量そのものではない）。起動時間はコンテナ開始からHTTP `/health`応答、gRPCポート応答、または空queueワンショット/既適用migration終了までの1回測定であり、性能保証ではない。

| target | 圧縮 | イメージ展開 | バイナリ | 起動/終了 |
|---|---:|---:|---:|---:|
| http | 36,736,700 | 112,781,356 | 15,344,472 | 0.226秒 |
| grpc | 31,459,448 | 99,743,204 | 2,306,320 | 0.213秒 |
| worker | 32,454,986 | 101,572,172 | 4,135,288 | 0.21秒 |
| mail-worker | 32,790,343 | 102,293,092 | 4,856,208 | 0.18秒 |
| admin | 32,386,461 | 101,378,251 | 3,938,656 | 0.16秒 |

生成物のHTTP-first/gRPC-first回帰テスト、全feature入りHTTP-first生成アプリの`cargo check --workspace --locked --offline`とgRPC-firstからHTTP/jobを追加した生成アプリの`cargo check --workspace --offline`、Rust 1.94のworkspace `fmt --check`、`clippy --all-targets -D warnings`、`test --workspace`が通過。新規crate追加はなく、lettre既存依存の`rustls-native-certs`機能だけ有効化した（同crateのMSRV 1.71、ライセンスApache-2.0/ISC/MIT）。DockerのBuildKit named contextには対応するKouga source checkoutが必要。実クラウド配備・レジストリpush・Lambda adapter検証はT32へ引き継ぐ。

### T32 — 配備先への実行対応

- 状態: 統合済み
- 担当者: Codex
- ブランチ: `task/T32-platform-runtime`
- worktree: `.worktrees/T32-platform-runtime`
- 依存: T31、T27
- 対応仕様: 3.3、4.15.1
- 主担当領域: Lambda adapter・配備先実行設定・手順

**実装すること**: Cloud Run/ECSの実行条件とLambda専用入口を整え、通常イメージへadapterを混入させない。

**完了条件**

- [x] ローカルで可能なPORT/終了/ワンショットとLambdaイベントadapterを結合検証する。
- [x] バイナリ本文・headers・認証・flush・期限を確認し、gRPCとの対応差と実クラウド未検証事項を記録する。

**今回含めないこと**: 許可なしの実クラウドdeploy、PostgreSQL queueからの自動クラウド起動。

**検証結果・後続への引き継ぎ**: `kouga add lambda`で`apps/lambda`の独立packageを生成し、AWS公式`lambda_http` 1.3.1のRuntime Interface ClientでFunction URL/API Gateway HTTP API v2を既存Kouga routerへ接続する。`kouga dockerfile`には追加時だけ`lambda-http`最終targetが出る。HTTP-firstとgRPC-firstの両生成順序、`add otel`の前後順序をCLIテストで確認。HTTP/worker側`cargo tree`には`lambda_http`がなく、Lambda packageのみに存在する。生成DB＋ジョブ＋Lambda workspaceと、OTel追加済みLambda workspaceの`cargo check --workspace --offline`が通過。HTTP/gRPC同居でもネイティブgRPCは別入口のままで、Lambda HTTP adapterを流用しない。

生成Lambdaのunitテストはbase64由来のバイナリ本文、認証ヘッダー、Cookie等の応答ヘッダー、401/403、Function URL v2の`sourceIp`（偽装された`x-forwarded-for`より優先）、期限切れ/欠落/有効期限を確認。生成した実バイナリをローカルRuntime APIモックから起動して`/health`の200応答を確認し、OTel有効化後も同じ経路が成功。OTel三signalは`Telemetry::flush(deadline)`で`shutdown`前にCollectorへ到達し、Collector停止時でもHTTP 200を維持した。ローカルDockerで`lambda-http` release imageをビルドし、展開サイズ113,241,764バイト、UID/GID 65532、`--read-only --tmpfs /tmp`で起動確認。T31で確認した通常HTTPのPORT/SIGTERM、worker `--once`、DB/SMTP結合はT32で変更しておらず、T31の回帰テストをworkspace全テストで再実行した。Rust 1.94のworkspace `fmt --check`、`clippy --all-targets -D warnings`、`test --workspace`が通過。生成OTel入りアプリの`clippy --all-targets -D warnings`も通過。

新規直接依存`lambda_http` 1.3.1はApache-2.0、MSRV 1.84.0。依存するAWS公式`lambda_runtime` 1.4.0はApache-2.0/MSRV 1.84.0、`aws_lambda_events` 1.2.0はMIT/MSRV 1.84.0。Rust 1.94で生成アプリ全体の型検査を実施した。公式資料を踏まえてCloud Run service/Jobs、ECS service/task、Lambdaコンテナの実行条件・設定例を利用者ガイドへ追記した。実Cloud Run/ECS/Lambdaへのpush・deploy、実AWS Function URLとIAM/VPC、外部レジストリ転送量は未検証。Lambda workerの自動queue起動、REST API v1/ALB/WebSocket/ネイティブgRPCイベントも非対応。直接Invokeではイベント内`sourceIp`を偽装できるため、入口権限を制限すること。

### T33 — 利用者ガイドと通しのサンプル

- 状態: 統合済み
- 担当者: Codex
- ブランチ: `task/T33-user-journey`
- worktree: `.worktrees/T33-user-journey`
- 依存: T13、T14、T19、T24、T25、T27、T32
- 対応仕様: 第6節、user-docs全体
- 主担当領域: 通しのサンプル・利用者文書

**実装すること**: 最初のAPIから全機能までの実行可能なサンプルを用意し、文書のコマンド・コード・出力を実物へ合わせる。

**完了条件**

- [x] 新規ディレクトリからCRUD/認証/関連/メール/添付/WebSocket/gRPC/OTel/イメージを再現する。
- [x] コード例をコンパイル・実行し、提案から確定したAPIを文書へ反映する。未実装・未検証を提供済みと表現しない。

**今回含めないこと**: GitHub Pages公開、未検証の性能や導入実績の訴求。

**検証結果・後続への引き継ぎ**: [通しの実行手順](../user-docs/tutorial.md)と[追加テスト](../user-docs/examples/advanced.rs)を新設。README・機能別ガイド・CLIリファレンスを現行CLI/生成型/ファイル配置へ更新した。`db rollback/repair/schema/seed`、添付HTTP route generator、任意の業務RPC/認可policy/ジョブ送信handlerの自動生成は提供済みと扱わない。生成アプリはCLIをビルドしたKouga checkoutへの絶対path依存であり、配布・実クラウド配備は未検証。

T33 checkoutから新規ディレクトリへ生成したアプリで、実PostgreSQLの`db create/migrate`、CRUDの201/422と生成テスト、認証登録200と認証テスト、通常job投入→`worker --once`→`succeeded`、gRPC実クライアントの`Hello, Kouga!`を確認。`generate auth/model/job/mailer/channel`・`add grpc/otel`を同じアプリへ適用した`cargo check --workspace --offline`と、実DB付き`cargo test --workspace --offline`を通した。生成auth-mail-workerのテストではローカルSMTPシンクへの送信とreset tokenハッシュ保存を確認。追加テストは実DBの外部キー付き`belongs_to`/preload、ローカル添付の保存・attach・他人拒否・取得・削除、認証ticketでのWebSocket購読と別Channelインスタンスからの通知を再現した。OTel付き生成HTTPに独自metricを加えて実DBリクエスト後にSIGTERMで終了し、ローカルmock Collectorで`/v1/traces`・`/v1/metrics`・`/v1/logs`を受信した。

新規生成アプリの`http`/`worker` Docker最終targetをBuildKit named context付きでビルドした。展開サイズは各115,670,588/104,854,508バイト、UID/GID 65532。HTTPはread-only rootでPORT指定の`/health` 200と外部DBの`/tasks` 200、workerもread-only rootの`--once`で実ジョブを完了。検証用コンテナは停止・削除済み。レジストリpushやクラウドdeployは行っていない。

通し検証で見つかった生成コードの不具合を最小修正した。auth-mail-workerテストのmigration pathをworker packageからの絶対基準へ変更し、shutdown関数をtest module前へ置き、`WorkerOptions`初期化をlint適合にした。resource作成時のCopy項目には不要なcloneを付けず、gRPC Greetingが最後のルートの場合に余分な`let router`を生成しない。`kouga-cli/tests/t33.rs`はHTTP→auth→gRPCとHTTP→gRPC→resourceの両順序を確認し、新規生成アプリの`clippy --workspace --all-targets -D warnings`も通った。既存の編集済みアプリは自動書換えしない。

Rust 1.94のKouga workspace `fmt --check`・`clippy --workspace --all-targets --locked --offline -- -D warnings`・`test --workspace --locked --offline -- --test-threads=1`は最終実行で通過。全体テストの初回だけ既存OTel flushテストが1回失敗したが、同テスト単独で計4回連続成功し、全体テスト再実行も成功した。telemetry本体は変更していない。今回の添付はローカル保存を検証し、S3結合は既存T24テストの範囲。業務アプリごとの関連付け・添付route・WebSocket policy・独自メールhandler、実Cloud Run/ECS/Lambda、性能・全機能横断判定はT34以降/利用アプリ側の責務として残す。

### T34 — 初版の横断検証・計測

- 状態: 統合済み（初版完成判定は未達）
- 担当者: Codex
- ブランチ: `task/T34-release-verification`
- worktree: `.worktrees/T34-release-verification`
- 依存: T33
- 対応仕様: 全機能の受け入れ条件、第5・6節
- 主担当領域: 横断検証・性能/サイズ記録・完成判定

**実装すること**: 仕様の各要件をタスクと検証結果へ対応付け、初版の完成条件を監査する。

**完了条件**

- [x] 並行更新・障害・再起動・権限・context漏れ・旧payload互換・依存分離の未解決事項を確認し、必須失敗があれば完了にしない。
- [x] 固定条件でthroughput/latency/memory/起動時間/イメージサイズを測定し、対応版・再現手順・制約・未検証事項を記録する。

**今回含めないこと**: 自動公開・自動release、各機能のテストをここまで先送りすること。

**検証結果・後続への引き継ぎ**: [横断監査と実測値](release-verification.md)に仕様の基本22条件・追加条件、第5・6節の証拠/不足を対応付けた。4 VU×15秒の同一Mac/Docker Desktop環境で、JSON 5,931 req/s・DB単件1,419 req/s・CRUD一巡307巡/s、別workerの簡単なjob 1000件は約305件/s、HTTP起動0.292秒を実測。メモリ1点観測、p50/p95/p99、イメージ展開サイズ、再現条件と制約を同文書に記録した。これは性能保証ではない。

Rust 1.94 `fmt --check`、workspace `clippy --all-targets --locked --offline -- -D warnings`は成功。実DB付きworkspace全テスト初回は共有PostgreSQLの`pg_stat_statements`未事前ロード（SQLSTATE 55000）で停止したため、共有設定を変えず、同拡張を事前ロードしたT34専用PostgreSQL 17で再実行して成功。`kouga-worker --features otel --test trace_flow`も実DB/ローカルCollectorで成功。S3はendpoint未提供のため今回の実サービス再確認対象外。T34専用HTTP/worker/PostgreSQLコンテナは停止・削除済み、計測専用DB `kouga_t34_bench`は共有検証用PostgreSQL内で他DBと分離して残置。

**初版完成判定: 未達。** 単一生成アプリでの所有者別CRUD、集計cache無効化、実worker強制終了→再起動時の冪等業務更新、生成HTTP添付route/権限、WebSocket業務policy、password reset後の接続失効、HTTP/gRPCから同じ業務処理への認可付き呼び出し、および旧payloadを新workerが読む更新試験が不足。実Cloud Run/ECS/Lambdaと実S3再確認も未実施。これらを満たすまで初版完成・公開可能としない。

## 5. T34監査後の残件

[横断監査](release-verification.md)で明らかになった初版必須シナリオを、同じ生成アプリを段階的に拡張する作業へ分ける。T35とT39は並行可能。T36〜T38はT35の共通fixtureを取り込み、それぞれ別のブランチ・worktreeで並行し、一件ずつ統合する。T40は機能の代替ではなく再監査である。実クラウドdeployや外部レジストリpushは引き続き行わない。

### T35 — 認可付き業務サンプル基盤

- 状態: 統合済み
- ブランチ: `task/T35-business-sample`
- worktree: `.worktrees/T35-business-sample`
- 依存: T34
- 対応仕様: 4.3、4.4、4.6、4.11、第6節2〜4

**実装すること**: 再生成可能な単一Taskboardアプリfixtureを作り、User→Project→Taskの所有者scope、外部キー・一意制約、関連preload・ページング、集計cacheの更新時無効化を接続する。

**完了条件**: 別ユーザーの一覧・詳細・更新・削除が拒否され、未知属性/不正入力ではcontrollerとDB更新が起きない。並行重複書き込みはDB制約で拒否され、HTTP以外の経路でも業務不変条件を守る。新規ディレクトリから生成・実DBテスト・文書化できる。

**実装・検証**: [再生成手順とAPI](../examples/taskboard/README.md)、`generate.sh`、overlayを追加。CLIの`new`→`generate auth`から単一アプリを生成し、`Board`にowner-scoped CRUDを集約。生成modelの低水準CRUDはprivateにし、DBのowner-matching複合FK・一意制約・CHECKでHTTP外も保護。Task一覧の型付き関連preloadと安定したページング、project集計のPostgreSQL cacheとtask変更transaction内での無効化を実装。実PostgreSQL上で登録、他ユーザーの一覧/詳細/変更拒否、未知属性・不正値の書込防止、並行重複の409、FK、preload/page、cache invalidation、runnerからの所有者判定と完了状態の不可逆性を確認。生成アプリの認証/workerを含むworkspace全テスト、Rust 1.94 fmt/clippyとKouga workspace全テストを実行。T36以降のqueue通知、添付/WebSocket、gRPC接続は未実装・未検証。

### T36 — queue再起動・旧payload互換

- 状態: レビュー待ち
- ブランチ: `task/T36-queue-compat`
- worktree: `.worktrees/T36-queue-compat`
- 依存: T35
- 対応仕様: 4.9、4.10、第6節5〜6、12

**実装すること**: T35の業務処理でTask作成と通知job投入を同一transactionへ接続し、実workerの強制終了→lease再取得・冪等更新、再試行・メール配送、旧payloadを新workerが処理する更新試験を用意する。

**完了条件**: 途中終了後の重複副作用がなく、失敗の再試行と恒久失敗が区別される。旧versionのpayloadを新workerが読むか、互換性方針に従う明示的な移行を検証する。HTTP/workerの依存分離を維持する。

**実装メモ**: Task作成と`taskboard.task_created` v2のenqueueを`Board::create_task`の同一transactionに入れる。v1は`task_id`のみ、v2は`task_id`と`owner_id`で、両handlerを新workerへ登録する。専用`task-mail` queueを使い、既存の認証メールworkerとの誤取得を防ぐ。workerのDB上の通知効果は安定したjob IDを主キーとして冪等化する。ただしSMTP承認後・queue ack前の停止ではメール重複があり得る。T38の共通Board crateは同じ`board.rs`を読むため、統合後に依存とgRPC側通知件数を再確認する。

**検証**: Taskboard生成fixtureでRust 1.94のfmt、workspace Clippy `-D warnings`、workspace全テストを通過（実PostgreSQLとローカルSMTP）。Task/jobの同時commit・enqueue拒否時rollback、実worker kill→lease再取得・attempt 2・DB効果1回、v1 payloadの新worker処理、SMTPの一時失敗→再試行成功、所有者不一致の恒久失敗を確認。`cargo tree`でHTTP packageに`kouga-mailer`/`kouga-worker`が無く、worker packageに`kouga-http`が無いことを確認。Kouga本体workspaceのfmt/Clippyも通過。本体workspace全テストは共有ディスク空き約2GiBのため新規リンクを避け、統合後に再実行する。

**統合後追記（T40）**: T37/T38を含む最新mainから再生成した単一fixtureの実PostgreSQL/loopback SMTP付きworkspace全テストが成功。本体workspace全テスト不足はT37/T38の統合検証で解消済み。旧payload・強制終了・DB効果一回のfixture試験もこの再実行で成功した。

### T37 — 添付・WebSocket業務連携

- 状態: レビュー待ち（main未統合）
- ブランチ: `task/T37-storage-channel-app`
- worktree: `.worktrees/T37-storage-channel-app`
- 依存: T35
- 対応仕様: 4.12、4.13、第6節7〜9

**実装すること**: T35の業務アプリへ所有者限定の添付upload/download/delete、削除失敗の清掃再試行、Task変更通知と認可付き購読を接続する。

**完了条件**: 別HTTP/Channel processへ通知が届き、他ユーザーの添付・購読を拒否する。password reset後の旧token/ticket/接続の扱いと確認間隔を実DB・実通信で確認し、ローカル保存と既存S3 adapterの差を明記する。

**実装・検証**: 単一Taskboard overlayに所有者限定のmultipart upload、stream download、delete、清掃ワンショットbinary、Task所有者をDBで再確認する添付triggerを追加。Task変更のPostgreSQL NOTIFYは同一transactionで発行し、別binaryのChannel processが所有者channelへの購読だけ許可する。`BOARD_CHANNEL_AUTH_CHECK_MS`で失効確認間隔を100〜60000msに設定できる。ローカル保存とS3 adapterの差・設定は[利用者向け説明](../examples/taskboard/ATTACHMENTS_AND_CHANNELS.md)に記録。Rust 1.94のfmt、生成fixtureとKouga本体のworkspace clippy `-D warnings`、workspace全テストを通過。生成fixture全テストではPostgreSQL 17と独立Channel子プロセスの実TCP WebSocketを使い、他ownerの添付・購読拒否、別HTTP側のTask更新通知、password reset後の旧token/未使用ticket拒否・既存接続切断（100ms設定）、ローカルオブジェクト削除失敗→`delete_pending`→cleanup再試行を確認。T36を取り込んだ同一fixtureでも生成、fmt/clippy、実DB/SMTP/worker全テストを再実行し、task作成transaction内のjob投入と変更通知、worker強制停止・再取得・旧payload処理の両立を確認。実S3は未検証（T34の公開判定項目）。T38の`Board`分離との統合時に接続点を再確認する。

### T38 — 共通業務処理へのgRPC入口

- 状態: レビュー待ち
- ブランチ: `task/T38-business-grpc`
- worktree: `.worktrees/T38-business-grpc`
- 依存: T35
- 対応仕様: 4.19、第6節14

**実装すること**: T35の同じ業務操作をHTTP/JSONとgRPC/Protobufから呼び、認証・認可・validation・DB制約・job投入・エラー変換を両入口で検証する。

**完了条件**: 実クライアントで同じDB結果が得られ、不正入力・他ユーザー操作・期限・失敗statusがそれぞれ正しく拒否される。HTTP/gRPCを独立ビルドし、通常HTTPにProtobuf依存を混入させない。

**実装・検証**: Taskboard overlayに独立した`taskboard-board`/`taskboard-rpc`/`taskboard-grpc`と`proto/taskboard.proto`を追加。HTTPとgRPCは同一`src/board.rs`の所有者限定Boardを使い、T36の通知jobとT37の添付清掃・変更通知を同じtransactionで維持する。gRPC実クライアントでHTTP作成Project→gRPC Task作成→HTTP参照・完了、無認証、他owner、不正入力、DB unique制約、期限statusを実PostgreSQLで確認。gRPCからV2 jobが1件だけ登録され、別workerが実SMTP sinkへ送信し冪等effectを記録する結合テストも通過。`kouga-cache`のHTTP adapterを既定有効のfeatureに分離。通常依存treeでHTTP packageにtonic/prost、gRPC packageに`kouga-http`/`kouga-openapi`が入らない。生成fixtureとKouga本体でRust 1.94のfmt、workspace clippy `-D warnings`、実DB付きworkspace全テストを通過し、HTTP/gRPCそれぞれのdebug binaryを独立ビルドした。生成Dockerfileの別`http`/`grpc` targetは確認済みだが、T38統合版のDocker release image実ビルドは容量制約で未検証。T31の基本target実証とは分け、T40で再検証する。

### T39 — DB管理CLIの残項目

- 状態: 統合済み
- ブランチ: `task/T39-db-cli-completion`
- worktree: `.worktrees/T39-db-cli-completion`
- 依存: T34
- 対応仕様: 4.5、4.17

**実装すること**: migration libraryには存在するがCLIから使えない`rollback/status/schema/seed/repair`を、既存の破壊的操作の安全策とCLIエラー契約に合わせて接続する。

**完了条件**: 生成アプリの実DBで正常・失敗・dirty・改変・同時実行を検証し、`reset`は確認なしの本番破壊を許さない。利用者ガイドのコマンド例とCLIの実装が一致する。

**実装・検証**: `db migrate/status/rollback/repair/reset`をmigration APIへ接続し、statusに可逆性を表示。`db schema`は`pg_dump`、`db seed`は登録済み`task-seed`を実行し、未登録・失敗は非ゼロ。resetはDB名・環境・破壊許可を必須にし、本番は追加許可、`KOUGA_ENV`設定時は一致を検査、`--seed`未登録は削除前に拒否する。生成アプリの専用PostgreSQL DBで同時migrate一度だけ、通常DDL失敗、不可逆rollbackの全件事前拒否、改変検出、非transactional dirtyと改変時repair拒否・手動修復後のpending復旧、resetの拒否と成功を確認。Docker内の実`pg_dump`経由でschemaの成功とデータ・所有者・権限の非出力を確認。CLI失敗は非ゼロで、出力にDB接続URLを含まない。`cargo +1.94.0 fmt --all --check`、workspace全体clippy `-D warnings`、workspace全体test、実DB CLIテストが成功。seedの登録処理は利用アプリ側で実装する契約とし、テストでは登録・未登録・異常終了のCLI伝播を確認（モデルAPIを使う個別seedの内容は利用アプリ次第）。

### T40 — 初版の残件再監査

- 状態: レビュー待ち
- ブランチ: `task/T40-release-closure`
- worktree: `.worktrees/T40-release-closure`
- 依存: T36、T37、T38、T39
- 対応仕様: 第5・6節、全受け入れ条件

**実装すること**: T34の[監査表](release-verification.md)を実証に基づいて更新し、単一生成アプリの全シナリオ、実サービス・依存グラフ・配布/互換性方針を再判定する。

**完了条件**: 必須の未達が残れば「初版完成」とせず、追加の実装タスクを明示する。固定条件の性能測定を再実施し、回帰・制約・未検証を記録する。公開・deployは行わない。

**T40検証・判定**: [現行の横断再監査](release-verification.md)に第5節、基本22条件と追加条件、第6節14シナリオの証拠/未達を記録。最新CLIで単一Taskboardを再生成し、Rust 1.94のfmt、生成workspace clippy `-D warnings`、実PostgreSQL/ローカルSMTP/別Channel processを含む生成workspace全テストを通過。Kouga本体fmt/clippy `-D warnings`も成功。T40はRust実装を変更しない監査/計測タスクのため、本体workspace全テストは重複再実行せず、T37/T38統合時の実DB付き全テストを証拠とする。通常依存treeのHTTP/gRPC/worker分離を確認。HTTP/admin/gRPC/auth-mail-workerの統合版Docker release imageを個別ビルドし、非root/read-onlyでadminの5 migration、HTTP health/認証付きCRUD、HTTP発行tokenで別gRPCコンテナの業務RPC、workerワンショットexit 0、HTTP/gRPCのSIGTERM exit 0を確認。固定条件のk6 4 VU×15秒を再実施（数値は監査文書）。Taskboard独自通知worker/Channel/添付清掃のDocker target欠落、Taskboard OTel/seed/OpenAPI差分/DB停止等、実S3・公開互換性は必須未達。T41〜T43を後続とし、**初版完成とは判定しない**。push/deployはしていない。

### T41 — Taskboard全役割の独立イメージ

- 状態: レビュー待ち
- ブランチ: `task/T41-taskboard-images`
- worktree: `.worktrees/T41-taskboard-images`
- 依存: T40
- 対応仕様: 3.2〜3.3、4.10、4.12〜4.13、第5節、第6節12・14

**実装すること**: 生成Dockerfileをアプリ固有binaryにも拡張できる公開手順/設定にし、Taskboardの`task-notice-worker`、`taskboard-channel`、`taskboard-storage-cleanup`をHTTP/gRPC/auth-mail-worker/adminから独立したtargetとして再生成可能にする。必要なコードとCA/DNSだけを含め、不要なHTTP/SMTP/gRPC依存を混入させない。

**完了条件**: 全targetをLinux/arm64で個別release buildし、非root・read-only root、PORT/終了シグナル、外部DB・SMTP・共有storageで実起動。Task作成→別通知worker→mail、別Channelへの通知、添付清掃ワンショットと旧payload互換更新をイメージ間で確認。各展開/圧縮サイズと通常依存treeを記録する。クラウドdeploy/pushは行わない。

**T41実装・公開API**: `kouga dockerfile --binary TARGET=PACKAGE_DIR:BINARY`を繰り返し指定し、生成アプリ固有の実行ファイルを独立targetへ追加できる。target/package/binary名、アプリ外path、重複、対応する`src/bin/*.rs`の有無を生成前に検証し、不正入力でDockerfileを部分更新しない。Taskboard生成scriptは通知worker、Channel、添付清掃を独立targetへ登録する。Channelと清掃は専用Cargo packageで、Channelは共有realtime policyを直接利用し、通常依存treeにHTTP/SMTP/gRPC/OTel worker runtimeを含まない。手順はTaskboard READMEと利用者向け配備文書へ記載した。

**T41検証**: Rust 1.94の本体workspaceでfmt、Clippy `-D warnings`、実PostgreSQL付き全テストが成功。T42統合後に再生成したTaskboard fixtureでもfmt、Clippy、実PostgreSQL付き全workspaceテストが成功した。通知workerのクラッシュ回収試験は短すぎるテスト用leaseを2秒にし、固定sleepに代えてDBの`lease_until`失効を待つよう安定化した。T42統合後fixtureの7 targetはすべてLinux/arm64 release build済み。最終再生成fixtureとの差分は通知worker本体/試験と自動生成されたauth migrationの時刻付きファイル名のみで、Dockerfileと他5 targetのコードは同一と`diff -qr`で確認し、影響する通知worker/admin imageを再ビルドした。

| target | 展開image bytes | `docker save` gzip bytes |
|---|---:|---:|
| http | 119806716 | 37041329 |
| grpc | 107090668 | 32437587 |
| worker (auth mail) | 105509892 | 31828548 |
| task-notice-worker | 105509964 | 31821869 |
| taskboard-channel | 103604164 | 31101868 |
| taskboard-storage-cleanup | 101637652 | 30239666 |
| admin | 101381817 | 30186513 |

圧縮値はローカル`docker save | gzip`の参考値で、registry転送量ではない。全imageがarm64、`USER 65532:65532`で、CA bundleを保持する。T42統合前の7 imageを個別非root/read-onlyで実起動し、外部PostgreSQLで5 migration、HTTP/別gRPCの認証付き業務連携、HTTP→別Channel WebSocket通知、HTTP/gRPC→別通知worker→実SMTP/TLS（CA未信頼時の失敗後に信頼して再送）、旧v1 payload、共有storageの添付削除→別清掃ワンショット、PORT・DNS・SIGTERM正常終了を確認した。auth mail workerも空queueのワンショットexit 0。T42統合後の再生成imageではadminを専用新DB・非root/read-onlyで起動して5 migration成功、通知workerも同条件で空queueの`--once` exit 0を確認した。実クラウドdeploy/pushおよびT42統合後7 imageの全役割再実起動は未実施で、S3/障害横断の再監査はT43へ引き継ぐ。

### T42 — 単一アプリの観測・OpenAPI・運用経路

- 状態: 統合済み
- ブランチ: `task/T42-taskboard-observability`
- worktree: `.worktrees/T42-taskboard-observability`
- 依存: T40（T41と並行可能）
- 対応仕様: 4.2、4.15〜4.18、第5節、第6節1・10・11・13

**実装すること**: Taskboardへ登録済みseed、DB readiness、複数HTTP process共有rate-limit、OTel exporterと独自span/log/metricsを接続。HTTP/gRPC/worker別`service.name`とjob contextをテストCollectorで追い、Collector停止中も上限付きbuffer/業務継続、ワンショットflushを検証する。CRUD/認証/添付を含む生成OpenAPIのschema・開発UI・本番非公開、Request/認証変更後の`kouga openapi check`失敗→再生成成功を通し試験にする。

**完了条件**: 同一生成アプリでDB停止時readiness/副作用なし、rate超過、終了処理、同時HTTP/gRPC/workerのcontext分離と機密非記録を確認。seedとOpenAPIの文書化コマンドを実行し、変更時差分をCIで検知する。性能再計測は環境と反復回数を固定して記録する。

**T42実装・公開API**: 生成Taskboardに`task-seed`（登録済み利用者/Project/Task、冪等・本番禁止）、`GET /ready`（DB疎通）、PostgreSQL共有rate-limit（`BOARD_RATE_LIMIT_PER_MINUTE`）、HTTP/gRPC/通知worker別のOTel設定・独自span/log/metric、ジョブtrace context、停止時のbest-effort flushを追加。添付のbinary応答を含むOpenAPI schema、`check-openapi.sh`、CI差分検査を追加。Collector確認用`test-collector.py`と固定条件計測用`benchmarks/t42-taskboard.sh`を追加した。生成コマンドと環境変数はTaskboard READMEに記載。

**T42検証**: Rust 1.94の本体workspace/生成fixture双方でfmt、Clippy `-D warnings`、全テストを通過（実PostgreSQL使用）。専用DBでmigration 5件とseed 2回を実行し、登録件数1/1/1、本番seed拒否を確認。生成OpenAPIのCRUD/認証/添付schema、開発UIと本番404、Request制約/認証設定変更で`openapi check`失敗→復元・再生成成功を確認。2つのHTTP processで同一IPのrate上限を共有し429と`Retry-After`、DB停止時readiness 503・副作用なし、両processのSIGTERM exit 0を確認。実CollectorでHTTPのtraces/metrics/logs、通知workerのジョブspanとtraceparent link、gRPCの独立`service.name`を受信。Collector停止中の業務継続、bounded shutdown、テスト用秘密値の非記録は自動結合試験で確認した。

**T42性能・残件**: macOS arm64、Rust debug、Docker PostgreSQL 17、k6 4 VU×15秒×各2回。JSONは502.93/450.57 req/s、readは47.82/70.35 req/s、CRUDは65.24/20.99 req/s、全run失敗率0。共有ホストでT41のビルドと時間帯が重なるため参考値であり、T40比較・本番性能判定には使わない。実認証付きgRPC業務RPCからworkerまでの同時context分離、全役割Docker imageと障害横断はT41/T43で再確認する。クラウドdeploy/pushなし。

### T43 — 実ストレージ・境界/障害横断・配布方針

- 状態: 統合済み（初版公開の残件はT44、サポート期間は判断待ち）
- ブランチ: `task/T43-release-hardening`
- worktree: `.worktrees/T43-release-hardening`
- 依存: T41、T42
- 対応仕様: 4.4、4.9、4.12、第5・6節、公開前の互換性方針

**実装すること**: TaskboardのS3互換storageを実サービスで確認し、複数process/同時更新、worker/SMTP/DB/Collector障害・再起動、添付付きメール、タイムアウト後の副作用と境界上限を統合イメージで再検証する。ローカル絶対path依存を前提としない配布方式、Kouga/生成コード/ジョブpayloadの互換性・更新順序・サポート期間を定め、公開前のライセンス/依存素材監査を行う。

**完了条件**: 第5節の制約と第6節14シナリオを単一アプリで再監査し、未達/実クラウド未検証を明示。Rust 1.94、Linuxイメージ、実サービス、性能の再現手順を残す。実クラウドdeployや公開は別途明示依頼があるまで行わない。

**T43実装・公開API**: `kouga_storage::AmazonS3Builder`を公開し、TaskboardのHTTP/清掃/通知workerが共通`BOARD_STORAGE_BACKEND=local|s3`設定を使用する。S3ではbucket/region/custom endpoint、AWS標準資格情報を使用し、本番のHTTP許可を拒否する。Taskboardには所有者限定`POST /tasks/{id}/attachments/{file_id}/email`（202、`data.job_id`）と`taskboard.attachment_mail` v1 jobを追加。HTTPはjob投入のみ、別workerが所有者/添付状態を再確認して実storageから取得しSMTP送信する。`package-source.sh`はcleanなKouga commitの追跡済みCargo/cratesソースだけをアプリ内へsnapshot化し、相対path依存、nested workspace除外、commit記録を追加する。既存snapshotを上書きしない。Kougaの汎用`kouga new`が最初から相対pathになるわけではない。公開API変更と使い方・互換性・更新順序は[配布方針](distribution-compatibility.md)とTaskboard READMEを参照。

**T43検証**: Rust 1.94のKouga本体fmt、全target Clippy `-D warnings`、専用PostgreSQL 17＋実S3（SeaweedFS）付きworkspace全テスト成功（本体Rustの最終変更後）。生成Taskboard Dも全workspace test、全target Clippy、OpenAPI generate/check成功。最後に追加したdebug/test用timeout hook後は生成fixture全target Clippy/fmtと実DB＋S3 `storage_s3`を再実行して成功し、全workspace test/OpenAPI checkはhook前の成功を根拠とする。最新OpenAPI check再実行は容量3.8 GiBで中断した。`kouga-storage`で11 MiB分割保存/署名URL/削除失敗→清掃、生成Taskboardで認証付きS3 upload/download、別router instance共有、他owner 404、危険なファイル名拒否、10 MiB超過413、S3障害時503/非公開/清掃を実行。別worker processがS3添付をMIMEとしてloopback SMTP sinkへ実配送し、偽造owner/削除済み添付は送らない。job確定後に応答を遅らせたクライアントdeadline試験では100 ms timeout後もjobがDBに残存。通常依存treeでHTTPにSMTP/worker、workerにHTTP/OpenAPIが含まれないことを確認。clean commitから新規Taskboardを別path生成・snapshot化し、1.2 MiBのvendorに`.git`/`target`/`.env`と絶対Cargo pathがないこと、全workspace `cargo check --locked --offline`および同snapshotの実DB＋S3試験成功を確認。二重package実行は既存snapshot保護で終了2。実S3用Dockerコンテナ`kouga-t43-s3`と専用DB`kouga-t43-postgres`は試験後に停止し、削除せず残置する。具体コマンド/証拠と14シナリオ判定は[再監査](release-verification.md)冒頭に記録。

**公開前残件・未検証**: T41のLinux/arm64全役割イメージはT42統合版までの証拠であり、T43 S3/添付メール変更版imageのbuild/非root read-only実起動はディスク容量を優先して未実施。実クラウド、外部TLS relay、全役割同時障害/再起動、複数processの同時更新、認証付きgRPC→worker並行trace context分離、サーバー504/gRPC deadline後・SMTP受理後の副作用、S3/worker/Collectorを同時に動かす性能計測は未実施。ライセンスmetadataはregistry由来404 packageで欠落0だが、Kougaライセンス本文・third-party notices/SBOMは未整備。0.x公開後のサポート期間/EOLは対外的な約束となるためユーザー判断待ち。未実証の必須範囲はT44に引き継ぐ。**初版公開可能とは判定しない**。push/deployなし。

### T44 — 公開前の複合障害・配布・ライセンス仕上げ

- 状態: レビュー待ち（公開判定保留。法務・公開条件は未達）
- 依存: T43レビュー・統合後
- 対応仕様: 第5・6節の未実証範囲、配布物と法務上の公開条件
- ブランチ: `task/T44-final-hardening`
- worktree: `.worktrees/T44-final-hardening`

**実装・検証すること**: T43変更を含む7役割のLinux release imageをcleanなsource snapshotと`Cargo.lock`からbuildし、非root/read-onlyで起動する。実PostgreSQL・実S3・SMTP sink・Collectorを同時に接続し、別HTTP/gRPC/worker/Channel/cleanup processで同時更新・worker強制終了/再取得・DB/SMTP/S3/Collector停止と復旧・reset後WebSocket失効・認証付きgRPC→workerの並行trace context分離を故障注入する。server 504/gRPC deadline後のDB/SMTP副作用と重複抑止境界、10 MiB添付/11 MiB本文/queue/connection上限を実測し、固定条件で性能を再計測する。外部TLS relay・クラウドIAM/署名等は安全な検証環境がある場合のみ実施し、未実施なら限定と記す。

**配布監査と完了条件**: 実際のrelease targetごとにCargo依存・同梱資産/ベースイメージのライセンスを棚卸し、権利者を確認したKougaライセンス本文、third-party notices、SBOMを整備して法務レビューへ渡す。脆弱性・MSRV・再現可能なtag/commit・移行順序も確認する。サポート期間/EOLはユーザー判断を受けて初版公開前に文書化する。仕様第6節14件を単一配布Taskboardで再判定し、未達があれば公開判定を保留する。push/deploy/公開は別途明示依頼があるまで行わない。

**T44検証結果（公開判定保留）**: 現行CLIで再生成したTaskboardを最終clean commit `320082b`のsource snapshotへ固定し、実DB全テスト・実S3専用テスト・fmt/全target Clippy・OpenAPI差分検査を実施。同snapshotから7役割Linux/arm64 imageをbuildし、全役割の非root/read-only起動、実DB・実S3・SMTP sinkでgRPC→worker通知と添付メール配送を確認。先行故障試験と最終image IDが同じ5役割を照合し、DB/S3停止・復旧、失敗後清掃、実worker kill→lease再取得、10/11 MiB境界の結果を継承。生成fixtureのdebug統合テストでserver 504とgRPC deadline後のDB/job確定が成功。最終imageの認証付きHTTP/gRPC並行作成から別workerへの2 trace分離、生成fixtureのreset後WebSocket close、固定条件のHTTP＋Collector軽量性能も再測定。結果・試験条件・未達は[再監査](release-verification.md)のT44節に記録。役割別Rust依存CycloneDX候補と7 imageのDebian OS package inventoryは[配布方針](distribution-compatibility.md)に記録。Kouga権利者/ライセンス本文、法的third-party notices、CVE scan、SMTP受理直後故障、全役割同時復旧、外部TLS/クラウドIAM、14件一括合格は未完了。公開可能とは判定しない。
