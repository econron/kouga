use crate::Error;
use axum::body::Body;
use axum::http::{HeaderMap, Request, request::Parts};
use axum::middleware::Next as AxumNext;
use axum::response::Response;
use kouga_core::{Error as CoreError, ErrorKind};
use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

pub type MiddlewareFuture = Pin<Box<dyn Future<Output = Result<Response, Error>> + Send>>;
type Handler<S> = dyn Fn(HttpRequest<S>, Next<S>) -> MiddlewareFuture + Send + Sync;

/// Actual peer or a forwarded client address accepted from a configured trusted peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientIp(pub IpAddr);

#[derive(Debug, Clone)]
pub struct HttpOptions {
    pub max_body_bytes: usize,
    pub max_in_flight: usize,
    pub timeout: Duration,
    pub cors_origins: Vec<String>,
    pub cors_credentials: bool,
    pub trusted_proxies: Vec<IpAddr>,
    /// Only these direct TCP peers may supply a distributed trace parent.
    pub trusted_trace_peers: Vec<IpAddr>,
}

impl Default for HttpOptions {
    fn default() -> Self {
        Self {
            max_body_bytes: 1_048_576,
            max_in_flight: 256,
            timeout: Duration::from_secs(30),
            cors_origins: Vec::new(),
            cors_credentials: false,
            trusted_proxies: Vec::new(),
            trusted_trace_peers: Vec::new(),
        }
    }
}

impl HttpOptions {
    pub fn validate(&self) -> Result<(), Error> {
        if self.max_body_bytes == 0
            || self.max_in_flight == 0
            || self.timeout.is_zero()
            || (self.cors_credentials && self.cors_origins.iter().any(|origin| origin == "*"))
            || self.cors_origins.iter().any(|origin| {
                if origin == "*" {
                    return false;
                }
                let Ok(uri) = origin.parse::<http::Uri>() else {
                    return true;
                };
                !matches!(uri.scheme_str(), Some("http" | "https"))
                    || uri.authority().is_none()
                    || uri.path() != "/"
                    || uri.query().is_some()
                    || http::HeaderValue::from_str(origin).is_err()
            })
        {
            return Err(Error(CoreError::new(
                ErrorKind::Internal,
                "invalid_http_options",
                "Invalid HTTP options",
            )));
        }
        Ok(())
    }
}

/// HTTP request and read-only application state; the body stays streaming.
pub struct HttpRequest<S> {
    request: Request<Body>,
    state: S,
}

impl<S> HttpRequest<S> {
    pub(crate) fn new(request: Request<Body>, state: S) -> Self {
        Self { request, state }
    }
    pub fn state(&self) -> &S {
        &self.state
    }
    pub fn headers(&self) -> &HeaderMap {
        self.request.headers()
    }
    pub fn headers_mut(&mut self) -> &mut HeaderMap {
        self.request.headers_mut()
    }
    pub fn extensions(&self) -> &http::Extensions {
        self.request.extensions()
    }
    pub fn extensions_mut(&mut self) -> &mut http::Extensions {
        self.request.extensions_mut()
    }
    pub fn body_mut(&mut self) -> &mut Body {
        self.request.body_mut()
    }
    pub fn into_parts(self) -> (S, Parts, Body) {
        let (parts, body) = self.request.into_parts();
        (self.state, parts, body)
    }
    pub fn from_parts(state: S, parts: Parts, body: Body) -> Self {
        Self {
            request: Request::from_parts(parts, body),
            state,
        }
    }
    pub(crate) fn into_request(self) -> Request<Body> {
        self.request
    }
}

pub struct Middleware<S> {
    handler: Arc<Handler<S>>,
    pub(crate) security: Option<&'static str>,
}

impl<S> Clone for Middleware<S> {
    fn clone(&self) -> Self {
        Self {
            handler: self.handler.clone(),
            security: self.security,
        }
    }
}

impl<S> Middleware<S> {
    pub fn new<F, Fut>(function: F) -> Self
    where
        F: Fn(HttpRequest<S>, Next<S>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Response, Error>> + Send + 'static,
    {
        Self {
            handler: Arc::new(move |request, next| Box::pin(function(request, next))),
            security: None,
        }
    }
}

pub trait IntoMiddleware<S> {
    fn into_middleware(self) -> Middleware<S>;
}

impl<S, F, Fut> IntoMiddleware<S> for F
where
    F: Fn(HttpRequest<S>, Next<S>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Response, Error>> + Send + 'static,
{
    fn into_middleware(self) -> Middleware<S> {
        Middleware::new(self)
    }
}

impl<S> IntoMiddleware<S> for Middleware<S> {
    fn into_middleware(self) -> Middleware<S> {
        self
    }
}

pub fn bearer_auth<S, F, Fut>(function: F) -> Middleware<S>
where
    F: Fn(HttpRequest<S>, Next<S>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Response, Error>> + Send + 'static,
{
    let mut middleware = Middleware::new(function);
    middleware.security = Some("bearerAuth");
    middleware
}

/// Consumed when the downstream handler is run; the same continuation cannot be run twice.
pub struct Next<S> {
    chain: Arc<Vec<Middleware<S>>>,
    index: usize,
    next: AxumNext,
    state: S,
}

impl<S: Clone + Send + Sync + 'static> Next<S> {
    pub(crate) fn new(chain: Arc<Vec<Middleware<S>>>, next: AxumNext, state: S) -> Self {
        Self {
            chain,
            index: 0,
            next,
            state,
        }
    }

    pub async fn run(self, request: HttpRequest<S>) -> Result<Response, Error> {
        if let Some(middleware) = self.chain.get(self.index) {
            let middleware = middleware.clone();
            let next = Self {
                chain: self.chain,
                index: self.index + 1,
                next: self.next,
                state: self.state,
            };
            (middleware.handler)(request, next).await
        } else {
            Ok(self.next.run(request.into_request()).await)
        }
    }
}
