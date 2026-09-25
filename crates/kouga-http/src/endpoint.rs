use crate::{
    Created, Extension, IntoMiddleware, Json, Middleware, Multipart, NoContent, Page, Path,
    Validated, ValidatedQuery,
};
use axum::handler::Handler;
use axum::routing::{MethodRouter, delete, get, patch, post, put};
use http::{Method, StatusCode};
use kouga_validation::{ApiSchema, SchemaDirection};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Parameter {
    pub name: String,
    pub location: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResponseMeta {
    pub status: u16,
    pub content_type: Option<&'static str>,
    pub data_schema: Option<serde_json::Value>,
    pub paginated: bool,
}

/// One registered handler and its OpenAPI input, built from the same endpoint declaration.
#[derive(Debug, Clone)]
pub struct Operation {
    pub method: Method,
    pub path: String,
    pub operation_id: &'static str,
    pub parameters: Vec<Parameter>,
    pub path_schema: Option<serde_json::Value>,
    pub query_schema: Option<serde_json::Value>,
    pub request_body: Option<serde_json::Value>,
    pub request_content_type: &'static str,
    pub responses: Vec<ResponseMeta>,
    pub security: Vec<String>,
    pub summary: Option<&'static str>,
    pub description: Option<&'static str>,
    pub tags: Vec<String>,
    pub deprecated: bool,
}

impl Operation {
    pub fn new(operation_id: &'static str) -> Self {
        Self {
            method: Method::GET,
            path: String::new(),
            operation_id,
            parameters: Vec::new(),
            path_schema: None,
            query_schema: None,
            request_body: None,
            request_content_type: "application/json",
            responses: Vec::new(),
            security: Vec::new(),
            summary: None,
            description: None,
            tags: Vec::new(),
            deprecated: false,
        }
    }

    pub fn json_input<T: ApiSchema>(mut self) -> Self {
        self.request_body = Some(input_schema::<T>());
        self.request_content_type = "application/json";
        self
    }

    /// Describe multipart input for low-level handlers. Prefer `Multipart<T>` in `#[endpoint]`.
    pub fn multipart_input<T: ApiSchema>(mut self) -> Self {
        self.request_body = Some(input_schema::<T>());
        self.request_content_type = "multipart/form-data";
        self
    }

    pub fn path_input<T: schemars::JsonSchema>(mut self) -> Self {
        self.path_schema = output_schema::<T>();
        self
    }

    pub fn query_input<T: ApiSchema>(mut self) -> Self {
        self.query_schema = Some(input_schema::<T>());
        self
    }

    pub fn response<T: ApiOutput>(mut self) -> Self {
        self.responses.push(T::metadata());
        self
    }

    pub fn input<T: ApiInput>(self) -> Self {
        T::describe(self)
    }

    pub(crate) fn at(mut self, method: Method, path: &str) -> Self {
        self.method = method;
        self.path = path.to_owned();
        for part in path.split('/') {
            if let Some(name) = part.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                self.parameters.push(Parameter {
                    name: name.to_owned(),
                    location: "path",
                });
            }
        }
        self
    }
}

pub trait ApiInput {
    fn describe(operation: Operation) -> Operation;
}

impl<T: kouga_validation::Request + ApiSchema> ApiInput for Validated<T> {
    fn describe(operation: Operation) -> Operation {
        operation.json_input::<T>()
    }
}
impl<T: ApiSchema> ApiInput for Multipart<T> {
    fn describe(operation: Operation) -> Operation {
        operation.multipart_input::<T>()
    }
}
impl<T: schemars::JsonSchema> ApiInput for Path<T> {
    fn describe(operation: Operation) -> Operation {
        operation.path_input::<T>()
    }
}
impl<T: kouga_validation::Request + ApiSchema> ApiInput for ValidatedQuery<T> {
    fn describe(operation: Operation) -> Operation {
        operation.query_input::<T>()
    }
}
impl<T: Clone + Send + Sync + 'static> ApiInput for Extension<T> {
    fn describe(operation: Operation) -> Operation {
        operation
    }
}

fn input_schema<T: ApiSchema>() -> serde_json::Value {
    let mut generator = schemars::SchemaGenerator::default();
    let schema = T::schema(&mut generator, SchemaDirection::Input);
    let mut value = serde_json::to_value(schema).expect("Input schema must serialize");
    if !generator.definitions().is_empty() {
        value["$defs"] =
            serde_json::to_value(generator.definitions()).expect("Definitions must serialize");
    }
    value
}

pub trait ApiOutput {
    fn metadata() -> ResponseMeta;
}

fn output_schema<T: schemars::JsonSchema>() -> Option<serde_json::Value> {
    Some(serde_json::to_value(schemars::schema_for!(T)).expect("Response schema must serialize"))
}

impl<T: Serialize + schemars::JsonSchema> ApiOutput for Json<T> {
    fn metadata() -> ResponseMeta {
        ResponseMeta {
            status: StatusCode::OK.as_u16(),
            content_type: Some("application/json"),
            data_schema: output_schema::<T>(),
            paginated: false,
        }
    }
}
impl<T: Serialize + schemars::JsonSchema> ApiOutput for Created<T> {
    fn metadata() -> ResponseMeta {
        ResponseMeta {
            status: StatusCode::CREATED.as_u16(),
            content_type: Some("application/json"),
            data_schema: output_schema::<T>(),
            paginated: false,
        }
    }
}
impl<T: Serialize + schemars::JsonSchema> ApiOutput for Page<T> {
    fn metadata() -> ResponseMeta {
        ResponseMeta {
            status: StatusCode::OK.as_u16(),
            content_type: Some("application/json"),
            data_schema: output_schema::<Vec<T>>(),
            paginated: true,
        }
    }
}
impl ApiOutput for NoContent {
    fn metadata() -> ResponseMeta {
        ResponseMeta {
            status: StatusCode::NO_CONTENT.as_u16(),
            content_type: None,
            data_schema: None,
            paginated: false,
        }
    }
}

pub struct Endpoint<S> {
    pub operation: Operation,
    route: Box<dyn Fn(Method) -> MethodRouter<S> + Send + Sync>,
    middlewares: Vec<Middleware<S>>,
}

impl<S: Clone + Send + Sync + 'static> Endpoint<S> {
    /// Generated adapters use this low-level constructor. Manually supplied metadata is not
    /// checked against a handler's Rust signature; use `#[endpoint]` for normal registration.
    #[doc(hidden)]
    pub fn handler<H, T>(handler: H, operation: Operation) -> Self
    where
        H: Handler<T, S> + Clone + Send + Sync + 'static,
        T: 'static,
    {
        Self {
            operation,
            middlewares: Vec::new(),
            route: Box::new(move |method| match method {
                Method::GET => get(handler.clone()),
                Method::POST => post(handler.clone()),
                Method::PUT => put(handler.clone()),
                Method::PATCH => patch(handler.clone()),
                Method::DELETE => delete(handler.clone()),
                _ => MethodRouter::new(),
            }),
        }
    }

    pub fn middleware<M: IntoMiddleware<S>>(mut self, middleware: M) -> Self {
        let middleware = middleware.into_middleware();
        if let Some(name) = middleware.security {
            self.operation.security.push(name.to_owned());
        }
        self.middlewares.push(middleware);
        self
    }

    pub(crate) fn prepend_middlewares(mut self, mut group: Vec<Middleware<S>>) -> Self {
        for middleware in &group {
            if let Some(name) = middleware.security {
                self.operation.security.push(name.to_owned());
            }
        }
        group.append(&mut self.middlewares);
        self.middlewares = group;
        self
    }

    pub(crate) fn into_route(
        self,
        method: Method,
    ) -> (MethodRouter<S>, Operation, Vec<Middleware<S>>) {
        ((self.route)(method), self.operation, self.middlewares)
    }
}
