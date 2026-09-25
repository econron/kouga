# 時間のかかる処理を、workerへ渡す

[← ガイドの入口](README.md)

> ドキュメント・プレビュー。ジョブの宣言、handlerの登録、メールAPI、ワンショットのオプションは設計案です。

リクエスト内で完了する必要のないメール送信や集計は、ジョブとして登録します。HTTPは応答を返し、workerが後から処理します。

## ジョブを作る

```sh
kouga generate job SendWelcomeEmail user_id:uuid
```

最初のジョブを作るときに、workerのパッケージとビルド対象も用意します。

```text
crates/contracts/src/jobs/send_welcome_email.rs  ジョブ名と引数
apps/worker/src/jobs/send_welcome_email.rs       実行する処理
apps/worker/src/main.rs                         handlerの登録
```

HTTPとworkerで共有するのは、ジョブの契約です。

```rust
#[derive(Job)]
#[job(name = "send_welcome_email", version = 1, queue = "mail")]
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

handlerの案です。

```rust
pub async fn send_welcome_email(
    job: SendWelcomeEmail,
    ctx: JobContext,
) -> Result<(), JobError> {
    let Some(user) = User::find(&ctx.db, job.user_id).await? else {
        return Ok(()); // 送信前に退会済みなら、何もせず完了する。
    };

    Welcome::to(&user.email)
        .deliver(&ctx.mailer)
        .await?;
    Ok(())
}
```

実行処理はworker側で登録します。

```rust
worker.register::<SendWelcomeEmail>(send_welcome_email);
```

通常はgeneratorが登録箇所を用意します。HTTP側はこのhandlerにも、SMTPライブラリにも依存しません。

## 常駐して処理する

```sh
kouga worker --queue mail
```

queueを監視し、新しいジョブを処理し続けます。メールと画像処理を別々にスケールさせたい場合は、queueとworkerのビルド対象を分けられます。

## ワンショットタスクとして処理する

```sh
kouga worker --queue mail --once --max-jobs 100 --max-duration 60s
```

この案では、最大100件・最大60秒・queueが空になる、のいずれかで新規取得を止め、終了します。`--once`はプロセスを一度起動して終了するという意味で、必ず1件だけ処理する指定ではありません。

取得停止後の処理には終了猶予時間を設けます。実行中のジョブを無制限に待つことはせず、完了できなかった分はleaseと再試行の規則に従います。

Cloud Run JobsやECSの単発taskで、まとめて処理するバッチに使う想定です。queueを使わない登録済みの業務処理は、`kouga runner <task>`でも実行できます。

## 失敗を確認する

```sh
kouga jobs list
kouga jobs show <job-id>
kouga jobs retry <job-id>
```

workerは失敗したジョブを間隔を空けて再試行し、上限に達したものをdead状態にします。失敗内容を確認してから再投入できます。

ジョブは少なくとも一度の実行を目指す仕組みです。同じジョブが再実行されることがあるため、課金や残高更新はjob ID・一意制約などで重複に備えます。SMTP送信後に応答が失われた場合など、メールの重複送信も起こり得ます。

## HTTPとworkerを別々に届ける

```sh
docker build --target http -t taskboard-http .
docker build --target worker -t taskboard-worker .
```

引数の形式を変えるときは、先に新形式を読めるworkerを配備します。queueに旧形式のジョブが残る間は、その形式を扱うhandlerも維持してください。

**次へ：[デプロイ](deployment.md)**
