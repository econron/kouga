//! HTTP adapter for the transport-independent `kouga_auth` token store.

use crate::{Error, HttpRequest, Middleware, Next, bearer_auth};
use axum::http::header;
use kouga_auth::authenticate;
use kouga_core::{Error as CoreError, ErrorKind};
use kouga_db::Db;

/// Registers bearer security metadata and checks the token before request validation.
/// The application supplies its database through a state accessor.
pub fn require_bearer<S, F>(database: F) -> Middleware<S>
where
    S: Clone + Send + Sync + 'static,
    F: Fn(&S) -> &Db + Send + Sync + 'static,
{
    bearer_auth(move |mut request: HttpRequest<S>, next: Next<S>| {
        let token = request
            .headers()
            .get_all(header::AUTHORIZATION)
            .iter()
            .map(|value| value.to_str().ok())
            .collect::<Vec<_>>();
        let token = match token.as_slice() {
            [Some(value)] => value.strip_prefix("Bearer "),
            _ => None,
        }
        .filter(|value| !value.is_empty() && !value.contains(char::is_whitespace))
        .map(str::to_owned);
        let db = database(request.state()).clone();
        async move {
            let Some(token) = token else {
                return Err(unauthorized());
            };
            let actor = authenticate(&db, &token)
                .await
                .map_err(|error| {
                    Error(
                        CoreError::new(
                            ErrorKind::Unavailable,
                            "auth_unavailable",
                            "Authentication unavailable",
                        )
                        .with_source(error),
                    )
                })?
                .ok_or_else(unauthorized)?;
            request.extensions_mut().insert(actor);
            next.run(request).await
        }
    })
}

fn unauthorized() -> Error {
    Error(CoreError::new(
        ErrorKind::Unauthorized,
        "unauthorized",
        "Unauthorized",
    ))
}
