# データを保存し、関連を読む

[← ガイドの入口](README.md)

> ドキュメント・プレビュー。以下の`#[derive(Model)]`と`Project::find/create/query`は後続T12の設計案です。T11では手動`Model`実装、`Column`/`Query`と`kouga_model::{find, create, update, delete}`を実装済みです。

現時点の型付きqueryは次の形で使えます。列名は`Model::COLUMNS`の許可リストで照合され、値はSQLxでbindします。

```rust
#[derive(sqlx::FromRow)]
struct Project { id: Uuid, name: String }

impl kouga_model::Model for Project {
    const TABLE: &'static str = "projects";
    const COLUMNS: &'static [&'static str] = &["id", "name"];
}

let name = kouga_model::Column::<Project, String>::new("name");
let rows = kouga_model::Query::<Project>::new()
    .filter(name.eq("Kouga".to_owned()))
    .fetch_all(&db).await?;
```

modelはDBのデータを表すRustのstructです。HTTPの入力ルールはRequestに置き、modelはcontrollerからもworkerからも使えます。

## modelだけを追加する

```sh
kouga generate model Project name:string
```

modelとmigrationが生成されます。HTTP APIも一緒に作りたい場合は`generate resource`を使います。

```rust
#[derive(Model)]
#[model(table = "projects")]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

テーブル・列の変更は[migration](migrations.md)で適用します。structを編集しただけでは、DBを書き換えません。

## 取得・作成・更新・削除

```rust
let project = Project::find(&db, project_id).await?;
```

存在すれば`Some(project)`、存在しなければ`None`です。DB接続の失敗は`Err`になります。HTTPの404へ変換する場所はcontrollerです。

```rust
let project = Project::create(&db, NewProject {
    name: "次のリリース".into(),
}).await?;

let updated = Project::update(&db, project.id, UpdateProject {
    name: Patch::Value("秋のリリース".into()),
    ..Default::default()
}).await?;

let deleted = Project::delete(&db, project.id).await?;
```

`NewProject`と`UpdateProject`はmodel宣言から生成する属性型です。更新では、指定した項目だけを書き換えます。`updated`は保存結果のOption、`deleted`は削除できたかどうかのboolです。

idと作成・更新日時を、毎回手で設定する必要はありません。既存データか新規データかを推測する`save()`はなく、操作がそのままコードに現れます。

## 一覧を絞り込む

```rust
let tasks = Task::query()
    .filter(Task::completed.eq(false))
    .order_by(Task::created_at.desc())
    .limit(20)
    .fetch_all(&db)
    .await?;
```

queryの組み立てではSQLは実行されません。`fetch_all`などを呼んだところでDBへ問い合わせます。列名にはmodelから生成された参照を使い、値はバインドされます。

HTTPの一覧ではページングを使います。生成controllerは標準で1ページ20件、最大100件に制限します。並び順に主キーを補い、同順位の順序も決めます。

## associationは、欲しいときに読む

Taskにproject_idを追加した例です。先にmigrationで列と外部キーを作ります。

```rust
#[derive(Model)]
#[model(table = "tasks")]
#[belongs_to(Project, key = project_id, name = project)]
pub struct Task {
    pub id: Uuid,
    pub project_id: Uuid,
    pub title: String,
    pub completed: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

関連を取得する場所も明示します。

```rust
let project = task.project(&db).await?;
```

フィールドに触れたり、JSONへ変換したりするだけでSQLが発行されることはありません。

| 関連 | 例 | 必要なDB側の定義 |
|---|---|---|
| belongs_to | TaskからProject | Task側の外部キー |
| has_many | ProjectからTasks | Task側の外部キー |
| has_one | UserからProfile | Profile側の外部キー＋UNIQUE |
| many_to_many | TaskとTag | 中間model＋二つの外部キー＋組み合わせのUNIQUE |

関連の宣言だけではmigrationは変わりません。削除時の動作も、DBの外部キーで明示します。

## 一覧では、まとめて読む

```rust
let tasks = Task::query()
    .preload(task::relations::project())
    .limit(20)
    .fetch_all(&db)
    .await?;
```

タスクを取得してから、必要なプロジェクトをまとめて取得します。タスク一件ごとにプロジェクトを問い合わせる必要はありません。

preloadの結果は`Vec<Loaded<Task, Project>>`です。各要素の`row.model`がTask、`row.related`が取得済みProjectです。任意の関連ならOption、複数の関連先ならVecになります。通常のmodelと型を分けるので、「まだ取得していない」と「取得したけれど存在しない」を混同しません。

`task::relations`はModelから生成する関連指定用のモジュールです。単件の取得メソッド`task.project(...)`と名前を衝突させずに使えます。詳しい契約は[共通API契約](../docs/api-contracts.md)を参照してください（実装前）。

関連が非常に多い場合は、関連用のqueryでページングしてください。また、関連をたどれることと、利用者に見せてよいことは別です。[認証と認可](auth-and-middleware.md)の条件を関連queryにも適用します。

## 一緒に成功させたい処理は、同じトランザクションへ

```rust
let mut tx = db.begin().await?;

let project = Project::create(&mut tx, NewProject {
    name: "次のリリース".into(),
}).await?;

Task::create(&mut tx, NewTask {
    project_id: project.id,
    title: "仕様を確認する".into(),
    completed: false,
}).await?;

tx.commit().await?;
```

どちらかでエラーになれば、commitせずに終了してrollbackします。関連操作や[ジョブの登録](jobs-and-mail.md)にも同じtxを渡せます。

## 業務ルールを置く場所

「タイトルは100文字以内」はRequestへ。「完了した請求を未処理へ戻せない」のように、HTTP以外からも守る必要がある条件はmodelの業務メソッドへ置きます。必要なら生成CRUDを非公開にします。

同時更新が起こる操作には、行ロックや現在の状態を条件に含めた更新を使います。一意性はDBのUNIQUE制約で保証します。通常のupdateに、あらゆる競合を自動解決する仕組みはありません。

**次へ：[Migration](migrations.md)**
