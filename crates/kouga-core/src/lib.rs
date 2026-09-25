//! HTTP、gRPC、workerから共有する、通信方式に依存しない型。

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::error::Error as StdError;
use std::fmt;

/// 更新時の「省略」と「指定された値」を区別する。
/// `Patch<Option<T>>`では`Value(None)`が明示的なnullを表す。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Patch<T> {
    #[default]
    Missing,
    Value(T),
}

impl<T> Patch<T> {
    pub fn is_missing(&self) -> bool {
        matches!(self, Self::Missing)
    }
}

impl<T: Serialize> Serialize for Patch<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Value(value) => value.serialize(serializer),
            Self::Missing => Err(serde::ser::Error::custom(
                "Patch::Missing must be skipped with skip_serializing_if",
            )),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Patch<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        T::deserialize(deserializer).map(Self::Value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    BadRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    Validation,
    TooLarge,
    UnsupportedMediaType,
    RateLimited,
    Unavailable,
    Timeout,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ErrorDetail {
    pub field: String,
    pub code: String,
}

/// 外部に出してよいcode/messageと、内部記録用sourceを分ける。
#[derive(Debug)]
pub struct Error {
    pub kind: ErrorKind,
    pub code: &'static str,
    pub message: &'static str,
    pub details: Vec<ErrorDetail>,
    source: Option<Box<dyn StdError + Send + Sync>>,
}

impl Error {
    pub fn new(kind: ErrorKind, code: &'static str, message: &'static str) -> Self {
        Self {
            kind,
            code,
            message,
            details: Vec::new(),
            source: None,
        }
    }

    pub fn with_source(mut self, source: impl StdError + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source.as_deref().map(|source| source as _)
    }
}

#[cfg(test)]
mod tests {
    use super::Patch;
    use serde::{Deserialize, Serialize};

    #[derive(Deserialize, Serialize)]
    #[serde(deny_unknown_fields)]
    struct Update {
        #[serde(default, skip_serializing_if = "Patch::is_missing")]
        description: Patch<Option<String>>,
        #[serde(default, skip_serializing_if = "Patch::is_missing")]
        title: Patch<String>,
    }

    #[test]
    fn patch_distinguishes_missing_null_and_value() {
        let missing: Update = serde_json::from_str("{}").unwrap();
        assert_eq!(missing.description, Patch::Missing);
        assert_eq!(serde_json::to_string(&missing).unwrap(), "{}");

        let null: Update = serde_json::from_str(r#"{"description":null}"#).unwrap();
        assert_eq!(null.description, Patch::Value(None));
        assert_eq!(
            serde_json::to_string(&null).unwrap(),
            r#"{"description":null}"#
        );

        let value: Update = serde_json::from_str(r#"{"description":"text"}"#).unwrap();
        assert_eq!(value.description, Patch::Value(Some("text".into())));
        assert!(serde_json::from_str::<Update>(r#"{"title":null}"#).is_err());
    }
}
