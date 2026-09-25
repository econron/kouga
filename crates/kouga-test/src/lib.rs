//! Portless HTTP requests and isolated PostgreSQL schemas for application tests.

use std::{env, error::Error, io, path::Path, time::Duration};

use axum::{Router, body::Body, http::Request, response::Response};
use kouga_db::Db;
use kouga_migration::{MigrationSet, Migrator, MigratorOptions};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

type TestResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

/// Sends requests through the real router and middleware without binding a port.
/// Authentication is supplied through ordinary headers; authorization still runs.
pub struct TestClient(Router);

impl TestClient {
    pub fn new(router: Router) -> Self {
        Self(router)
    }

    pub async fn send(&self, request: Request<Body>) -> Response {
        self.0
            .clone()
            .oneshot(request)
            .await
            .expect("router service")
    }
}

/// One schema per test. Call `close` to remove it after the pool has drained.
pub struct TestDb {
    db: Db,
    admin: Db,
    schema: String,
}

impl TestDb {
    /// Requires TEST_DATABASE_URL; refuses to use the same URL as DATABASE_URL.
    pub async fn from_env(migrations: impl AsRef<Path>) -> TestResult<Self> {
        let url = env::var("TEST_DATABASE_URL")?;
        if env::var("DATABASE_URL").is_ok_and(|development| development == url) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "TEST_DATABASE_URL must differ from DATABASE_URL",
            )
            .into());
        }
        Self::connect(&url, migrations).await
    }

    pub async fn connect(url: &str, migrations: impl AsRef<Path>) -> TestResult<Self> {
        let set = MigrationSet::load(migrations)?;
        let admin = kouga_db::connect(url, 2, Duration::from_secs(5)).await?;
        let schema = format!("kouga_test_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&admin)
            .await?;
        let search_path = schema.clone();
        let db = PgPoolOptions::new()
            .max_connections(5)
            .after_connect(move |connection, _| {
                let search_path = search_path.clone();
                Box::pin(async move {
                    sqlx::query("SELECT set_config('search_path', $1, false)")
                        .bind(search_path)
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .connect(url)
            .await;
        let db = match db {
            Ok(db) => db,
            Err(error) => {
                sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
                    .execute(&admin)
                    .await?;
                return Err(error.into());
            }
        };
        let isolated = Self { db, admin, schema };
        if let Err(error) = Migrator::new(isolated.db.clone(), set, MigratorOptions::default())
            .migrate()
            .await
        {
            isolated.close().await?;
            return Err(error.into());
        }
        Ok(isolated)
    }

    pub fn db(&self) -> &Db {
        &self.db
    }

    pub async fn close(self) -> TestResult<()> {
        self.db.close().await;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP SCHEMA {} CASCADE",
            self.schema
        )))
        .execute(&self.admin)
        .await?;
        self.admin.close().await;
        Ok(())
    }
}
