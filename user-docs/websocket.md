# WebSocket通知

[← ガイドの入口](README.md)

`kouga generate channel Events`は、認証追加後に別バイナリを生成します。生成されたpolicyはすべて拒否するため、下記の許可例を業務要件に合わせて実装してください。[新規生成アプリでの実WebSocket結合テスト](tutorial.md#追加テスト関連添付websocket)では、ticket・購読・別インスタンス通知を確認できます。

`kouga-channel`は、オンライン接続へのbest-effort通知です。履歴を保存しません。切断・再接続後は通常のHTTP APIで最新状態を取得してください。長時間接続できないLambda実行モードは対象外です。

認証migrationを適用した後、`crates/kouga-channel/migrations`を適用します。HTTPプロセスごとに同じPostgreSQLへ接続します。

```rust,ignore
use kouga_channel::{Action, Channel, Options};

let channel = Channel::start(db.clone(), Options {
    allowed_origins: vec!["https://app.example.com".into()],
    ..Options::default()
}, |actor, action, name| {
    // 実際の業務ルールに置き換える。拒否を既定にする。
    actor.id == owner_id && name == "orders" && action == Action::Subscribe
}).await?;

let app = http_router.with_state(state).merge(channel.router());
```

認証済みAPIから `POST /_kouga/ws-ticket` に `Authorization: Bearer <token>` を送ると、30秒有効・一度限りのticketが返ります。WebSocket URLにはtokenもticketも付けません。ブラウザでは次のように接続します。

```js
const { ticket } = await fetch('/_kouga/ws-ticket', {
  method: 'POST',
  headers: { Authorization: `Bearer ${token}` },
}).then(r => r.json());
const ws = new WebSocket('wss://api.example.com/_kouga/ws',
  ['kouga', `kouga-ticket.${ticket}`]);
ws.addEventListener('open', () =>
  ws.send(JSON.stringify({ type: 'subscribe', channel: 'orders' })));
// {"type":"subscribed","channel":"orders"} を待ってから通知を期待する。
```

ブラウザの`Origin`は起動時に指定した値と完全一致で検証します。ticketはサブプロトコルヘッダー内にあり、URLのアクセスログへは出ません。独自のヘッダーログでも`Sec-WebSocket-Protocol`を記録しないでください。接続ticketは元のBearerが失効・期限切れになると、設定した確認間隔内に切断されます。

受信コマンドは`subscribe`、`unsubscribe`、`publish`です。`publish`には`data`が必要で、購読と配信はそれぞれ`Action::Subscribe`/`Action::Publish`のpolicyが許可した場合だけ動きます。サーバー側からは`channel.publish(actor, "orders", value).await?`を呼びます。通知は`{"channel":"orders","data":...}`です。購読は接続ごとに保持し、切断すると消えます。

接続数、受信・通知サイズ、配信バッファ、送信タイムアウト、heartbeat、認証の再確認間隔は`Options`で制限します。遅い受信者や失効した接続は切断します。各HTTPプロセスのlistenerはDB接続を1本専有するため、pool上限に余裕を持たせてください。通知channel名はDB schemaから導出し、別schemaのアプリと混線しません。PostgreSQL `NOTIFY`は非永続のため、プロセス停止中の通知は再送されません。
