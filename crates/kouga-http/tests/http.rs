use axum::body::{Body, to_bytes};
use http::{Method, Request as HttpRequest, StatusCode, header};
use kouga_core::{Error as CoreError, ErrorKind};
use kouga_http::{Created, Error, Json, NoContent, Page, Path, Router, Validated};
use kouga_validation::Request;
use kouga_validation::axum::ContextFromRequest;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tower::ServiceExt;

#[derive(Request)]
struct Input {
    #[validate(length(min = 1))]
    title: String,
}

struct AuthContext;
impl ContextFromRequest<()> for AuthContext {
    async fn from_request(parts: &mut http::request::Parts, _: &()) -> Result<Self, CoreError> {
        if parts.headers.contains_key(header::AUTHORIZATION) {
            Ok(Self)
        } else {
            Err(CoreError::new(
                ErrorKind::Unauthorized,
                "unauthorized",
                "Unauthorized",
            ))
        }
    }
}

#[derive(Request)]
#[request(context = AuthContext)]
struct AuthInput {
    title: String,
}

async fn body(response: axum::response::Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn routes_and_responses() {
    let router = Router::<()>::new()
        .get(
            "/tasks/{id}",
            |Path(id): Path<String>| async move { Json(id) },
        )
        .unwrap()
        .get("/tasks/new", || async { Json("static") })
        .unwrap()
        .post("/tasks", || async { Created::new("/tasks/1", "created") })
        .unwrap()
        .delete("/tasks/{id}", || async { NoContent })
        .unwrap();
    assert_eq!(router.routes().len(), 4);
    let app = router.with_state(());
    let response = app
        .clone()
        .oneshot(
            HttpRequest::builder()
                .uri("/tasks/new")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(body(response).await["data"], "static");
    let response = app
        .clone()
        .oneshot(
            HttpRequest::builder()
                .uri("/tasks/42")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(body(response).await["data"], "42");
    let response = app
        .clone()
        .oneshot(
            HttpRequest::builder()
                .method(Method::HEAD)
                .uri("/tasks/new")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        to_bytes(response.into_body(), 1024)
            .await
            .unwrap()
            .is_empty()
    );
    let response = app
        .clone()
        .oneshot(
            HttpRequest::builder()
                .method(Method::OPTIONS)
                .uri("/tasks")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(response.headers()[header::ALLOW], "OPTIONS, POST");
    let response = app
        .clone()
        .oneshot(
            HttpRequest::builder()
                .method(Method::PUT)
                .uri("/tasks")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.headers()[header::ALLOW], "OPTIONS, POST");
    assert_eq!(body(response).await["error"]["code"], "method_not_allowed");
    let response = app
        .clone()
        .oneshot(
            HttpRequest::builder()
                .uri("/missing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = app
        .clone()
        .oneshot(
            HttpRequest::builder()
                .method(Method::POST)
                .uri("/tasks")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(response.headers()[header::LOCATION], "/tasks/1");
    assert_eq!(body(response).await["data"], "created");
    let response = app
        .clone()
        .oneshot(
            HttpRequest::builder()
                .method(Method::DELETE)
                .uri("/tasks/1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(
        to_bytes(response.into_body(), 1024)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        Router::<()>::new()
            .get("/a/{id}", || async { NoContent })
            .unwrap()
            .get("/a/{name}", || async { NoContent })
            .is_err()
    );
}

#[tokio::test]
async fn validation_precedes_controller_and_errors_are_safe() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let app = Router::<()>::new()
        .post("/tasks", move |input: Validated<Input>| {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Json(input.title.clone())
            }
        })
        .unwrap()
        .with_state(());
    for (payload, expected) in [
        (r#"{"title":""}"#, StatusCode::UNPROCESSABLE_ENTITY),
        (r#"{"title":3}"#, StatusCode::BAD_REQUEST),
        (r#"{"title":"ok","admin":true}"#, StatusCode::BAD_REQUEST),
    ] {
        let response = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/tasks")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(payload))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let response = app
        .clone()
        .oneshot(
            HttpRequest::builder()
                .method(Method::POST)
                .uri("/tasks")
                .header(header::CONTENT_TYPE, "text/plain")
                .body(Body::from("hi"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let nested = format!("{}0{}", "[".repeat(33), "]".repeat(33));
    let response = app
        .clone()
        .oneshot(
            HttpRequest::builder()
                .method(Method::POST)
                .uri("/tasks")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(nested))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(body(response).await["error"]["code"], "too_deep");
    let response = app
        .oneshot(
            HttpRequest::builder()
                .method(Method::POST)
                .uri("/tasks")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"title":"ok"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(body(response).await["data"], "ok");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let error = Error(
        CoreError::new(
            ErrorKind::Internal,
            "internal_error",
            "Internal server error",
        )
        .with_source(std::io::Error::other("secret sql")),
    );
    let rendered = body(axum::response::IntoResponse::into_response(error))
        .await
        .to_string();
    assert!(!rendered.contains("secret sql"));
    let list = body(axum::response::IntoResponse::into_response(Page::<i32> {
        data: vec![],
        page: 1,
        per_page: 20,
        has_next: false,
    }))
    .await;
    assert_eq!(list["meta"]["has_next"], false);
}

#[tokio::test]
async fn context_is_checked_before_json_decode() {
    let app = Router::<()>::new()
        .post("/private", |input: Validated<AuthInput>| async move {
            Json(input.title.clone())
        })
        .unwrap()
        .with_state(());
    let request = HttpRequest::builder()
        .method(Method::POST)
        .uri("/private")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("invalid json"))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
