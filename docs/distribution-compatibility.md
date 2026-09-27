# ソース配布・互換性方針（初版公開前の案）

Kougaのcrateは現時点でcrates.ioに公開していない。`kouga new`が生成するローカル絶対path依存は開発用であり、そのまま配布してはいけない。配布するアプリは、レビュー済みのKouga commitをアプリ内`vendor/kouga`へソースsnapshotとして固定する。Taskboardでは`examples/taskboard/package-source.sh APP_DIRECTORY KOUGA_CHECKOUT`が、**cleanなcheckoutの追跡済み**`Cargo.toml`、`Cargo.lock`、`crates/`だけを`git archive`でコピーし、アプリのKouga依存を相対pathへ書き換え、入れ子workspaceを外側から除外する。`vendor/kouga-revision.txt`にcommit SHAを残す。`.git`、`.worktrees`、`target`、`.env`はarchive対象外で、スクリプトは既存snapshotを上書きしない。実行後は`cargo +1.94.0 test --workspace --locked`を行い、`Cargo.lock`をアプリ側でcommitする。これは外部crateのオフライン配布ではないため、初回ビルドには通常のCargo registryへのアクセスか事前キャッシュが要る。

Dockerは生成済みDockerfileを維持し、アプリから`docker build --build-context kouga=./vendor/kouga --target http ...`を実行する。named contextに同じsnapshotを渡し、アプリのbuild context内のvendor copyを実行用イメージへ入れない。各targetは必要なbinaryだけを最終イメージへ含む。snapshot更新ではまずKouga側でcommitを確定し、新しい空のアプリまたはレビュー済みの差分で再生成・再packageする。`package-source.sh`は既存vendorを自動置換しない。現時点でGitHub release、crate publish、registry pushは行っていない。

## 互換性と更新順序

- Rust最低版は1.94.0、edition 2024。0.x期間中は同じminor内のpatchで公開Rust API、生成CLIの入力、設定名、既存JSON error codeを破壊しない。新しいminorでは破壊的変更を許すが、変更表と移行手順を先に出す。1.0以降の安定期間は公開前に再定義する。現時点でサポート済みリリース系列は存在しない。
- 生成コードは利用アプリのソースであり、Kougaのバージョンを上げても自動で上書きしない。生成物・Kouga source revision・`Cargo.lock`を一組として検証する。異なるKouga minorの生成CLIで既存アプリへ追加生成する際は、差分と既存編集の衝突をレビューする。
- DB migrationは先に後方互換の追加（expand）を適用し、両版が稼働可能になってからproducer/入口を更新する。旧列や旧契約の削除（contract）は全旧processと旧queue payloadがなくなってから別migrationで行う。適用済みSQLを書き換えず、rollbackでデータ復元を保証しない。
- Jobは`name`と`version`を契約とする。新payloadを投入する前に対応workerを配備し、旧payloadがpending/retry/deadから消えるまで旧handlerを保持する。Taskboardの`taskboard.task_created`はv1とv2を現時点で並行受理する。未知versionは隔離する。ジョブはat-least-onceであり、DB効果はjob IDで冪等化するが、SMTP受理後のack失敗は重複メールになり得る。
- HTTPとgRPCのwire契約はそれぞれOpenAPI/Protobufからレビューする。フィールド削除・意味変更・既存の認証要件緩和/強化をpatchに混ぜない。HTTP/gRPC/worker/Channel/cleanupは独立イメージで更新できるが、共有DB schemaとjob payloadの互換窓を保つ。
- セキュリティ修正はサポート対象minorへ必要に応じてbackportする方針。ただし公開前である現段階ではサポート期間、SLA、EOL日は未設定であり、公開時に明記する。

## ライセンス・素材監査（2026-09-26）

`cargo +1.94.0 metadata --locked --offline --format-version 1`でKouga workspaceのregistry由来404 packageを列挙し、`license`と`license_file`の両方が欠落するpackageは0件だった。再現用の確認式は`cargo +1.94.0 metadata --locked --offline --format-version 1 | jq '[.packages[] | select(.source != null)] | length'`と、同じmetadataの`[.packages[] | select(.source != null and .license == null and .license_file == null)] | length`で、それぞれ404/0となる。主な式は`MIT OR Apache-2.0` 221件、`MIT` 77件、`Apache-2.0 OR MIT` 23件、`Unicode-3.0` 18件、`Apache-2.0` 16件で、残り49件にはBSD、ISC、Zlib、CDLA-Permissive-2.0等が含まれる。`r-efi`等の選択肢にLGPLが含まれていても、MIT/Apacheを選べる式である。これは**ライセンス適合の法的承認ではない**。metadataは全workspaceの候補依存であり、release targetごとの実際のリンク/同梱、ライセンス本文、notice義務、ベースイメージOS packageやvendored Swagger UI素材を網羅しない。公開前にtarget別SBOM/third-party noticesを生成・レビューする。

Kouga workspaceは`MIT OR Apache-2.0`を宣言する一方、現在のGit追跡ファイルにはトップレベルのライセンス本文がない。公開前に権利者を確定し、MITとApache-2.0本文を追加する必要がある。Swagger UI vendored素材とOpenAPI schemaの版・checksum・ライセンス参照は[API契約](api-contracts.md)に記録済み。検証用SeaweedFSコンテナはKougaの配布物へ含めない。実クラウド、外部TLS relay、全プラットフォームの法務・脆弱性監査は未実施。
