use kouga_validation::{
    ApiSchema, DerivedRequest, Request, SchemaDirection, ValidationError, ValidationErrors,
    kouga_core::{Error, ErrorKind, Patch},
    schemars::SchemaGenerator,
    serde_json::{Value, json},
    validate,
};

fn non_blank(value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        Err(ValidationError::new("blank"))
    } else {
        Ok(())
    }
}

fn check_pair(value: &Create) -> Result<(), ValidationError> {
    if value.start > value.end {
        Err(ValidationError::new("order").at("end"))
    } else {
        Ok(())
    }
}

async fn check_owner(value: &str, context: &String) -> Result<(), Error> {
    if value == "taken@example.com" {
        return Err(Error::new(ErrorKind::Validation, "taken", "taken"));
    }
    if value == context {
        Ok(())
    } else {
        Err(Error::new(ErrorKind::Unavailable, "db_down", "unavailable"))
    }
}

#[derive(Debug, Request)]
#[request(context = String)]
struct Child {
    #[validate(length(min = 1, max = 3), custom = non_blank)]
    #[schema(description = "表示名")]
    name: String,
}

#[derive(Debug, Request)]
#[request(context = String)]
#[validate(custom = check_pair)]
struct Create {
    #[request(rename = "ownerName")]
    #[validate(email, custom_async = check_owner)]
    #[schema(description = "owner")]
    owner: String,
    #[validate(range(min = 1, max = 5))]
    start: i32,
    end: i32,
    #[validate(nested)]
    children: Vec<Child>,
}

#[derive(Debug, Request)]
struct Update {
    #[validate(length(min = 1))]
    title: Patch<String>,
    description: Patch<Option<String>>,
}

#[derive(Debug, Request)]
struct Node {
    #[validate(nested)]
    next: Option<Box<Node>>,
}

#[derive(Debug, Request)]
enum Choice {
    Skip,
    Set {
        #[validate(length(min = 2, max = 4))]
        name: String,
    },
}

fn chain(depth: usize) -> Value {
    let mut value = json!({"next": null});
    for _ in 1..depth {
        value = json!({"next": value});
    }
    value
}

#[tokio::test]
async fn validation_and_schema() {
    let value: Create = serde_json::from_value(
        json!({"ownerName":"bad","start":6,"end":0,"children":[{"name":""}]}),
    )
    .unwrap();
    let err = validate(value, &"other".to_owned()).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Validation);
    assert_eq!(
        err.details
            .iter()
            .map(|v| (v.field.as_str(), v.code.as_str()))
            .collect::<Vec<_>>(),
        [
            ("ownerName", "email"),
            ("start", "range"),
            ("children[0].name", "length"),
            ("children[0].name", "blank"),
            ("end", "order")
        ]
    );
    let value: Create = serde_json::from_value(
        json!({"ownerName":"a@b.com","start":1,"end":2,"children":[{"name":"あ"}]}),
    )
    .unwrap();
    assert_eq!(
        validate(value, &"other".to_owned()).await.unwrap_err().kind,
        ErrorKind::Unavailable
    );
    let value: Create = serde_json::from_value(
        json!({"ownerName":"taken@example.com","start":1,"end":2,"children":[]}),
    )
    .unwrap();
    let err = validate(value, &"taken@example.com".to_owned())
        .await
        .unwrap_err();
    assert_eq!(err.details[0].field, "ownerName");
    assert_eq!(err.details[0].code, "taken");
    let schema: Value =
        Create::schema(&mut SchemaGenerator::default(), SchemaDirection::Input).into();
    assert_eq!(
        schema["required"],
        json!(["ownerName", "start", "end", "children"])
    );
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(schema["properties"]["start"]["minimum"], 1);
    assert_eq!(schema["properties"]["ownerName"]["format"], "email");
    assert!(
        serde_json::from_value::<Create>(
            json!({"ownerName":"a@b.com","start":1,"end":2,"children":[],"secret":1})
        )
        .is_err()
    );
    let child = Child { name: "x".into() };
    let mut errors = ValidationErrors::default();
    child.validate_sync_nested(&mut errors, "items[0]", 33);
    assert_eq!(errors.as_slice()[0].field, "items[0]");
    assert_eq!(errors.as_slice()[0].code, "too_deep");
}

#[tokio::test]
async fn patch_three_states() {
    let missing: Update = serde_json::from_value(json!({})).unwrap();
    assert!(matches!(missing.description, Patch::Missing));
    assert_eq!(
        validate(missing, &()).await.unwrap_err().details[0].code,
        "empty_patch"
    );
    let null: Update = serde_json::from_value(json!({"description":null})).unwrap();
    assert!(matches!(null.description, Patch::Value(None)));
    validate(null, &()).await.unwrap();
    assert!(serde_json::from_value::<Update>(json!({"title":null})).is_err());
    let schema: Value =
        Update::schema(&mut SchemaGenerator::default(), SchemaDirection::Input).into();
    assert_eq!(schema["required"], json!([]));
    assert_eq!(schema["properties"]["title"]["minLength"], 1);
    assert_eq!(
        schema["properties"]["description"]["type"],
        json!(["string", "null"])
    );
}

#[tokio::test]
async fn typed_decode_has_depth_limit_and_recovers_after_failure() {
    let deep = chain(33);
    assert!(
        serde_json::from_value::<Node>(deep)
            .unwrap_err()
            .to_string()
            .contains("nesting exceeds 32")
    );
    let allowed: Node = serde_json::from_value(chain(32)).unwrap();
    validate(allowed, &()).await.unwrap();
    let again: Node = serde_json::from_value(chain(1)).unwrap();
    validate(again, &()).await.unwrap();
}

#[tokio::test]
async fn enum_unit_and_named_variants_share_decode_validation_schema() {
    let unit: Choice = serde_json::from_value(json!("Skip")).unwrap();
    validate(unit, &()).await.unwrap();
    let named: Choice = serde_json::from_value(json!({"Set":{"name":"a"}})).unwrap();
    assert_eq!(
        validate(named, &()).await.unwrap_err().details[0].field,
        "Set.name"
    );
    let named: Choice = serde_json::from_value(json!({"Set":{"name":"あい"}})).unwrap();
    validate(named, &()).await.unwrap();
    assert!(serde_json::from_value::<Choice>(json!({"Set":{"name":"x","unknown":1}})).is_err());
    let schema: Value =
        Choice::schema(&mut SchemaGenerator::default(), SchemaDirection::Input).into();
    assert_eq!(schema["oneOf"][0]["const"], "Skip");
    assert_eq!(
        schema["oneOf"][1]["properties"]["Set"]["properties"]["name"]["minLength"],
        2
    );
}
