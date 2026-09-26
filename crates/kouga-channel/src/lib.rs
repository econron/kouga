//! Best-effort WebSocket channels shared between HTTP processes through PostgreSQL NOTIFY.
//! The application still serves its authoritative state through HTTP after reconnection.

use axum::{
    Json, Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::StreamExt;
use kouga_auth::CurrentUser;
use kouga_db::Db;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::postgres::PgListener;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore, broadcast},
    task::AbortHandle,
    time::{interval, timeout},
};
use uuid::Uuid;

pub const SCHEMA_SQL: &str =
    include_str!("../migrations/20260925000025_create_channel_tickets.up.sql");
const NOTIFY_PREFIX: &str = "kouga_channel_";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Subscribe,
    Publish,
}

#[derive(Clone)]
pub struct Options {
    pub allowed_origins: Vec<String>,
    pub ticket_ttl: Duration,
    pub auth_check_interval: Duration,
    pub heartbeat_interval: Duration,
    pub max_connections: usize,
    pub max_message_bytes: usize,
    pub outbound_buffer: usize,
    pub send_timeout: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            allowed_origins: Vec::new(),
            ticket_ttl: Duration::from_secs(30),
            auth_check_interval: Duration::from_secs(30),
            heartbeat_interval: Duration::from_secs(20),
            max_connections: 1000,
            max_message_bytes: 4096,
            outbound_buffer: 64,
            send_timeout: Duration::from_secs(5),
        }
    }
}

impl Options {
    fn validate(&self) -> Result<(), ChannelError> {
        if self.allowed_origins.is_empty()
            || self.allowed_origins.iter().any(|origin| {
                let host = origin
                    .strip_prefix("https://")
                    .or_else(|| origin.strip_prefix("http://"));
                !host.is_some_and(|host| {
                    !host.is_empty()
                        && !host
                            .bytes()
                            .any(|byte| matches!(byte, b'/' | b'@' | b'?' | b'#' | b' '))
                })
            })
            || self.ticket_ttl.as_secs() == 0
            || self.ticket_ttl.as_secs() > 3600
            || self.auth_check_interval.is_zero()
            || self.heartbeat_interval.is_zero()
            || self.send_timeout.is_zero()
            || self.max_connections == 0
            || self.outbound_buffer == 0
            || !(1..=7000).contains(&self.max_message_bytes)
        {
            return Err(ChannelError::InvalidOptions);
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum ChannelError {
    InvalidOptions,
    InvalidChannel,
    TooLarge,
    Unauthorized,
    Database(sqlx::Error),
}

impl std::fmt::Display for ChannelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidOptions => "invalid channel options",
            Self::InvalidChannel => "invalid channel",
            Self::TooLarge => "channel message too large",
            Self::Unauthorized => "unauthorized",
            Self::Database(_) => "channel unavailable",
        })
    }
}
impl std::error::Error for ChannelError {}
impl From<sqlx::Error> for ChannelError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(value)
    }
}

type Policy = dyn Fn(CurrentUser, Action, &str) -> bool + Send + Sync;

struct Inner {
    db: Db,
    options: Options,
    policy: Arc<Policy>,
    sender: broadcast::Sender<Event>,
    notify_channel: String,
    permits: Arc<Semaphore>,
    listener: Mutex<Option<AbortHandle>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(handle) = self.listener.lock().expect("listener mutex").take() {
            handle.abort();
        }
    }
}

#[derive(Clone)]
pub struct Channel(Arc<Inner>);

#[derive(Clone, Serialize, Deserialize)]
struct Event {
    channel: String,
    data: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Command {
    #[serde(rename = "type")]
    kind: String,
    channel: String,
    #[serde(default)]
    data: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct TicketResponse {
    ticket: String,
    expires_in: u64,
}

impl Channel {
    /// Start listening before accepting any upgrades. Only live sockets receive notifications.
    pub async fn start(
        db: Db,
        options: Options,
        policy: impl Fn(CurrentUser, Action, &str) -> bool + Send + Sync + 'static,
    ) -> Result<Self, ChannelError> {
        options.validate()?;
        let schema: String = sqlx::query_scalar("SELECT current_schema()")
            .fetch_one(&db)
            .await?;
        let digest = format!("{:x}", Sha256::digest(schema.as_bytes()));
        let notify_channel = format!("{NOTIFY_PREFIX}{}", &digest[..40]);
        let mut listener = PgListener::connect_with(&db).await?;
        listener.listen(&notify_channel).await?;
        let (sender, _) = broadcast::channel(options.outbound_buffer);
        let inner = Arc::new(Inner {
            db,
            permits: Arc::new(Semaphore::new(options.max_connections)),
            options,
            policy: Arc::new(policy),
            sender: sender.clone(),
            notify_channel,
            listener: Mutex::new(None),
        });
        let task = tokio::spawn(async move {
            loop {
                match listener.recv().await {
                    Ok(notification) => {
                        if let Ok(event) = serde_json::from_str::<Event>(notification.payload()) {
                            let _ = sender.send(event);
                        }
                    }
                    Err(_) => {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        });
        *inner.listener.lock().expect("listener mutex") = Some(task.abort_handle());
        Ok(Self(inner))
    }

    /// Mount separately with `app.merge(channel.router())`; do not put a long-term bearer in the URL.
    pub fn router(&self) -> Router {
        Router::new()
            .route("/_kouga/ws-ticket", post(issue_ticket_http))
            .route("/_kouga/ws", get(upgrade_http))
            .with_state(self.clone())
    }

    /// Publish an authorized business event. PostgreSQL rejects delivery when unavailable.
    pub async fn publish(
        &self,
        actor: CurrentUser,
        channel: &str,
        data: serde_json::Value,
    ) -> Result<(), ChannelError> {
        validate_channel(channel)?;
        if !(self.0.policy)(actor, Action::Publish, channel) {
            return Err(ChannelError::Unauthorized);
        }
        let event = Event {
            channel: channel.to_owned(),
            data,
        };
        let payload = serde_json::to_string(&event).expect("JSON event");
        if payload.len() > self.0.options.max_message_bytes {
            return Err(ChannelError::TooLarge);
        }
        sqlx::query("SELECT pg_notify($1, $2)")
            .bind(&self.0.notify_channel)
            .bind(payload)
            .execute(&self.0.db)
            .await?;
        Ok(())
    }

    pub async fn issue_ticket(&self, bearer: &str) -> Result<Option<String>, ChannelError> {
        if bearer.len() != 64 || !bearer.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Ok(None);
        }
        sqlx::query("DELETE FROM kouga_channel_tickets WHERE expires_at <= now()")
            .execute(&self.0.db)
            .await?;
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let seconds = self.0.options.ticket_ttl.as_secs() as i64;
        let inserted: Option<i32> = sqlx::query_scalar(
            "INSERT INTO kouga_channel_tickets (ticket_hash, bearer_hash, expires_at) \
             SELECT $1, token_hash, now() + $3 * interval '1 second' FROM kouga_auth_tokens \
             WHERE token_hash = $2 AND revoked_at IS NULL AND expires_at > now() RETURNING 1",
        )
        .bind(hash(&token))
        .bind(hash(bearer))
        .bind(seconds)
        .fetch_optional(&self.0.db)
        .await?;
        Ok(inserted.map(|_| token))
    }

    async fn consume_ticket(
        &self,
        ticket: &str,
    ) -> Result<Option<(CurrentUser, Vec<u8>)>, ChannelError> {
        if ticket.len() != 64 || !ticket.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Ok(None);
        }
        let row: Option<(Uuid, Vec<u8>)> = sqlx::query_as(
            "DELETE FROM kouga_channel_tickets AS t USING kouga_auth_tokens AS a \
             WHERE t.ticket_hash = $1 AND t.bearer_hash = a.token_hash \
             AND t.expires_at > now() AND a.expires_at > now() AND a.revoked_at IS NULL \
             RETURNING a.user_id, t.bearer_hash",
        )
        .bind(hash(ticket))
        .fetch_optional(&self.0.db)
        .await?;
        Ok(row.map(|(id, hash)| (CurrentUser { id }, hash)))
    }
}

fn hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

fn validate_channel(channel: &str) -> Result<(), ChannelError> {
    if channel.is_empty()
        || channel.len() > 128
        || !channel
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'_' | b'-' | b'.'))
    {
        return Err(ChannelError::InvalidChannel);
    }
    Ok(())
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let value = values.next()?.to_str().ok()?;
    if values.next().is_some() {
        return None;
    }
    value
        .strip_prefix("Bearer ")
        .filter(|token| !token.is_empty() && !token.contains(char::is_whitespace))
}

async fn issue_ticket_http(State(channel): State<Channel>, headers: HeaderMap) -> Response {
    let Some(token) = bearer(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    match channel.issue_ticket(token).await {
        Ok(Some(ticket)) => Json(TicketResponse {
            ticket,
            expires_in: channel.0.options.ticket_ttl.as_secs(),
        })
        .into_response(),
        Ok(None) => StatusCode::UNAUTHORIZED.into_response(),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

async fn upgrade_http(
    State(channel): State<Channel>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let mut origins = headers.get_all(header::ORIGIN).iter();
    let origin = origins.next().and_then(|value| value.to_str().ok());
    if !origin.is_some_and(|origin| {
        channel
            .0
            .options
            .allowed_origins
            .iter()
            .any(|allowed| allowed == origin)
    }) || origins.next().is_some()
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    // Browser WebSocket cannot set Authorization. The one-use ticket travels as a
    // subprotocol header, never in a URL that an access log may record.
    let mut protocols = headers.get_all(header::SEC_WEBSOCKET_PROTOCOL).iter();
    let Some(ticket) = protocols
        .next()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value
                .split(',')
                .map(str::trim)
                .find_map(|protocol| protocol.strip_prefix("kouga-ticket."))
        })
    else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if protocols.next().is_some() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Ok(permit) = channel.0.permits.clone().try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let (actor, bearer_hash) = match channel.consume_ticket(ticket).await {
        Ok(Some(value)) => value,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    ws.max_message_size(channel.0.options.max_message_bytes)
        .max_frame_size(channel.0.options.max_message_bytes)
        .protocols(["kouga"])
        .on_upgrade(move |socket| async move {
            channel.socket(socket, actor, bearer_hash, permit).await
        })
}

impl Channel {
    async fn socket(
        &self,
        mut socket: WebSocket,
        actor: CurrentUser,
        bearer_hash: Vec<u8>,
        _permit: OwnedSemaphorePermit,
    ) {
        let mut events = self.0.sender.subscribe();
        let mut subscriptions = HashSet::new();
        let mut auth_timer = interval(self.0.options.auth_check_interval);
        let mut heartbeat = interval(self.0.options.heartbeat_interval);
        auth_timer.tick().await;
        heartbeat.tick().await;
        let mut awaiting_pong = false;
        loop {
            tokio::select! {
                _ = auth_timer.tick() => {
                    let valid: Result<bool, _> = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM kouga_auth_tokens WHERE token_hash = $1 AND revoked_at IS NULL AND expires_at > now())")
                        .bind(&bearer_hash).fetch_one(&self.0.db).await;
                    if !matches!(valid, Ok(true)) { break; }
                }
                _ = heartbeat.tick() => {
                    if awaiting_pong || !matches!(timeout(self.0.options.send_timeout, socket.send(Message::Ping(Vec::new().into()))).await, Ok(Ok(()))) { break; }
                    awaiting_pong = true;
                }
                incoming = socket.next() => {
                    match incoming {
                        Some(Ok(Message::Pong(_))) => awaiting_pong = false,
                        Some(Ok(Message::Text(text))) => {
                            if text.len() > self.0.options.max_message_bytes { break; }
                            let Ok(command) = serde_json::from_str::<Command>(&text) else { break; };
                            if validate_channel(&command.channel).is_err() { break; }
                            match command.kind.as_str() {
                                "subscribe" if (self.0.policy)(actor, Action::Subscribe, &command.channel) => {
                                    subscriptions.insert(command.channel.clone());
                                    let ack = serde_json::json!({"type":"subscribed","channel":command.channel}).to_string();
                                    if !matches!(timeout(self.0.options.send_timeout, socket.send(Message::Text(ack.into()))).await, Ok(Ok(()))) { break; }
                                }
                                "unsubscribe" => { subscriptions.remove(&command.channel); }
                                "publish" => {
                                    let Some(data) = command.data else { break; };
                                    if self.publish(actor, &command.channel, data).await.is_err() { break; }
                                }
                                _ => break,
                            }
                        }
                        Some(Ok(Message::Binary(_))) | Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                        _ => {}
                    }
                }
                event = events.recv() => {
                    let event = match event { Ok(event) => event, Err(_) => break };
                    if subscriptions.contains(&event.channel) {
                        let payload = serde_json::to_string(&event).expect("JSON event");
                        if !matches!(timeout(self.0.options.send_timeout, socket.send(Message::Text(payload.into()))).await, Ok(Ok(()))) { break; }
                    }
                }
            }
        }
        let _ = timeout(
            self.0.options.send_timeout,
            socket.send(Message::Close(None)),
        )
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_untrusted_names_and_limits() {
        assert!(validate_channel("users:123.events").is_ok());
        assert!(validate_channel("../private/path").is_err());
        assert!(validate_channel(&"a".repeat(129)).is_err());
        assert!(
            Options {
                allowed_origins: vec!["*".into()],
                ..Options::default()
            }
            .validate()
            .is_err()
        );
    }
}
