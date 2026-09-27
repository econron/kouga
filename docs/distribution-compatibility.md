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

### T44の役割別Rust依存インベントリ（法務承認前）

ソースsnapshot化したTaskboardに対し、Python 3.11以上で`python3.12 examples/taskboard/release-inventory.py APP_DIRECTORY OUTPUT_DIRECTORY`を実行すると、7つのDocker targetごとにCycloneDX 1.6 JSONを出力する。`Cargo.lock`と`cargo tree --target aarch64-unknown-linux-gnu -e normal --locked --offline`を照合し、固定したKouga commit、registry checksum、Cargo metadataのライセンス申告を記録する。`admin`と`http`、2種類のworkerはそれぞれ同じCargo packageの通常依存をビルドするため、同じ候補集合となる。これは**リンク済みbinaryの厳密な同梱一覧ではなく、Cargo通常依存の候補インベントリ**である。ビルド依存・dev依存は含めず、ベースイメージのDebian package、CA証明書、Swagger UI等の埋込素材、実際の権利者/notice義務は別途監査する。生成JSONをそのまま法的なthird-party noticesや完成SBOMとして公開しない。

T44のclean commit `84a4d31`から固定したTaskboardでのRust package候補はHTTP/admin各296、gRPC 246、認証/通知worker各245、Channel 186、storage cleanup 192。ローカルTaskboard packageはライセンス未申告。T43の古いsnapshotでのworker 221件は現行結果として使わない。T44の最終固定commitから再生成した成果物でも件数・欠落・OS素材を再監査する。Kouga本体のライセンス本文・著作権者は権利者確認後に確定し、未確認のまま推定して追加しない。

同じ7つのLinux/arm64 release imageをTrivy 0.66の`--scanners license --format cyclonedx --skip-db-update --offline-scan`で個別に棚卸ししたところ、各imageでDebian 12.15のOS package 88件とOS component 1件が得られた。Trivyは静的Rust binaryからCargo packageを検出しなかったため、上記のRust候補インベントリと**両方**を法務レビューに渡す。`libcrypt1`、`libgcc-s1`、`libstdc++6`はTrivyのlicense欄が空だが、3件ともimage内の`/usr/share/doc/<package>/copyright`が存在することを確認した。内容と義務は未判定。ビルドステージからコピーするCA bundle、埋込Swagger UI、OpenAPI schemaと各copyright/NOTICE義務も別途確認が必要。これらの候補インベントリは権利者を確定したthird-party notices本文の代わりにならない。脆弱性DBを用いた現在のCVEスキャンも未実施である。
