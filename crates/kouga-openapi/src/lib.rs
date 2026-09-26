//! OpenAPI 3.1.1 generated from registered HTTP operations.
//! The emitted JSON is also valid YAML 1.2, so it can be stored as `openapi.yml`.

use kouga_http::{Operation, Router};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}

pub fn generate<S>(router: &Router<S>, title: &str, version: &str) -> Result<String, Error>
where
    S: Clone + Send + Sync + 'static,
{
    let mut paths: BTreeMap<String, BTreeMap<String, Value>> = BTreeMap::new();
    let mut ids = BTreeSet::new();
    let mut definitions = Map::new();
    let mut bearer = false;
    for route in router.routes() {
        if !ids.insert(route.operation_id) {
            return Err(Error(format!(
                "duplicate operation ID: {}",
                route.operation_id
            )));
        }
        let path = paths.entry(route.path.clone()).or_default();
        let method = route.method.as_str().to_ascii_lowercase();
        if path.contains_key(&method) {
            return Err(Error(format!("duplicate route: {} {}", method, route.path)));
        }
        let operation = operation(route, &mut definitions)?;
        bearer |= !route.security.is_empty();
        path.insert(method, operation);
    }
    let mut components = json!({"schemas": definitions});
    if bearer {
        components["securitySchemes"] = json!({"bearerAuth": {"type":"http", "scheme":"bearer"}});
    }
    let document = json!({
        "openapi": "3.1.1",
        "info": {"title": title, "version": version},
        "paths": paths,
        "components": components,
    });
    validate_references(&document)?;
    let specification: Value = serde_json::from_str(include_str!("openapi-3.1-schema.json"))
        .map_err(|error| Error(format!("invalid bundled OpenAPI schema: {error}")))?;
    let validator = jsonschema::validator_for(&specification)
        .map_err(|error| Error(format!("invalid bundled OpenAPI schema: {error}")))?;
    validator
        .validate(&document)
        .map_err(|error| Error(format!("OpenAPI schema validation failed: {error}")))?;
    let _: utoipa::openapi::OpenApi = serde_json::from_value(document.clone())
        .map_err(|error| Error(format!("invalid OpenAPI document: {error}")))?;
    Ok(format!(
        "{}\n",
        serde_json::to_string_pretty(&document).map_err(|e| Error(e.to_string()))?
    ))
}

/// Mount vendored Swagger UI and the generated document only when explicitly enabled.
pub fn serve<S>(
    router: Router<S>,
    state: S,
    title: &str,
    version: &str,
    enabled: bool,
) -> Result<axum::Router, Error>
where
    S: Clone + Send + Sync + 'static,
{
    if enabled
        && router.routes().iter().any(|route| {
            route.path == "/openapi.yml"
                || route.path == "/docs"
                || route.path.starts_with("/docs/")
        })
    {
        return Err(Error(
            "documentation path conflicts with application route".to_owned(),
        ));
    }
    let document = if enabled {
        Some(generate(&router, title, version)?)
    } else {
        None
    };
    let app = router.with_state(state);
    if let Some(document) = document {
        let value: Value = serde_json::from_str(&document).map_err(|e| Error(e.to_string()))?;
        let ui: axum::Router = utoipa_swagger_ui::SwaggerUi::new("/docs")
            .external_url_unchecked("/openapi.yml", value)
            .into();
        Ok(app.merge(ui))
    } else {
        Ok(app)
    }
}

fn operation(route: &Operation, definitions: &mut Map<String, Value>) -> Result<Value, Error> {
    let mut parameters = Vec::new();
    if let Some(schema) = &route.path_schema {
        let schema = normalize(schema.clone(), definitions)?;
        let properties = schema.get("properties").and_then(Value::as_object);
        for (index, parameter) in route.parameters.iter().enumerate() {
            let value = properties
                .and_then(|props| props.get(&parameter.name))
                .cloned()
                .or_else(|| {
                    schema
                        .get("prefixItems")
                        .and_then(Value::as_array)
                        .and_then(|items| items.get(index))
                        .cloned()
                })
                .unwrap_or_else(|| schema.clone());
            parameters
                .push(json!({"name":parameter.name,"in":"path","required":true,"schema":value}));
        }
    }
    if let Some(schema) = &route.query_schema {
        let schema = normalize(schema.clone(), definitions)?;
        let properties = schema
            .get("properties")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                Error(format!(
                    "query input must be an object: {}",
                    route.operation_id
                ))
            })?;
        let required = schema.get("required").and_then(Value::as_array);
        for (name, value) in properties {
            let is_required = required.is_some_and(|items| items.iter().any(|item| item == name));
            parameters
                .push(json!({"name":name,"in":"query","required":is_required,"schema":value}));
        }
    }
    let mut responses = Map::new();
    for response in &route.responses {
        let mut item = json!({"description": format!("HTTP {}", response.status)});
        if let Some(content_type) = response.content_type {
            let data = response
                .data_schema
                .clone()
                .ok_or_else(|| Error(format!("missing response schema: {}", route.operation_id)))?;
            let data = normalize(data, definitions)?;
            let body = if content_type != "application/json" {
                data
            } else if response.paginated {
                json!({"type":"object","required":["data","meta"],"properties":{
                    "data":data,"meta":{"type":"object","required":["page","per_page","has_next"],"properties":{
                        "page":{"type":"integer"},"per_page":{"type":"integer"},"has_next":{"type":"boolean"}
                    }}
                }})
            } else {
                json!({"type":"object","required":["data"],"properties":{"data":data}})
            };
            item["content"] = json!({content_type: {"schema":body}});
        }
        if response.status == 201 {
            item["headers"] = json!({"Location":{"description":"Created resource URL","schema":{"type":"string"}}});
        }
        if responses
            .insert(response.status.to_string(), item)
            .is_some()
        {
            return Err(Error(format!(
                "duplicate response status: {}",
                route.operation_id
            )));
        }
    }
    let error_schema = json!({"type":"object","required":["error"],"properties":{
        "error":{"type":"object","required":["code","message","details"],"properties":{
            "code":{"type":"string"},"message":{"type":"string"},"details":{"type":"array","items":{"type":"object","required":["field","code"],"properties":{"field":{"type":"string"},"code":{"type":"string"}}}}
        }}
    }});
    for (status, description) in [
        (400, "Bad request"),
        (404, "Not found"),
        (422, "Validation failed"),
        (500, "Internal server error"),
    ] {
        responses.entry(status.to_string()).or_insert_with(|| json!({"description":description,"content":{"application/json":{"schema":error_schema}}}));
    }
    if !route.security.is_empty() {
        responses.entry("401".to_owned()).or_insert_with(|| json!({"description":"Unauthorized","content":{"application/json":{"schema":error_schema}}}));
        responses.entry("403".to_owned()).or_insert_with(|| json!({"description":"Forbidden","content":{"application/json":{"schema":error_schema}}}));
    }
    if route.request_body.is_some() {
        responses.entry("415".to_owned()).or_insert_with(|| json!({"description":"Unsupported media type","content":{"application/json":{"schema":error_schema}}}));
    }
    let mut result =
        json!({"operationId":route.operation_id,"parameters":parameters,"responses":responses});
    if let Some(body) = &route.request_body {
        let content_type = route.request_content_type;
        result["requestBody"] = json!({"required":true,"content":{content_type:{"schema":normalize(body.clone(), definitions)?}}});
    }
    if let Some(summary) = route.summary {
        result["summary"] = json!(summary);
    }
    if let Some(description) = route.description {
        result["description"] = json!(description);
    }
    if !route.tags.is_empty() {
        result["tags"] = json!(route.tags);
    }
    if route.deprecated {
        result["deprecated"] = json!(true);
    }
    if !route.security.is_empty() {
        if route.security.iter().any(|name| name != "bearerAuth") {
            return Err(Error(format!(
                "unsupported security scheme: {}",
                route.operation_id
            )));
        }
        result["security"] = json!([{"bearerAuth":[]}]);
    }
    Ok(result)
}

fn normalize(mut schema: Value, definitions: &mut Map<String, Value>) -> Result<Value, Error> {
    if let Some(local) = schema
        .as_object_mut()
        .and_then(|object| object.remove("$defs"))
    {
        let local = local
            .as_object()
            .ok_or_else(|| Error("invalid $defs".to_owned()))?;
        for (name, value) in local {
            let mut value = value.clone();
            rewrite_refs(&mut value)?;
            match definitions.get(name) {
                Some(existing) if existing != &value => {
                    return Err(Error(format!("conflicting schema definition: {name}")));
                }
                None => {
                    definitions.insert(name.clone(), value);
                }
                _ => {}
            }
        }
    }
    rewrite_refs(&mut schema)?;
    Ok(schema)
}

fn rewrite_refs(value: &mut Value) -> Result<(), Error> {
    match value {
        Value::Object(object) => {
            object.remove("$schema");
            if let Some(reference) = object.get_mut("$ref") {
                let text = reference
                    .as_str()
                    .ok_or_else(|| Error("invalid $ref".to_owned()))?;
                if let Some(name) = text.strip_prefix("#/$defs/") {
                    *reference = json!(format!("#/components/schemas/{name}"));
                }
            }
            for value in object.values_mut() {
                rewrite_refs(value)?;
            }
        }
        Value::Array(items) => {
            for value in items {
                rewrite_refs(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_references(document: &Value) -> Result<(), Error> {
    if !document.is_object() {
        return Err(Error("invalid OpenAPI document".to_owned()));
    }
    validate_references_in(document, document)
}

fn validate_references_in(value: &Value, document: &Value) -> Result<(), Error> {
    match value {
        Value::Object(object) => {
            if let Some(reference) = object.get("$ref") {
                let text = reference
                    .as_str()
                    .ok_or_else(|| Error("invalid $ref".to_owned()))?;
                let pointer = text
                    .strip_prefix('#')
                    .ok_or_else(|| Error(format!("external reference is unsupported: {text}")))?;
                if document.pointer(pointer).is_none() {
                    return Err(Error(format!("unresolved reference: {text}")));
                }
            }
            for child in object.values() {
                validate_references_in(child, document)?;
            }
        }
        Value::Array(items) => {
            for child in items {
                validate_references_in(child, document)?;
            }
        }
        _ => {}
    }
    Ok(())
}
