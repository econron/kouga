//! Thin gRPC adapter: application services own their generated protobuf types and handlers.
//! Build `.proto` files with `tonic-prost-build` and implement the generated tonic service trait.

use kouga_auth::{CurrentUser, authenticate};
use kouga_core::{Error, ErrorKind};
use kouga_db::Db;
use kouga_validation::{Request, Validated, validate};
use std::{error::Error as StdError, future::Future, sync::Arc, time::Duration};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tonic::{Code, Status, metadata::MetadataMap};
use tracing::Instrument;

/// Wrap a generated handler after authenticating the transport peer. By default callers
/// should pass `false`; forwarding headers never establish peer trust.
pub async fn trace_request<T>(
    metadata: &MetadataMap,
    trusted_peer: bool,
    work: impl Future<Output = T>,
) -> T {
    let span = tracing::info_span!("kouga.grpc.request");
    #[cfg(feature = "otel")]
    if trusted_peer
        && metadata.get_all("traceparent").iter().count() == 1
        && metadata.get_all("tracestate").iter().count() <= 1
    {
        kouga_telemetry::propagation::extract(
            metadata
                .get("traceparent")
                .and_then(|value| value.to_str().ok()),
            metadata
                .get("tracestate")
                .and_then(|value| value.to_str().ok()),
            &span,
        );
    }
    #[cfg(not(feature = "otel"))]
    let _ = (metadata, trusted_peer);
    work.instrument(span).await
}

/// Apply with `Server::builder().layer(tower::util::MapResponseLayer::new(...))`.
/// Tonic 0.14 encodes an oversized incoming message as OUT_OF_RANGE before a handler runs;
/// Kouga's public contract uses RESOURCE_EXHAUSTED. Other OUT_OF_RANGE errors are untouched.
pub fn normalize_message_size_status<B>(mut response: http::Response<B>) -> http::Response<B> {
    let headers = response.headers();
    let oversized = headers
        .get(Status::GRPC_STATUS)
        .is_some_and(|value| value == "11")
        && headers.get(Status::GRPC_MESSAGE).is_some_and(|value| {
            value
                .as_bytes()
                .starts_with(b"Error,%20decoded%20message%20length%20too%20large:")
        });
    if oversized {
        response
            .headers_mut()
            .insert(Status::GRPC_STATUS, http::HeaderValue::from_static("8"));
        response.headers_mut().insert(
            Status::GRPC_MESSAGE,
            http::HeaderValue::from_static("Message%20too%20large"),
        );
    }
    response
}

/// Normalize only tonic's client-local timeout; a server cannot change an error returned
/// before its response is received. Apply at the generated client boundary in T29.
pub fn normalize_client_timeout(status: Status) -> Status {
    if status.code() == Code::Cancelled {
        let mut source = StdError::source(&status);
        while let Some(error) = source {
            if error.is::<tonic::TimeoutExpired>() {
                return Status::deadline_exceeded("Deadline exceeded");
            }
            source = error.source();
        }
    }
    status
}

/// Bound handler work and return DEADLINE_EXCEEDED; dropping the future cancels in-flight work.
pub async fn within_deadline<T>(
    limit: Duration,
    work: impl Future<Output = Result<T, Status>>,
) -> Result<T, Status> {
    tokio::time::timeout(limit, work)
        .await
        .map_err(|_| Status::deadline_exceeded("Deadline exceeded"))?
}

/// Maps transport-independent errors without exposing private sources or database constraint names.
pub fn to_status(error: Error) -> Status {
    let code = match error.kind {
        ErrorKind::BadRequest | ErrorKind::Validation => Code::InvalidArgument,
        ErrorKind::Unauthorized => Code::Unauthenticated,
        ErrorKind::Forbidden => Code::PermissionDenied,
        ErrorKind::NotFound => Code::NotFound,
        ErrorKind::Conflict => Code::FailedPrecondition,
        ErrorKind::TooLarge | ErrorKind::RateLimited => Code::ResourceExhausted,
        ErrorKind::Unavailable => Code::Unavailable,
        ErrorKind::Timeout => Code::DeadlineExceeded,
        ErrorKind::UnsupportedMediaType | ErrorKind::Internal => Code::Internal,
    };
    Status::new(code, error.message)
}

/// Perform async token lookup in a handler, never a synchronous tonic interceptor.
#[tracing::instrument(skip_all, name = "grpc.authenticate")]
pub async fn require_bearer(metadata: &MetadataMap, db: &Db) -> Result<CurrentUser, Status> {
    let mut values = metadata.get_all("authorization").iter();
    let value = values.next();
    if values.next().is_some() {
        return Err(Status::unauthenticated("Authentication required"));
    }
    let token = value
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty() && !value.bytes().any(|byte| byte.is_ascii_whitespace()))
        .ok_or_else(|| Status::unauthenticated("Authentication required"))?;
    authenticate(db, token)
        .await
        .map_err(|error| to_status(error.into_core()))?
        .ok_or_else(|| Status::unauthenticated("Authentication required"))
}

/// Share the same request validation rules with HTTP and workers.
#[tracing::instrument(skip_all, name = "grpc.validate")]
pub async fn validate_input<T: Request>(
    input: T,
    context: &T::Context,
) -> Result<Validated<T>, Status> {
    validate(input, context).await.map_err(to_status)
}

/// Global in-flight RPC cap; acquire once at the start of each generated handler.
#[derive(Clone)]
pub struct InFlight(Arc<Semaphore>);

impl InFlight {
    pub fn new(max: usize) -> Self {
        Self(Arc::new(Semaphore::new(max)))
    }

    pub fn try_acquire(&self) -> Result<OwnedSemaphorePermit, Status> {
        self.0
            .clone()
            .try_acquire_owned()
            .map_err(|_| Status::resource_exhausted("Server busy"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kouga_core::ErrorKind;

    #[test]
    fn maps_safe_errors_and_sheds_load() {
        assert_eq!(
            to_status(Error::new(
                ErrorKind::Conflict,
                "internal_constraint",
                "Conflict"
            ))
            .code(),
            Code::FailedPrecondition
        );
        let cap = InFlight::new(1);
        let permit = cap.try_acquire().unwrap();
        assert_eq!(
            cap.try_acquire().unwrap_err().code(),
            Code::ResourceExhausted
        );
        drop(permit);
        assert!(cap.try_acquire().is_ok());
        let unrelated =
            normalize_message_size_status(Status::out_of_range("index").into_http::<()>());
        assert_eq!(unrelated.headers()[Status::GRPC_STATUS], "11");
        assert_eq!(
            normalize_client_timeout(Status::cancelled("cancelled")).code(),
            Code::Cancelled
        );
    }
}
