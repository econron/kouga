//! HTTP-only adapter; the validation core has no HTTP dependency.
use crate::{MAX_NESTING_DEPTH, Request, Validated, validate};
use axum::extract::{FromRequest, FromRequestParts, Json, Query};
use axum::http::{Request as HttpRequest, StatusCode, request::Parts};
use axum::response::{IntoResponse, Response};
use kouga_core::{Error, ErrorKind};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::future::Future;
use std::ops::Deref;

pub trait ContextFromRequest<S>: Sized {
    fn from_request(
        parts: &mut Parts,
        state: &S,
    ) -> impl Future<Output = Result<Self, Error>> + Send;
}

impl<S: Sync> ContextFromRequest<S> for () {
    async fn from_request(_: &mut Parts, _: &S) -> Result<Self, Error> {
        Ok(())
    }
}

fn safe_error(error: Error) -> Response {
    let status = match error.kind {
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
        Json(serde_json::json!({"error": {
            "code": error.code, "message": error.message, "details": &error.details
        }})),
    )
        .into_response();
    response.extensions_mut().insert(std::sync::Arc::new(error));
    response
}

fn too_deep(value: &Value, depth: usize) -> bool {
    if depth > MAX_NESTING_DEPTH {
        return true;
    }
    match value {
        Value::Array(values) => values.iter().any(|v| too_deep(v, depth + 1)),
        Value::Object(values) => values.values().any(|v| too_deep(v, depth + 1)),
        _ => false,
    }
}

/// Query input that runs the same Request validation as JSON input.
pub struct ValidatedQuery<T>(Validated<T>);

impl<T> ValidatedQuery<T> {
    pub fn into_inner(self) -> T {
        self.0.into_inner()
    }
}

impl<T> Deref for ValidatedQuery<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<S, T> FromRequestParts<S> for ValidatedQuery<T>
where
    S: Send + Sync,
    T: Request + DeserializeOwned,
    T::Context: ContextFromRequest<S>,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let context = T::Context::from_request(parts, state)
            .await
            .map_err(safe_error)?;
        let Query(value) = Query::<T>::from_request_parts(parts, state)
            .await
            .map_err(|_| {
                safe_error(Error::new(
                    ErrorKind::BadRequest,
                    "invalid_query",
                    "Invalid query parameters",
                ))
            })?;
        validate(value, &context)
            .await
            .map(Self)
            .map_err(safe_error)
    }
}

impl<S, T> FromRequest<S> for Validated<T>
where
    S: Send + Sync,
    T: Request + DeserializeOwned,
    T::Context: ContextFromRequest<S>,
{
    type Rejection = Response;

    async fn from_request(
        request: HttpRequest<axum::body::Body>,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let (mut parts, body) = request.into_parts();
        let context = T::Context::from_request(&mut parts, state)
            .await
            .map_err(safe_error)?;
        let request = HttpRequest::from_parts(parts, body);
        let Json(value) =
            Json::<Value>::from_request(request, state)
                .await
                .map_err(|rejection| {
                    let kind = if rejection.status() == StatusCode::UNSUPPORTED_MEDIA_TYPE {
                        ErrorKind::UnsupportedMediaType
                    } else if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
                        ErrorKind::TooLarge
                    } else {
                        ErrorKind::BadRequest
                    };
                    safe_error(Error::new(kind, "invalid_json", "Invalid JSON request"))
                })?;
        if too_deep(&value, 1) {
            return Err(safe_error(Error::new(
                ErrorKind::BadRequest,
                "too_deep",
                "JSON nesting limit exceeded",
            )));
        }
        let value = serde_json::from_value(value).map_err(|_| {
            safe_error(Error::new(
                ErrorKind::BadRequest,
                "invalid_json",
                "Invalid JSON request",
            ))
        })?;
        validate(value, &context).await.map_err(safe_error)
    }
}
