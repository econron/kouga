//! Typed job contracts without database, HTTP, or worker dependencies.

pub use kouga_job_derive::{Job, job};
pub use serde;
use serde::{Serialize, de::DeserializeOwned};

pub trait Job: Serialize + DeserializeOwned + Send + Sync + 'static {
    const NAME: &'static str;
    const VERSION: u32;
    const QUEUE: &'static str;
}
