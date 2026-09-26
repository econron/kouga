# 時間のかかる処理を、workerへ渡す

[← ガイドの入口](README.md)

> 開発プレビュー。ジョブ契約・queue・worker、生成コマンドと`--once`はローカルcheckoutで動作します。生成handlerは処理例なので、実業務の処理に置き換えてください。

リクエスト内で完了する必要のないメール送信や集計は、ジョブとして登録します。HTTPは応答を返し、workerが後から処理します。

## ジョブを作る

```sh
kouga generate job SendWelcomeEmail user_id:uuid
```

最初のジョブを作るときに、別のworkerバイナリとqueue migrationも用意します。

```text
src/jobs/send_welcome_email.rs       ジョブ名と引数
src/bin/job-worker.rs               handlerの登録と実行入口
migrations/*_create_kouga_jobs.up.sql queueと失敗履歴
```

HTTPとworkerで共有するのは、ジョブの契約です。

```rust
#[kouga_job::job(name = "send_welcome_email", version = 1, queue = "mail")]
pub struct SendWelcomeEmail {
    pub user_id: Uuid,
}
```

ここにSMTPやメール本文は入れません。

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

worker側に、mailerとテキスト・HTMLのテンプレートを生成します。SMTP接続先や認証情報はworkerの実行環境へ設定します。開発・テストでは外部送信せず、生成したメールを確認できます。

現在使えるメールAPIは次の形です。テンプレートの変数は`render_html`でエスケープします。SMTPが必要なのはworkerバイナリだけです。

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

handlerの案です。

```rust
pub async fn send_welcome_email(
    job: SendWelcomeEmail,
    ctx: JobContext<WorkerState>,
) -> Result<(), JobError> {
    let Some(user) = User::find(&ctx.state.db, job.user_id).await? else {
        return Ok(()); // 送信前に退会済みなら、何もせず完了する。
    };

    Welcome::to(&user.email)
        .deliver(&ctx.state.mailer)
        .await?;
    Ok(())
}
```

実行処理はworker側で登録します。

```rust
worker.register::<SendWelcomeEmail>(send_welcome_email)?;
```

generatorは`src/bin/job-worker.rs`へ登録例を追加します。生成直後のhandlerは完了を記録する最小例なので、投入前に業務処理と失敗時の`JobError`へ置き換えてください。HTTP側の投入は`kouga_queue::Enqueue`をimportして`enqueue`を使います。`kouga generate mailer Welcome`の本文構築例は`src/mailers/welcome.rs`に置かれます。SMTP資格情報はworker実行環境だけへ渡してください。

## 常駐して処理する

```sh
kouga worker --queue mail
```

queueを監視し、新しいジョブを処理し続けます。メールと画像処理を別々にスケールさせたい場合は、queueとworkerのビルド対象を分けられます。

## ワンショットタスクとして処理する

```sh
kouga worker --queue mail --once
```

現行の生成workerは最大1件・30秒で新規取得を止め、終了します。件数や時間を変える場合は、生成されたworkerの`run_once(max_jobs, max_duration, ...)`を編集します。

取得停止後の処理には終了猶予時間を設けます。実行中のジョブを無制限に待つことはせず、完了できなかった分はleaseと再試行の規則に従います。

Cloud Run JobsやECSの単発taskで、まとめて処理するバッチに使う想定です。queueを使わない登録済みの業務処理は、`kouga runner <task>`でも実行できます。

## 失敗を確認する

```sh
kouga jobs list
kouga jobs show <job-id>
kouga jobs retry <job-id>
```

workerは失敗したジョブを間隔を空けて再試行し、上限に達したものをdead状態にします。`jobs show`は機密情報を避けてpayload・失敗理由を表示しません。失敗理由はworkerの安全なログで確認してから再投入してください。`retry`はdead/quarantined、`cancel`はpendingだけを変更します。

ジョブは少なくとも一度の実行を目指す仕組みです。同じジョブが再実行されることがあるため、課金や残高更新はjob ID・一意制約などで重複に備えます。SMTP送信後に応答が失われた場合など、メールの重複送信も起こり得ます。

## HTTPとworkerを別々に届ける

```sh
docker build --target http -t taskboard-http .
docker build --target worker -t taskboard-worker .
```

引数の形式を変えるときは、先に新形式を読めるworkerを配備します。queueに旧形式のジョブが残る間は、その形式を扱うhandlerも維持してください。

**次へ：[デプロイ](deployment.md)**
