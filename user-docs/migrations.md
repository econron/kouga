# DBの変更を、履歴に残す

[← ガイドの入口](README.md)

> `generate migration`、`db create`、`db migrate`、`db status`はローカルCLIで利用できます。rollback/repair/schema/seedのCLIコマンドはまだありません。

`kouga-migration`の低レベルAPIにはrollbackやschema出力もありますが、現在のCLIには接続されていません。schema出力にはシステムの`pg_dump`が必要です。

migrationには、DBへ何を変更するかを書きます。Kougaは適用順と履歴を管理し、PostgreSQLのSQLを実行します。

## 生成されたSQLを読む

`generate model`や`generate resource`は、modelに対応するmigrationを一緒に生成します。独自の変更は名前を付けて追加します。

```sh
kouga generate migration add_description_to_tasks
```

```text
migrations/
  20260925000100_add_description_to_tasks.up.sql
  20260925000100_add_description_to_tasks.down.sql
```

upには適用する変更を書きます。

```sql
ALTER TABLE tasks ADD COLUMN description text;
```

downには、その変更を戻す処理を書きます。

```sql
ALTER TABLE tasks DROP COLUMN description;
```

このdownは列を削除するため、その列のデータは失われます。rollbackは、データを過去の状態へ復元するバックアップではありません。

## 適用する

```sh
kouga db migrate
kouga db status
```

未適用のmigrationが古い順に実行されます。標準では、一つのmigrationが一つのトランザクションです。途中で失敗すると、そのmigrationをrollbackして停止します。

適用後はmodelへ列を追加します。

```rust
pub description: Option<String>,
```

APIから変更したいなら更新Request、返したいなら公開レスポンス型にも追加します。DBの内部項目が、自動的にAPIへ公開されることはありません。

## 制約はDBにも書く

同じ名前をプロジェクト内で重複させたくない場合の例です。Taskにproject_idがある前提です。

```sql
ALTER TABLE tasks
  ADD CONSTRAINT tasks_project_title_unique UNIQUE (project_id, title);
```

Requestで事前確認しても、並行した登録は起こり得ます。最後の保証はDBに置きます。既存データに重複がある場合は、解消してから適用します。

制約名はmodel側で業務エラーへ対応付けできます。利用者へSQLやDB内部の詳細をそのまま返す必要はありません。

## 戻す・修正する

CLIに`db rollback`はありません。生成されるdown SQLは手動でレビューするためのひな形です。適用済みファイルを書き換えるとchecksumの不一致を検知します。共有環境へ適用した変更は、新しいmigrationで修正してください。

## トランザクションに入れられない変更

例えば並行index作成では、ファイル先頭に実行方法を指定します。

```sql
-- kouga: transaction=false
CREATE INDEX CONCURRENTLY tasks_created_at_idx ON tasks (created_at);
```

この形式の途中失敗では、DBに変更の一部が残ることがあります。Kougaはdirty状態として後続を停止します。`db status`で対象を確認し、バックアップと履歴を参照してDBの実際の状態を修復してください。CLIに`db repair`はありません。

## schemaとseed

`kouga db schema`と`kouga db seed`は未実装です。初期データはアプリの`src/bin/task-<name>.rs`に処理を書き、`kouga runner <name>`で実行できます。migrationはテーブル構造の履歴、runnerは任意のバッチ処理です。

本番へのmigrationは、HTTPサーバーの起動と分けて一度実行します。列を削除するような変更では、先にその列を使わないアプリを配備するなど、更新順序も考慮してください。

**次へ：[認証とmiddleware](auth-and-middleware.md)**
