//! 通信方式に依存しないRequest検証。deriveは別crateで追加する。

use std::future::Future;
use std::ops::Deref;

use kouga_core::{Error, ErrorDetail, ErrorKind};

pub use kouga_request_derive::Request;
pub use {kouga_core, schemars, serde, serde_json};

pub const MAX_ERRORS: usize = 100;
pub const MAX_NESTING_DEPTH: usize = 32;

thread_local! {
    static DECODE_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// deriveのDeserializeが保持するguard。失敗時も深さを戻す。
#[doc(hidden)]
pub struct DecodeDepth;

impl DecodeDepth {
    pub fn enter() -> Option<Self> {
        DECODE_DEPTH.with(|depth| {
            if depth.get() >= MAX_NESTING_DEPTH {
                None
            } else {
                depth.set(depth.get() + 1);
                Some(Self)
            }
        })
    }
}

impl Drop for DecodeDepth {
    fn drop(&mut self) {
        DECODE_DEPTH.with(|depth| depth.set(depth.get() - 1));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaDirection {
    Input,
    Output,
}

pub trait ApiSchema {
    fn schema(
        generator: &mut schemars::SchemaGenerator,
        direction: SchemaDirection,
    ) -> schemars::Schema;
}

/// Deriveが生成するネスト用接続口。通常のRequest利用者は`validate`だけを呼ぶ。
#[doc(hidden)]
pub trait DerivedRequest: Request {
    fn validate_sync_nested(&self, errors: &mut ValidationErrors, path: &str, depth: usize);
    fn validate_async_nested<'a>(
        &'a self,
        context: &'a Self::Context,
        errors: &'a mut ValidationErrors,
        path: &'a str,
        depth: usize,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>>;
}

#[doc(hidden)]
pub fn join_path(base: &str, field: &str) -> String {
    if base.is_empty() {
        field.to_owned()
    } else {
        format!("{base}.{field}")
    }
}

#[doc(hidden)]
pub fn record_error(errors: &mut ValidationErrors, base: &str, mut error: ValidationError) {
    error.field = if error.field.is_empty() {
        base.to_owned()
    } else {
        join_path(base, &error.field)
    };
    errors.push(error);
}

#[doc(hidden)]
pub fn record_async_error(
    errors: &mut ValidationErrors,
    base: &str,
    error: Error,
) -> Result<(), Error> {
    if error.kind != ErrorKind::Validation {
        return Err(error);
    }
    if error.details.is_empty() {
        errors.push(ValidationError::new(error.code).at(base));
    } else {
        for detail in error.details {
            record_error(
                errors,
                base,
                ValidationError::new(detail.code).at(detail.field),
            );
        }
    }
    Ok(())
}

#[doc(hidden)]
pub fn depth_error(errors: &mut ValidationErrors, path: &str, depth: usize) -> bool {
    if depth > MAX_NESTING_DEPTH {
        errors.push(ValidationError::new("too_deep").at(path));
        true
    } else {
        false
    }
}

/// 組み込みルールの実行時表現。T08のderiveは同じ値をschemaにも使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule<T = ()> {
    Length {
        min: Option<usize>,
        max: Option<usize>,
    },
    Range {
        min: Option<T>,
        max: Option<T>,
    },
    Email,
}

impl<T> Rule<T> {
    /// 文字列は`value.chars().count()`、配列は`value.len()`を渡す。
    pub fn check_length(self, length: usize) -> bool {
        match self {
            Self::Length { min, max } => {
                min.is_none_or(|min| length >= min) && max.is_none_or(|max| length <= max)
            }
            _ => false,
        }
    }

    pub fn check_range(&self, value: &T) -> bool
    where
        T: PartialOrd,
    {
        match self {
            Self::Range { min, max } => {
                min.as_ref().is_none_or(|min| value >= min)
                    && max.as_ref().is_none_or(|max| value <= max)
            }
            _ => false,
        }
    }

    pub fn check_email(self, value: &str) -> bool {
        matches!(self, Self::Email) && email_address::EmailAddress::is_valid(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub field: String,
    pub code: String,
}

impl ValidationError {
    pub fn new(code: impl Into<String>) -> Self {
        Self {
            field: String::new(),
            code: code.into(),
        }
    }

    pub fn at(mut self, field: impl Into<String>) -> Self {
        self.field = field.into();
        self
    }
}

#[derive(Debug, Default)]
pub struct ValidationErrors {
    errors: Vec<ValidationError>,
}

impl ValidationErrors {
    /// 上限到達時はfalseを返し、以後のエラーは保存しない。
    pub fn push(&mut self, error: ValidationError) -> bool {
        if self.is_full() {
            return false;
        }
        self.errors.push(error);
        true
    }

    pub fn as_slice(&self) -> &[ValidationError] {
        &self.errors
    }

    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.errors.len() >= MAX_ERRORS
    }

    pub fn len(&self) -> usize {
        self.errors.len()
    }

    fn into_error(self) -> Error {
        let mut error = Error::new(
            ErrorKind::Validation,
            "validation_failed",
            "Validation failed",
        );
        error.details = self
            .errors
            .into_iter()
            .map(|item| ErrorDetail {
                field: item.field,
                code: item.code,
            })
            .collect();
        error
    }
}

pub trait Request: Sized + Send + Sync {
    type Context: Send + Sync;

    fn validate_sync(&self, errors: &mut ValidationErrors);

    fn validate_async<'a>(
        &'a self,
        context: &'a Self::Context,
        errors: &'a mut ValidationErrors,
    ) -> impl Future<Output = Result<(), Error>> + Send + 'a;
}

/// 同期検証の成功後だけ非同期検証を行う。基盤障害はvalidationに変換しない。
pub async fn validate<T: Request>(value: T, context: &T::Context) -> Result<Validated<T>, Error> {
    let mut errors = ValidationErrors::default();
    value.validate_sync(&mut errors);
    if !errors.is_empty() {
        return Err(errors.into_error());
    }
    value.validate_async(context, &mut errors).await?;
    if !errors.is_empty() {
        return Err(errors.into_error());
    }
    Ok(Validated(value))
}

/// 外部から未検証値を包むconstructorは公開しない。
///
/// ```compile_fail
/// use kouga_validation::Validated;
/// let unchecked = Validated("unvalidated");
/// ```
/// ```compile_fail
/// use kouga_validation::Validated;
/// fn change(value: &mut Validated<String>) { value.push_str("changed"); }
/// ```
#[derive(Debug)]
pub struct Validated<T>(T);

impl<T> Validated<T> {
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> AsRef<T> for Validated<T> {
    fn as_ref(&self) -> &T {
        &self.0
    }
}

impl<T> Deref for Validated<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
