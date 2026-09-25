use crate::{Endpoint, Error, Operation};
use axum::response::IntoResponse;
use axum::routing::MethodRouter;
use http::{Method, StatusCode, header};
use kouga_core::{Error as CoreError, ErrorKind};
use std::collections::BTreeMap;

/// Register routes without constructing application state.
pub struct Router<S> {
    routes: BTreeMap<String, (MethodRouter<S>, Vec<Operation>)>,
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

    fn add(mut self, path: &str, method: Method, endpoint: Endpoint<S>) -> Result<Self, Error> {
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
        if endpoint.operation.operation_id.is_empty()
            || endpoint.operation.responses.is_empty()
            || self
                .routes
                .values()
                .flat_map(|(_, operations)| operations)
                .any(|operation| operation.operation_id == endpoint.operation.operation_id)
        {
            return Err(Error(CoreError::new(
                ErrorKind::Internal,
                "route_conflict",
                "Duplicate or missing operation ID",
            )));
        }
        let (route, operation) = endpoint.into_route(method.clone());
        let entry = self
            .routes
            .entry(path.to_owned())
            .or_insert_with(|| (MethodRouter::new(), Vec::new()));
        if entry.1.iter().any(|operation| operation.method == method) {
            return Err(Error(CoreError::new(
                ErrorKind::Internal,
                "route_conflict",
                "Duplicate route",
            )));
        }
        entry.0 = entry.0.clone().merge(route);
        entry.1.push(operation.at(method, path));
        Ok(self)
    }

    pub fn get(self, path: &str, endpoint: Endpoint<S>) -> Result<Self, Error> {
        self.add(path, Method::GET, endpoint)
    }
    pub fn post(self, path: &str, endpoint: Endpoint<S>) -> Result<Self, Error> {
        self.add(path, Method::POST, endpoint)
    }
    pub fn put(self, path: &str, endpoint: Endpoint<S>) -> Result<Self, Error> {
        self.add(path, Method::PUT, endpoint)
    }
    pub fn patch(self, path: &str, endpoint: Endpoint<S>) -> Result<Self, Error> {
        self.add(path, Method::PATCH, endpoint)
    }
    pub fn delete(self, path: &str, endpoint: Endpoint<S>) -> Result<Self, Error> {
        self.add(path, Method::DELETE, endpoint)
    }

    pub fn routes(&self) -> Vec<&Operation> {
        self.routes
            .iter()
            .flat_map(|(_, (_, operations))| operations.iter())
            .collect()
    }

    pub fn with_state(self, state: S) -> axum::Router {
        let mut router = axum::Router::new();
        for (path, (route, operations)) in self.routes {
            let mut allowed: Vec<&str> = operations
                .iter()
                .map(|operation| operation.method.as_str())
                .collect();
            if operations
                .iter()
                .any(|operation| operation.method == Method::GET)
            {
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
