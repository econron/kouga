use super::{GenerateCommand, check_app, invalid};
use std::{error::Error, fs, io, path::Path};

struct Field {
    name: String,
    ty: &'static str,
    sql: &'static str,
    default: Option<bool>,
}

pub(super) fn snake(name: &str) -> Option<String> {
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    let mut out = String::new();
    for (index, ch) in name.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index != 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    Some(out)
}

pub(super) fn identifier(name: &str) -> bool {
    !name.is_empty()
        && name.as_bytes()[0].is_ascii_lowercase()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        && !matches!(
            name,
            "id" | "created_at"
                | "updated_at"
                | "self"
                | "super"
                | "crate"
                | "type"
                | "mod"
                | "fn"
                | "pub"
                | "use"
                | "async"
                | "await"
                | "match"
                | "if"
                | "else"
                | "loop"
                | "for"
                | "while"
                | "true"
                | "false"
                | "struct"
                | "enum"
                | "impl"
                | "let"
                | "const"
                | "static"
                | "mut"
                | "ref"
                | "where"
                | "trait"
                | "dyn"
                | "move"
                | "return"
                | "break"
                | "continue"
                | "in"
                | "as"
                | "extern"
                | "unsafe"
                | "union"
                | "gen"
                | "try"
                | "yield"
                | "macro"
                | "box"
                | "do"
                | "abstract"
                | "become"
                | "final"
                | "override"
                | "priv"
                | "typeof"
                | "unsized"
                | "virtual"
        )
}

fn fields(args: Vec<String>) -> Result<Vec<Field>, io::Error> {
    if args.is_empty() {
        return Err(invalid("at least one field is required"));
    }
    let mut result = Vec::new();
    for arg in args {
        let (name, definition) = arg
            .split_once(':')
            .ok_or_else(|| invalid("field must be name:type"))?;
        if !identifier(name)
            || name.len() > 63
            || result.iter().any(|field: &Field| field.name == name)
        {
            return Err(invalid("invalid or duplicate field name"));
        }
        let (kind, default) = definition
            .split_once('=')
            .map_or((definition, None), |(kind, value)| (kind, Some(value)));
        let (ty, sql) = match kind {
            "string" => ("String", "text"),
            "bool" => ("bool", "boolean"),
            "integer" | "int" => ("i32", "integer"),
            "bigint" => ("i64", "bigint"),
            _ => {
                return Err(invalid(
                    "supported field types: string, bool, integer, bigint",
                ));
            }
        };
        let default = match default {
            None => None,
            Some("true") if ty == "bool" => Some(true),
            Some("false") if ty == "bool" => Some(false),
            _ => {
                return Err(invalid(
                    "only boolean defaults (=true or =false) are supported",
                ));
            }
        };
        result.push(Field {
            name: name.into(),
            ty,
            sql,
            default,
        });
    }
    Ok(result)
}

pub fn generate(command: GenerateCommand) -> Result<(), Box<dyn Error>> {
    check_app()?;
    if Path::new("apps/http/Cargo.toml").is_file() && !Path::new("src/lib.rs").is_file() {
        std::env::set_current_dir("apps/http")?;
    }
    match command {
        GenerateCommand::Auth => super::auth::generate(),
        GenerateCommand::Resource { name, fields } => build(&name, fields, true),
        GenerateCommand::Model { name, fields } => build(&name, fields, false),
        GenerateCommand::Request { name, fields } => request_only(&name, fields),
        GenerateCommand::Migration { name } => {
            if !identifier(&name) {
                return Err(invalid("migration name must be snake_case").into());
            }
            let root = Path::new("migrations");
            let path = kouga_migration::generate_migration(root, &name)?;
            println!("Created {}", path.display());
            Ok(())
        }
        GenerateCommand::Middleware { name } => super::features::middleware(&name),
        GenerateCommand::Mailer { name } => super::features::mailer(&name),
        GenerateCommand::Job { name, fields } => super::features::job(&name, &fields),
        GenerateCommand::Channel { name } => super::features::channel(&name),
    }
}

fn names(name: &str) -> Result<(String, String), io::Error> {
    if !name.as_bytes().first().is_some_and(u8::is_ascii_uppercase) {
        return Err(invalid("model name must be PascalCase (for example, Task)"));
    }
    let singular = snake(name).ok_or_else(|| invalid("model name must be ASCII PascalCase"))?;
    if !identifier(&singular) {
        return Err(invalid("invalid model name"));
    }
    // ponytail: s/es only; add an explicit table/path option when irregular names matter.
    let plural = if singular.ends_with('s') {
        format!("{singular}es")
    } else {
        format!("{singular}s")
    };
    if plural.len() > 63 {
        return Err(invalid("model name is too long for PostgreSQL"));
    }
    Ok((singular, plural))
}

fn request_only(name: &str, args: Vec<String>) -> Result<(), Box<dyn Error>> {
    let (_, plural) = names(name)?;
    let fields = fields(args)?;
    let path = format!("src/requests/{plural}.rs");
    if fs::symlink_metadata(&path).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{path} already exists"),
        )
        .into());
    }
    let module = Path::new("src/requests/mod.rs");
    let fresh = !module.exists();
    let lib = fs::read_to_string("src/lib.rs")?;
    let manifest = fs::read_to_string("Cargo.toml")?;
    if fresh
        && lib != include_str!("../templates/lib.rs.txt")
        && super::api::without_greeting(&lib).as_deref()
            != Some(include_str!("../templates/lib.rs.txt"))
    {
        return Err(invalid("lib.rs was edited; register Request manually").into());
    }
    if !fresh && fs::read_to_string(module)?.contains(&format!("pub mod {plural};")) {
        return Err(invalid("Request already registered").into());
    }
    let new_manifest = if fresh {
        request_manifest(&manifest)?
    } else {
        manifest.clone()
    };
    if fresh {
        write_new("src/requests/mod.rs", "")?;
    }
    write_new(&path, &requests(name, &fields))?;
    append("src/requests/mod.rs", &format!("pub mod {plural};\n"))?;
    if fresh {
        fs::write("src/lib.rs", format!("pub mod requests;\n{lib}"))?;
        fs::write("Cargo.toml", new_manifest)?;
    }
    println!("Created {path}");
    Ok(())
}

pub(crate) fn timestamp() -> Result<String, Box<dyn Error>> {
    let latest = fs::read_dir("migrations")
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .filter_map(|name| {
            name.get(..14)
                .and_then(|date| chrono::NaiveDateTime::parse_from_str(date, "%Y%m%d%H%M%S").ok())
        })
        .max();
    let now = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();
    let mut time = chrono::NaiveDateTime::parse_from_str(&now, "%Y%m%d%H%M%S")?;
    if let Some(latest) = latest.filter(|latest| *latest >= time) {
        time = latest + chrono::Duration::seconds(1);
    }
    Ok(time.format("%Y%m%d%H%M%S").to_string())
}

pub(crate) fn build(name: &str, args: Vec<String>, resource: bool) -> Result<(), Box<dyn Error>> {
    let (singular, plural) = names(name)?;
    let fields = fields(args)?;
    let version = timestamp()?;
    let prefix = format!("migrations/{version}_create_{plural}");
    let model_path = format!("src/models/{singular}.rs");
    let request_path = format!("src/requests/{plural}.rs");
    let controller_path = format!("src/controllers/{plural}.rs");
    let test_path = format!("tests/{plural}.rs");
    let mut files = vec![
        (model_path, model(name, &plural, &fields)),
        (format!("{prefix}.up.sql"), migration_up(&plural, &fields)),
        (
            format!("{prefix}.down.sql"),
            format!("DROP TABLE \"{plural}\";\n"),
        ),
    ];
    if resource {
        files.push((request_path, requests(name, &fields)));
        files.push((
            controller_path,
            controller(name, &singular, &plural, &fields),
        ));
        files.push((test_path, test(&plural, &fields)?));
    }
    let first = !Path::new("src/models/mod.rs").exists();
    let manifest = fs::read_to_string("Cargo.toml")?;
    let lib = fs::read_to_string("src/lib.rs")?;
    let server = fs::read_to_string("src/bin/server.rs")?;
    let new_manifest = if first {
        app_manifest(&manifest)?
    } else {
        manifest.clone()
    };
    let new_server = if first {
        app_server(&server)?
    } else {
        server.clone()
    };
    let new_lib = app_lib(&lib, &plural, first, resource)?;
    for (path, _) in &files {
        if fs::symlink_metadata(path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{path} already exists"),
            )
            .into());
        }
    }
    for path in [
        "src/models/mod.rs",
        "src/requests/mod.rs",
        "src/controllers/mod.rs",
        "src/bin/db-create.rs",
        "src/bin/db-migrate.rs",
        "src/bin/db-status.rs",
    ] {
        if first && path != "src/requests/mod.rs" && fs::symlink_metadata(path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{path} already exists"),
            )
            .into());
        }
    }
    if !first && fs::read_to_string("src/models/mod.rs")?.contains(&format!("pub mod {singular};"))
    {
        return Err(invalid("model already registered").into());
    }
    if !first
        && resource
        && fs::read_to_string("src/controllers/mod.rs")?.contains(&format!("pub mod {plural};"))
    {
        return Err(invalid("resource already registered").into());
    }
    if first {
        write_new("src/models/mod.rs", "")?;
        if !Path::new("src/requests/mod.rs").exists() {
            write_new("src/requests/mod.rs", "")?;
        }
        write_new("src/controllers/mod.rs", "")?;
        write_new(
            "src/bin/db-create.rs",
            include_str!("../templates/db-create.rs.txt"),
        )?;
        write_new(
            "src/bin/db-migrate.rs",
            include_str!("../templates/db-migrate.rs.txt"),
        )?;
        write_new(
            "src/bin/db-status.rs",
            include_str!("../templates/db-status.rs.txt"),
        )?;
    }
    for (path, content) in &files {
        write_new(path, content)?;
    }
    append("src/models/mod.rs", &format!("pub mod {singular};\n"))?;
    if resource {
        append("src/requests/mod.rs", &format!("pub mod {plural};\n"))?;
        append("src/controllers/mod.rs", &format!("pub mod {plural};\n"))?;
    }
    if new_manifest != manifest {
        fs::write("Cargo.toml", new_manifest)?;
    }
    if new_server != server {
        fs::write("src/bin/server.rs", new_server)?;
    }
    fs::write("src/lib.rs", new_lib)?;
    println!(
        "Generated {name} {}",
        if resource { "resource" } else { "model" }
    );
    Ok(())
}

fn write_new(path: &str, content: &str) -> io::Result<()> {
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent)?;
    }
    use std::io::Write;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(content.as_bytes())
}
fn append(path: &str, content: &str) -> io::Result<()> {
    use std::io::Write;
    fs::OpenOptions::new()
        .append(true)
        .open(path)?
        .write_all(content.as_bytes())
}

fn app_manifest(old: &str) -> Result<String, io::Error> {
    if !old.contains("[package.metadata.kouga]")
        || !old.contains("[dependencies]\n")
        || old.contains("kouga-model =")
        || old.contains("[dev-dependencies]")
        || ["serde =", "serde_json =", "schemars ="]
            .into_iter()
            .any(|dependency| old.contains(dependency))
    {
        return Err(invalid(
            "Cargo.toml was edited; add DB dependencies manually",
        ));
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| invalid("source workspace unavailable"))?;
    let dep = |name: &str| {
        format!(
            "{name} = {{ path = {:?} }}\n",
            root.join("crates").join(name).display().to_string()
        )
    };
    let mut additions = String::new();
    for name in [
        "kouga-model",
        "kouga-validation",
        "kouga-migration",
        "kouga-core",
    ] {
        if !old.contains(&format!("{name} =")) {
            additions.push_str(&dep(name));
        }
    }
    additions.push_str("serde = { version = \"=1.0.229\", features = [\"derive\"] }\nserde_json = \"=1.0.151\"\nschemars = { version = \"=1.2.2\", default-features = false, features = [\"std\"] }\n");
    Ok(format!(
        "{}\n[dev-dependencies]\n{}",
        old.replacen(
            "[dependencies]\n",
            &format!("[dependencies]\n{additions}"),
            1,
        ),
        dep("kouga-test")
    ))
}

fn request_manifest(old: &str) -> Result<String, io::Error> {
    if !old.contains("[package.metadata.kouga]") || !old.contains("[dependencies]\n") {
        return Err(invalid("not a generated Kouga application"));
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| invalid("source workspace unavailable"))?;
    let additions = ["kouga-validation", "kouga-core"]
        .into_iter()
        .filter(|name| !old.contains(&format!("{name} =")))
        .map(|name| {
            format!(
                "{name} = {{ path = {:?} }}\n",
                root.join("crates").join(name).display().to_string()
            )
        })
        .collect::<String>();
    Ok(old.replacen(
        "[dependencies]\n",
        &format!("[dependencies]\n{additions}"),
        1,
    ))
}

fn app_server(old: &str) -> Result<String, io::Error> {
    let app = app_crate()?;
    let plain = include_str!("../templates/server.rs.txt").replace("APP_CRATE", &app);
    let database = include_str!("../templates/server-db.rs.txt").replace("APP_CRATE", &app);
    if old == plain {
        return Ok(database);
    }
    if old.contains("// kouga: otel begin") {
        let service = format!("{}-http", super::otel::service_name()?);
        if old == super::otel::instrument(&plain, &service, true)? {
            return super::otel::instrument(&database, &service, true);
        }
    }
    Err(invalid("server.rs was edited; register DB state manually"))
}

fn app_crate() -> Result<String, io::Error> {
    let manifest = fs::read_to_string("Cargo.toml")?;
    let name = manifest
        .lines()
        .find_map(|line| {
            line.strip_prefix("name = \"")
                .and_then(|s| s.strip_suffix('"'))
        })
        .ok_or_else(|| invalid("missing package name"))?;
    Ok(name.replace('-', "_"))
}

fn app_lib(old: &str, plural: &str, first: bool, resource: bool) -> Result<String, io::Error> {
    if first {
        let standard = include_str!("../templates/lib.rs.txt");
        let without_greeting = super::api::without_greeting(old);
        let original = without_greeting.as_deref().unwrap_or(old);
        let prefix = original
            .strip_suffix(standard)
            .ok_or_else(|| invalid("lib.rs was edited; register resource manually"))?;
        if !prefix.lines().all(|line| {
            matches!(
                line,
                "pub mod requests;" | "pub mod mailers;" | "pub mod jobs;" | "pub mod middlewares;"
            )
        }) {
            return Err(invalid("lib.rs was edited; register resource manually"));
        }
        let mut text = include_str!("../templates/lib-db.rs.txt").to_owned();
        let prefix = prefix.replace("pub mod requests;\n", "");
        text = format!("{prefix}{text}");
        if resource {
            text = text.replace("// kouga: resource routes\n", &format!("let router = controllers::{plural}::routes(router);\n    // kouga: resource routes\n"));
        }
        if let Some(snippet) = super::api::greeting_snippet(old) {
            text = text.replacen("    router\n}", &format!("{snippet}    router\n}}"), 1);
        }
        return Ok(text);
    }
    if !old.contains("// kouga: resource routes") {
        return Err(invalid("resource route marker missing; register manually"));
    }
    if resource {
        Ok(old.replacen("    // kouga: resource routes", &format!("    let router = controllers::{plural}::routes(router);\n    // kouga: resource routes"), 1))
    } else {
        Ok(old.to_owned())
    }
}

fn model(name: &str, table: &str, fields: &[Field]) -> String {
    let mut out = format!(
        "use kouga_model::{{Model, Uuid, sqlx::types::chrono::{{DateTime, Utc}}}};\n\n#[derive(Debug, Model)]\n#[model(table = \"{table}\")]\npub struct {name} {{\n    pub id: Uuid,\n"
    );
    for field in fields {
        if field.default.is_some() {
            out.push_str("    #[model(default)]\n");
        }
        out.push_str(&format!("    pub {}: {},\n", field.name, field.ty));
    }
    out.push_str("    pub created_at: DateTime<Utc>,\n    pub updated_at: DateTime<Utc>,\n}\n");
    out
}

fn migration_up(table: &str, fields: &[Field]) -> String {
    let mut out = format!("CREATE TABLE \"{table}\" (\n    id uuid PRIMARY KEY,\n");
    for field in fields {
        out.push_str(&format!(
            "    \"{}\" {} NOT NULL{},\n",
            field.name,
            field.sql,
            field
                .default
                .map_or(String::new(), |v| format!(" DEFAULT {v}"))
        ));
    }
    out.push_str("    created_at timestamptz NOT NULL DEFAULT now(),\n    updated_at timestamptz NOT NULL DEFAULT now()\n);\n");
    out
}

fn requests(name: &str, fields: &[Field]) -> String {
    let mut out = format!(
        "use kouga_validation::Request;\nuse kouga_core::Patch;\n\n#[derive(Debug, Request)]\npub struct Create{name}Request {{\n"
    );
    for field in fields {
        if field.ty == "String" {
            out.push_str("    #[validate(length(min = 1))]\n");
        }
        out.push_str(&format!(
            "    pub {}: {}{},\n",
            field.name,
            if field.default.is_some() {
                "Option<"
            } else {
                ""
            },
            if field.default.is_some() {
                format!("{}>", field.ty)
            } else {
                field.ty.to_owned()
            }
        ));
    }
    out.push_str(&format!(
        "}}\n\n#[derive(Debug, Request)]\npub struct Update{name}Request {{\n"
    ));
    for field in fields {
        if field.ty == "String" {
            out.push_str("    #[validate(length(min = 1))]\n");
        }
        out.push_str(&format!("    pub {}: Patch<{}>,\n", field.name, field.ty));
    }
    out.push_str("}\n");
    out
}

fn controller(name: &str, singular: &str, plural: &str, fields: &[Field]) -> String {
    let mut output_fields = String::new();
    let mut conversions = String::new();
    let mut schema_required = vec!["\"id\"".to_owned()];
    let mut schema_properties = Vec::new();
    let mut create_fields = String::new();
    let mut update_fields = String::new();
    let mut empty = Vec::new();
    for field in fields {
        output_fields.push_str(&format!("    pub {}: {},\n", field.name, field.ty));
        schema_required.push(format!("\"{}\"", field.name));
        schema_properties.push(format!(
            "\"{}\": {{\"type\": \"{}\"}}",
            field.name,
            match field.ty {
                "String" => "string",
                "bool" => "boolean",
                _ => "integer",
            }
        ));
        conversions.push_str(&format!(
            "            {}: value.{},\n",
            field.name, field.name
        ));
        create_fields.push_str(&format!(
            "            {}: input.{}.clone(),\n",
            field.name, field.name
        ));
        update_fields.push_str(&format!(
            "            {}: input.{}.clone(),\n",
            field.name, field.name
        ));
        empty.push(format!("input.{}.is_missing()", field.name));
    }
    include_str!("../templates/controller.rs.txt")
        .replace("MODEL_NAME", name)
        .replace("MODEL_MOD", singular)
        .replace("PLURAL", plural)
        .replace("OUTPUT_FIELDS", &output_fields)
        .replace("OUTPUT_SCHEMA_REQUIRED", &schema_required.join(", "))
        .replace("OUTPUT_SCHEMA_PROPERTIES", &schema_properties.join(", "))
        .replace("OUTPUT_CONVERSIONS", &conversions)
        .replace("CREATE_FIELDS", &create_fields)
        .replace("UPDATE_FIELDS", &update_fields)
        .replace("EMPTY_CHECK", &empty.join(" && "))
}

fn test(plural: &str, fields: &[Field]) -> Result<String, io::Error> {
    let mut body = Vec::new();
    let mut invalid = Vec::new();
    let mut patch = Vec::new();
    let mut default_asserts = String::new();
    let first_string = fields
        .iter()
        .find(|field| field.ty == "String")
        .map(|field| field.name.as_str());
    for field in fields {
        if let Some(value) = field.default {
            default_asserts.push_str(&format!(
                "    assert_eq!(body[\"data\"][\"{}\"], serde_json::json!({value}));\n",
                field.name
            ));
        }
        let value = match field.ty {
            "String" => "\"example\"",
            "bool" => "true",
            _ => "1",
        };
        if field.default.is_none() {
            body.push(format!("\"{}\":{value}", field.name));
            invalid.push(format!(
                "\"{}\":{}",
                field.name,
                if Some(field.name.as_str()) == first_string {
                    "\"\""
                } else {
                    value
                }
            ));
        }
        let updated = match field.ty {
            "String" => "\"updated\"",
            "bool" => "true",
            _ => "2",
        };
        patch.push(format!("\"{}\":{updated}", field.name));
    }
    Ok(include_str!("../templates/resource-test.rs.txt")
        .replace("APP_CRATE", &app_crate()?)
        .replace("PLURAL", plural)
        .replace("CREATE_JSON", &format!("{{{}}}", body.join(",")))
        .replace("DEFAULT_ASSERTS", &default_asserts)
        .replace("PATCH_JSON", &format!("{{{}}}", patch.join(",")))
        .replace(
            "INVALID_JSON",
            &if first_string.is_some() {
                format!("{{{}}}", invalid.join(","))
            } else {
                "{\"__unknown\":true}".to_owned()
            },
        )
        .replace(
            "INVALID_STATUS",
            if first_string.is_some() {
                "UNPROCESSABLE_ENTITY"
            } else {
                "BAD_REQUEST"
            },
        ))
}

#[cfg(test)]
mod tests {
    use super::{fields, names};

    #[test]
    fn rejects_unsafe_resource_arguments() {
        assert_eq!(names("Task").unwrap(), ("task".into(), "tasks".into()));
        assert!(names("../Task").is_err());
        assert!(fields(vec!["title:string".into(), "title:bool".into()]).is_err());
        assert!(fields(vec!["id:string".into()]).is_err());
        assert!(fields(vec!["struct:string".into()]).is_err());
        assert!(fields(vec!["admin;DROP:string".into()]).is_err());
        assert!(fields(vec!["completed:bool=no".into()]).is_err());
    }
}
