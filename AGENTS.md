# Kouga リポジトリの作業前提

日本語で簡潔かつ丁寧に回答してください。

## 何を作るか

KougaはRailsのAPIモードに着想を得たRust製APIフレームワークです。CLI名は`kouga`。HTTP/JSONとgRPC/Protobufを同じプロジェクトで併用し、業務コードを共有しながら入口・workerごとに別のバイナリとコンテナイメージを作ります。標準DBはPostgreSQL、非同期ランタイムはマルチスレッドTokioです。

HTTPの基本経路はmiddleware → Requestの検証 → controller → model → DB。入力検証はmodelから分け、業務上の不変条件はHTTP以外の経路でも守り、整合性はDB制約で担保します。ルート・Request・レスポンスの同じ定義からOpenAPI 3.1.1を生成します。mail/queue/worker、認証、計測などの機能範囲は仕様書を参照してください。

## 正本と現在地

- [docs/specification.md](docs/specification.md): 初版の機能範囲と受け入れ条件。
- [docs/api-contracts.md](docs/api-contracts.md): 公開型、crate境界、採用ライブラリの合意済み契約。具体的なAPI記法はこちらを優先。
- [docs/development-tasks.md](docs/development-tasks.md): T00〜T34の依存・担当範囲・進捗。着手前に対象カードと依存を確認する。
- [user-docs/README.md](user-docs/README.md): 利用者体験の草案。記載されたCLI・機能の大半はまだ動かない。実装済みと表現しない。

実装済み範囲を文書の日付やこのファイルから推測せず、タスク表、Git、Cargo workspaceを確認してください。T02時点のworkspaceは`kouga-core`、`kouga-runtime`、`kouga-validation`のみで、後二者は後続タスクの骨格です。

## 実装とworktree

- 1タスク=1ブランチ・1worktree・1レビュー単位。依存タスクがmainへ統合されてから着手する。ブランチ名は`task/Txx-short-name`、worktreeは`.worktrees/Txx-short-name`。
- 既存worktreeの分岐元が古い場合は、その作業状態を確認してからmainを取り込む。別worktreeの変更や未commit作業を上書きしない。
- `Cargo.toml`、`Cargo.lock`、CI、共通契約の変更は競合しやすい。担当タスクに必要な差分だけ加え、統合は一件ずつ行う。公開APIを変える場合は契約文書と利用者向け例も更新する。
- 既存のHTTP・DB・暗号・Protobufライブラリを利用し、未使用機能のcrateや将来用の抽象化を先に増やさない。HTTP/gRPC/workerの依存方向を守る。
- SQL migrationがDB構造の正本。HTTP Requestの検証だけを整合性の保証にしない。認証・秘密情報・エラー応答を扱う変更では失敗時も検証する。
- GitHubリポジトリ作成、push、GitHub Pages公開、クラウドへのデプロイは明示依頼があるまで行わない。mainへのmergeもユーザーの依頼または承認後に行う。

## 確認コマンド

Rustの最低対応版は1.94.0、edition 2024。Rustを変更したタスクでは関連するテストに加えて以下を実行してください。

```sh
cargo +1.94.0 fmt --all --check
cargo +1.94.0 clippy --workspace --all-targets --locked -- -D warnings
cargo +1.94.0 test --workspace --locked
```

新しい依存を追加したら最低Rust版・ライセンス・依存グラフを確認します。DB/SMTP/storage等は、該当タスクで実サービスとの結合確認も必要です。完了時には変更内容、公開API、検証結果、未検証事項をタスク表のカードへ記録してください。
