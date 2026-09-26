use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use kouga_auth::{CurrentUser, issue_token, revoke_token};
use kouga_channel::{Action, Channel, Options, SCHEMA_SQL};
use kouga_test::{TestClient, TestDb};
use sha2::{Digest, Sha256};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

async fn next_text(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> String {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => break text.to_string(),
                Some(Ok(Message::Ping(payload))) => {
                    ws.send(Message::Pong(payload)).await.unwrap();
                }
                Some(Ok(Message::Pong(_))) => continue,
                other => panic!("expected text, got {other:?}"),
            }
        }
    })
    .await
    .unwrap()
}

fn options() -> Options {
    Options {
        allowed_origins: vec!["http://localhost:3000".into()],
        auth_check_interval: Duration::from_millis(100),
        heartbeat_interval: Duration::from_secs(2),
        send_timeout: Duration::from_millis(100),
        max_message_bytes: 512,
        ..Options::default()
    }
}

#[tokio::test]
async fn two_servers_tickets_origin_policy_and_revocation() {
    let Ok(url) = std::env::var("KOUGA_TEST_DATABASE_URL") else {
        return;
    };
    let isolated = TestDb::connect(
        &url,
        concat!(env!("CARGO_MANIFEST_DIR"), "/../kouga-auth/migrations"),
    )
    .await
    .unwrap();
    let db = isolated.db();
    sqlx::raw_sql(SCHEMA_SQL).execute(db).await.unwrap();
    let actor = CurrentUser {
        id: uuid::Uuid::new_v4(),
    };
    let bearer = issue_token(db, actor.id, Duration::from_secs(60))
        .await
        .unwrap();
    let policy = move |user: CurrentUser, action: Action, channel: &str| {
        user.id == actor.id && channel == "public" && action == Action::Subscribe
    };
    let source = Channel::start(db.clone(), options(), policy).await.unwrap();
    let destination = Channel::start(db.clone(), options(), policy).await.unwrap();
    let ticket = source.issue_ticket(&bearer).await.unwrap().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let current = Arc::new(AtomicUsize::new(1));
    let state = current.clone();
    let http = axum::Router::new().route(
        "/state",
        get(move || {
            let state = state.clone();
            async move { state.load(Ordering::SeqCst).to_string() }
        }),
    );
    let client = TestClient::new(http.clone());
    let app = http.merge(destination.router());
    drop(destination);
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });

    let request = |ticket: &str, origin: &str| {
        let mut req = format!("ws://{address}/_kouga/ws")
            .into_client_request()
            .unwrap();
        req.headers_mut()
            .insert(header::ORIGIN, origin.parse().unwrap());
        req.headers_mut().insert(
            header::SEC_WEBSOCKET_PROTOCOL,
            format!("kouga, kouga-ticket.{ticket}").parse().unwrap(),
        );
        req
    };
    let denied = connect_async(request(&ticket, "http://evil.test"))
        .await
        .unwrap_err();
    assert!(
        matches!(denied, tokio_tungstenite::tungstenite::Error::Http(response) if response.status() == StatusCode::FORBIDDEN)
    );
    let expired_ticket = source.issue_ticket(&bearer).await.unwrap().unwrap();
    sqlx::query("UPDATE kouga_channel_tickets SET expires_at = now() - interval '1 second'")
        .execute(db)
        .await
        .unwrap();
    let expired = connect_async(request(&expired_ticket, "http://localhost:3000"))
        .await
        .unwrap_err();
    assert!(
        matches!(expired, tokio_tungstenite::tungstenite::Error::Http(response) if response.status() == StatusCode::UNAUTHORIZED)
    );
    let denied_ticket = source.issue_ticket(&bearer).await.unwrap().unwrap();
    let (mut denied_ws, _) = connect_async(request(&denied_ticket, "http://localhost:3000"))
        .await
        .unwrap();
    denied_ws
        .send(Message::Text(
            r#"{"type":"subscribe","channel":"private"}"#.into(),
        ))
        .await
        .unwrap();
    let denied_message = tokio::time::timeout(Duration::from_secs(2), denied_ws.next())
        .await
        .unwrap();
    assert!(matches!(denied_message, Some(Ok(Message::Close(_))) | None));
    let denied_operation_ticket = source.issue_ticket(&bearer).await.unwrap().unwrap();
    let (mut denied_operation_ws, _) =
        connect_async(request(&denied_operation_ticket, "http://localhost:3000"))
            .await
            .unwrap();
    denied_operation_ws
        .send(Message::Text(
            r#"{"type":"publish","channel":"public","data":1}"#.into(),
        ))
        .await
        .unwrap();
    let denied_operation = tokio::time::timeout(Duration::from_secs(2), denied_operation_ws.next())
        .await
        .unwrap();
    assert!(matches!(
        denied_operation,
        Some(Ok(Message::Close(_))) | None
    ));
    drop(denied_ws);
    drop(denied_operation_ws);
    let ticket = source.issue_ticket(&bearer).await.unwrap().unwrap();
    let (mut ws, response) = connect_async(request(&ticket, "http://localhost:3000"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
    let replay = connect_async(request(&ticket, "http://localhost:3000"))
        .await
        .unwrap_err();
    assert!(
        matches!(replay, tokio_tungstenite::tungstenite::Error::Http(response) if response.status() == StatusCode::UNAUTHORIZED)
    );
    ws.send(Message::Text(
        r#"{"type":"subscribe","channel":"public"}"#.into(),
    ))
    .await
    .unwrap();
    assert!(next_text(&mut ws).await.contains("subscribed"));
    // The source is a distinct listener/server instance; only PostgreSQL bridges them.
    let publisher = Channel::start(db.clone(), options(), |_, action, channel| {
        action == Action::Publish && channel == "public"
    })
    .await
    .unwrap();
    publisher
        .publish(actor, "public", serde_json::json!({"value":1}))
        .await
        .unwrap();
    assert!(next_text(&mut ws).await.contains("\"value\":1"));
    let ping = tokio::time::timeout(Duration::from_secs(3), async {
        match ws.next().await {
            Some(Ok(Message::Ping(payload))) => payload,
            other => panic!("expected heartbeat ping, got {other:?}"),
        }
    })
    .await
    .unwrap();
    ws.send(Message::Pong(ping)).await.unwrap();
    publisher
        .publish(actor, "public", serde_json::json!({"after_heartbeat":true}))
        .await
        .unwrap();
    assert!(next_text(&mut ws).await.contains("after_heartbeat"));
    ws.close(None).await.unwrap();
    drop(ws);
    current.store(2, Ordering::SeqCst);
    let new_ticket = source.issue_ticket(&bearer).await.unwrap().unwrap();
    let (mut ws, _) = connect_async(request(&new_ticket, "http://localhost:3000"))
        .await
        .unwrap();
    let response = client
        .send(Request::get("/state").body(Body::empty()).unwrap())
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(&to_bytes(response.into_body(), 16).await.unwrap()[..], b"2");
    assert!(
        publisher
            .publish(
                actor,
                "public",
                serde_json::json!({"large":"x".repeat(600)})
            )
            .await
            .is_err()
    );
    assert!(
        source
            .publish(actor, "public", serde_json::json!(1))
            .await
            .is_err()
    );
    revoke_token(db, &bearer).await.unwrap();
    let closed = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => continue,
                value => break value,
            }
        }
    })
    .await
    .unwrap();
    assert!(matches!(closed, Some(Ok(Message::Close(_))) | None));
    let _ = shutdown_tx.send(());
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
    drop(ws);
    drop(client);
    drop(source);
    drop(publisher);
    isolated.close().await.unwrap();
}

#[tokio::test]
async fn ticket_http_requires_bearer_and_hides_secret() {
    let Ok(url) = std::env::var("KOUGA_TEST_DATABASE_URL") else {
        return;
    };
    let isolated = TestDb::connect(
        &url,
        concat!(env!("CARGO_MANIFEST_DIR"), "/../kouga-auth/migrations"),
    )
    .await
    .unwrap();
    let db = isolated.db();
    sqlx::raw_sql(SCHEMA_SQL).execute(db).await.unwrap();
    let channel = Channel::start(db.clone(), options(), |_, _, _| false)
        .await
        .unwrap();
    let client = TestClient::new(channel.router());
    let denied = client
        .send(
            Request::post("/_kouga/ws-ticket")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    let bearer = issue_token(db, uuid::Uuid::new_v4(), Duration::from_secs(60))
        .await
        .unwrap();
    let issued = client
        .send(
            Request::post("/_kouga/ws-ticket")
                .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(issued.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(issued.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(body["ticket"].as_str().unwrap().len(), 64);
    assert!(!body.to_string().contains(&bearer));
    drop(client);
    drop(channel);
    isolated.close().await.unwrap();
}

#[tokio::test]
async fn child_server() {
    if std::env::var("KOUGA_CHANNEL_CHILD").is_err() {
        return;
    }
    let url = std::env::var("KOUGA_TEST_DATABASE_URL").unwrap();
    let schema = std::env::var("KOUGA_CHANNEL_SCHEMA").unwrap();
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .after_connect(move |connection, _| {
            let schema = schema.clone();
            Box::pin(async move {
                sqlx::query("SELECT set_config('search_path', $1, false)")
                    .bind(schema)
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .unwrap();
    let channel = Channel::start(db, options(), |_, action, name| {
        action == Action::Subscribe && name == "public"
    })
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    println!("CHANNEL_ADDR={}", listener.local_addr().unwrap());
    axum::serve(listener, channel.router()).await.unwrap();
}

struct ChildServer(std::process::Child);
impl Drop for ChildServer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn another_process_receives_notification() {
    let Ok(url) = std::env::var("KOUGA_TEST_DATABASE_URL") else {
        return;
    };
    let isolated = TestDb::connect(
        &url,
        concat!(env!("CARGO_MANIFEST_DIR"), "/../kouga-auth/migrations"),
    )
    .await
    .unwrap();
    let db = isolated.db();
    sqlx::raw_sql(SCHEMA_SQL).execute(db).await.unwrap();
    let schema: String = sqlx::query_scalar("SELECT current_schema()")
        .fetch_one(db)
        .await
        .unwrap();
    let actor = CurrentUser {
        id: uuid::Uuid::new_v4(),
    };
    let bearer = issue_token(db, actor.id, Duration::from_secs(60))
        .await
        .unwrap();
    let publisher = Channel::start(db.clone(), options(), |_, action, name| {
        action == Action::Publish && name == "public"
    })
    .await
    .unwrap();
    let ticket = publisher.issue_ticket(&bearer).await.unwrap().unwrap();
    let mut child = ChildServer(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "child_server", "--nocapture"])
            .env("KOUGA_CHANNEL_CHILD", "1")
            .env("KOUGA_CHANNEL_SCHEMA", schema)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut lines = std::io::BufReader::new(child.0.stdout.take().unwrap());
    let mut line = String::new();
    let address = loop {
        line.clear();
        assert!(
            std::io::BufRead::read_line(&mut lines, &mut line).unwrap() > 0,
            "child did not start"
        );
        if let Some((_, value)) = line.trim().split_once("CHANNEL_ADDR=") {
            break value.to_owned();
        }
    };
    let mut request = format!("ws://{address}/_kouga/ws")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert(header::ORIGIN, "http://localhost:3000".parse().unwrap());
    request.headers_mut().insert(
        header::SEC_WEBSOCKET_PROTOCOL,
        format!("kouga, kouga-ticket.{ticket}").parse().unwrap(),
    );
    let (mut ws, _) = connect_async(request).await.unwrap();
    ws.send(Message::Text(
        r#"{"type":"subscribe","channel":"public"}"#.into(),
    ))
    .await
    .unwrap();
    assert!(next_text(&mut ws).await.contains("subscribed"));
    publisher
        .publish(actor, "public", serde_json::json!({"other_process":true}))
        .await
        .unwrap();
    assert!(next_text(&mut ws).await.contains("other_process"));
    drop(ws);
    drop(child);
    drop(publisher);
    isolated.close().await.unwrap();
}

#[tokio::test]
async fn slow_subscriber_is_disconnected_when_broadcast_buffer_overflows() {
    let Ok(url) = std::env::var("KOUGA_TEST_DATABASE_URL") else {
        return;
    };
    let isolated = TestDb::connect(
        &url,
        concat!(env!("CARGO_MANIFEST_DIR"), "/../kouga-auth/migrations"),
    )
    .await
    .unwrap();
    let db = isolated.db();
    sqlx::raw_sql(SCHEMA_SQL).execute(db).await.unwrap();
    let actor = CurrentUser {
        id: uuid::Uuid::new_v4(),
    };
    let bearer = issue_token(db, actor.id, Duration::from_secs(60))
        .await
        .unwrap();
    let mut limits = options();
    limits.outbound_buffer = 1;
    limits.heartbeat_interval = Duration::from_secs(10);
    let channel = Channel::start(db.clone(), limits, |_, action, name| {
        action == Action::Subscribe && name == "public"
    })
    .await
    .unwrap();
    let ticket = channel.issue_ticket(&bearer).await.unwrap().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, channel.router()).await.unwrap();
    });
    let mut request = format!("ws://{address}/_kouga/ws")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert(header::ORIGIN, "http://localhost:3000".parse().unwrap());
    request.headers_mut().insert(
        header::SEC_WEBSOCKET_PROTOCOL,
        format!("kouga, kouga-ticket.{ticket}").parse().unwrap(),
    );
    let (mut ws, _) = connect_async(request).await.unwrap();
    ws.send(Message::Text(
        r#"{"type":"subscribe","channel":"public"}"#.into(),
    ))
    .await
    .unwrap();
    assert!(next_text(&mut ws).await.contains("subscribed"));
    // One SQL statement emits many distinct notifications in a burst; the receiver
    // intentionally does not read while the bounded local queue is flooded.
    let schema: String = sqlx::query_scalar("SELECT current_schema()")
        .fetch_one(db)
        .await
        .unwrap();
    let digest = format!("{:x}", Sha256::digest(schema.as_bytes()));
    let notify_channel = format!("kouga_channel_{}", &digest[..40]);
    sqlx::query("SELECT pg_notify($1, json_build_object('channel', 'public', 'data', i)::text) FROM generate_series(1, 2000) AS i")
        .bind(notify_channel).execute(db).await.unwrap();
    let closed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(_)))
                | Some(Ok(Message::Ping(_)))
                | Some(Ok(Message::Pong(_))) => continue,
                other => break other,
            }
        }
    })
    .await
    .unwrap();
    assert!(matches!(closed, Some(Ok(Message::Close(_))) | None));
    drop(ws);
    server.abort();
    isolated.close().await.unwrap();
}
