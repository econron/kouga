use crate::{ClientIp, Error, HttpOptions, HttpRequest, Middleware, Next};
use axum::body::Body;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Request};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::from_fn;
use axum::response::{IntoResponse, Response};
use futures_util::FutureExt;
use http_body_util::BodyExt;
use kouga_core::{Error as CoreError, ErrorKind, RequestId};
use std::net::{IpAddr, SocketAddr};
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;
use tower_http::cors::{AllowHeaders, Any, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;

pub(crate) fn apply<S>(
    app: axum::Router,
    state: S,
    chain: Vec<Middleware<S>>,
    options: HttpOptions,
) -> axum::Router
where
    S: Clone + Send + Sync + 'static,
{
    let chain = Arc::new(chain);
    let custom_state = state.clone();
    let app = app.layer(from_fn(move |request, next| {
        let chain = chain.clone();
        let state = custom_state.clone();
        async move {
            let request = HttpRequest::new(request, state.clone());
            Next::new(chain, next, state)
                .run(request)
                .await
                .unwrap_or_else(IntoResponse::into_response)
        }
    }));

    let limit = Arc::new(Semaphore::new(options.max_in_flight));
    let timeout = options.timeout;
    let app = app.layer(from_fn(
        move |request: Request, next: axum::middleware::Next| {
            let limit = limit.clone();
            async move {
                let Ok(permit) = limit.try_acquire_owned() else {
                    return Error(CoreError::new(
                        ErrorKind::Unavailable,
                        "overloaded",
                        "Server overloaded",
                    ))
                    .into_response();
                };
                let response = match tokio::time::timeout(timeout, next.run(request)).await {
                    Ok(response) => response,
                    Err(_) => Error(CoreError::new(
                        ErrorKind::Timeout,
                        "timeout",
                        "Request timed out",
                    ))
                    .into_response(),
                };
                let (parts, body) = response.into_parts();
                let body = body.map_frame(move |frame| {
                    let _ = &permit;
                    frame
                });
                Response::from_parts(parts, Body::new(body))
            }
        },
    ));
    let app = app
        .layer(DefaultBodyLimit::disable())
        .layer(RequestBodyLimitLayer::new(options.max_body_bytes));

    let mut cors = CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::HEAD,
            Method::OPTIONS,
        ])
        .allow_headers(AllowHeaders::mirror_request());
    if options.cors_origins.iter().any(|origin| origin == "*") {
        cors = cors.allow_origin(Any);
    } else if !options.cors_origins.is_empty() {
        let origins: Vec<HeaderValue> = options
            .cors_origins
            .iter()
            .map(|origin| HeaderValue::from_str(origin).expect("validated origin"))
            .collect();
        cors = cors.allow_origin(origins);
    }
    if options.cors_credentials {
        cors = cors.allow_credentials(true);
    }
    let app = if options.cors_origins.is_empty() {
        app
    } else {
        app.layer(cors)
    };

    let no_cors = options.cors_origins.is_empty();
    app.layer(from_fn(move |mut request: Request, next: axum::middleware::Next| {
        let trusted = options.trusted_proxies.clone();
        async move {
            let id = RequestId(uuid::Uuid::new_v4().to_string());
            let denied_preflight = request.method() == Method::OPTIONS
                && request.headers().contains_key(header::ORIGIN)
                && request.headers().contains_key(header::ACCESS_CONTROL_REQUEST_METHOD)
                && no_cors;
            let client_ip = client_ip(&request, &trusted);
            request.extensions_mut().insert(id.clone());
            if let Some(ip) = client_ip { request.extensions_mut().insert(ClientIp(ip)); }
            let method = request.method().clone();
            let path = request.uri().path().to_owned();
            let started = Instant::now();
            let mut response = if denied_preflight {
                Error(CoreError::new(ErrorKind::Forbidden, "cors_forbidden", "Origin not allowed")).into_response()
            } else { match AssertUnwindSafe(next.run(request)).catch_unwind().await {
                Ok(response) => response,
                Err(_) => {
                    tracing::error!(request_id = %id, "HTTP handler panicked");
                    Error(CoreError::new(ErrorKind::Internal, "internal_error", "Internal server error")).into_response()
                }
            }};
            if response.status() == StatusCode::PAYLOAD_TOO_LARGE {
                let error = CoreError::new(ErrorKind::TooLarge, "payload_too_large", "Request body too large");
                replace_error_body(&mut response, &error, &id);
            } else if let Some(error) = response.extensions_mut().remove::<Arc<CoreError>>() {
                replace_error_body(&mut response, &error, &id);
            }
            response.headers_mut().insert("x-request-id", HeaderValue::from_str(&id.0).expect("UUID header"));
            tracing::info!(request_id = %id, method = %method, path, status = response.status().as_u16(), duration_ms = started.elapsed().as_millis() as u64, client_ip = ?client_ip, "HTTP request");
            response
        }
    }))
}

fn client_ip(request: &Request, trusted: &[IpAddr]) -> Option<IpAddr> {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()?
        .0
        .ip();
    if !trusted.contains(&peer) {
        return Some(peer);
    }
    let Some(forwarded) = request
        .headers()
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
    else {
        return Some(peer);
    };
    let mut selected = peer;
    for part in forwarded.split(',').rev() {
        let Ok(ip) = part.trim().parse() else {
            return Some(peer);
        };
        if !trusted.contains(&ip) {
            return Some(ip);
        }
        selected = ip;
    }
    Some(selected)
}

fn replace_error_body(response: &mut Response, error: &CoreError, id: &RequestId) {
    let payload = serde_json::json!({"error": {
        "code": error.code, "message": error.message, "details": &error.details
    }, "request_id": id.0});
    *response.body_mut() =
        Body::from(serde_json::to_vec(&payload).expect("error envelope must serialize"));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response.headers_mut().remove(header::CONTENT_LENGTH);
}
