use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use http::{Request, StatusCode, header};
use kouga_core::{Error as CoreError, ErrorKind, RequestId};
use kouga_http::{
    ClientIp, Endpoint, Error, Extension, HttpOptions, HttpRequest, Json, Middleware, Next,
    Operation, Router, Validated, bearer_auth,
};
use kouga_validation::Request as Validatable;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;
use tower::ServiceExt;

fn record(name: &'static str, calls: Arc<Mutex<Vec<&'static str>>>) -> Middleware<()> {
    Middleware::new(move |request: HttpRequest<()>, next: Next<()>| {
        let calls = calls.clone();
        async move {
            calls.lock().unwrap().push(name);
            let response = next.run(request).await?;
            calls.lock().unwrap().push(match name {
                "global" => "global-out",
                "group" => "group-out",
                _ => "route-out",
            });
            Ok(response)
        }
    })
}

#[derive(Validatable)]
struct Input {
    #[validate(length(min = 1))]
    title: String,
}

async fn json(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap()).unwrap()
}

#[tokio::test]
async fn middleware_order_extensions_and_security_metadata() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let router = Router::<()>::new()
        .middleware(record("global", calls.clone()))
        .group("/api")
        .unwrap()
        .middleware(record("group", calls.clone()))
        .middleware(bearer_auth(
            |mut request: HttpRequest<()>, next: Next<()>| async move {
                request.extensions_mut().insert(7_u32);
                next.run(request).await
            },
        ))
        .get(
            "/items",
            Endpoint::handler(
                |Extension(value): Extension<u32>| async move { Json(value) },
                Operation::new("items.show").response::<Json<u32>>(),
            )
            .middleware(record("route", calls.clone())),
        )
        .unwrap()
        .finish();
    assert_eq!(router.routes()[0].security, ["bearerAuth"]);
    let response = router
        .with_state(())
        .oneshot(
            Request::builder()
                .uri("/api/items")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().contains_key("x-request-id"));
    assert_eq!(json(response).await["data"], 7);
    assert_eq!(
        *calls.lock().unwrap(),
        [
            "global",
            "group",
            "route",
            "route-out",
            "group-out",
            "global-out"
        ]
    );
}

#[tokio::test]
async fn short_circuit_precedes_validation_and_error_has_request_id() {
    let router = Router::<()>::new()
        .middleware(|_request: HttpRequest<()>, _next: Next<()>| async {
            Err(Error(CoreError::new(
                ErrorKind::Unauthorized,
                "unauthorized",
                "Unauthorized",
            )))
        })
        .post(
            "/items",
            Endpoint::handler(
                |_input: Validated<Input>| async { Json("called") },
                Operation::new("items.create")
                    .json_input::<Input>()
                    .response::<Json<&str>>(),
            ),
        )
        .unwrap();
    let response = router
        .with_state(())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/items")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let id = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    let payload = json(response).await;
    assert_eq!(payload["error"]["code"], "unauthorized");
    assert_eq!(payload["request_id"], id);
}

#[tokio::test]
async fn cors_preflight_skips_auth_and_errors_keep_cors_headers() {
    let mut options = HttpOptions::default();
    options.cors_origins.push("https://example.test".to_owned());
    let router = Router::<()>::new()
        .configure(options)
        .unwrap()
        .middleware(|_request: HttpRequest<()>, _next: Next<()>| async {
            Err(Error(CoreError::new(
                ErrorKind::Unauthorized,
                "unauthorized",
                "Unauthorized",
            )))
        })
        .get(
            "/items",
            Endpoint::handler(
                || async { Json("ok") },
                Operation::new("items.list").response::<Json<&str>>(),
            ),
        )
        .unwrap();
    let app = router.with_state(());
    let preflight = app
        .clone()
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/items")
                .header(header::ORIGIN, "https://example.test")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(preflight.status(), StatusCode::OK);
    assert_eq!(
        preflight.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "https://example.test"
    );
    assert!(preflight.headers().contains_key("x-request-id"));
    let denied = app
        .oneshot(
            Request::builder()
                .uri("/items")
                .header(header::ORIGIN, "https://example.test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        denied.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "https://example.test"
    );
    assert_eq!(json(denied).await["error"]["code"], "unauthorized");
}

#[tokio::test]
async fn limits_timeout_and_trusted_proxy() {
    let mut options = HttpOptions {
        max_body_bytes: 4,
        timeout: Duration::from_millis(5),
        ..HttpOptions::default()
    };
    options
        .trusted_proxies
        .push(IpAddr::V4(Ipv4Addr::LOCALHOST));
    let router = Router::<()>::new().configure(options).unwrap()
        .post("/items", Endpoint::handler(
            |_input: Validated<Input>| async { Json("ok") },
            Operation::new("items.create").json_input::<Input>().response::<Json<&str>>(),
        )).unwrap()
        .get("/ip", Endpoint::handler(
            |Extension(ip): Extension<ClientIp>, Extension(id): Extension<RequestId>| async move { Json(format!("{}:{}", ip.0, id.0)) },
            Operation::new("items.ip").response::<Json<String>>(),
        )).unwrap()
        .get("/slow", Endpoint::handler(
            || async { tokio::time::sleep(Duration::from_millis(30)).await; Json("late") },
            Operation::new("items.slow").response::<Json<&str>>(),
        )).unwrap();
    let app = router.with_state(());
    let big = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/items")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::CONTENT_LENGTH, "100")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(big.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(json(big).await["error"]["code"], "payload_too_large");
    let streamed = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/items")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from_stream(futures_util::stream::once(async {
                    Ok::<_, std::convert::Infallible>(axum::body::Bytes::from_static(
                        b"{\"title\":\"x\"}",
                    ))
                })))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(streamed.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(json(streamed).await["error"]["code"], "payload_too_large");
    let slow = app
        .clone()
        .oneshot(Request::builder().uri("/slow").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(slow.status(), StatusCode::GATEWAY_TIMEOUT);
    let mut request = Request::builder()
        .uri("/ip")
        .header("x-forwarded-for", "198.51.100.4")
        .header("x-forwarded-for", "203.0.113.9, 127.0.0.1")
        .body(Body::empty())
        .unwrap();
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from((Ipv4Addr::LOCALHOST, 1234))));
    let response = app.oneshot(request).await.unwrap();
    let id = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    assert_eq!(json(response).await["data"], format!("203.0.113.9:{id}"));
}

#[tokio::test]
async fn streaming_response_has_absolute_deadline_and_releases_slot() {
    let gate = Arc::new(Notify::new());
    let router = Router::<()>::new()
        .configure(HttpOptions {
            max_in_flight: 1,
            timeout: Duration::from_millis(40),
            ..HttpOptions::default()
        })
        .unwrap()
        .get(
            "/stream",
            Endpoint::handler(
                {
                    let gate = gate.clone();
                    move || {
                        let gate = gate.clone();
                        async move {
                            Body::from_stream(futures_util::stream::once(async move {
                                gate.notified().await;
                                Ok::<_, std::convert::Infallible>(axum::body::Bytes::from_static(
                                    b"done",
                                ))
                            }))
                        }
                    }
                },
                Operation::new("stream.show").response::<kouga_http::NoContent>(),
            ),
        )
        .unwrap();
    let app = router.with_state(());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let overloaded = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(overloaded.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(json(overloaded).await["error"]["code"], "overloaded");
    tokio::time::sleep(Duration::from_millis(80)).await;
    let next = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(next.status(), StatusCode::OK);
    assert!(
        tokio::time::timeout(
            Duration::from_millis(20),
            to_bytes(response.into_body(), 1024)
        )
        .await
        .unwrap()
        .is_err()
    );
    drop(next);
    let after_drop = app
        .oneshot(
            Request::builder()
                .uri("/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(after_drop.status(), StatusCode::OK);
}

#[tokio::test]
async fn missing_route_and_disabled_cors_preflight_have_common_errors() {
    let app = Router::<()>::new().with_state(());
    let missing = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/missing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert!(missing.headers().contains_key("x-request-id"));
    assert_eq!(json(missing).await["error"]["code"], "not_found");
    let preflight = app
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/missing")
                .header(header::ORIGIN, "https://example.test")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(preflight.status(), StatusCode::FORBIDDEN);
    assert!(preflight.headers().contains_key("x-request-id"));
    assert_eq!(json(preflight).await["error"]["code"], "cors_forbidden");
}
