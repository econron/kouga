use std::{
    future::Future,
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    Unavailable,
}

impl HealthStatus {
    /// HTTP adapters can use this without making the runtime depend on HTTP.
    pub const fn http_status(self) -> u16 {
        match self {
            Self::Healthy => 200,
            Self::Unavailable => 503,
        }
    }
}

/// Liveness means the process is running; readiness also checks required dependencies.
pub struct Health {
    accepting: AtomicBool,
}

impl Default for Health {
    fn default() -> Self {
        Self::new()
    }
}

impl Health {
    pub const fn new() -> Self {
        Self {
            accepting: AtomicBool::new(true),
        }
    }

    /// Call before draining requests on shutdown.
    pub fn stop_accepting(&self) {
        self.accepting.store(false, Ordering::Release);
    }

    pub const fn liveness(&self) -> HealthStatus {
        HealthStatus::Healthy
    }

    /// The application supplies its required dependency probe, e.g. `SELECT 1` on its DB.
    pub async fn readiness<E>(&self, probe: impl Future<Output = Result<(), E>>) -> HealthStatus {
        if !self.accepting.load(Ordering::Acquire) {
            return HealthStatus::Unavailable;
        }
        let ready = probe.await.is_ok();
        if ready && self.accepting.load(Ordering::Acquire) {
            HealthStatus::Healthy
        } else {
            HealthStatus::Unavailable
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn live_and_ready_are_distinct() {
        let health = Health::new();
        assert_eq!(health.liveness().http_status(), 200);
        assert_eq!(
            health
                .readiness(async { Ok::<(), ()>(()) })
                .await
                .http_status(),
            200
        );
        assert_eq!(
            health
                .readiness(async { Err::<(), ()>(()) })
                .await
                .http_status(),
            503
        );
        health.stop_accepting();
        assert_eq!(health.liveness().http_status(), 200);
        assert_eq!(
            health
                .readiness(async { Ok::<(), ()>(()) })
                .await
                .http_status(),
            503
        );
    }
}
