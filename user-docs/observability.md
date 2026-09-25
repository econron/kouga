# ログから、処理の流れをたどる

[← ガイドの入口](README.md)

> ドキュメント・プレビュー。KougaのOTel統合、追加コマンド、標準計測は未実装の設計案です。`tracing`とOpenTelemetryは既存の仕組みを利用します。

最初は標準出力のログで十分です。サービスが増えて「このリクエストから、どのジョブが動いたのか」「どこで時間がかかったのか」を知りたくなったら、OpenTelemetryを追加できます。

## カスタムロガーとの違い

| 見たいこと | 使う情報 |
|---|---|
| 何が起きたか | logs：エラーや業務上の出来事 |
| どの処理が、どのくらいかかったか | traces：HTTP・DB・ジョブなどの処理区間と関連 |
| 全体の調子はどうか | metrics：件数、処理時間、失敗率などの集計 |

OpenTelemetryは、こうした情報を収集して外部へ送るための仕組みです。ログの出力先変更もその一部ですが、複数のサービスをまたぐ調査にも使います。保存やグラフ表示をする画面は、対応するバックエンドで用意します。[OpenTelemetry公式](https://opentelemetry.io/docs/concepts/signals/)

## ログを一行、追加する

```rust
tracing::info!(operation = "task.create", "タスクを作成しました");
```

Kougaでは`tracing`のログを、標準の構造化ログへ載せる設計です。リクエスト処理中ならrequest ID、ジョブ処理中ならjob IDを関連付けます。独自のloggerを作ってcontrollerへ渡し回す必要はありません。

トークンやRequest全体をそのまま記録せず、調査に必要な項目を選んで書きます。

## OTelを追加する

導入コマンドの案です。

```sh
kouga add otel
```

HTTPと既存workerに、OTelの依存と起動時の設定を追加します。編集済みコードを自動変更できない箇所は、必要な差分を表示します。OTelを使わないアプリには、SDKや送信処理を含めません。

続いて、HTTPのサービス名と、CollectorなどのOTLP受信先を指定します。下記はローカルで受信先が起動している場合の例です。

```sh
export OTEL_SERVICE_NAME=taskboard-http
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
kouga server
```

初版はOTLP/HTTP protobufで送る案です。endpoint未設定なら外部送信しません。endpointを設定すると、標準ではtracesとmetricsを送ります。Collectorは別途用意し、コンテナから送る場合はコンテナから到達できるアドレスを指定します。

workerは別のターミナル・環境で、別のサービス名にします。

```sh
export OTEL_SERVICE_NAME=taskboard-worker
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
kouga worker --queue mail
```

依存を追加したため、本番用のDockerイメージは再ビルドします。OTelを含まない既存イメージに環境変数を渡すだけでは有効になりません。

設定名は[OpenTelemetryの標準環境変数](https://opentelemetry.io/docs/specs/otel/configuration/sdk-environment-variables/)に合わせ、Kougaが対応する範囲を明示します。

## まずは標準の計測を見る

Kouga経由のHTTP・DB操作・ジョブ投入と実行・メール送信を計測します。HTTPとworkerを別イメージにしていても、ジョブに保存したtrace contextで関連をたどれます。

```text
HTTPの処理
├─ DBへの保存
└─ メールジョブの投入
       └─ 関連付け → workerの実行 → メール送信
```

worker側は、実行のたびに新しいspanを作り、投入元へlinkで関連付けます。待機中ずっとHTTPの計測を続けるわけではありません。再試行は同じjob IDと試行番号で区別します。linkの表示方法はバックエンドによって異なります。

## 自分の処理時間を測る

処理区間の記録をspanと呼びます。関数へ`instrument`を付けるだけで、その関数のspanを追加できます。

```rust
#[tracing::instrument(skip_all, name = "task.complete")]
async fn complete_task(db: &Db, id: Uuid) -> Result<(), Error> {
    Task::update(db, id, UpdateTask {
        completed: Patch::Value(true),
        ..Default::default()
    }).await?.ok_or_else(Error::not_found)?;

    tracing::info!("タスクを完了しました");
    Ok(())
}
```

これはKougaのmodel APIと既存の`tracing`を組み合わせた抜粋です。`skip_all`で引数の自動記録を止め、必要な情報だけ明示します。ログとspanを別々の独自APIで書き直す必要はありません。[tracing公式](https://docs.rs/tracing/latest/tracing/)

## ログもOTLPで送りたいとき

標準出力のログは、そのまま使えます。有効なtrace contextがあるときはtrace_idとspan_idも付け、ログから処理の追跡につなげます。

OTLPでログを直接送る場合は、明示的に有効化します。

```sh
export OTEL_LOGS_EXPORTER=otlp
```

標準出力も別の収集システムで取り込んでいる場合は、同じログを二度保存しないよう経路を選びます。OTelへのログ送信を止める指定は`OTEL_LOGS_EXPORTER=none`です。

## 出力や送り先を細かく変える

標準設定で足りない場合は、起動処理で`tracing`のsubscriber/layerやOTelのproviderを構成できる拡張口を用意します。専用のloggerクラスを継承する方式にはしません。具体的な起動APIは設計中です。

アプリ独自のメトリクスはOTelのmeter APIへ接続する方針です。ユーザーIDやtrace IDをメトリクスのラベルへ入れず、操作名・結果など種類の限られた項目を使います。

送信先が停止しても、APIやジョブの業務処理は継続します。計測データにはバッファと送信時間の上限を設けるため、取りこぼしはあり得ます。欠落できない監査履歴は、業務データとして別に保存してください。

ワンショット終了時には送信を待つ処理を入れます。Lambdaでも呼び出し終了前に送信する設計にし、プロセスが終了するまで待つことはありません。

**[デプロイのガイドへ](deployment.md)** · **[ガイドの入口へ](README.md)**
