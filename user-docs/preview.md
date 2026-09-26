# 開発版の現在地

[← ガイドの入口](README.md)

Kougaはローカルcheckoutからビルドして試せる開発版です。crate・CLIの配布、安定性保証、性能値、実クラウド配備の検証はまだありません。記載の型・コマンドは、[通しの実行手順](tutorial.md)で実物と照合した範囲と、低レベルcrateの実装を分けて説明します。

| 項目 | 現在の状態 |
|---|---|
| HTTP CRUD、Request検証、OpenAPI | generatorと実PostgreSQLで動作確認済み |
| 認証、queue、mailer、worker | generator・実DB・SMTP結合テストあり。独自ジョブの実処理は利用者が書く |
| model association | deriveとpreload APIあり。外部キーmigrationと利用側コードは手動 |
| 添付 | `kouga-storage`の低レベルAPIあり。添付HTTP routeのgeneratorなし |
| WebSocket | `kouga-channel`と拒否を既定にした別バイナリを生成。policyは手動 |
| gRPC | Greetingのunaryサンプルを別packageで生成。任意の業務RPCは手動 |
| OTel | `add otel`で標準HTTP/workerに追加。独自gRPCバイナリは手動 |
| Docker | `dockerfile`で役割別targetを生成。BuildKit named contextに同じKouga checkoutが必要 |
| Cloud Run/ECS/Lambda | 実行条件とLambda HTTP adapterを用意。実クラウドへのpush/deployは未検証 |

## まだないCLI機能

`kouga db rollback/repair/schema/seed/reset`は利用できます。schemaには外部`pg_dump`、seedにはアプリ登録済み`task-seed`が必要です。`kouga generate storage`はありません。ドキュメント中の抜粋コードは、明記のない限り完全なアプリを自動生成する意味ではありません。

## 先に試すなら

[最初のAPI](getting-started.md)でCRUDを動かし、[通しの実行手順](tutorial.md)で認証・worker・gRPCまで進めてください。現在の制限を確認したうえで、機能別ガイドを参照できます。

GitHub Pages公開、クラウド配備、レジストリへのpushは、このドキュメントの検証では行っていません。
