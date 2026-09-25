# Kouga — worktree単位の開発タスク

作成日: 2026-09-25  
状態: T00〜T05・T07〜T09・T11・T20・T26統合済み。T06・T10・T12・T15・T17・T21・T22レビュー待ち
対象: 初版の全機能（35タスク）

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
| T06 | migrationの巻き戻し・復旧・管理操作 | T05 | レビュー待ち |
| T07 | validationの基本型と実行 | T02 | 完了 |
| T08 | Requestのderiveと検証メタデータ | T07 | 完了 |
| T09 | HTTP router・controller・レスポンス | T03、T08 | 完了 |
| T10 | middleware基盤と標準middleware | T09 | レビュー待ち |
| T11 | modelのCRUD・query実行 | T04 | 完了 |
| T12 | modelのderive・属性型生成 | T11 | レビュー待ち |
| T13 | association・preload | T12 | 未着手 |
| T14 | OpenAPI生成と開発用Docs | T08、T09、T10 | 未着手 |
| T15 | CLI基盤と新規アプリ生成 | T03、T09 | レビュー待ち |
| T16 | model・resource・Requestのgenerator | T06、T12、T14、T15、T17 | 未着手 |
| T17 | テスト支援基盤 | T04、T05、T09 | レビュー待ち |
| T18 | 認証の共通処理・policy | T10、T12、T17 | 未着手 |
| T19 | 認証API・リセット・auth生成 | T18、T21、T22、T23、T16 | 未着手 |
| T20 | ジョブ契約・queue投入 | T04 | 完了 |
| T21 | worker・retry・ワンショット | T20、T03 | レビュー待ち |
| T22 | mailer・SMTP・メールテスト支援 | T20、T03 | レビュー待ち |
| T23 | キャッシュ・共有レート制限 | T04、T10 | 未着手 |
| T24 | アップロード・ストレージ | T10、T18、T21 | 未着手 |
| T25 | WebSocket・複数サーバー配信 | T10、T18 | 未着手 |
| T26 | 計測基盤・OTel exporter | T03、T04 | 完了 |
| T27 | 処理間のtrace連携 | T26、T10、T21、T22、T28 | 未着手 |
| T28 | gRPC入口・Protobuf・handler | T03、T04、T07、T18 | 未着手 |
| T29 | HTTP/gRPC同居と追加generator | T28、T15、T16 | 未着手 |
| T30 | 補助CLI・機能追加generator | T06、T19、T21、T22、T24、T25、T26、T29 | 未着手 |
| T31 | 役割別Dockerイメージ | T29、T30 | 未着手 |
| T32 | 配備先への実行対応 | T31、T27 | 未着手 |
| T33 | 利用者ガイドと通しのサンプル | T13、T14、T19、T24、T25、T27、T32 | 未着手 |
| T34 | 初版の横断検証・計測 | T33 | 未着手 |

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

- 状態: 未着手
- 担当者: 未割当
- ブランチ: `task/T06-migration-admin`
- worktree: `.worktrees/T06-migration-admin`（作成前）
- 依存: T05
- 対応仕様: 4.5.2、4.5.3
- 主担当領域: migration管理API・管理用SQL実行

**実装すること**: rollback、非トランザクション適用とdirty/repair、DB作成/reset、schema出力、seed実行を追加する。

**完了条件**

- [ ] 不可逆な対象を含むrollbackは全件未変更で拒否し、非トランザクション中断から手動修復後に継続できる。
- [ ] 破壊操作の明示許可、schema出力、seedを確認する。reset時の接続終了と排他も検証し、通常起動でmigrationしない。

**今回含めないこと**: クラウドのDB作成、バックアップ復元、自動的な破壊変更。

**検証結果・後続への引き継ぎ**: 未記入。

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

- 状態: 未着手
- 担当者: 未割当
- ブランチ: `task/T10-middleware`
- worktree: `.worktrees/T10-middleware`（作成前）
- 依存: T09
- 対応仕様: 4.2、4.2.1、3.1
- 主担当領域: HTTP middleware

**実装すること**: 通常の非同期関数と消費するNextを使う登録API、標準middlewareを提供する。

**完了条件**

- [ ] 順序・途中終了・extensions、CORS/preflight、request ID・アクセスログ・共通エラーを検証する。
- [ ] timeout・サイズ制限・信頼プロキシ・同時処理上限を検証し、streamingを壊さない。共有レート制限を後付けできる。

**今回含めないこと**: 認証ストア、共有レート制限ストア。

**検証結果・後続への引き継ぎ**: 未記入。

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

- 状態: 未着手
- 担当者: 未割当
- ブランチ: `task/T12-model-derive`
- worktree: `.worktrees/T12-model-derive`（作成前）
- 依存: T11
- 対応仕様: 4.4.1、4.4.2、4.4.4
- 主担当領域: model用proc macro

**実装すること**: model宣言から型付き列、New/Update属性とCRUD接続を生成する。

**完了条件**

- [ ] 型マッピング・NULL・enum・既定値・日時列と、生成コードからのCRUDを検証する。
- [ ] CRUDを非公開にして業務メソッドへ集約できる。不正なmodel宣言をコンパイル時に拒否する。

**今回含めないこと**: Requestへの自動変換、HTTP出力へのmodel全属性の自動公開。

**検証結果・後続への引き継ぎ**: 未記入。

### T13 — association・preload

- 状態: 未着手
- 担当者: 未割当
- ブランチ: `task/T13-associations`
- worktree: `.worktrees/T13-associations`（作成前）
- 依存: T12
- 対応仕様: 4.4.3
- 主担当領域: 関連宣言・関連query・関連取得結果

**実装すること**: 四種類の関連を明示的に読み、一括取得できるAPIを追加する。必要なmacro変更も本タスクが所有する。

**完了条件**

- [ ] belongs_to/has_one/has_many/多対多、空関連・未存在・未取得の区別を検証する。
- [ ] preloadのSQL件数、ID分割、ページ境界、関連の認可条件と明示したネストだけの取得を確認する。

**今回含めないこと**: 暗黙のlazy loading、関連の自動保存、polymorphic関連。

**検証結果・後続への引き継ぎ**: 未記入。

### T14 — OpenAPI生成と開発用Docs

- 状態: 未着手
- 担当者: 未割当
- ブランチ: `task/T14-openapi`
- worktree: `.worktrees/T14-openapi`（作成前）
- 依存: T08、T09、T10
- 対応仕様: 4.18
- 主担当領域: OpenAPI生成・開発Docs

**実装すること**: 共通メタデータからOpenAPI 3.1.1のYAMLと開発用閲覧UIを提供する。

**完了条件**

- [ ] 型・検証・認証・エラー・multipart・本文なし応答が仕様に一致し、未解決参照や重複operation IDを拒否する。
- [ ] 外部サービスなしの決定的生成、差分check、失敗時の元ファイル保持、本番での標準非公開を検証する。

**今回含めないこと**: gRPCからOpenAPIへの自動変換、独自UIの開発。

**検証結果・後続への引き継ぎ**: 未記入。

### T15 — CLI基盤と新規アプリ生成

- 状態: 未着手
- 担当者: 未割当
- ブランチ: `task/T15-cli-core`
- worktree: `.worktrees/T15-cli-core`（作成前）
- 依存: T03、T09
- 対応仕様: 4.17、3.2
- 主担当領域: CLIの共通処理・new/server/routes

**実装すること**: 新規アプリの作成、開発起動、ルート表示と安全なファイル生成を実装する。

**完了条件**

- [ ] 新しい一時ディレクトリから生成アプリがビルド・起動できる。
- [ ] 名称・生成先・衝突検査、既存ファイル保護、失敗終了コード、秘密情報の非表示を確認する。

**今回含めないこと**: resourceなどの機能別テンプレート、クラウドの公開操作。

**検証結果・後続への引き継ぎ**: 未記入。

### T16 — model・resource・Requestのgenerator

- 状態: 未着手
- 担当者: 未割当
- ブランチ: `task/T16-resource-generator`
- worktree: `.worktrees/T16-resource-generator`（作成前）
- 依存: T06、T12、T14、T15、T17
- 対応仕様: 4.17、4.18、利用者ガイド最初のAPI
- 主担当領域: CRUD関連CLIサブコマンド・テンプレート

**実装すること**: model/migration/Request/controller/出力/route/テストを一緒に生成し、OpenAPIも連携する。

**完了条件**

- [ ] 利用者ガイドの最初のAPIを新規生成から実行でき、入力不正・PATCH・作成と更新を検証する。
- [ ] 生成コードが通常のRustとして編集でき、ルートの安全な自動登録または差分提示を行う。

**今回含めないこと**: auth/jobなどの後続generator、編集済みコードの強制上書き。

**検証結果・後続への引き継ぎ**: 未記入。

### T17 — テスト支援基盤

- 状態: 未着手
- 担当者: 未割当
- ブランチ: `task/T17-test-support`
- worktree: `.worktrees/T17-test-support`（作成前）
- 依存: T04、T05、T09
- 対応仕様: 4.16
- 主担当領域: HTTP/DBテスト支援

**実装すること**: ポート不要のリクエストテスト、専用DB構築、データ分離と認証主体注入の接続口を提供する。

**完了条件**

- [ ] 生成テストがcargo testで動作し、並列テストと開発DBのデータを分離する。
- [ ] 正常/不正入力/DB障害を再現でき、認証主体の注入が認可を省略しない。

**今回含めないこと**: 全機能のテストの集中実装。メール・ストレージ固有支援は各機能担当が持つ。

**検証結果・後続への引き継ぎ**: 未記入。

### T18 — 認証の共通処理・policy

- 状態: 未着手
- 担当者: 未割当
- ブランチ: `task/T18-auth-core`
- worktree: `.worktrees/T18-auth-core`（作成前）
- 依存: T10、T12、T17
- 対応仕様: 4.3、4.2.1
- 主担当領域: 認証runtime・policy・認証middleware

**実装すること**: パスワード処理、token保存・期限・失効、CurrentUser、policyと所有者範囲を提供する。

**完了条件**

- [ ] 平文・tokenの非保存、失効・期限・未認証の拒否、DB障害と認証失敗の区別を確認する。
- [ ] 所有者以外の取得/更新/一覧への混入を拒否し、照合・policyをgRPCからも再利用できる。

**今回含めないこと**: メールリセットのAPI、OAuth/OIDC/MFA。

**検証結果・後続への引き継ぎ**: 未記入。

### T19 — 認証API・リセット・auth生成

- 状態: 未着手
- 担当者: 未割当
- ブランチ: `task/T19-auth-api`
- worktree: `.worktrees/T19-auth-api`（作成前）
- 依存: T18、T21、T22、T23、T16
- 対応仕様: 4.3、4.17
- 主担当領域: 認証HTTP API・authテンプレート

**実装すること**: 登録・ログイン・ログアウト・現在ユーザー・パスワードリセットと、そのgeneratorを実装する。

**完了条件**

- [ ] 生成アプリでtoken発行から失効・メールリセットまで実行できる。
- [ ] 並行リセットで一度しかtokenを使えず、試行制限・存在を漏らさない応答・認証OpenAPIを確認する。

**今回含めないこと**: 外部認証サービスとの連携。

**検証結果・後続への引き継ぎ**: 未記入。

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

- 状態: 未着手
- 担当者: 未割当
- ブランチ: `task/T21-queue-worker`
- worktree: `.worktrees/T21-queue-worker`（作成前）
- 依存: T20、T03
- 対応仕様: 4.10、3.3
- 主担当領域: queue実行crate・worker runtime

**実装すること**: handler登録、実行権、retry、管理操作、常駐/ワンショットと終了を実装する。

**完了条件**

- [ ] 複数worker・強制終了・lease再取得・旧workerの完了拒否・retry/dead/未知payloadを検証する。
- [ ] 件数/時間/空queueの終了、待機中キャンセル、失敗表示・再投入、graceful shutdownを確認する。

**今回含めないこと**: SMTP処理、クラウドの起動スケジューラー。

**検証結果・後続への引き継ぎ**: 未記入。

### T22 — mailer・SMTP・メールテスト支援

- 状態: 未着手
- 担当者: 未割当
- ブランチ: `task/T22-mailer`
- worktree: `.worktrees/T22-mailer`（作成前）
- 依存: T20、T03
- 対応仕様: 4.9、4.16、3.2
- 主担当領域: mailer runtime・テンプレート・メール検査

**実装すること**: テキスト/HTML/添付、同期送信とジョブ投入、開発用記録を提供する。

**完了条件**

- [ ] SMTPへの送信、TLS検証、HTML escape、ヘッダー注入拒否、失敗時の結果を検証する。
- [ ] HTTP側からSMTP実装を除外でき、worker handlerへ組み込める。外部送信なしでメールを検査できる。

**今回含めないこと**: SMTPサーバー運用、メール送信のexactly-once保証。

**検証結果・後続への引き継ぎ**: 未記入。

### T23 — キャッシュ・共有レート制限

- 状態: 未着手
- 担当者: 未割当
- ブランチ: `task/T23-cache-rate-limit`
- worktree: `.worktrees/T23-cache-rate-limit`（作成前）
- 依存: T04、T10
- 対応仕様: 4.11、4.2
- 主担当領域: cache runtime・レート制限の追加middleware

**実装すること**: メモリ/DBキャッシュと、複数プロセスで共有するレート制限を実装する。

**完了条件**

- [ ] TTL・容量上限・名前空間・清掃と、キャッシュ障害時の元データ取得を検証する。
- [ ] 同時要求でも上限を守り、429/Retry-Afterを返す。認可に関わる制限をキャッシュ同様にfail-openしない。

**今回含めないこと**: stampede完全防止、キャッシュを使った業務処理の一度限り保証。

**検証結果・後続への引き継ぎ**: 未記入。

### T24 — アップロード・ストレージ

- 状態: 未着手
- 担当者: 未割当
- ブランチ: `task/T24-storage`
- worktree: `.worktrees/T24-storage`（作成前）
- 依存: T10、T18、T21
- 対応仕様: 4.12、4.16
- 主担当領域: storage runtime・関連metadata・清掃ジョブ

**実装すること**: ローカル/S3互換のstreaming保存、認可付き取得、削除と失敗時清掃を実装する。

**完了条件**

- [ ] 両backendで保存/取得/削除し、サイズ・種別・パス・無認可アクセスを検証する。
- [ ] 途中失敗・未関連ファイル・削除再試行と署名付きURLを確認し、一時ストレージのテスト支援を提供する。

**今回含めないこと**: 画像変換、ウイルススキャン、ブラウザ直接アップロード。

**検証結果・後続への引き継ぎ**: 未記入。

### T25 — WebSocket・複数サーバー配信

- 状態: 未着手
- 担当者: 未割当
- ブランチ: `task/T25-websocket`
- worktree: `.worktrees/T25-websocket`（作成前）
- 依存: T10、T18
- 対応仕様: 4.13
- 主担当領域: channel runtime・配信・接続ticket

**実装すること**: 認証・購読認可・DBによるサーバー間通知・接続管理を実装する。

**完了条件**

- [ ] 一度限りのticket、Origin、購読/操作、期限/失効、接続切断を検証する。
- [ ] 別プロセスへの配信、サイズ上限、遅い受信者、heartbeatと再接続後のHTTP取得を確認する。

**今回含めないこと**: 永続配信・切断中の履歴再送。

**検証結果・後続への引き継ぎ**: 未記入。

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

- 状態: 未着手
- 担当者: 未割当
- ブランチ: `task/T27-trace-propagation`
- worktree: `.worktrees/T27-trace-propagation`（作成前）
- 依存: T26、T10、T21、T22、T28
- 対応仕様: 4.15.1、4.19
- 主担当領域: OTel context伝播・各機能の接続

**実装すること**: HTTP/gRPCからDB・queue・別worker・mailerまでを関連付ける。

**完了条件**

- [ ] テストCollectorで標準/custom spanとjobのlink、試行番号、service名を確認する。
- [ ] 並行context混入、信頼しない入力、秘密情報、contextなしの既存ジョブ、ワンショットflushを検証する。

**今回含めないこと**: 業務payloadへのtrace情報の混入、監査ログの配送保証。

**検証結果・後続への引き継ぎ**: 未記入。

### T28 — gRPC入口・Protobuf・handler

- 状態: 未着手
- 担当者: 未割当
- ブランチ: `task/T28-grpc-core`
- worktree: `.worktrees/T28-grpc-core`（作成前）
- 依存: T03、T04、T07、T18
- 対応仕様: 4.19
- 主担当領域: gRPC runtime・生成型接続・検証/認証adapter

**実装すること**: unary RPCの生成・登録・実行を提供し、HTTP非依存の業務コードを呼べるようにする。

**完了条件**

- [ ] metadata認証・入力検証・policy・status変換・deadline・サイズ/負荷制限を検証する。
- [ ] presence/既定値を考慮し、共通modelとtxを利用できる。OTelを後付けできる計測点を用意する。

**今回含めないこと**: 同一ポート多重化、grpc-web、streamingの必須対応。

**検証結果・後続への引き継ぎ**: 未記入。

### T29 — HTTP/gRPC同居と追加generator

- 状態: 未着手
- 担当者: 未割当
- ブランチ: `task/T29-grpc-coexistence`
- worktree: `.worktrees/T29-grpc-coexistence`（作成前）
- 依存: T28、T15、T16
- 対応仕様: 4.17、4.19、利用者ガイドHTTPとgRPC
- 主担当領域: 入口追加CLI・gRPCテンプレート

**実装すること**: gRPC単独作成、add grpc/add http、入口別起動と生成型のビルドを実装する。

**完了条件**

- [ ] HTTP追加前後とgRPC追加前後で既存コード・ルートを維持し、両入口から同じ業務操作を呼べる。
- [ ] 起動対象の既定値・重複追加・既存ファイル保護と、入口ごとの独立ビルドを検証する。

**今回含めないこと**: 同一実行ファイルでの統合起動、OpenAPIと.protoの相互変換。

**検証結果・後続への引き継ぎ**: 未記入。

### T30 — 補助CLI・機能追加generator

- 状態: 未着手
- 担当者: 未割当
- ブランチ: `task/T30-cli-features`
- worktree: `.worktrees/T30-cli-features`（作成前）
- 依存: T06、T19、T21、T22、T24、T25、T26、T29
- 対応仕様: 4.17、4.15.1
- 主担当領域: CLIの残項目・機能別テンプレート

**実装すること**: jobs/maintenance/console/runner、middleware/mailer/job/channel生成、add otelを統合する。

**完了条件**

- [ ] 各コマンドの正常/異常終了を確認し、生成された各機能をビルドして動作させる。
- [ ] 編集済み箇所の保護・差分提示、後付けworkerへの設定継承、秘密情報の非表示を検証する。

**今回含めないこと**: Rust REPL、内蔵cron、デプロイCLI。

**検証結果・後続への引き継ぎ**: 未記入。

### T31 — 役割別Dockerイメージ

- 状態: 未着手
- 担当者: 未割当
- ブランチ: `task/T31-docker-images`
- worktree: `.worktrees/T31-docker-images`（作成前）
- 依存: T29、T30
- 対応仕様: 3.2、3.3
- 主担当領域: Dockerテンプレート・ビルド検証

**実装すること**: HTTP/gRPC/worker/管理用の独立targetを提供し、必要なバイナリと素材のみを含める。

**完了条件**

- [ ] 対象ごとの依存グラフと最終イメージを確認し、ソース・toolchain・未使用runtimeが入らない。
- [ ] 非root・read-only root、CA/TLS・DNS、PORT、終了シグナル、外部DB接続を実行確認する。

**今回含めないこと**: レジストリへのpush、根拠のないサイズ目標。

**検証結果・後続への引き継ぎ**: 未記入。

### T32 — 配備先への実行対応

- 状態: 未着手
- 担当者: 未割当
- ブランチ: `task/T32-platform-runtime`
- worktree: `.worktrees/T32-platform-runtime`（作成前）
- 依存: T31、T27
- 対応仕様: 3.3、4.15.1
- 主担当領域: Lambda adapter・配備先実行設定・手順

**実装すること**: Cloud Run/ECSの実行条件とLambda専用入口を整え、通常イメージへadapterを混入させない。

**完了条件**

- [ ] ローカルで可能なPORT/終了/ワンショットとLambdaイベントadapterを結合検証する。
- [ ] バイナリ本文・headers・認証・flush・期限を確認し、gRPCとの対応差と実クラウド未検証事項を記録する。

**今回含めないこと**: 許可なしの実クラウドdeploy、PostgreSQL queueからの自動クラウド起動。

**検証結果・後続への引き継ぎ**: 未記入。

### T33 — 利用者ガイドと通しのサンプル

- 状態: 未着手
- 担当者: 未割当
- ブランチ: `task/T33-user-journey`
- worktree: `.worktrees/T33-user-journey`（作成前）
- 依存: T13、T14、T19、T24、T25、T27、T32
- 対応仕様: 第6節、user-docs全体
- 主担当領域: 通しのサンプル・利用者文書

**実装すること**: 最初のAPIから全機能までの実行可能なサンプルを用意し、文書のコマンド・コード・出力を実物へ合わせる。

**完了条件**

- [ ] 新規ディレクトリからCRUD/認証/関連/メール/添付/WebSocket/gRPC/OTel/イメージを再現する。
- [ ] コード例をコンパイル・実行し、提案から確定したAPIを文書へ反映する。未実装・未検証を提供済みと表現しない。

**今回含めないこと**: GitHub Pages公開、未検証の性能や導入実績の訴求。

**検証結果・後続への引き継ぎ**: 未記入。

### T34 — 初版の横断検証・計測

- 状態: 未着手
- 担当者: 未割当
- ブランチ: `task/T34-release-verification`
- worktree: `.worktrees/T34-release-verification`（作成前）
- 依存: T33
- 対応仕様: 全機能の受け入れ条件、第5・6節
- 主担当領域: 横断検証・性能/サイズ記録・完成判定

**実装すること**: 仕様の各要件をタスクと検証結果へ対応付け、初版の完成条件を監査する。

**完了条件**

- [ ] 並行更新・障害・再起動・権限・context漏れ・旧payload互換・依存分離の未解決事項を確認し、必須失敗があれば完了にしない。
- [ ] 固定条件でthroughput/latency/memory/起動時間/イメージサイズを測定し、対応版・再現手順・制約・未検証事項を記録する。

**今回含めないこと**: 自動公開・自動release、各機能のテストをここまで先送りすること。

**検証結果・後続への引き継ぎ**: 未記入。
