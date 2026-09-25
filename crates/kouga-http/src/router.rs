use crate::Error;
use axum::handler::Handler;
use axum::response::IntoResponse;
use axum::routing::{MethodRouter, delete, get, patch, post, put};
use http::{Method, StatusCode, header};
use kouga_core::{Error as CoreError, ErrorKind};
use std::collections::BTreeMap;

/// Register routes without constructing application state.
pub struct Router<S> {
    routes: BTreeMap<String, (MethodRouter<S>, Vec<Method>)>,
}

impl<S> Default for Router<S> {
    fn default() -> Self {
        Self {
            routes: BTreeMap::new(),
        }
    }
}

impl<S> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    pub fn new() -> Self {
        Self::default()
    }

    fn add(mut self, path: &str, method: Method, route: MethodRouter<S>) -> Result<Self, Error> {
        if !path.starts_with('/') {
            return Err(Error(CoreError::new(
                ErrorKind::Internal,
                "invalid_route",
                "Route must begin with /",
            )));
        }
        let canonical = |path: &str| {
            path.split('/')
                .map(|part| {
                    if part.starts_with('{') || part.starts_with(':') {
                        "{}"
                    } else {
                        part
                    }
                })
                .collect::<Vec<_>>()
                .join("/")
        };
        if self
            .routes
            .keys()
            .any(|existing| existing != path && canonical(existing) == canonical(path))
        {
            return Err(Error(CoreError::new(
                ErrorKind::Internal,
                "route_conflict",
                "Ambiguous route",
            )));
        }
        let entry = self
            .routes
            .entry(path.to_owned())
            .or_insert_with(|| (MethodRouter::new(), Vec::new()));
        if entry.1.contains(&method) {
            return Err(Error(CoreError::new(
                ErrorKind::Internal,
                "route_conflict",
                "Duplicate route",
            )));
        }
        entry.0 = entry.0.clone().merge(route);
        entry.1.push(method);
        Ok(self)
    }

    pub fn get<H, T>(self, path: &str, handler: H) -> Result<Self, Error>
    where
        H: Handler<T, S>,
        T: 'static,
    {
        self.add(path, Method::GET, get(handler))
    }
    pub fn post<H, T>(self, path: &str, handler: H) -> Result<Self, Error>
    where
        H: Handler<T, S>,
        T: 'static,
    {
        self.add(path, Method::POST, post(handler))
    }
    pub fn put<H, T>(self, path: &str, handler: H) -> Result<Self, Error>
    where
        H: Handler<T, S>,
        T: 'static,
    {
        self.add(path, Method::PUT, put(handler))
    }
    pub fn patch<H, T>(self, path: &str, handler: H) -> Result<Self, Error>
    where
        H: Handler<T, S>,
        T: 'static,
    {
        self.add(path, Method::PATCH, patch(handler))
    }
    pub fn delete<H, T>(self, path: &str, handler: H) -> Result<Self, Error>
    where
        H: Handler<T, S>,
        T: 'static,
    {
        self.add(path, Method::DELETE, delete(handler))
    }

    pub fn routes(&self) -> Vec<(Method, &str)> {
        self.routes
            .iter()
            .flat_map(|(path, (_, methods))| {
                methods
                    .iter()
                    .cloned()
                    .map(move |method| (method, path.as_str()))
            })
            .collect()
    }

    pub fn with_state(self, state: S) -> axum::Router {
        let mut router = axum::Router::new();
        for (path, (route, methods)) in self.routes {
            let mut allowed: Vec<&str> = methods.iter().map(Method::as_str).collect();
            if methods.contains(&Method::GET) {
                allowed.push("HEAD");
            }
            allowed.push("OPTIONS");
            allowed.sort_unstable();
            let allow = allowed.join(", ");
            let options_allow = allow.clone();
            let route = route.options(move || async move {
                ([(header::ALLOW, options_allow)], StatusCode::NO_CONTENT)
            });
            let route = route.fallback(move || async move {
                (
                    StatusCode::METHOD_NOT_ALLOWED,
                    [(header::ALLOW, allow)],
                    Error(CoreError::new(
                        ErrorKind::NotFound,
                        "method_not_allowed",
                        "Method not allowed",
                    )),
                )
                    .into_response()
            });
            router = router.route(&path, route);
        }
        router
            .fallback(|| async {
                Error(CoreError::new(
                    ErrorKind::NotFound,
                    "not_found",
                    "Not found",
                ))
            })
            .with_state(state)
    }
}
