# DBの変更を、履歴に残す

[← ガイドの入口](README.md)

> ドキュメント・プレビュー。SQLを標準にする案と、CLIの利用体験を示しています。Kougaのコマンドは未実装です。

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

直前の一つを戻す操作です。

```sh
kouga db rollback --steps 1
```

downがない、または空の場合は不可逆として拒否します。適用済みファイルを書き換えるとchecksumの不一致を検知します。共有環境へ適用した変更は、新しいmigrationで修正してください。

## トランザクションに入れられない変更

例えば並行index作成では、ファイル先頭に実行方法を指定します。

```sql
-- kouga: transaction=false
CREATE INDEX CONCURRENTLY tasks_created_at_idx ON tasks (created_at);
```

この形式の途中失敗では、DBに変更の一部が残ることがあります。Kougaはdirty状態として後続を停止します。`db status`で対象を確認し、DBの実際の状態を修復してから、`db repair`で履歴を揃えます。

`repair`はSQLを再実行するコマンドではありません。対象versionと修復後の状態を指定する管理操作です。詳細な復旧手順は実装時に整備します。

## schemaとseed

```sh
kouga db schema
kouga db seed
```

`schema`は現在の構造を`db/schema.sql`へ出力します。`seed`は登録した初期データ投入処理を実行します。migrationがテーブル構造の履歴、seedが初期データ、という役割分担です。

本番へのmigrationは、HTTPサーバーの起動と分けて一度実行します。列を削除するような変更では、先にその列を使わないアプリを配備するなど、更新順序も考慮してください。

**次へ：[認証とmiddleware](auth-and-middleware.md)**
