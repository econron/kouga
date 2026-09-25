//! Requestの検証契約。ルール実行とderiveはT07・T08で追加する。

use std::future::Future;
use std::ops::Deref;

use kouga_core::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub field: String,
    pub code: String,
}

#[derive(Debug, Default)]
pub struct ValidationErrors {
    pub errors: Vec<ValidationError>,
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

/// 生成経路はT07が追加する。外部から未検証値を包むconstructorは公開しない。
///
/// ```compile_fail
/// use kouga_validation::Validated;
/// let unchecked = Validated("unvalidated");
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
