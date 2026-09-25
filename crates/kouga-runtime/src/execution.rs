use crate::Config;
use futures_util::future::{Either, select};
use std::{fmt, future::Future, sync::Arc, time::Duration};
use tokio::{runtime, sync::Semaphore, task::JoinError, time};
use tokio_util::sync::CancellationToken;

pub struct Runtime(runtime::Runtime);

impl Runtime {
    pub fn build(config: &Config) -> Result<Self, std::io::Error> {
        runtime::Builder::new_multi_thread()
            .worker_threads(config.runtime_threads)
            .enable_all()
            .build()
            .map(Self)
    }

    pub fn block_on<F: Future>(&self, future: F) -> F::Output {
        self.0.block_on(future)
    }
}

#[derive(Clone)]
pub struct BlockingPool {
    running: Arc<Semaphore>,
    waiting: Arc<Semaphore>,
    wait_timeout: Duration,
}

#[derive(Debug)]
pub enum BlockingError {
    Full,
    Timeout,
    Closed,
    Panicked(JoinError),
}

impl fmt::Display for BlockingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full => f.write_str("blocking queue is full"),
            Self::Timeout => f.write_str("blocking queue wait timed out"),
            Self::Closed => f.write_str("blocking pool is closed"),
            Self::Panicked(_) => f.write_str("blocking task panicked"),
        }
    }
}

impl std::error::Error for BlockingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Panicked(error) => Some(error),
            _ => None,
        }
    }
}

impl BlockingPool {
    pub fn new(config: &Config) -> Self {
        Self {
            running: Arc::new(Semaphore::new(config.blocking_concurrency)),
            waiting: Arc::new(Semaphore::new(config.blocking_waiters)),
            wait_timeout: Duration::from_millis(config.blocking_wait_timeout_ms),
        }
    }

    pub async fn run<F, T>(&self, work: F) -> Result<T, BlockingError>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let permit = match self.running.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                let waiting = self
                    .waiting
                    .clone()
                    .try_acquire_owned()
                    .map_err(|_| BlockingError::Full)?;
                let result = time::timeout(self.wait_timeout, self.running.clone().acquire_owned())
                    .await
                    .map_err(|_| BlockingError::Timeout)?
                    .map_err(|_| BlockingError::Closed)?;
                drop(waiting);
                result
            }
        };
        // The permit lives in the blocking closure, even if the async caller is cancelled.
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            work()
        })
        .await
        .map_err(BlockingError::Panicked)
    }
}

#[derive(Clone)]
pub struct Shutdown {
    token: CancellationToken,
    grace: Duration,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ShutdownError {
    Timeout,
}

impl Shutdown {
    pub fn new(config: &Config) -> Self {
        Self {
            token: CancellationToken::new(),
            grace: Duration::from_millis(config.shutdown_timeout_ms),
        }
    }

    pub fn token(&self) -> CancellationToken {
        self.token.clone()
    }

    pub fn cancel(&self) {
        self.token.cancel();
    }

    /// Returns after Ctrl-C, SIGTERM (Unix), or an explicit `cancel`.
    pub async fn wait_for_signal(&self) -> Result<(), std::io::Error> {
        #[cfg(unix)]
        {
            let mut term =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
            let signals = select(Box::pin(tokio::signal::ctrl_c()), Box::pin(term.recv()));
            match select(Box::pin(self.token.cancelled()), Box::pin(signals)).await {
                Either::Left(_) => {}
                Either::Right((Either::Left((result, _)), _)) => result?,
                Either::Right((Either::Right(_), _)) => {}
            }
        }
        #[cfg(not(unix))]
        match select(
            Box::pin(self.token.cancelled()),
            Box::pin(tokio::signal::ctrl_c()),
        )
        .await
        {
            Either::Left(_) => {}
            Either::Right((result, _)) => result?,
        }
        self.cancel();
        Ok(())
    }

    /// Stop intake first; drain and flush share one deadline.
    pub async fn graceful<D, F>(&self, drain: D, flush: F) -> Result<(), ShutdownError>
    where
        D: Future<Output = ()>,
        F: Future<Output = ()>,
    {
        self.cancel();
        time::timeout(self.grace, async {
            drain.await;
            flush.await;
        })
        .await
        .map_err(|_| ShutdownError::Timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Environment;
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        sync::mpsc,
    };

    fn config() -> Config {
        Config::load(
            std::env::temp_dir().join("kouga-runtime-no-config"),
            Environment::Test,
        )
        .unwrap()
    }

    #[test]
    fn runtime_is_multi_thread() {
        let mut config = config();
        config.runtime_threads = 2;
        let runtime = Runtime::build(&config).unwrap();
        assert_eq!(runtime.block_on(async { 2 + 2 }), 4);
    }

    #[test]
    fn blocking_limit_survives_caller_cancellation() {
        runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let mut config = config();
                config.blocking_concurrency = 1;
                config.blocking_waiters = 1;
                config.blocking_wait_timeout_ms = 100;
                let pool = BlockingPool::new(&config);
                let (release_tx, release_rx) = mpsc::channel();
                let (started_tx, started_rx) = tokio::sync::oneshot::channel();
                let first = tokio::spawn({
                    let pool = pool.clone();
                    async move {
                        pool.run(move || {
                            started_tx.send(()).unwrap();
                            release_rx.recv().unwrap();
                        })
                        .await
                    }
                });
                started_rx.await.unwrap();
                first.abort();
                let waiting = tokio::spawn({
                    let pool = pool.clone();
                    async move { pool.run(|| 1).await }
                });
                while pool.waiting.available_permits() != 0 {
                    tokio::task::yield_now().await;
                }
                assert!(matches!(pool.run(|| 3).await, Err(BlockingError::Full)));
                assert!(matches!(
                    waiting.await.unwrap(),
                    Err(BlockingError::Timeout)
                ));
                release_tx.send(()).unwrap();
                assert_eq!(pool.run(|| 2).await.unwrap(), 2);
            });
    }

    #[test]
    fn shutdown_cancels_then_drains_then_flushes() {
        runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let shutdown = Shutdown::new(&config());
                let order = AtomicUsize::new(0);
                shutdown
                    .graceful(
                        async {
                            assert!(shutdown.token().is_cancelled());
                            order.store(1, Ordering::SeqCst);
                        },
                        async {
                            assert_eq!(order.load(Ordering::SeqCst), 1);
                        },
                    )
                    .await
                    .unwrap();
                shutdown.wait_for_signal().await.unwrap();
                let mut short = shutdown.clone();
                short.grace = Duration::from_millis(1);
                assert_eq!(
                    short.graceful(std::future::pending(), async {}).await,
                    Err(ShutdownError::Timeout)
                );
            });
    }
}
