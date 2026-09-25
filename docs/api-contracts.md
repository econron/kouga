# Kouga — 共通API契約（T01）

状態: レビュー承認済み・実装前。2026-09-25。

[仕様書](specification.md)の機能・受け入れ条件を変更せず、後続タスクが共有する型・依存方向・生成記法を固定する。コードは実装の契約であり、現在コンパイルできる製品コードではない。変更が必要な場合は本書を先にレビューし、依存タスクへ反映する。

## 1. 技術選定と確認範囲

Rust **1.94.0以上**、edition 2024、Cargo resolver 3を採用する。SQLx 0.9.0の申告MSRVを基準にした設計上の下限であり、依存全体でのビルド保証はT02のMSRV CIで確認する。手元の確認環境はrustc/cargo 1.95.0。PostgreSQL専用とし、別DB用の抽象化や独自HTTPサーバーは作らない。

以下は`cargo info <crate>@<version>`相当のレジストリ情報で確認した初期採用版。最新版への追従を要求する表ではない。workspace.dependenciesに版を集約し、実解決版はT02以降のCargo.lockで固定する。crate追加時にはMSRV・ライセンス・脆弱性を再確認する。以下のライセンス確認は直接依存の申告値であり、推移依存・配布素材の監査は別途必要。

| 用途 / crate | 初期版 | 申告MSRV | ライセンス |
|---|---|---|---|
| runtime: tokio / tokio-util | 1.53.1 / 0.7.19 | 1.71 / 1.71 | MIT |
| HTTP: axum / tower / tower-http | 0.8.9 / 0.5.3 / 0.7.1 | 1.80 / 1.64 / 1.65 | MIT |
| HTTP共通型: http | 1.5.0 | 1.57 | MIT OR Apache-2.0 |
| DB: sqlx | 0.9.0 | 1.94 | MIT OR Apache-2.0 |
| serde / serde_json | 1.0.229 / 1.0.151 | 1.56 / 1.71 | MIT OR Apache-2.0 |
| uuid / chrono | 1.26.1 / 0.4.45 | 1.85 / 1.62 | MIT OR Apache-2.0 |
| rust_decimal | 1.43.0 | 1.67.1 | MIT |
| tracing / tracing-subscriber | 0.1.44 / 0.3.23 | 1.65 / 1.65 | MIT |
| config: toml | 1.1.6+spec-1.1.0 | 1.85 | MIT OR Apache-2.0 |
| CLI: clap | 4.6.7 | 1.85 | MIT OR Apache-2.0 |
| macro: syn / quote / proc-macro2 | 3.0.6 / 1.0.47 / 1.0.107 | 1.71 | MIT OR Apache-2.0 |
| schema: schemars | 1.2.2 | 1.74 | MIT |
| OpenAPI文書型: utoipa | 6.0.0 | 1.88 | MIT OR Apache-2.0 |
| YAML: serde_yaml_ng | 0.10.0 | 1.64 | MIT |
| 開発UI: utoipa-swagger-ui | 10.0.1 | 1.88 | MIT OR Apache-2.0 |
| schema検証（開発依存）: jsonschema | 0.57.0 | 1.85 | MIT |
| email形式: email_address | 0.2.9 | 未申告 | MIT |
| gRPC: tonic / tonic-prost / tonic-prost-build | 0.14.6 | 1.88 | MIT |
| Protobuf: prost | 0.14.4 | 1.85 | Apache-2.0 |
| password: argon2 | 0.6.0 | 1.85 | MIT OR Apache-2.0 |
| digest / 乱数: sha2 / getrandom | 0.11.0 / 0.4.3 | 1.85 | MIT OR Apache-2.0 |
| SMTP: lettre | 0.11.23 | 1.85 | MIT |
| mail template: minijinja | 2.12.0 | 1.70 | Apache-2.0 |
| storage: object_store | 0.14.2 | 1.85 | MIT / Apache-2.0 |
| OTel: opentelemetry / opentelemetry_sdk / opentelemetry-otlp | 0.33.0 | 1.75 | Apache-2.0 |
| OTel logs: opentelemetry-appender-tracing | 0.33.0 | 1.75 | Apache-2.0 |
| OTel spans: tracing-opentelemetry | 0.34.0 | 1.75 | MIT |

確認元は各版の`https://crates.io/crates/<crate>/<version>`と配布Cargo.toml。特に[SQLx](https://crates.io/crates/sqlx/0.9.0)、[axum](https://crates.io/crates/axum/0.8.9)、[tonic](https://crates.io/crates/tonic/0.14.6)の版を共通境界として扱う。minijinjaは確認時の3系がalphaだったため安定版2.12.0を選ぶ。Rust全版・全crateのコンパイルやセキュリティ監査が済んだという意味ではない。

- Tokioは`rt-multi-thread, macros, net, signal, sync, time`を基本とし、fs/process等は使うcrateだけが追加する。`full`を一括で有効にしない。
- SQLxはdefault-featuresを切り、`postgres, runtime-tokio, tls-rustls-ring-native-roots, uuid, chrono, rust_decimal, json, derive`。動的SQLはQueryBuilderとbindを使う。コンパイル時のDB接続を必須にしない。
- axumは必要なJSON/query/routing/server機能だけを有効にし、multipart/wsは追加機能。tower-httpのCORS等を再利用する。
- OTel・SMTP・gRPC・storage・UIは必要な実行パッケージだけに追加する。tonic内部のaxumなど、共通の通信部品は許容する。「HTTP非依存」とはKougaのHTTP controller・schema/UIへの非依存であり、HTTP/2ライブラリまで排除する意味ではない。
- Swagger UIはvendored素材を使い、実行時CDNを要求しない。OpenAPIの検証は公式3.1.1 schemaを固定してjsonschemaで行う。外部参照の自動取得は無効。素材の版・checksum・ライセンスはT14で記録する。

## 2. crateと所有範囲

下表は配置の予約であり、空crateを今すべて作る指示ではない。T02は`kouga-core`、`kouga-runtime`、`kouga-validation`の最小骨格とCIだけを作り、残りは担当タスクで追加する。公開importは当面各crateから行い、全機能を引く巨大なfacadeを作らない。

| `crates/`以下 | 所有タスク | 責務・依存先（Kouga内） |
|---|---|---|
| kouga-core | T02、変更は統合担当 | Patch、共通Error。runtime/HTTP/DBなし |
| kouga-runtime | T03、T26接続 | config、起動、blocking上限、通常ログ。core |
| kouga-validation | T07、T08、HTTP adapterのみT09 | Request trait、Validated、検証/schema。core |
| kouga-request-derive | T08 | Request生成。validationのruntimeに逆依存させない |
| kouga-db | T04 | SQLx型再公開、DbError。core |
| kouga-migration | T05/T06 | SQL履歴・実行器。db |
| kouga-model / kouga-model-derive | T11/T13 / T12 | query・関連 / model生成。db、core |
| kouga-http | T09/T10 | router、middleware、応答、endpoint macro。core、runtime、validation |
| kouga-http-derive | T09 | endpoint登録情報の生成 |
| kouga-openapi | T14 | route/schema → YAMLとUI。http |
| kouga-cli | T15/T16/T19/T29/T30 | CLIとtemplates。テンプレートは機能別ディレクトリ |
| kouga-test | T17 | HTTP/DB検証支援。http、migration |
| kouga-auth | T18 | token照合・password・policy。db、runtime。HTTP専用関数はhttp側 |
| kouga-job / kouga-job-derive | T20 | payload契約 / derive。serdeのみ、DB/workerなし |
| kouga-queue | T20 | Enqueue拡張trait、queue schema。job、db |
| kouga-worker | T21 | handler、lease、retry。queue、runtime |
| kouga-mailer | T22 | SMTP/記録、メール構築。runtime。workerへは依存しない |
| kouga-cache / kouga-storage / kouga-channel | T23 / T24 / T25 | 各機能。共通DB・runtimeを利用、HTTP adapterはhttp側の機能別module |
| kouga-telemetry | T26/T27 | 任意OTel統合。runtime。各通信adapterは各入口側 |
| kouga-grpc | T28 | tonic接続・status変換。core、runtime、validation、auth |

deriveは実装コードを生成するだけで、生成先が利用するruntimeへの循環依存を作らない。`kouga-validation/axum`だけが任意のaxum依存を持ち、自身のValidatedへextractorを実装する。別crateでforeign traitとforeign typeを組み合わせる孤児規則違反を避ける。標準featureは空で、worker/gRPCはこのfeatureを要求しない。

生成アプリではdomain → model/db/core、contracts → job、http → domain/contracts/http/queue、grpc → domain/contracts/rpc/grpc/queue、worker → domain/contracts/worker/必要なmailerの方向とする。共通業務型からHTTP/gRPC型を参照しない。Cargo.lock・ルートCargo.toml・CI・本書は統合担当が一件ずつ調整する。

## 3. 共通型とエラー

```rust,ignore
// kouga_core
pub enum Patch<T> { Missing, Value(T) } // Default = Missing
pub enum ErrorKind {
    BadRequest, Unauthorized, Forbidden, NotFound, Conflict,
    Validation, TooLarge, UnsupportedMediaType, RateLimited,
    Unavailable, Timeout, Internal,
}
// Error: kind/code/安全なmessage/detailsと、非公開sourceを持つ。
// 内部sourceはSerializeしない。request_idは入口が付ける。
```

Patchはserdeで「フィールド省略→Missing」「指定値→Value」に対応する。nullableだけがValue(None)を許す。Missingのserializeはフィールドのskipと組み合わせ、単体をnullと同一視しない。Request deriveがdefault/skipの実装を生成する。model側は同じPatchを使い、validation crateに依存しない。

UUID=`uuid::Uuid`（生成はv4）、UTC日時=`chrono::DateTime<Utc>`、Date=`chrono::NaiveDate`、Decimal=`rust_decimal::Decimal`。Decimalの表現範囲を超えるnumericはdecodeエラーとし丸めない。生成migrationは精度・scaleを明示する。JSON出力はDecimalを文字列、日時をUTC RFC3339にする。出力型deriveも同じ変換とschemaを使う。

| 共通の失敗 | HTTP | gRPC |
|---|---|---|
| decode/入力ルール違反 | 400 / 422 | INVALID_ARGUMENT |
| 未認証 / 認可拒否 / 不在 | 401 / 403 / 404 | UNAUTHENTICATED / PERMISSION_DENIED / NOT_FOUND |
| 業務競合 | 409 | FAILED_PRECONDITION（既存ならALREADY_EXISTSを明示選択可） |
| サイズ/頻度/処理枠超過 | 413 / 429 / 503 | RESOURCE_EXHAUSTED |
| 未対応Content-Type | 415 | HTTP固有。RPCへ機械変換しない |
| 基盤停止 / timeout / 内部不具合 | 503 / 504 / 500 | UNAVAILABLE / DEADLINE_EXCEEDED / INTERNAL |

DBのUNIQUE/FK/CHECKは、既知の制約を業務層がConflict/Validationへ明示変換する。知らない制約は500とし内部名を公開しない。接続・pool timeoutは503。commit結果不明は自動retryしない。HTTPのErrorからgRPCへ変換するのではなく、共通Errorからそれぞれ変換する。

## 4. Request / validation（T07・T08の入口）

```rust,ignore
pub trait Request: Sized + Send + Sync {
    type Context: Send + Sync;
    fn validate_sync(&self, errors: &mut ValidationErrors);
    fn validate_async<'a>(
        &'a self, context: &'a Self::Context,
        errors: &'a mut ValidationErrors,
    ) -> impl Future<Output = Result<(), Error>> + Send + 'a;
}
pub async fn validate<T: Request>(
    value: T, context: &T::Context,
) -> Result<Validated<T>, Error>;
// Validated<T>: private field、Deref<Target=T>/AsRef<T>のみ。
// new、From<T>、Deserialize、DerefMut、as_mutは公開しない。
```

`into_inner(self) -> T`は所有権を渡す経路として提供するが、戻った値は未検証型でありValidatedへ無検査で戻せない。derive対象は通常の値型struct/enumであり、Cell/Mutex等の内部可変性を含む入力型は拒否する。任意の手書きtrait実装まで「検証を偽装できない」とは保証しない。

- deriveは`Request` traitとserde Deserialize、schema実装を生成する。同じDeserializeを二重deriveしない。unknown fieldsを標準拒否し、renameは`#[request(rename = "...")]`としてdecode・error path・schemaへ一括反映する。
- ルールはlength/range/email/custom/custom_async。最小・最大は包含。lengthはUnicode scalar / 配列要素数。emailはemail_addressによる構文検証のみ。Option(None)とPatch(Missing)、Patch(Value(None))の値ルールは省略する。
- `#[request(context = ValidationContext)]`がある場合だけその型をContextにし、省略時は`()`。contextはアプリの通常のstruct（DB handle、認証主体等）で、T07はDB・HTTPに依存しない。
- customは`fn(&Field) -> Result<(), ValidationError>`、Stringは`&str`、Vecはsliceへ借用して渡す。custom_asyncは`async fn(&Field, &Context) -> Result<(), Error>`。入力不正は`Error::validation(...)`、DB障害はUnavailableとして返す。全体ルールは`&RequestStruct`を受ける。
- ネストは`#[validate(nested)]`を明示し、Vec/Option/Patch内も再帰する。子は親と同じContextを使う（context指定のない木はすべて`()`）。同期段階を木全体で終えてから非同期段階へ移る。
- 宣言順→各ルール順→struct全体ルール順。同期エラーがあれば非同期を実行しない。非同期は逐次実行し、基盤障害で中止。エラー上限100、入力のネスト上限32。上限到達は失敗のまま打ち切り、黙って成功しない。型付き入力のdecodeにも同じ深さ上限を適用する。
- `ValidationError { field: String, code: String }`に入力値を保存しない。配列pathは`items[0].name`。空PATCHは生成するstruct全体ルールで拒否する。

HTTP用extractorはvalidation crateのaxum feature内に定義する`ContextFromRequest<S>`を使ってbodyより先にcontextを作る。接続メソッドは`fn from_request(parts: &mut http::request::Parts, state: &S) -> impl Future<Output = Result<Self, Error>> + Send`。unitには常に成功する実装を用意し、DB/CurrentUserを含むアプリcontextには生成コードが実装する。このtraitはHTTP adapter専用で、検証本体は知らない。gRPC/workerは`validate(value, &context).await`を直接使う。

extractorのRejectionはaxum::Responseとし、coreの安全な`ErrorEnvelope`をserializeする。HTTP側も同じEnvelopeを使用する。coreのErrorへHTTP traitを実装するために逆依存を追加しない。request IDはHTTP基盤がextensionsに設定したcoreのRequestIdから取得する。

## 5. schemaとルート登録

型構造はschemarsのJSON Schema 2020-12を使い、独自schema DSLを作らない。Request deriveがserdeと同じフィールド名・必須性・null許容を生成し、組み込みルールから作る`Rule`（Length/Range/Email）をruntimeの判定とschema変換で共有する。custom関数はschema化せずdescriptionを要求（不足は警告）する。

schemaの公開接続口は`ApiSchema::schema(&mut SchemaGenerator, SchemaDirection) -> Schema`。DirectionはInput/Output。PatchはInputでoptional、内側Optionだけnullable。Requestと出力では方向を分ける。schemarsの参照をOpenAPI componentsへ移すときは全参照を解決・検証する。utoipaの型へserde経由で変換し、二重のToSchema deriveを利用者へ要求しない。

HTTP routeはhandlerと`Operation`を一緒に保持する。Operationはmethod/path/operation_id/parameters/request_body/responses/security/summary/description/tags/deprecatedを持つ。引数extractorと戻り値に`ApiInput`/`ApiOutput`を実装し、endpoint macroが収集する。

T09の登録APIは`Router::<S>::new().post(path, create_endpoint())?`。`#[endpoint]`は元の関数を残し、戻り値schema等を持つ`Endpoint<S>`を生成する。クエリは`ValidatedQuery<T>`（`T: Request`）を使い、`Query<T>`は検証を迂回するためendpoint引数にはしない。パスは`/{id}`形式のみを受け付け、`Path<T>`のschemaとルート変数の不一致を登録時に拒否する。`router.routes()`でstateなしに`Operation`を参照できる。現時点の`Json<T>`/`Created<T>`/`Page<T>`の内側には`schemars::JsonSchema`が必要。OpenAPI文書化・request ID・middlewareはT14/T10で接続する。

```rust,ignore
#[kouga_http::endpoint(operation_id = "tasks.create")]
pub async fn create(
    state: State<AppState>,
    input: Validated<CreateTaskRequest>,
) -> Result<Created<TaskResponse>, Error> {
    let task = Task::create(&state.db, NewTask {
        title: input.title.clone(), completed: false,
    }).await?;
    Ok(Created::new(format!("/tasks/{}", task.id), TaskResponse::from(task)))
}
// macroは関数を残し、create_endpoint()も生成する。
router.post("/tasks", create_endpoint());
```

上例のTaskはcompletedにDB defaultを持たない例。default付き属性の扱いは第7節。生成resourceの`tasks::routes()`は各endpointをまとめたResourceRoutesで、`.only([Index, Show, Create])`を持つ。登録時に重複名/パス・未説明の入出力を拒否する。動的応答はendpointのresponses補足で明示し、黙って200だけを生成しない。

`routes() -> Router<AppState>`は接続・秘密情報を要求しない。`router.openapi()`/`router.routes()`はstateなしで実行でき、配信時だけ`router.with_state(state)`を呼ぶ。OpenAPIは3.1.1を明示、安定順序・生成時刻なし。標準エラー、認証、validation、201 Location、204本文なし、data/metaを登録から出力する。任意のcontrollerの本体を解析して推測しない。

## 6. HTTP・middleware

State、HeaderValue、body/responseはaxum/httpの既存型を使う。`HttpRequest<S>`はbody付きrequestと共有stateを包む薄い型とし、`state() -> &S`、headers/extensions/body分解・復元を提供する。生成例では`HttpRequest<AppState>`と型引数を明記する。

この節の`Error`は`kouga_http::Error`（core::Errorを包むローカル型）で、IntoResponseを実装する。core::ErrorからのFrom変換を提供し、DB/validationは共通Errorへの変換規則を共有する。middleware chainはこのHTTP側Errorを運び、domainはcore::Errorだけを使う。

```rust,ignore
async fn middleware(
    request: HttpRequest<AppState>, next: Next<AppState>,
) -> Result<Response, Error>;
// Next<S>::run(self, request: HttpRequest<S>) -> Result<Response, Error>
```

NextはCloneを実装せず、内部を公開しない。axumのNextはClone可能かつResponseを直接返すため、そのままの再公開では本契約を満たさない。Kougaのmiddleware chainはTower Serviceの`Error=Error`を保持し、最外周で一度だけHTTP応答へ変換する。bodyはstreamのまま渡す。axumのextractor/controllerの拒否も共通エラー形式へ揃える。

endpoint macroのadapterはcontrollerが返すResultをIntoResponseより先に受け取る。extractorが既にResponseを返した場合は共通Envelopeの応答として扱い、bodyを逆解析してErrorへ戻さない。`next.run`のErrはcontroller/middleware由来、extractor拒否はOk(error response)となる。応答ヘッダーを成功時だけ加工したいmiddlewareはstatusも検査する。

入口の順序はrequest ID・計測 → 共通エラー応答/CORS → サイズ/受付/timeout → 全体middleware → グループ → ルート → context/decode/validation → controller。各階層で登録順、戻りは逆順。preflightは認証前に短絡。timeout・panic・404/405もrequest ID付き共通応答、HEADはbodyを送信しない。`bearer_auth(fn)`が実処理とsecurity metadataを一緒に登録する。認可policyは対象を取得する業務処理側で適用する。

`Created<T>`=201/Location/data、`Json<T>`=200/data、`Page<T>`=200/data/meta、`NoContent`=204/bodyなし。Location不正など応答構築の失敗はInternalとする。ModelをそのままSerializeせず公開出力型を生成する。

T10時点の登録APIは`Router::middleware(fn)`、`Router::group("/prefix")?.middleware(fn).get(...).finish()`、`Endpoint::middleware(fn)`。`Router::configure(HttpOptions { ..Default::default() })?`で本文上限（既定1 MiB）、同時実行上限（256）、timeout（30秒）、CORS許可origin、trusted proxyの正確なIPを指定する。CORSは明示したoriginのみ有効で、credential付き`*`は拒否する。`ClientIp`と`RequestId`はrequest extensionsに入り、controllerでは`Extension<T>`で受け取れる。共有レート制限はmiddlewareを追加する拡張点のみ提供し、ストアは含まない。`X-Request-ID`を全応答に付与し、共通エラーには`request_id`を含める。

## 7. DB・model・query

`Db = sqlx::PgPool`、`Transaction<'a> = sqlx::Transaction<'a, Postgres>`を再公開する。独自executor traitは作らない。各操作は`A: sqlx::Acquire<'c, Database=Postgres> + Send`を受け、先頭でacquireして得たconnectionを操作終了まで使う。内部helperは`&mut PgConnection`を取る。これにより`&db`と`&mut tx`を共通で扱う。複数SQLの途中でpoolへ戻らない。

```rust,ignore
async fn find<'c, A>(db: A, id: Uuid) -> Result<Option<Self>, DbError>
where A: Acquire<'c, Database = Postgres> + Send;
// create(db, NewTask) -> Result<Task, DbError>
// update(db, id, UpdateTask) -> Result<Option<Task>, DbError>
// delete(db, id) -> Result<bool, DbError>
let mut tx = db.begin().await?;
let project = Project::create(&mut tx, NewProject { name: "計画".into() }).await?;
tx.commit().await?;
```

Send futureの生成はSQLx Acquireのlifetimeを保持する。必要なら通常fn→`impl Future + Send`形式で生成する。`&mut tx`の長期保持やparallel実行を要求しない。commit/rollbackはselfを消費する。未commit dropはSQLxのrollback処理に委ね、未処理接続の再利用防止をT04で試験する。

`DbError`はConstraint(kind, private_name)、Connection、PoolTimeout、Decode、Deadlock、Serialization、InvalidInput、Integrity、CommitUnknown、Otherを区別する。SQLx由来のsourceは内部保持する。poolから直接開始したtxのcommit時のSQLxエラーは`commit_transaction(tx)` helperでCommitUnknown等へ分類できる。標準の生成コードはこのhelperを使う（例の直接commitも結果を盲目的にretryしない）。

- Model deriveはSQLx FromRow、CRUD、NewX/UpdateX、型付きcolumnを生成する。structのmodel名をsnake_caseにした補助moduleに`columns`/`relations`を置く。`#[model(module = task_meta)]`で衝突回避できる。table/column名は引用し、利用者の値をSQLへ連結しない。
- `Task::completed`等の列定数は互換の短縮形として生成可能だが、正本は`task::columns::completed`。modelの手書きメソッドと衝突した場合はmacroが診断する。
- `#[model(table = "tasks", crud_visibility = "pub(crate)")]`で生成CRUDと属性型の可視性を制限できる。業務メソッドからだけ利用する。UUID id・時刻列はNew/Updateの通常属性から除外する。明示IDは`create_with_id(db, id, attrs)`。
- defaultなしの列はNewで元のT、DB default付きは`#[model(default)]`を付け、Newでは`Option<T>`にする。NoneでINSERT列を省く。nullable/default付きは`Option<Option<T>>`（None=default、Some(None)=NULL）。生成controllerが明示変換する。DB defaultをRustへ複製しない。
- Updateの全項目はPatch<T>、DefaultでMissing。id/created_at/updated_atのmass assignmentは生成しない。空更新はInvalidInput、更新日時はDB時刻。INSERT/UPDATEはRETURNINGで取得する。
- queryは`Query<M>`。Column<M,T>のeq/lt/le/gt/ge/in_list/is_null、Predicate<M>のand/or、asc/desc、filter/order_by/limit/offsetを持つ。型の異なる列・NULLへのeqはcompile error。nullable値のNULLは専用演算。
- fetch_all→Vec、fetch_optional→Option、count→i64、exists→bool。fetch_optionalは複数件を黙って一件にしない。ページ取得は`page(page, per_page).fetch(db)`→`PageResult<M> { items, page, per_page, has_next }`。page>=1、標準20/最大100、一意順のid補完、余分に一件取得。
- 大きなINはPostgreSQLの配列bind/ANYを優先する。preloadは既定1,000 IDごとに分割。全SQLのparameter上限を超えるqueryを無制限に分割してorder/limitの意味を壊さず、表現不能ならInvalidInputを返す。空INはfalse。
- 行ロックは`query.for_update().fetch_all(&mut tx)`。ロック用queryはtx専用の引数にし、poolを渡すコードはcompile error。条件付き更新は通常の業務メソッド内のbind付きSQLで記述する。

### 関連とpreload

`task.project(&db).await`は維持するが、同名の関連定数とメソッドは併設できないため、preloadは別moduleのdescriptorを使う。

```rust,ignore
let rows = Task::query()
    .preload(task::relations::project())
    .limit(20).fetch_all(&db).await?;
for row in rows {
    let task: Task = row.model;
    let project: Project = row.related;
}
// 戻り値: Vec<Loaded<Task, Project>>
pub struct Loaded<M, R> { pub model: M, pub related: R }
```

必須belongs_toはR=Project、任意belongs_to/has_oneはOption<Project>、has_many/多対多はVec<Project>。通常のTaskにはrelatedがなく、未取得と取得済み空を型で区別する。必須関連が欠落または認可scope外ならIntegrity（公開は内部情報を隠す）として失敗し、認可を外して再取得しない。

複数関連は`.preload((task::relations::project(), task::relations::tags()))`→`Loaded<Task, (Project, Vec<Tag>)>`。ネストは`task::relations::project().preload(project::relations::owner())`→`Loaded<Task, Loaded<Project, User>>`。連続したpreload呼び出しは不可とし、一つの明示した木にまとめる。初版tupleは1〜4関連、より多い関連は親のID集合に対して別のbatch取得を明示する。

関連条件はdescriptorの`.filter(Project::owner_id.eq(actor.id))`、順序は`.order_by(...)`。親取得後、関連ごと・ID chunkごとにまとめてSQLを発行し、親の順序・件数を保持する。関連を黙ってlimitしない。単件の関連queryは`task.project_query()`、`project.tasks_query()`、多対多は明示した中間modelを経由するdescriptorを生成する。必要なら同じtxでrepeatable readを選ぶ。

## 8. Migration

SQLxの接続・raw_sqlを使い、履歴とdirty/repairはKougaが管理する。SQLx Migratorの履歴と二重管理しない。独自schema DSL・SQL文のセミコロン分割は作らない。

公開入口は`MigrationSet::load(path) -> Result<MigrationSet, MigrationError>`と、`Migrator::new(db, set, options)`の`status/migrate/rollback(steps)/repair(version, state, reason)`（各async）。MigrationErrorはInvalidFile/HistoryMismatch/OutOfOrder/LockTimeout/Irreversible/Dirty/Databaseを区別する。

**T05時点の実装済みAPI**: `MigrationSet::load(path)`、`Migrator::new(db: kouga_db::Db, set, MigratorOptions)`、`status().await -> Result<Vec<MigrationStatus>, MigrationError>`、`migrate().await -> Result<usize, MigrationError>`。`MigratorOptions`は`lock_timeout`と`statement_timeout`を持つ。`migrate`は通常のtransactional SQLだけを適用し、非transactional指定は適用前に拒否する。`rollback`・`repair`・SQLひな形の生成・`kouga db ...` CLIは未実装で、T06以降の対象。通常サーバーから自動実行しない。

versionは14桁UTC、up必須。downが欠落・空白/コメントだけなら不可逆。checksumはファイルの生bytesのSHA-256（改行も対象）、up/down各別に保持する。migration履歴tableは`_kouga_migrations`、repair履歴は`_kouga_migration_repairs`。version/name/checksums/mode/state/direction/applied_atと修復理由を保存する。dirtyの方向を残し、rollback中断も識別する。

検査→専用接続のsession advisory lock→履歴再検査→実行の順。同一DB用の固定lock keyをmigrate/rollback/reset/repairで共有する。通常は各migrationのDDLと履歴を同一txで確定。非txはdirty永続化→SQL→履歴確定で、失敗時はdirtyを保持する。非txファイルはトップレベル一文を契約とする。複数文のsimple queryには暗黙transactionが生じ得るため、CONCURRENTLYなどは一文ずつ別migrationへ分ける。単純分割で回避しない。

rollbackは全対象のdown/checksumを事前確認してから変更する。repairはdirtyに限定し理由必須、実DB修復を行わない。SQL migrationの追加は各機能が所有し、管理CLIとschema dumpはT06。通常サーバーはmigrationを実行しない。

## 9. Job / worker / mailer

```rust,ignore
// kouga_job: DBにも実行handlerにも依存しない。
pub trait Job: Serialize + DeserializeOwned + Send + Sync + 'static {
    const NAME: &'static str;
    const VERSION: u32;
    const QUEUE: &'static str;
}
// 実装済みの #[kouga_job::job(...)] 属性はserdeも生成する。
// #[derive(Job)] も使えるが、その場合serdeのderiveは別途必要。
// kouga_queue::Enqueueをimportすると任意のJobで利用可能。
let job_id: Uuid = SendWelcomeEmail { user_id }.enqueue(&mut tx).await?;
worker.register::<SendWelcomeEmail>(send_welcome_email)?;
async fn send_welcome_email(job: SendWelcomeEmail, ctx: JobContext<WorkerState>)
    -> Result<(), JobError>;
```

enqueueはmodelと同じAcquire引数、戻り値Result<Uuid, QueueError>。`enqueue_at(db, DateTime<Utc>)`で遅延投入。JobContext<S>は`state: Arc<S>, job_id, attempt, cancellation`。mailer/DBを固定で内蔵せず、`ctx.state.db`/`ctx.state.mailer`から使う。Worker<S>::registerのhandlerはimpl Fn(J, JobContext<S>)→Send Futureで型を推論し、指定genericはJ一つ。重複(name, version)は起動エラー。

payloadとは別にid/name/version/queue/available_at/attempt/lease_token/lease_untilとtrace metadataを保存する。trace metadataはoptionalなtraceparent/tracestate文字列で、OTelの型をcontractsに持ち込まない。認証情報は保存しない。enqueue呼び出し時のcontext注入点をqueueへ用意し、OTel未搭載時は空にする。即時投入の時刻はDBの`now()`を使う。queue SQL migrationは`kouga-queue/migrations/`に置き、アプリのmigrationへ組み込む。

JobErrorはRetryable/Permanent、基盤障害のsourceは内部保持。queueはat-least-once。完了更新にはlease token照合が必要で、副作用のexactly-onceを保証しない。unknown payloadは隔離する。常駐/ワンショットで同じhandlerを使い、`--once --max-jobs N --max-duration D`は件数・時間・空queueで新規取得を停止する。残り時間と終了猶予は分ける。

mailerはlettreのMessage/SMTPを再利用し、MiniJinjaはHTML autoescapeを有効にする。生成したWelcome等はアプリのworker内に配置する。メモリ送信は同じMailMessageを記録する。mailerから自動で独自ジョブを生成せず、通常のJob handlerがdeliverを呼ぶ。

## 10. 設定・起動・計測（T03の入口）

`Config::load(root, Environment) -> Result<Config, ConfigError>`は既定→`config/base.toml`→`config/{environment}.toml`→環境変数の順に上書きする。`KOUGA_ENV`はdevelopment/test/production、既定development。未知キー、0上限、矛盾する設定は起動エラー。DATABASE_URL/同_FILEと秘密値の両指定は拒否。Secret<T>はDebug/Displayで秘匿し、明示的なexposeのみ許す。

| 設定（環境変数） | 標準値 |
|---|---|
| KOUGA_RUNTIME_THREADS | available_parallelism（取得不能時1） |
| HOST / PORT / GRPC_PORT | 0.0.0.0 / 3000 / 50051 |
| KOUGA_HTTP_MAX_IN_FLIGHT / KOUGA_HTTP_TIMEOUT_MS | 256 / 30000（待ちqueueなし、超過503） |
| KOUGA_BODY_LIMIT_BYTES | 1048576（multipartはT24の個別上限） |
| KOUGA_DB_MAX_CONNECTIONS / KOUGA_DB_ACQUIRE_TIMEOUT_MS | 10 / 5000 |
| KOUGA_BLOCKING_CONCURRENCY / KOUGA_BLOCKING_WAITERS / KOUGA_BLOCKING_WAIT_TIMEOUT_MS | 4 / 16 / 1000 |
| KOUGA_SHUTDOWN_TIMEOUT_MS | 30000 |
| RUST_LOG | info（生成ログはJSON、本文/SQL値なし） |

runtime構築は同期mainで設定読込後に行い、Tokio multi_thread Builderへthread数を渡す。`Runtime::build(&config)`、`runtime.block_on(run(config))`を入口とする。DBは必要なbinaryだけがrun内で作成する。

`BlockingPool::run(closure).await -> Result<T, BlockingError>`は待機枠と実行枠を制限し、実行permitをclosureへ移動する。caller timeout後もclosure終了まで枠を保持する。`Shutdown`はCancellationTokenを再利用、受付停止→処理drain→上限付きtelemetry flushの順。core/validation自体はsignal/global runtimeを初期化しない。

通常ログは`logging::init(&LogConfig)`をbinaryが一度呼ぶ。独自subscriber利用時はこれを呼ばず、tracing-subscriberのLayerを組み立てる。二重global登録はエラー。OTel追加時はtelemetry側がfmt layerとOTel layerをまとめて初期化するため、通常initと両方を呼ばない。

標準span名は`kouga.http.request / kouga.db.query / kouga.queue.enqueue / kouga.job.run / kouga.mail.send / kouga.grpc.request`。属性はoperation/table/route template/job kind/attempt等の許可リスト。生SQL・bind・URL query・token・宛先は記録しない。tracing spanはFuture::instrumentで伝播し、enter guardをawait越しに保持しない。

T26の`Telemetry::init(config)`はguardとproviderを保持し、`shutdown(deadline).await`を提供する。利用者providerも同じshutdown入口へ登録できる。SDK/exporterは独立crateだけに置く。OTEL_SERVICE_NAME/RESOURCE_ATTRIBUTES/EXPORTER_OTLP_ENDPOINT/HEADERS、TRACES_EXPORTER/METRICS_EXPORTER/LOGS_EXPORTER（otlp/none）、TRACES_SAMPLER（always_on/always_off/parentbased_traceidratio）とSAMPLER_ARGを初版対応とする。endpointなしでは送信なし、logsは明示有効化。OTLP/HTTP protobufのみ、queue metadataから各試行spanへlink、baggage既定無効。細かい送信上限はT26で追加してもT03の公開型を変更しない。

## 11. gRPCと生成コード

`.proto`→tonic-prost-build（build依存のみ）→`crates/rpc`。handlerはtonic生成service traitを実装し、`tonic::Request<rpc::Input>`からmetadata認証→業務入力へTryFrom→共有validate→業務関数の順に呼ぶ。非同期DB認証を同期interceptorへ押し込まず、handler wrapperでawaitする。

```rust,ignore
async fn create(&self, request: tonic::Request<rpc::CreateTaskRequest>)
    -> Result<tonic::Response<rpc::Task>, tonic::Status> {
    let actor = self.auth.authenticate(request.metadata()).await.map_err(to_status)?;
    let input = CreateTaskInput::try_from(request.into_inner()).map_err(to_status)?;
    let context = ValidationContext { db: self.db.clone(), actor };
    let input = validate(input, &context).await.map_err(to_status)?;
    let task = create_task(&self.db, &input).await.map_err(to_status)?;
    Ok(tonic::Response::new(task.into()))
}
```

CreateTaskInputはdomainの型でrpc/HTTPへ依存しない。公開auth lookupもmetadataではなく抽出済みtokenを取る共通関数とし、上例self.authはgRPC adapter。unaryだけを初版必須とし、streaming/reflection/grpc-webは生成しない。

PATCHは`.proto`のoptionalまたはoneofでpresenceを表す。nullable更新はoneofの「値/明示null」とoneof自体の不在をPatchへ変換する。scalar既定値から省略を推測しない。deadline/cancel/サイズ/過負荷とstatus変換を各RPCへ適用する。同一workspaceでHTTPと併用し、別binary/port/imageが標準。

## 12. 実装への引き継ぎと並行作業

1. T01をレビュー・mainへ統合後、T02で最小workspaceと契約のcompile fixtureを作る。T01では本体や空の全crateを追加しない。
2. T02統合後にT03（runtime）とT07（validation）を並列着手する。core変更は統合担当へ寄せる。
3. T03後はT04（DB）、T07後はT08（derive）が並列可能。T04後はT05・T11・T20・T26を分けられる。
4. すでに作成したT03/T07 worktreeは古いmainが基点。作業開始前に、未commit変更がないことを確認して統合済みmainを各task branchへmergeする。未着手でもworktreeを作り直す必要はない。ここでは自動mergeしない。

T02で確認する型のfixtureはAcquireでpool/txを渡すSend future、Patchのserde三状態、Validatedの非公開構築、RequestのContextとFuture、Loadedのtuple/nested形。T09でmiddlewareの失敗伝播とextractor、T14でschema runtime一致、T28でProtobuf presenceを実証する。未実装APIをコンパイル検証済みと扱わない。

T01で固定しないものは各機能内だけに閉じる運用値・Dockerベース/Lambda adapter・配布手順。共有型に影響する追加は本書を先に変更する。元の全機能は引き続き必須であり、この限定は機能削除ではない。
