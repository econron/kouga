//! Worker-side mail delivery. HTTP binaries can depend only on their own job payloads.

use std::{
    future::Future,
    sync::{Arc, Mutex},
};

pub use lettre::transport::smtp::authentication::Credentials;
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{Attachment, Mailbox, MultiPart, SinglePart, header::ContentType},
};
use minijinja::{AutoEscape, Environment};
use serde::Serialize;

#[derive(Debug)]
pub enum MailError {
    InvalidAddress(lettre::address::AddressError),
    InvalidMessage(lettre::error::Error),
    InvalidHeader,
    Smtp(lettre::transport::smtp::Error),
    Template(minijinja::Error),
}

impl std::fmt::Display for MailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidAddress(_) => f.write_str("invalid mail address"),
            Self::InvalidMessage(_) => f.write_str("invalid mail message"),
            Self::InvalidHeader => f.write_str("mail header contains a newline"),
            Self::Smtp(_) => f.write_str("SMTP delivery failed"),
            Self::Template(_) => f.write_str("mail template rendering failed"),
        }
    }
}

impl std::error::Error for MailError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidAddress(e) => Some(e),
            Self::InvalidMessage(e) => Some(e),
            Self::Smtp(e) => Some(e),
            Self::Template(e) => Some(e),
            Self::InvalidHeader => None,
        }
    }
}

/// A mail's typed inputs. The same value is retained by the in-memory transport.
#[derive(Clone, Debug)]
pub struct MailMessage {
    from: Mailbox,
    to: Mailbox,
    subject: String,
    text: String,
    html: Option<String>,
    attachments: Vec<(String, ContentType, Vec<u8>)>,
}

impl MailMessage {
    pub fn new(
        from: &str,
        to: &str,
        subject: impl Into<String>,
        text: impl Into<String>,
    ) -> Result<Self, MailError> {
        let subject = subject.into();
        reject_newline(&subject)?;
        reject_newline(from)?;
        reject_newline(to)?;
        Ok(Self {
            from: from.parse().map_err(MailError::InvalidAddress)?,
            to: to.parse().map_err(MailError::InvalidAddress)?,
            subject,
            text: text.into(),
            html: None,
            attachments: Vec::new(),
        })
    }

    pub fn html(mut self, html: impl Into<String>) -> Self {
        self.html = Some(html.into());
        self
    }

    pub fn attach(
        mut self,
        filename: impl Into<String>,
        mime: ContentType,
        bytes: Vec<u8>,
    ) -> Result<Self, MailError> {
        let filename = filename.into();
        reject_newline(&filename)?;
        self.attachments.push((filename, mime, bytes));
        Ok(self)
    }

    pub fn from(&self) -> &Mailbox {
        &self.from
    }
    pub fn to(&self) -> &Mailbox {
        &self.to
    }
    pub fn subject(&self) -> &str {
        &self.subject
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn html_body(&self) -> Option<&str> {
        self.html.as_deref()
    }
    pub fn attachment_count(&self) -> usize {
        self.attachments.len()
    }

    pub fn to_message(&self) -> Result<Message, MailError> {
        let mut body = MultiPart::alternative().singlepart(SinglePart::plain(self.text.clone()));
        if let Some(html) = &self.html {
            body = body.singlepart(SinglePart::html(html.clone()));
        }
        let body = if self.attachments.is_empty() {
            body
        } else {
            let mut mixed = MultiPart::mixed().multipart(body);
            for (name, mime, bytes) in &self.attachments {
                mixed = mixed
                    .singlepart(Attachment::new(name.clone()).body(bytes.clone(), mime.clone()));
            }
            mixed
        };
        Message::builder()
            .from(self.from.clone())
            .to(self.to.clone())
            .subject(&self.subject)
            .multipart(body)
            .map_err(MailError::InvalidMessage)
    }

    #[tracing::instrument(name = "kouga.mail.send", skip_all)]
    pub async fn deliver<M: Mailer>(&self, mailer: &M) -> Result<(), MailError> {
        mailer.deliver(self).await
    }
}

fn reject_newline(value: &str) -> Result<(), MailError> {
    if value.contains(['\r', '\n']) {
        Err(MailError::InvalidHeader)
    } else {
        Ok(())
    }
}

/// Render worker-owned HTML templates with escaping even for inline templates.
pub fn render_html<T: Serialize>(template: &str, context: T) -> Result<String, MailError> {
    let mut env = Environment::new();
    env.set_auto_escape_callback(|_| AutoEscape::Html);
    env.render_str(template, context)
        .map_err(MailError::Template)
}

pub trait Mailer: Send + Sync {
    fn deliver(&self, mail: &MailMessage) -> impl Future<Output = Result<(), MailError>> + Send;
}

pub struct SmtpMailer(AsyncSmtpTransport<Tokio1Executor>);

impl SmtpMailer {
    /// Implicit TLS with certificate and hostname verification (normally port 465).
    pub fn relay(
        host: &str,
        port: u16,
        credentials: Option<Credentials>,
    ) -> Result<Self, MailError> {
        let mut builder = AsyncSmtpTransport::<Tokio1Executor>::relay(host)
            .map_err(MailError::Smtp)?
            .port(port);
        if let Some(credentials) = credentials {
            builder = builder.credentials(credentials);
        }
        Ok(Self(builder.build()))
    }

    /// Required STARTTLS with certificate and hostname verification (normally port 587).
    pub fn starttls(
        host: &str,
        port: u16,
        credentials: Option<Credentials>,
    ) -> Result<Self, MailError> {
        let mut builder = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)
            .map_err(MailError::Smtp)?
            .port(port);
        if let Some(credentials) = credentials {
            builder = builder.credentials(credentials);
        }
        Ok(Self(builder.build()))
    }

    /// Plaintext SMTP for a local development sink only. Never use with secrets.
    pub fn insecure_local(port: u16) -> Self {
        Self(
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous("127.0.0.1")
                .port(port)
                .build(),
        )
    }
}

impl Mailer for SmtpMailer {
    async fn deliver(&self, mail: &MailMessage) -> Result<(), MailError> {
        self.0
            .send(mail.to_message()?)
            .await
            .map_err(MailError::Smtp)?;
        Ok(())
    }
}

#[derive(Clone, Default)]
pub struct MemoryMailer(Arc<Mutex<Vec<MailMessage>>>);

impl MemoryMailer {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn recorded(&self) -> Vec<MailMessage> {
        self.0.lock().unwrap().clone()
    }
}

impl Mailer for MemoryMailer {
    async fn deliver(&self, mail: &MailMessage) -> Result<(), MailError> {
        mail.to_message()?;
        self.0.lock().unwrap().push(mail.clone());
        Ok(())
    }
}
