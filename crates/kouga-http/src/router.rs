use crate::{
    Endpoint, Error, HttpOptions, HttpRequest, IntoMiddleware, Middleware, Next, Operation,
};
use axum::middleware::from_fn;
use axum::response::IntoResponse;
use axum::routing::MethodRouter;
use http::{Method, StatusCode, header};
use kouga_core::{Error as CoreError, ErrorKind};
use std::collections::BTreeMap;
use std::sync::Arc;

struct RouteEntry<S> {
    route: MethodRouter<S>,
    operation: Operation,
    middlewares: Vec<Middleware<S>>,
}

/// Register routes without constructing application state.
pub struct Router<S> {
    routes: BTreeMap<String, Vec<RouteEntry<S>>>,
    middlewares: Vec<Middleware<S>>,
    options: HttpOptions,
}

pub struct Group<S> {
    router: Router<S>,
    prefix: String,
    middlewares: Vec<Middleware<S>>,
}

impl<S> Default for Router<S> {
    fn default() -> Self {
        Self {
            routes: BTreeMap::new(),
            middlewares: Vec::new(),
            options: HttpOptions::default(),
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

    pub fn configure(mut self, options: HttpOptions) -> Result<Self, Error> {
        options.validate()?;
        self.options = options;
        Ok(self)
    }

    pub fn middleware<M: IntoMiddleware<S>>(mut self, middleware: M) -> Self {
        let middleware = middleware.into_middleware();
        if let Some(name) = middleware.security {
            for entries in self.routes.values_mut() {
                for entry in entries {
                    entry.operation.security.push(name.to_owned());
                }
            }
        }
        self.middlewares.push(middleware);
        self
    }

    pub fn group(self, prefix: &str) -> Result<Group<S>, Error> {
        if route_parameters(prefix).is_none() || (prefix != "/" && prefix.ends_with('/')) {
            return Err(Error(CoreError::new(
                ErrorKind::Internal,
                "invalid_route",
                "Invalid group prefix",
            )));
        }
        Ok(Group {
            router: self,
            prefix: prefix.trim_end_matches('/').to_owned(),
            middlewares: Vec::new(),
        })
    }

    fn add(mut self, path: &str, method: Method, endpoint: Endpoint<S>) -> Result<Self, Error> {
        let Some(parameters) = route_parameters(path) else {
            return Err(Error(CoreError::new(
                ErrorKind::Internal,
                "invalid_route",
                "Invalid route path",
            )));
        };
        if !path_schema_matches(&parameters, endpoint.operation.path_schema.as_ref()) {
            return Err(Error(CoreError::new(
                ErrorKind::Internal,
                "invalid_route",
                "Route parameters do not match Path input",
            )));
        }
        let canonical = |path: &str| {
            path.split('/')
                .map(|part| if part.starts_with('{') { "{}" } else { part })
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
                .flat_map(|entries| entries.iter())
                .any(|entry| entry.operation.operation_id == endpoint.operation.operation_id)
        {
            return Err(Error(CoreError::new(
                ErrorKind::Internal,
                "route_conflict",
                "Duplicate or missing operation ID",
            )));
        }
        let (route, mut operation, middlewares) = endpoint.into_route(method.clone());
        for middleware in &self.middlewares {
            if let Some(name) = middleware.security {
                operation.security.push(name.to_owned());
            }
        }
        let entries = self.routes.entry(path.to_owned()).or_default();
        if entries.iter().any(|entry| entry.operation.method == method) {
            return Err(Error(CoreError::new(
                ErrorKind::Internal,
                "route_conflict",
                "Duplicate route",
            )));
        }
        entries.push(RouteEntry {
            route,
            operation: operation.at(method, path),
            middlewares,
        });
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
            .flat_map(|(_, entries)| entries.iter().map(|entry| &entry.operation))
            .collect()
    }

    pub fn with_state(self, state: S) -> axum::Router {
        let mut router = axum::Router::new();
        for (path, entries) in self.routes {
            let mut allowed: Vec<String> = entries
                .iter()
                .map(|entry| entry.operation.method.as_str().to_owned())
                .collect();
            if entries
                .iter()
                .any(|entry| entry.operation.method == Method::GET)
            {
                allowed.push("HEAD".to_owned());
            }
            let mut route = MethodRouter::new();
            for entry in entries {
                let route_entry = if entry.middlewares.is_empty() {
                    entry.route
                } else {
                    let chain = Arc::new(entry.middlewares);
                    let state = state.clone();
                    entry.route.layer(from_fn(move |request, next| {
                        let chain = chain.clone();
                        let state = state.clone();
                        async move {
                            let request = HttpRequest::new(request, state.clone());
                            Next::new(chain, next, state)
                                .run(request)
                                .await
                                .unwrap_or_else(axum::response::IntoResponse::into_response)
                        }
                    }))
                };
                route = route.merge(route_entry);
            }
            allowed.push("OPTIONS".to_owned());
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
        let app = router
            .fallback(|| async {
                Error(CoreError::new(
                    ErrorKind::NotFound,
                    "not_found",
                    "Not found",
                ))
            })
            .with_state(state.clone());
        crate::http_stack::apply(app, state, self.middlewares, self.options)
    }
}

impl<S: Clone + Send + Sync + 'static> Group<S> {
    pub fn middleware<M: IntoMiddleware<S>>(mut self, middleware: M) -> Self {
        self.middlewares.push(middleware.into_middleware());
        self
    }

    fn add(mut self, path: &str, method: Method, endpoint: Endpoint<S>) -> Result<Self, Error> {
        if !path.starts_with('/') {
            return Err(Error(CoreError::new(
                ErrorKind::Internal,
                "invalid_route",
                "Invalid group route",
            )));
        }
        let full_path = format!("{}{}", self.prefix, path);
        self.router = self.router.add(
            &full_path,
            method,
            endpoint.prepend_middlewares(self.middlewares.clone()),
        )?;
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
    pub fn finish(self) -> Router<S> {
        self.router
    }
}

fn route_parameters(path: &str) -> Option<Vec<&str>> {
    if !path.starts_with('/') {
        return None;
    }
    let mut names = Vec::new();
    for segment in path.split('/').skip(1) {
        if let Some(name) = segment.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                || names.contains(&name)
            {
                return None;
            }
            names.push(name);
        } else if segment.starts_with([':', '*']) || segment.contains(['{', '}']) {
            return None;
        }
    }
    Some(names)
}

fn path_schema_matches(parameters: &[&str], schema: Option<&serde_json::Value>) -> bool {
    match (parameters.len(), schema) {
        (0, None) => true,
        (0, Some(_)) | (_, None) => false,
        (count, Some(schema)) if schema["type"] == "object" => {
            schema["properties"].as_object().is_some_and(|properties| {
                properties.len() == count
                    && parameters.iter().all(|name| properties.contains_key(*name))
            })
        }
        (count, Some(schema)) if schema["type"] == "array" => schema["prefixItems"]
            .as_array()
            .is_some_and(|items| items.len() == count),
        (1, Some(_)) => true,
        _ => false,
    }
}
