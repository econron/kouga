# DBの変更を、履歴に残す

[← ガイドの入口](README.md)

> `generate migration` と `db create/migrate/status/rollback/repair/schema/seed/reset` はローカルCLIで利用できます。`db schema`にはシステムの`pg_dump`が必要です。`pg_dump`は接続先PostgreSQLサーバーと同じメジャーバージョン、またはそれ以降の対応バージョンを用意してください。

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

```sh
kouga db rollback                 # 最新の1件
kouga db rollback --steps 2       # 最新の2件
```

rollback対象のdown SQLは先に全件検査されます。downがない・空のmigrationを含む場合は何も戻しません。downはデータの復元を保証しません。適用済みファイルを書き換えるとchecksumの不一致で非ゼロ終了します。共有環境の変更は新しいmigrationで修正してください。

## トランザクションに入れられない変更

例えば並行index作成では、ファイル先頭に実行方法を指定します。

```sql
-- kouga: transaction=false
CREATE INDEX CONCURRENTLY tasks_created_at_idx ON tasks (created_at);
```

この形式の途中失敗では、DBに変更の一部が残ることがあります。Kougaはdirty状態として後続を停止します。`db status`で対象を確認し、バックアップと履歴を参照してDBの実際の状態を手動で修復してください。その後に限り、修復内容を理由として記録します。

```sh
kouga db repair --version 20260925000100 --state pending --reason '部分適用を手動で取り消し、未適用状態を確認した'
# または、upが完了していると確認できた場合だけ --state applied
```

repairはSQLを実行せず、dirty以外の履歴や改変されたmigrationは修復しません。理由にパスワード等の秘密情報を書かないでください。

## schemaとseed

```sh
kouga db schema                 # db/schema.sqlへスキーマのみ出力
kouga db schema --output db/review.sql
kouga db seed                   # 登録済みsrc/bin/task-seed.rsを実行
```

`db schema`は`pg_dump --schema-only --no-owner --no-privileges`を使い、データを含めません。出力は確認用であり、空DBの構築にはmigrationを使います。`db seed`は登録済みのRustバイナリがなければ非ゼロ終了します。`src/bin/task-seed.rs`へmodel APIなどによる登録処理を実装し、再実行するデータは一意キーとUPSERT等で冪等にしてください。seedはDDLやmigration履歴を変更しません。`kouga runner seed`も同じ登録済み処理を実行します。

開発・テストDBを作り直す場合だけ、接続先と環境を明示します。

```sh
KOUGA_ENV=test kouga db reset --database taskboard_test --environment test --allow-destructive
# seedも実行する場合は、登録済みのtask-seed.rsを確認してから --seed を追加
```

DB名は`DATABASE_URL`中の実際のDB名と一致しなければ拒否します。`KOUGA_ENV`が設定されていれば`--environment`と一致が必要です。本番はさらに`--environment production --allow-production`が必要で、確認なしのリセットを許しません。resetは接続中のセッションを切断しDBを削除・再作成する破壊操作です。実行前にバックアップ・対象・権限を確認し、通常の起動やデプロイから呼ばないでください。`--seed`未登録は削除前に拒否します。

本番へのmigrationは、HTTPサーバーの起動と分けて一度実行します。列を削除するような変更では、先にその列を使わないアプリを配備するなど、更新順序も考慮してください。

**次へ：[認証とmiddleware](auth-and-middleware.md)**
