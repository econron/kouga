use super::{invalid, resource};
use std::{error::Error, fs, io, path::Path};

pub fn generate() -> Result<(), Box<dyn Error>> {
    let manifest = fs::read_to_string("Cargo.toml")?;
    let lib = fs::read_to_string("src/lib.rs")?;
    if [
        "src/auth.rs",
        "src/auth_mail.rs",
        "src/bin/auth-mail-worker.rs",
        "src/models/user.rs",
        "tests/auth.rs",
    ]
    .into_iter()
    .any(|path| Path::new(path).exists())
    {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, "auth already generated").into());
    }
    if !lib.contains("// kouga: resource routes") {
        if lib != include_str!("../templates/lib.rs.txt") {
            return Err(invalid("lib.rs was edited; register auth manually").into());
        }
        let server = fs::read_to_string("src/bin/server.rs")?;
        let name = manifest
            .lines()
            .find_map(|line| {
                line.strip_prefix("name = \"")
                    .and_then(|s| s.strip_suffix('"'))
            })
            .ok_or_else(|| invalid("missing package name"))?
            .replace('-', "_");
        if server != include_str!("../templates/server.rs.txt").replace("APP_CRATE", &name) {
            return Err(invalid("server.rs was edited; register DB state manually").into());
        }
    }
    resource::build(
        "User",
        vec!["email:string".into(), "password_hash:string".into()],
        false,
    )?;
    let manifest = fs::read_to_string("Cargo.toml")?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| invalid("source workspace unavailable"))?;
    let mut additions = String::new();
    for name in [
        "kouga-auth",
        "kouga-cache",
        "kouga-queue",
        "kouga-job",
        "kouga-runtime",
        "kouga-worker",
        "kouga-mailer",
    ] {
        if !manifest.contains(&format!("{name} =")) {
            additions.push_str(&format!(
                "{name} = {{ path = {:?} }}\n",
                root.join("crates").join(name).display().to_string()
            ));
        }
    }
    additions.push_str(
        "sha2 = \"=0.10.9\"\ntokio-util = { version = \"=0.7.19\", features = [\"rt\"] }\n",
    );
    fs::write(
        "Cargo.toml",
        manifest.replacen(
            "[dependencies]\n",
            &format!("[dependencies]\n{additions}"),
            1,
        ),
    )?;
    let lib = fs::read_to_string("src/lib.rs")?;
    if !lib.contains("// kouga: resource routes") {
        return Err(invalid("resource route marker missing").into());
    }
    let lib = lib
        .replacen(
            "pub mod models;",
            "pub mod auth;\npub mod auth_mail;\npub mod models;",
            1,
        )
        .replacen(
            "    // kouga: resource routes",
            "    let router = auth::routes(router);\n    // kouga: resource routes",
            1,
        );
    fs::write("src/lib.rs", lib)?;
    let server = fs::read_to_string("src/bin/server.rs")?;
    if server.contains("axum::serve(listener, app).await?;") {
        fs::write("src/bin/server.rs", server.replace(
            "axum::serve(listener, app).await?;",
            "axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await?;",
        ))?;
    }
    fs::write("src/auth.rs", include_str!("../templates/auth.rs.txt"))?;
    fs::write(
        "src/auth_mail.rs",
        include_str!("../templates/auth-mail.rs.txt"),
    )?;
    fs::write(
        "src/bin/auth-mail-worker.rs",
        include_str!("../templates/auth-mail-worker.rs.txt").replace("APP_CRATE", &app_crate()?),
    )?;
    fs::create_dir_all("tests")?;
    fs::write(
        "tests/auth.rs",
        include_str!("../templates/auth-test.rs.txt").replace("APP_CRATE", &app_crate()?),
    )?;
    let version = resource::timestamp()?;
    let up = format!(
        "CREATE UNIQUE INDEX users_email_unique ON users (lower(email));\n{}\nALTER TABLE kouga_auth_tokens ADD CONSTRAINT kouga_auth_tokens_user_fk FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;\n{}\n{}\nCREATE TABLE kouga_password_resets (\n    token_hash bytea PRIMARY KEY CHECK (octet_length(token_hash) = 32),\n    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,\n    expires_at timestamptz NOT NULL,\n    consumed_at timestamptz\n);\nCREATE INDEX kouga_password_resets_user_idx ON kouga_password_resets (user_id);\n",
        include_str!("../../kouga-auth/migrations/20260925000018_create_kouga_auth_tokens.up.sql"),
        include_str!("../../kouga-cache/migrations/20260925000023_create_kouga_cache.up.sql"),
        include_str!("../../kouga-queue/migrations/20260925000020_create_kouga_jobs.up.sql")
    );
    fs::write(format!("migrations/{version}_create_auth.up.sql"), up)?;
    fs::write(
        format!("migrations/{version}_create_auth.down.sql"),
        "DROP TABLE kouga_password_resets;\nDROP TABLE kouga_jobs;\nDROP TABLE kouga_rate_limits;\nDROP TABLE kouga_cache;\nDROP TABLE kouga_auth_tokens;\nDROP INDEX users_email_unique;\n",
    )?;
    println!("Generated auth routes and mail worker");
    Ok(())
}

fn app_crate() -> Result<String, io::Error> {
    fs::read_to_string("Cargo.toml")?
        .lines()
        .find_map(|line| {
            line.strip_prefix("name = \"")
                .and_then(|s| s.strip_suffix('"'))
        })
        .map(|s| s.replace('-', "_"))
        .ok_or_else(|| invalid("missing package name"))
}
