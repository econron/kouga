//! Kouga's HTTP entry point. Transport details stay in axum.
use axum::Json as AxumJson;
use axum::response::{IntoResponse, Response};
use http::{HeaderValue, StatusCode, header};
use kouga_core::{Error as CoreError, ErrorKind};
use serde::Serialize;

pub use axum::extract::{Extension, Path, State};
pub use kouga_http_derive::endpoint;
pub use kouga_validation::Validated;
pub use kouga_validation::axum::ValidatedQuery;
pub mod auth;
mod http_stack;
pub mod middleware;
pub use middleware::{
    ClientIp, HttpOptions, HttpRequest, IntoMiddleware, Middleware, Next, bearer_auth,
};
mod endpoint;
mod multipart;
pub use endpoint::{ApiInput, ApiOutput, Endpoint, Operation, Parameter, ResponseMeta};
pub use multipart::Multipart;
pub mod router;
pub use router::{Group, Router};

#[derive(Debug)]
pub struct Error(pub CoreError);

impl From<CoreError> for Error {
    fn from(value: CoreError) -> Self {
        Self(value)
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = match self.0.kind {
            ErrorKind::BadRequest => StatusCode::BAD_REQUEST,
            ErrorKind::Unauthorized => StatusCode::UNAUTHORIZED,
            ErrorKind::Forbidden => StatusCode::FORBIDDEN,
            ErrorKind::NotFound => StatusCode::NOT_FOUND,
            ErrorKind::Conflict => StatusCode::CONFLICT,
            ErrorKind::Validation => StatusCode::UNPROCESSABLE_ENTITY,
            ErrorKind::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            ErrorKind::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            ErrorKind::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            ErrorKind::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            ErrorKind::Timeout => StatusCode::GATEWAY_TIMEOUT,
            ErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let mut response = (
            status,
            AxumJson(serde_json::json!({"error": {
                "code": self.0.code, "message": self.0.message,
                "details": &self.0.details,
            }})),
        )
            .into_response();
        if status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        response
            .extensions_mut()
            .insert(std::sync::Arc::new(self.0));
        response
    }
}

pub struct Json<T>(pub T);
impl<T: Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        (
            StatusCode::OK,
            AxumJson(serde_json::json!({"data": self.0})),
        )
            .into_response()
    }
}

pub struct Created<T> {
    location: String,
    value: T,
}
impl<T> Created<T> {
    pub fn new(location: impl Into<String>, value: T) -> Self {
        Self {
            location: location.into(),
            value,
        }
    }
}
impl<T: Serialize> IntoResponse for Created<T> {
    fn into_response(self) -> Response {
        match HeaderValue::from_str(&self.location) {
            Ok(location) => {
                let mut response = (
                    StatusCode::CREATED,
                    AxumJson(serde_json::json!({"data": self.value})),
                )
                    .into_response();
                response.headers_mut().insert(header::LOCATION, location);
                response
            }
            Err(error) => Error(
                CoreError::new(
                    ErrorKind::Internal,
                    "internal_error",
                    "Internal server error",
                )
                .with_source(error),
            )
            .into_response(),
        }
    }
}

pub struct Page<T> {
    pub data: Vec<T>,
    pub page: usize,
    pub per_page: usize,
    pub has_next: bool,
}
impl<T: Serialize> IntoResponse for Page<T> {
    fn into_response(self) -> Response {
        (
            StatusCode::OK,
            AxumJson(serde_json::json!({"data": self.data, "meta": {
                "page": self.page, "per_page": self.per_page, "has_next": self.has_next
            }})),
        )
            .into_response()
    }
}

pub struct NoContent;
impl IntoResponse for NoContent {
    fn into_response(self) -> Response {
        StatusCode::NO_CONTENT.into_response()
    }
}
