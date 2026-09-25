use tracing_subscriber::{EnvFilter, fmt};

#[derive(Debug)]
pub struct LogConfig {
    pub filter: String,
}

impl From<&crate::Config> for LogConfig {
    fn from(config: &crate::Config) -> Self {
        Self {
            filter: config.rust_log.clone(),
        }
    }
}

#[derive(Debug)]
pub enum InitError {
    InvalidFilter,
    AlreadyInitialized,
}

impl std::fmt::Display for InitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFilter => f.write_str("invalid log filter"),
            Self::AlreadyInitialized => f.write_str("global subscriber already initialized"),
        }
    }
}

impl std::error::Error for InitError {}

/// Called once by the binary. Applications with custom layers should build their own subscriber.
pub fn init(config: &LogConfig) -> Result<(), InitError> {
    let filter = EnvFilter::try_new(&config.filter).map_err(|_| InitError::InvalidFilter)?;
    fmt()
        .json()
        .with_env_filter(filter)
        .try_init()
        .map_err(|_| InitError::AlreadyInitialized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialization_is_once_and_rejects_bad_filter() {
        assert!(matches!(
            init(&LogConfig { filter: "[".into() }),
            Err(InitError::InvalidFilter)
        ));
        let config = LogConfig {
            filter: "info".into(),
        };
        init(&config).unwrap();
        assert!(matches!(init(&config), Err(InitError::AlreadyInitialized)));
    }
}
