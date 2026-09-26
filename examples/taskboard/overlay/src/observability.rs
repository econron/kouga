//! Taskboard health and shared-store admission policy.
use axum::{http::StatusCode, response::IntoResponse};
use kouga_cache::{RateDecision, RateLimiter};
use kouga_http::{
    ClientIp, Endpoint, HttpRequest, Json, Middleware, Next, Operation, Router, State,
};
use kouga_model::{Db, sqlx};
use std::time::Duration;
use tracing::Instrument;

async fn ready(State(db): State<Db>) -> impl IntoResponse {
    let probe =
        tokio::time::timeout(Duration::from_secs(1), sqlx::query("SELECT 1").execute(&db)).await;
    if matches!(probe, Ok(Ok(_))) {
        (StatusCode::OK, Json("ready")).into_response()
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json("unavailable")).into_response()
    }
}

fn admission(limit: u64) -> Middleware<Db> {
    Middleware::new(move |request: HttpRequest<Db>, next: Next<Db>| async move {
        let (db, parts, body) = request.into_parts();
        let path = parts.uri.path();
        if path == "/health" || path == "/ready" {
            return next.run(HttpRequest::from_parts(db, parts, body)).await;
        }
        // ClientIp is derived from the direct TCP peer, never an untrusted forwarding header.
        let peer = parts
            .extensions
            .get::<ClientIp>()
            .map(|ip| ip.0.to_string());
        let Some(peer) = peer else {
            return Err(kouga_http::Error(kouga_core::Error::new(
                kouga_core::ErrorKind::Unavailable,
                "client_ip_unavailable",
                "Service unavailable",
            )));
        };
        let limiter =
            RateLimiter::new(db.clone(), "taskboard-http", limit, Duration::from_secs(60))
                .expect("positive rate limit");
        match limiter.check(&peer).await {
            Ok(RateDecision::Allowed) => {
                let span = tracing::info_span!("taskboard.http.business");
                let response = next
                    .run(HttpRequest::from_parts(db, parts, body))
                    .instrument(span)
                    .await?;
                opentelemetry::global::meter("taskboard")
                    .u64_counter("taskboard.http.requests")
                    .build()
                    .add(1, &[]);
                tracing::info!(
                    status = response.status().as_u16(),
                    "taskboard request completed"
                );
                Ok(response)
            }
            Ok(RateDecision::Denied { retry_after }) => {
                let mut response = kouga_http::Error(kouga_core::Error::new(
                    kouga_core::ErrorKind::RateLimited,
                    "rate_limited",
                    "Rate limit exceeded",
                ))
                .into_response();
                response.headers_mut().insert(
                    axum::http::header::RETRY_AFTER,
                    axum::http::HeaderValue::from_str(&retry_after.to_string())
                        .expect("integer header"),
                );
                Ok(response)
            }
            Err(_) => Err(kouga_http::Error(kouga_core::Error::new(
                kouga_core::ErrorKind::Unavailable,
                "rate_limit_unavailable",
                "Service unavailable",
            ))),
        }
    })
}

/// Attach before business routes so all HTTP processes use the same PostgreSQL counter.
pub fn routes(router: Router<Db>) -> Router<Db> {
    let limit = std::env::var("BOARD_RATE_LIMIT_PER_MINUTE")
        .ok()
        .map(|value| {
            value
                .parse::<u64>()
                .expect("BOARD_RATE_LIMIT_PER_MINUTE must be a positive integer")
        })
        .unwrap_or(120);
    routes_with_limit(router, limit)
}

pub fn routes_with_limit(router: Router<Db>, limit: u64) -> Router<Db> {
    assert!(limit > 0, "rate limit must be positive");
    let mut operation = Operation::new("health.ready").response::<Json<&str>>();
    operation.responses.push(kouga_http::ResponseMeta {
        status: 503,
        content_type: Some("application/json"),
        data_schema: Some(serde_json::json!({"type":"string"})),
        paginated: false,
    });
    router
        .get("/ready", Endpoint::handler(ready, operation))
        .expect("ready route")
        .middleware(admission(limit))
}
