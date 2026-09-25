use serde::Deserialize;
use std::{env, fmt, fs, path::Path};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Environment {
    Development,
    Test,
    Production,
}

impl Environment {
    pub fn from_env() -> Result<Self, ConfigError> {
        match env::var("KOUGA_ENV") {
            Ok(value) => value.parse(),
            Err(env::VarError::NotPresent) => Ok(Self::Development),
            Err(_) => Err(ConfigError::Invalid("KOUGA_ENV")),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Test => "test",
            Self::Production => "production",
        }
    }
}

impl std::str::FromStr for Environment {
    type Err = ConfigError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "development" => Ok(Self::Development),
            "test" => Ok(Self::Test),
            "production" => Ok(Self::Production),
            _ => Err(ConfigError::Invalid("KOUGA_ENV")),
        }
    }
}

/// Debug/Display never reveal the value. Call `expose` only at the consuming boundary.
#[derive(Clone, Eq, PartialEq)]
pub struct Secret<T>(T);

impl<T> Secret<T> {
    pub fn new(value: T) -> Self {
        Self(value)
    }

    pub fn expose(&self) -> &T {
        &self.0
    }
}

impl<T> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl<T> fmt::Display for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum ConfigError {
    Missing(&'static str),
    Invalid(&'static str),
    Conflict(&'static str),
    Read(&'static str),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(key) => write!(f, "missing configuration: {key}"),
            Self::Invalid(key) => write!(f, "invalid configuration: {key}"),
            Self::Conflict(key) => write!(f, "conflicting configuration: {key}"),
            Self::Read(key) => write!(f, "cannot read configuration: {key}"),
        }
    }
}

impl std::error::Error for ConfigError {}

#[derive(Debug)]
pub struct Config {
    pub environment: Environment,
    pub runtime_threads: usize,
    pub host: String,
    pub port: u16,
    pub grpc_port: u16,
    pub http_max_in_flight: usize,
    pub http_timeout_ms: u64,
    pub body_limit_bytes: usize,
    pub db_max_connections: u32,
    pub db_acquire_timeout_ms: u64,
    pub blocking_concurrency: usize,
    pub blocking_waiters: usize,
    pub blocking_wait_timeout_ms: u64,
    pub shutdown_timeout_ms: u64,
    pub rust_log: String,
    pub database_url: Option<Secret<String>>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawConfig {
    runtime_threads: Option<usize>,
    host: Option<String>,
    port: Option<u16>,
    grpc_port: Option<u16>,
    http_max_in_flight: Option<usize>,
    http_timeout_ms: Option<u64>,
    body_limit_bytes: Option<usize>,
    db_max_connections: Option<u32>,
    db_acquire_timeout_ms: Option<u64>,
    blocking_concurrency: Option<usize>,
    blocking_waiters: Option<usize>,
    blocking_wait_timeout_ms: Option<u64>,
    shutdown_timeout_ms: Option<u64>,
    rust_log: Option<String>,
}

impl Config {
    /// Binaries that use the database call this at startup; other targets need no DB secret.
    pub fn database_url_required(&self) -> Result<&Secret<String>, ConfigError> {
        self.database_url
            .as_ref()
            .ok_or(ConfigError::Missing("DATABASE_URL"))
    }

    pub fn load(root: impl AsRef<Path>, environment: Environment) -> Result<Self, ConfigError> {
        Self::load_with(root, environment, read_env)
    }

    fn load_with(
        root: impl AsRef<Path>,
        environment: Environment,
        read: impl Fn(&'static str) -> Result<Option<String>, ConfigError>,
    ) -> Result<Self, ConfigError> {
        let root = root.as_ref();
        let mut merged = toml::Table::new();
        for path in [
            root.join("config/base.toml"),
            root.join(format!("config/{}.toml", environment.name())),
        ] {
            let content = match fs::read_to_string(&path) {
                Ok(content) => content,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Err(ConfigError::Read("TOML")),
            };
            let table: toml::Table =
                toml::from_str(&content).map_err(|_| ConfigError::Invalid("TOML"))?;
            merged.extend(table);
        }
        let mut raw: RawConfig = toml::Value::Table(merged)
            .try_into()
            .map_err(|_| ConfigError::Invalid("TOML"))?;

        macro_rules! override_env {
            ($field:ident, $key:literal) => {
                if let Some(value) = read($key)? {
                    raw.$field = Some(value.parse().map_err(|_| ConfigError::Invalid($key))?);
                }
            };
        }
        override_env!(runtime_threads, "KOUGA_RUNTIME_THREADS");
        override_env!(host, "HOST");
        override_env!(port, "PORT");
        override_env!(grpc_port, "GRPC_PORT");
        override_env!(http_max_in_flight, "KOUGA_HTTP_MAX_IN_FLIGHT");
        override_env!(http_timeout_ms, "KOUGA_HTTP_TIMEOUT_MS");
        override_env!(body_limit_bytes, "KOUGA_BODY_LIMIT_BYTES");
        override_env!(db_max_connections, "KOUGA_DB_MAX_CONNECTIONS");
        override_env!(db_acquire_timeout_ms, "KOUGA_DB_ACQUIRE_TIMEOUT_MS");
        override_env!(blocking_concurrency, "KOUGA_BLOCKING_CONCURRENCY");
        override_env!(blocking_waiters, "KOUGA_BLOCKING_WAITERS");
        override_env!(blocking_wait_timeout_ms, "KOUGA_BLOCKING_WAIT_TIMEOUT_MS");
        override_env!(shutdown_timeout_ms, "KOUGA_SHUTDOWN_TIMEOUT_MS");
        override_env!(rust_log, "RUST_LOG");

        let direct = read("DATABASE_URL")?;
        let file = read("DATABASE_URL_FILE")?;
        if direct.is_some() && file.is_some() {
            return Err(ConfigError::Conflict("DATABASE_URL / DATABASE_URL_FILE"));
        }
        let database_url = match (direct, file) {
            (Some(value), None) => Some(value),
            (None, Some(path)) => Some(
                fs::read_to_string(path)
                    .map_err(|_| ConfigError::Read("DATABASE_URL_FILE"))?
                    .trim_end_matches(['\r', '\n'])
                    .to_owned(),
            ),
            _ => None,
        };
        if database_url.as_ref().is_some_and(String::is_empty) {
            return Err(ConfigError::Invalid("DATABASE_URL"));
        }

        let config = Self {
            environment,
            runtime_threads: raw
                .runtime_threads
                .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, usize::from)),
            host: raw.host.unwrap_or_else(|| "0.0.0.0".into()),
            port: raw.port.unwrap_or(3000),
            grpc_port: raw.grpc_port.unwrap_or(50051),
            http_max_in_flight: raw.http_max_in_flight.unwrap_or(256),
            http_timeout_ms: raw.http_timeout_ms.unwrap_or(30_000),
            body_limit_bytes: raw.body_limit_bytes.unwrap_or(1_048_576),
            db_max_connections: raw.db_max_connections.unwrap_or(10),
            db_acquire_timeout_ms: raw.db_acquire_timeout_ms.unwrap_or(5_000),
            blocking_concurrency: raw.blocking_concurrency.unwrap_or(4),
            blocking_waiters: raw.blocking_waiters.unwrap_or(16),
            blocking_wait_timeout_ms: raw.blocking_wait_timeout_ms.unwrap_or(1_000),
            shutdown_timeout_ms: raw.shutdown_timeout_ms.unwrap_or(30_000),
            rust_log: raw.rust_log.unwrap_or_else(|| "info".into()),
            database_url: database_url.map(Secret::new),
        };
        for (key, value) in [
            ("runtime_threads", config.runtime_threads as u64),
            ("port", config.port as u64),
            ("grpc_port", config.grpc_port as u64),
            ("http_max_in_flight", config.http_max_in_flight as u64),
            ("http_timeout_ms", config.http_timeout_ms),
            ("body_limit_bytes", config.body_limit_bytes as u64),
            ("db_max_connections", config.db_max_connections as u64),
            ("db_acquire_timeout_ms", config.db_acquire_timeout_ms),
            ("blocking_concurrency", config.blocking_concurrency as u64),
            ("blocking_waiters", config.blocking_waiters as u64),
            ("blocking_wait_timeout_ms", config.blocking_wait_timeout_ms),
            ("shutdown_timeout_ms", config.shutdown_timeout_ms),
        ] {
            if value == 0 {
                return Err(ConfigError::Invalid(key));
            }
        }
        if config.host.is_empty() || config.rust_log.is_empty() {
            return Err(ConfigError::Invalid("host / rust_log"));
        }
        Ok(config)
    }
}

fn read_env(key: &'static str) -> Result<Option<String>, ConfigError> {
    match env::var(key) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(_) => Err(ConfigError::Invalid(key)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::HashMap,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn root() -> std::path::PathBuf {
        let path = env::temp_dir().join(format!(
            "kouga-config-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join("config")).unwrap();
        path
    }

    fn load(root: &Path, vars: &[(&'static str, &str)]) -> Result<Config, ConfigError> {
        let vars: HashMap<_, _> = vars.iter().copied().collect();
        Config::load_with(root, Environment::Test, |key| {
            Ok(vars.get(key).map(|value| (*value).to_owned()))
        })
    }

    #[test]
    fn precedence_and_secret() {
        let root = root();
        fs::write(
            root.join("config/base.toml"),
            "port = 1000\nruntime_threads = 2",
        )
        .unwrap();
        fs::write(root.join("config/test.toml"), "port = 2000").unwrap();
        let secret_path = root.join("db-secret");
        fs::write(&secret_path, "postgres://password\n").unwrap();
        let config = load(
            &root,
            &[
                ("PORT", "3000"),
                ("DATABASE_URL_FILE", secret_path.to_str().unwrap()),
            ],
        )
        .unwrap();
        assert_eq!(config.port, 3000);
        assert_eq!(config.runtime_threads, 2);
        assert_eq!(
            config.database_url.as_ref().unwrap().expose(),
            "postgres://password"
        );
        assert!(!format!("{config:?}").contains("password"));
        assert_eq!(format!("{}", config.database_url.unwrap()), "[REDACTED]");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_unknown_zero_conflict_and_empty_secret() {
        let root = root();
        fs::write(root.join("config/base.toml"), "unknown = 1").unwrap();
        assert!(matches!(
            load(&root, &[]),
            Err(ConfigError::Invalid("TOML"))
        ));
        fs::write(root.join("config/base.toml"), "").unwrap();
        assert!(matches!(
            load(&root, &[]).unwrap().database_url_required(),
            Err(ConfigError::Missing("DATABASE_URL"))
        ));
        assert!(matches!(
            load(&root, &[("PORT", "0")]),
            Err(ConfigError::Invalid("port"))
        ));
        assert!(matches!(
            load(&root, &[("DATABASE_URL", "a"), ("DATABASE_URL_FILE", "b")]),
            Err(ConfigError::Conflict(_))
        ));
        assert!(matches!(
            load(&root, &[("DATABASE_URL", "")]),
            Err(ConfigError::Invalid("DATABASE_URL"))
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
