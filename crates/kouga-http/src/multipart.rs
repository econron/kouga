use axum::extract::{FromRequest, multipart::Field};
use axum::http::{Request, header};
use axum::response::{IntoResponse, Response};
use kouga_core::{Error as CoreError, ErrorKind};
use kouga_validation::ApiSchema;
use std::marker::PhantomData;

/// Streaming multipart input. `T` supplies the OpenAPI input schema; handlers
/// consume fields and apply file-specific limits/validation before storing them.
pub struct Multipart<T> {
    inner: axum::extract::Multipart,
    schema: PhantomData<fn() -> T>,
}

impl<T> Multipart<T> {
    pub async fn next_field(
        &mut self,
    ) -> Result<Option<Field<'_>>, axum::extract::multipart::MultipartError> {
        self.inner.next_field().await
    }
}

impl<S, T> FromRequest<S> for Multipart<T>
where
    S: Send + Sync,
    T: ApiSchema,
{
    type Rejection = Response;

    async fn from_request(
        request: Request<axum::body::Body>,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let is_multipart = request
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .is_some_and(|media_type| {
                media_type
                    .trim()
                    .eq_ignore_ascii_case("multipart/form-data")
            });
        if !is_multipart {
            return Err(crate::Error(CoreError::new(
                ErrorKind::UnsupportedMediaType,
                "invalid_multipart",
                "Expected multipart/form-data",
            ))
            .into_response());
        }
        let inner = axum::extract::Multipart::from_request(request, state)
            .await
            .map_err(|_| {
                crate::Error(CoreError::new(
                    ErrorKind::BadRequest,
                    "invalid_multipart",
                    "Invalid multipart request",
                ))
                .into_response()
            })?;
        Ok(Self {
            inner,
            schema: PhantomData,
        })
    }
}
