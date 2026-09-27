# 時間のかかる処理を、workerへ渡す

[← ガイドの入口](README.md)

> ジョブ契約・queue・worker、生成コマンドと`--once`はローカルcheckoutで動作します。生成handlerはjob IDを表示して完了する最小例で、メール送信には利用者がhandlerを編集します。

リクエスト内で完了する必要のないメール送信や集計は、ジョブとして登録します。HTTPは応答を返し、workerが後から処理します。

## ジョブを作る

```sh
kouga generate job SendWelcomeEmail user_id:uuid
```

最初のジョブを作るときに、別のworkerバイナリとqueue migrationも用意します。

```text
crates/contracts/src/jobs/send_welcome_email.rs ジョブ名と引数
apps/worker/src/bin/job-worker.rs   handlerの登録と実行入口
migrations/*_create_kouga_jobs.up.sql queueと失敗履歴
```

HTTPとworkerで共有するのは`crates/contracts` packageのジョブ契約です。worker packageはHTTP routerやOpenAPI UIに依存しません。

```rust
#[kouga_job::job(name = "send_welcome_email", version = 1, queue = "default")]
pub struct SendWelcomeEmail {
    pub user_id: Uuid,
}
```

生成直後のqueueは`default`です。mail専用queueへ変える場合は、この契約の`queue`とworkerの`WorkerOptions::queues`を揃えて編集してください。ここにSMTPやメール本文は入れません。

## HTTPから投入する

```rust
SendWelcomeEmail { user_id: user.id }
    .enqueue(&state.db)
    .await?;
```

登録が成功すると、ジョブはPostgreSQLに保存されています。HTTPプロセスが終了しても、登録済みのジョブはworkerが取得できます。

生成後に`kouga db migrate`を実行します。既にqueue migrationがある場合、同じテーブルを二重作成しません。

## 保存と投入を一緒に確定する

ユーザーを作れたのにジョブ登録だけ失敗する、といった中途半端な状態を避けるには、同じトランザクションを使います。password_hashは、事前に標準のパスワード処理で生成した値とします。

```rust
let mut tx = state.db.begin().await?;

let user = User::create(&mut tx, NewUser {
    email: input.email.clone(),
    password_hash,
}).await?;

SendWelcomeEmail { user_id: user.id }
    .enqueue(&mut tx)
    .await?;

tx.commit().await?;
```

途中でエラーになれば、ユーザー作成もジョブ登録もrollbackされます。メールの送信そのものは、このトランザクションに入れません。

## workerでメールを送る

```sh
kouga generate mailer Welcome
```

worker側に`apps/worker/src/mailers/welcome.rs`の`build(to, from)`関数を生成します。生成直後の本文は固定の最小例です。SMTP接続先や認証情報はworkerの実行環境へ設定します。開発・テストではMemoryMailerで送信内容を確認できます。

次は低レベルメールAPIの使用例です。生成されたjob handlerに自動配線されるコードではありません。テンプレートの変数は`render_html`でエスケープします。SMTPが必要なのはworkerバイナリだけです。

```rust
use kouga_mailer::{MailMessage, MemoryMailer, render_html};

let html = render_html("<p>こんにちは、{{ name }}さん</p>", &user)?;
let mail = MailMessage::new("hello@example.com", &user.email, "ようこそ", "ご登録ありがとうございます")?
    .html(html);
mail.deliver(&state.mailer).await?;

// テスト・開発環境では MemoryMailer を使い、SMTPなしで内容を確認できます。
let recorded = memory_mailer.recorded();
assert_eq!(recorded[0].subject(), "ようこそ");
```

本番では`SmtpMailer::relay(host, port, credentials)`または`starttls`を使い、TLS・証明書検証を必須にします。`insecure_local(port)`はローカルの開発用SMTPシンク専用です。SMTPの結果が不明な場合、ジョブ再試行で重複送信される可能性があります。

送信処理は生成された`apps/worker/src/bin/job-worker.rs`のhandler内へ、ユーザー取得・`mailers::welcome::build(to, from)`・`message.deliver(&mailer)`を組み込みます。生成直後のhandlerはジョブIDを表示して成功扱いにするだけで、メールは送信しません。

実行処理はworker側で登録します。

```rust
worker.register::<SendWelcomeEmail>(send_welcome_email)?;
```

generatorは`apps/worker/src/bin/job-worker.rs`へ登録例を追加します。生成直後のhandlerは完了を記録する最小例なので、投入前に業務処理と失敗時の`JobError`へ置き換えてください。HTTP側の投入は`kouga_queue::Enqueue`をimportして`enqueue`を使います。`kouga generate mailer Welcome`の本文構築例は`apps/worker/src/mailers/welcome.rs`に置かれます。SMTP資格情報はworker実行環境だけへ渡してください。

## 常駐して処理する

```sh
kouga worker --queue default
```

queueを監視し、新しいジョブを処理し続けます。メールと画像処理を別々にスケールさせたい場合は、queueとworkerのビルド対象を分けられます。

## ワンショットタスクとして処理する

```sh
kouga worker --queue default --once
```

現行の生成workerは最大1件・30秒で新規取得を止め、終了します。件数や時間を変える場合は、生成されたworkerの`run_once(max_jobs, max_duration, ...)`を編集します。

取得停止後の処理には終了猶予時間を設けます。実行中のジョブを無制限に待つことはせず、完了できなかった分はleaseと再試行の規則に従います。

Cloud Run JobsやECSの単発taskで、まとめて処理するバッチに使う想定です。認証のパスワードリセット用メールworkerは`kouga worker --queue mail --once`です。queueを使わない登録済みの業務処理は、`kouga runner <task>`でも実行できます。

## 失敗を確認する

```sh
kouga jobs list
kouga jobs show <job-id>
kouga jobs retry <job-id>
```

workerは失敗したジョブを間隔を空けて再試行し、上限に達したものをdead状態にします。`jobs show`は機密情報を避けてpayload・失敗理由を表示しません。失敗理由はworkerの安全なログで確認してから再投入してください。`retry`はdead/quarantined、`cancel`はpendingだけを変更します。

ジョブは少なくとも一度の実行を目指す仕組みです。同じジョブが再実行されることがあるため、課金や残高更新はjob ID・一意制約などで重複に備えます。SMTPサーバーが本文を受理した直後、queueの完了記録より先にworkerが停止すると、DB上の効果が1回でもメールは2通受理され得ます。メールの厳密な1回配送はKouga単体では保証しません。受信側の重複許容、または送信サービスが提供する冪等キーなどの外部対策を業務要件に合わせて用意してください。

## HTTPとworkerを別々に届ける

```sh
kouga dockerfile
docker build --build-context kouga=/path/to/kouga --target http -t taskboard-http .
docker build --build-context kouga=/path/to/kouga --target worker -t taskboard-worker .
```

引数の形式を変えるときは、先に新形式を読めるworkerを配備します。queueに旧形式のジョブが残る間は、その形式を扱うhandlerも維持してください。

**次へ：[デプロイ](deployment.md)**
