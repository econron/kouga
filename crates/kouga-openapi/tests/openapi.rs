use kouga_http::{
    Created, Endpoint, Json, Multipart, NoContent, Operation, ResponseMeta, Router, Validated,
    endpoint,
};
use kouga_openapi::generate;
use kouga_validation::{
    ApiSchema, Request, SchemaDirection, kouga_core::Patch, schemars, validate,
};
use serde_json::{Value, json};
use tower::ServiceExt;

#[derive(Debug, Request)]
struct UpdateTask {
    #[validate(length(min = 2, max = 8))]
    title: Patch<String>,
    note: Patch<Option<String>>,
}

fn router() -> Router<()> {
    Router::new()
        .post(
            "/tasks",
            Endpoint::handler(
                || async { Created::new("/tasks/1", "ok") },
                Operation::new("tasks.create").response::<Created<&str>>(),
            ),
        )
        .unwrap()
        .delete(
            "/tasks/{id}",
            Endpoint::handler(
                || async { NoContent },
                Operation::new("tasks.delete")
                    .path_input::<String>()
                    .response::<NoContent>(),
            ),
        )
        .unwrap()
}

#[test]
fn deterministic_and_matches_runtime_envelopes() {
    let first = generate(&router(), "Tasks", "1").unwrap();
    assert_eq!(first, generate(&router(), "Tasks", "1").unwrap());
    let document: Value = serde_json::from_str(&first).unwrap();
    assert_eq!(document["openapi"], "3.1.1");
    assert_eq!(
        document["paths"]["/tasks"]["post"]["responses"]["201"]["headers"]["Location"]["schema"]["type"],
        "string"
    );
    assert_eq!(
        document["paths"]["/tasks/{id}"]["delete"]["responses"]["204"].get("content"),
        None
    );
    assert_eq!(
        document["paths"]["/tasks/{id}"]["delete"]["parameters"][0]["required"],
        true
    );
}

#[test]
fn invalid_references_and_duplicate_ids_fail() {
    let mut operation = Operation::new("bad").response::<Json<&str>>();
    operation.request_body = Some(json!({"$ref":"#/$defs/Missing"}));
    let bad = Router::<()>::new()
        .get(
            "/bad",
            Endpoint::handler(|| async { Json("ok") }, operation),
        )
        .unwrap();
    assert!(
        generate(&bad, "Bad", "1")
            .unwrap_err()
            .to_string()
            .contains("unresolved reference")
    );
    // Router rejects duplicate IDs during registration; generation checks again defensively.
    assert!(
        Router::<()>::new()
            .get(
                "/one",
                Endpoint::handler(
                    || async { NoContent },
                    Operation::new("dup").response::<NoContent>()
                )
            )
            .unwrap()
            .get(
                "/two",
                Endpoint::handler(
                    || async { NoContent },
                    Operation::new("dup").response::<NoContent>()
                )
            )
            .is_err()
    );
}

struct UploadSchema;

impl ApiSchema for UploadSchema {
    fn schema(_: &mut schemars::SchemaGenerator, _: SchemaDirection) -> schemars::Schema {
        schemars::Schema::try_from(json!({
            "type":"object","required":["file"],
            "properties":{"file":{"type":"string","format":"binary"}}
        }))
        .unwrap()
    }
}

#[endpoint(operation_id = "files.upload")]
async fn upload(mut form: Multipart<UploadSchema>) -> Json<usize> {
    let mut size = 0;
    while let Some(mut field) = form.next_field().await.unwrap() {
        while let Some(chunk) = field.chunk().await.unwrap() {
            size += chunk.len();
        }
    }
    Json(size)
}

#[tokio::test]
async fn multipart_handler_streams_and_documents_same_input() {
    let routes = Router::<()>::new()
        .post("/files", upload_endpoint())
        .unwrap();
    let document: Value = serde_json::from_str(&generate(&routes, "Files", "1").unwrap()).unwrap();
    assert_eq!(
        document["paths"]["/files"]["post"]["requestBody"]["content"]["multipart/form-data"]["schema"]
            ["properties"]["file"]["format"],
        "binary"
    );
    let app = routes.with_state(());
    let response = app.clone().oneshot(
        axum::http::Request::builder()
            .method("POST")
            .uri("/files")
            .header("content-type", "multipart/form-data; boundary=test")
            .body(axum::body::Body::from("--test\r\nContent-Disposition: form-data; name=\"file\"; filename=\"file.txt\"\r\nContent-Type: text/plain\r\n\r\nhello\r\n--test--\r\n"))
            .unwrap(),
    ).await.unwrap();
    assert_eq!(response.status(), 200);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&body).unwrap()["data"], 5);
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/files")
                .header("content-type", "application/json")
                .body(axum::body::Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 415);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["error"]["code"],
        "invalid_multipart"
    );
}

#[test]
fn tuple_path_uses_each_item_schema() {
    let routes = Router::<()>::new()
        .get(
            "/bounds/{left}/{right}",
            Endpoint::handler(
                || async { NoContent },
                Operation::new("bounds.show")
                    .path_input::<(u32, String)>()
                    .response::<NoContent>(),
            ),
        )
        .unwrap();
    let document: Value = serde_json::from_str(&generate(&routes, "Bounds", "1").unwrap()).unwrap();
    let parameters = &document["paths"]["/bounds/{left}/{right}"]["get"]["parameters"];
    assert_eq!(parameters[0]["schema"]["type"], "integer");
    assert_eq!(parameters[1]["schema"]["type"], "string");
}

#[test]
fn binary_download_has_raw_body_schema_not_json_envelope() {
    let mut operation = Operation::new("files.download");
    operation.responses.push(ResponseMeta {
        status: 200,
        content_type: Some("application/octet-stream"),
        data_schema: Some(json!({"type":"string","format":"binary"})),
        paginated: false,
    });
    let routes = Router::<()>::new()
        .get("/files", Endpoint::handler(|| async { "bytes" }, operation))
        .unwrap();
    let document: Value = serde_json::from_str(&generate(&routes, "Files", "1").unwrap()).unwrap();
    let schema = &document["paths"]["/files"]["get"]["responses"]["200"]["content"]["application/octet-stream"]
        ["schema"];
    assert_eq!(schema["format"], "binary");
    assert!(schema.get("properties").is_none());
}

#[tokio::test]
async fn docs_are_opt_in_and_self_hosted() {
    let closed = kouga_openapi::serve(router(), (), "Tasks", "1", false).unwrap();
    let response = closed
        .oneshot(
            axum::http::Request::builder()
                .uri("/docs")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    let open = kouga_openapi::serve(router(), (), "Tasks", "1", true).unwrap();
    let asset = open
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/docs/swagger-ui-bundle.js")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(asset.status(), axum::http::StatusCode::OK);
    let response = open
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/openapi.yml")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let response = open
        .oneshot(
            axum::http::Request::builder()
                .uri("/docs/")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
}

#[tokio::test]
async fn request_rules_and_real_response_match_document() {
    let routes = Router::<()>::new()
        .patch(
            "/tasks",
            Endpoint::handler(
                |_input: Validated<UpdateTask>| async { Json("updated".to_owned()) },
                Operation::new("tasks.update")
                    .json_input::<UpdateTask>()
                    .response::<Json<String>>(),
            ),
        )
        .unwrap();
    let document: Value = serde_json::from_str(&generate(&routes, "Tasks", "1").unwrap()).unwrap();
    let schema = &document["paths"]["/tasks"]["patch"]["requestBody"]["content"]["application/json"]
        ["schema"];
    assert_eq!(schema["required"], json!([]));
    assert_eq!(schema["properties"]["title"]["minLength"], 2);
    assert_eq!(
        schema["properties"]["note"]["type"],
        json!(["string", "null"])
    );
    let input: UpdateTask = serde_json::from_value(json!({"title":"x"})).unwrap();
    assert!(validate(input, &()).await.is_err());
    let app = routes.with_state(());
    let rejected = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("PATCH")
                .uri("/tasks")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"title":"x"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        rejected.status(),
        axum::http::StatusCode::UNPROCESSABLE_ENTITY
    );
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method("PATCH")
                .uri("/tasks")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"title":"good"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let output = &document["paths"]["/tasks"]["patch"]["responses"]["200"]["content"]["application/json"]
        ["schema"];
    jsonschema::validator_for(output)
        .unwrap()
        .validate(&body)
        .unwrap();
}
