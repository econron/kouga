use kouga_mailer::{MailMessage, Mailer, MemoryMailer, SmtpMailer, render_html};
use lettre::message::header::ContentType;
use std::time::Duration;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};

fn mail() -> MailMessage {
    MailMessage::new(
        "sender@example.test",
        "receiver@example.test",
        "Welcome",
        "Hello",
    )
    .unwrap()
}

#[test]
fn worker_handler_future_is_send() {
    fn assert_send<F: std::future::Future<Output = Result<(), kouga_mailer::MailError>> + Send>(
        _: F,
    ) {
    }
    async fn handler<M: Mailer>(
        mail: MailMessage,
        mailer: M,
    ) -> Result<(), kouga_mailer::MailError> {
        mail.deliver(&mailer).await
    }
    assert_send(handler(mail(), MemoryMailer::new()));
}

#[tokio::test]
async fn memory_records_full_mail_without_external_delivery() {
    let html = render_html("<b>{{ name }}</b>", serde_json::json!({"name": "<script>"})).unwrap();
    assert_eq!(html, "<b>&lt;script&gt;</b>");
    let mail = mail()
        .html(html)
        .attach(
            "invoice.pdf",
            ContentType::parse("application/pdf").unwrap(),
            b"PDF".to_vec(),
        )
        .unwrap();
    let memory = MemoryMailer::new();
    mail.deliver(&memory).await.unwrap();
    let recorded = memory.recorded();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].subject(), "Welcome");
    assert_eq!(recorded[0].text(), "Hello");
    assert_eq!(recorded[0].html_body(), Some("<b>&lt;script&gt;</b>"));
    assert_eq!(recorded[0].attachment_count(), 1);
    let wire = String::from_utf8(recorded[0].to_message().unwrap().formatted()).unwrap();
    assert!(wire.contains("multipart/mixed"));
    assert!(wire.contains("multipart/alternative"));
    assert!(wire.contains("invoice.pdf"));
}

#[test]
fn header_injection_rejected() {
    assert!(
        MailMessage::new(
            "sender@example.test",
            "receiver@example.test",
            "ok\r\nBcc: attacker@example.test",
            "text"
        )
        .is_err()
    );
    assert!(
        MailMessage::new(
            "sender@example.test\nBcc: attacker@example.test",
            "receiver@example.test",
            "ok",
            "text"
        )
        .is_err()
    );
    assert!(
        mail()
            .attach("safe\r\nX-Hacked: yes", ContentType::TEXT_PLAIN, vec![])
            .is_err()
    );
    assert!(MailMessage::new("bad", "receiver@example.test", "ok", "text").is_err());
}

async fn smtp_server(status: &str, starttls: bool) -> (u16, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let status = status.to_owned();
    let task = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read, mut write) = socket.into_split();
        let mut read = BufReader::new(read);
        let mut lines = Vec::new();
        write.write_all(b"220 local SMTP\r\n").await.unwrap();
        loop {
            let mut line = String::new();
            if read.read_line(&mut line).await.unwrap() == 0 {
                break;
            }
            lines.push(line.trim_end().to_owned());
            if line.starts_with("EHLO") {
                write.write_all(b"250 local\r\n").await.unwrap();
                if starttls {
                    break;
                }
            } else if line.starts_with("MAIL FROM:") || line.starts_with("RCPT TO:") {
                write.write_all(b"250 OK\r\n").await.unwrap();
            } else if line.starts_with("DATA") {
                write.write_all(b"354 go\r\n").await.unwrap();
            } else if line == ".\r\n" {
                write.write_all(status.as_bytes()).await.unwrap();
                break;
            } else if line.starts_with("QUIT") {
                write.write_all(b"221 bye\r\n").await.unwrap();
                break;
            }
        }
        lines
    });
    (port, task)
}

#[tokio::test]
async fn smtp_accepts_and_reports_rejection() {
    for (status, success) in [("250 accepted\r\n", true), ("550 rejected\r\n", false)] {
        let (port, server) = smtp_server(status, false).await;
        let mailer = SmtpMailer::insecure_local(port);
        let result = tokio::time::timeout(Duration::from_secs(5), mailer.deliver(&mail()))
            .await
            .unwrap();
        assert_eq!(result.is_ok(), success);
        let lines = tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap();
        assert!(lines.iter().any(|line| line.starts_with("MAIL FROM:")));
        assert!(lines.iter().any(|line| line.starts_with("RCPT TO:")));
    }
}

#[tokio::test]
async fn starttls_does_not_downgrade() {
    let (port, server) = smtp_server("250 accepted\r\n", true).await;
    let mailer = SmtpMailer::starttls("localhost", port, None).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(5), mailer.deliver(&mail()))
            .await
            .unwrap()
            .is_err()
    );
    let lines = server.await.unwrap();
    assert!(!lines.iter().any(|line| line.starts_with("MAIL FROM:")));
}
