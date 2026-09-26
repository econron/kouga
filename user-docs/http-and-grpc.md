# HTTPもgRPCも、一つのプロジェクトで

[← ガイドの入口](README.md)

> 入口追加CLI、unaryのGreetingサンプル、役割別Docker targetは利用できます。認証・DB・ジョブをgRPC handlerへ自動配線する機能はありません。業務RPCは手動で実装します。

フロントエンドにはHTTP/JSON、別のサービスにはgRPC/Protobuf。同じ業務処理を、使う側に合った入口から提供できます。

最初にどちらかを選んでも、プロジェクト全体を作り直す必要はありません。

## HTTPで始めて、gRPCを追加する

```sh
kouga new taskboard
cd taskboard
kouga add grpc
```

標準はHTTPです。gRPCを追加すると、既存のHTTPルートを残したまま、`proto/greeting.proto`、`crates/domain`、`crates/rpc`、`apps/grpc`を生成します。`/greet/{name}`と`Greeting.Greet`は同じ`app_domain::greet`を呼びます。`protoc`はビルド時に必要です。Dockerfileは`kouga dockerfile`で生成します。

```sh
cargo build -p taskboard-grpc --bin server-grpc
kouga server --api grpc
# 別ターミナル: kouga server --api http
# さらに別のターミナル: curl http://127.0.0.1:3000/greet/Kouga
# 別ターミナル: cargo run -p taskboard-grpc --bin grpc-client -- Kouga
```

```text
HTTP / JSON → Request検証 → controller ─┐
                                        ├→ 共通の業務処理・model → DB
gRPC / Protobuf → 入力検証 → handler ────┘
```

例えば「タスクを完了する」という処理を共通のRust関数に置きます。controllerとhandlerは入力を変換し、同じ関数を呼び、結果をそれぞれの形式で返します。gRPC側からHTTP APIを呼んで共有を実現する構成ではありません。

## 最初からgRPCだけでも

```sh
kouga new taskboard --api grpc
```

gRPC用の入口と共通domainを用意し、HTTP controllerやOpenAPI UIは組み込みません。後からHTTPを追加する操作も対称です。

```sh
kouga add http
```

HTTP/gRPCは排他的なモードではなく、必要な入口の選択です。
HTTPから始めた場合のHTTPパッケージはルートのまま、gRPCから始めて後付けしたHTTPパッケージは`apps/http`に置かれます。どちらも`apps/grpc`とは別にビルドできます。

## 共有するものと、入口で扱うもの

| 共通で使うもの | 入口ごとに扱うもの |
|---|---|
| model・DB制約・migration | HTTPのルート / gRPCのservice・method |
| 業務ルール・トランザクション | JSONのRequest / `.proto`由来の入力型 |
| 再利用できる検証関数・認可policy | 認証情報の抽出と検証の呼び出し |
| queue投入・worker・mailer | JSONレスポンスとHTTP status / gRPCの結果とstatus |
| 通常ログと任意のOTel統合 | 各入口に対応する計測とcontextの受け渡し |

HTTPはRustのRequest・出力型からOpenAPIを生成し、gRPCは`.proto`を契約にします。HTTP用YAMLと`.proto`を自動で相互変換することは前提にしません。

認証が必要な業務操作では両方の入口で個別に認証・入力検証・認可を行ってください。生成されるGreetingサンプルは認証不要の公開操作で、gRPC側は空の名前を拒否します。HTTP側にmiddlewareを付けただけでgRPC側も保護されたとは扱いません。共有する業務処理はHTTPやgRPCのstatusに依存させず、入口でエラーを変換します。

## 同居しても、配備は別々に

入口ごとのCargoパッケージとDocker targetを個別にビルドできます。Dockerビルドには、生成元のKouga checkoutをBuildKit named contextで渡します。

```sh
kouga dockerfile
docker build --build-context kouga=/path/to/kouga --target http -t taskboard-http .
docker build --build-context kouga=/path/to/kouga --target grpc -t taskboard-grpc .
```

同じリポジトリの共通コードから作った、別の実行ファイル・別のイメージです。HTTPだけ台数を増やしたり、gRPCを内部ネットワークへ配置したりできます。

開発時も、それぞれ別のプロセスとして起動します。

```sh
kouga server --api http
```

別のターミナルで実行します。

```sh
kouga server --api grpc
```

入口が一つなら`kouga server`だけで起動します。両方ある場合はHTTPを標準にし、gRPCは明示して起動します。

**同居対応の標準は、同一プロジェクト・共通の業務コード・独立した実行単位です。** 同じプロセスや同じポートで両方を受ける構成は、初版の必須機能には含めません。ネイティブgRPCの配備先には、対応するプロトコル設定が必要です。HTTP向けのLambda adapterなどをそのまま共用するとは扱いません。

**[最初のHTTP APIへ](getting-started.md)** · **[コンテナとして届ける](deployment.md)**
