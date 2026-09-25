//! Small JSON caches and a PostgreSQL-backed, fail-closed HTTP rate limit.

use kouga_db::Db;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

pub const SCHEMA_SQL: &str = include_str!("../migrations/20260925000023_create_kouga_cache.up.sql");

#[derive(Debug)]
pub enum CacheError {
    InvalidInput,
    Database(kouga_db::DbError),
    Poisoned,
}

impl From<sqlx::Error> for CacheError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error.into())
    }
}

type Entry = (Value, Instant, u64);

/// Bounded process-local cache. The oldest entry is removed when full.
pub struct MemoryCache {
    entries: Mutex<HashMap<(String, String), Entry>>,
    capacity: usize,
    next: Mutex<u64>,
}

impl MemoryCache {
    pub fn new(capacity: usize) -> Result<Self, CacheError> {
        if capacity == 0 {
            return Err(CacheError::InvalidInput);
        }
        Ok(Self {
            entries: Mutex::new(HashMap::new()),
            capacity,
            next: Mutex::new(0),
        })
    }

    pub fn get(&self, namespace: &str, key: &str) -> Result<Option<Value>, CacheError> {
        let mut entries = self.entries.lock().map_err(|_| CacheError::Poisoned)?;
        let name = (namespace.to_owned(), key.to_owned());
        if entries
            .get(&name)
            .is_some_and(|(_, expires, _)| *expires <= Instant::now())
        {
            entries.remove(&name);
        }
        Ok(entries.get(&name).map(|(value, _, _)| value.clone()))
    }

    pub fn set(
        &self,
        namespace: &str,
        key: &str,
        value: Value,
        ttl: Duration,
    ) -> Result<(), CacheError> {
        let expires = Instant::now()
            .checked_add(ttl)
            .ok_or(CacheError::InvalidInput)?;
        if ttl.is_zero() || namespace.is_empty() || key.is_empty() {
            return Err(CacheError::InvalidInput);
        }
        let mut entries = self.entries.lock().map_err(|_| CacheError::Poisoned)?;
        let mut next = self.next.lock().map_err(|_| CacheError::Poisoned)?;
        entries.retain(|_, (_, expires, _)| *expires > Instant::now());
        if entries.len() >= self.capacity
            && !entries.contains_key(&(namespace.to_owned(), key.to_owned()))
            && let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, (_, _, order))| *order)
                .map(|(key, _)| key.clone())
        {
            entries.remove(&oldest);
        }
        *next = next.wrapping_add(1);
        entries.insert(
            (namespace.to_owned(), key.to_owned()),
            (value, expires, *next),
        );
        Ok(())
    }

    pub fn delete(&self, namespace: &str, key: &str) -> Result<(), CacheError> {
        self.entries
            .lock()
            .map_err(|_| CacheError::Poisoned)?
            .remove(&(namespace.to_owned(), key.to_owned()));
        Ok(())
    }

    pub fn clean(&self) -> Result<usize, CacheError> {
        let mut entries = self.entries.lock().map_err(|_| CacheError::Poisoned)?;
        let before = entries.len();
        entries.retain(|_, (_, expires, _)| *expires > Instant::now());
        Ok(before - entries.len())
    }

    pub fn fetch<F>(&self, namespace: &str, key: &str, ttl: Duration, source: F) -> Value
    where
        F: FnOnce() -> Value,
    {
        match self.get(namespace, key) {
            Ok(Some(value)) => return value,
            Err(error) => tracing::warn!(?error, "cache read failed"),
            Ok(None) => {}
        }
        let value = source();
        if let Err(error) = self.set(namespace, key, value.clone(), ttl) {
            tracing::warn!(?error, "cache write failed");
        }
        value
    }
}

#[derive(Clone)]
pub struct PgCache {
    db: Db,
}

impl PgCache {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub async fn get(&self, namespace: &str, key: &str) -> Result<Option<Value>, CacheError> {
        let row: Option<(Value,)> = sqlx::query_as("SELECT value FROM kouga_cache WHERE namespace = $1 AND key = $2 AND expires_at > now()")
            .bind(namespace).bind(key).fetch_optional(&self.db).await?;
        Ok(row.map(|(value,)| value))
    }

    pub async fn set(
        &self,
        namespace: &str,
        key: &str,
        value: &Value,
        ttl: Duration,
    ) -> Result<(), CacheError> {
        let micros = i64::try_from(ttl.as_micros()).map_err(|_| CacheError::InvalidInput)?;
        if micros == 0 || namespace.is_empty() || key.is_empty() {
            return Err(CacheError::InvalidInput);
        }
        sqlx::query("INSERT INTO kouga_cache(namespace, key, value, expires_at) VALUES ($1, $2, $3, now() + $4 * interval '1 microsecond') ON CONFLICT (namespace, key) DO UPDATE SET value = excluded.value, expires_at = excluded.expires_at")
            .bind(namespace).bind(key).bind(value).bind(micros).execute(&self.db).await?;
        Ok(())
    }

    pub async fn delete(&self, namespace: &str, key: &str) -> Result<(), CacheError> {
        sqlx::query("DELETE FROM kouga_cache WHERE namespace = $1 AND key = $2")
            .bind(namespace)
            .bind(key)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    pub async fn clean(&self) -> Result<u64, CacheError> {
        Ok(
            sqlx::query("DELETE FROM kouga_cache WHERE expires_at <= now()")
                .execute(&self.db)
                .await?
                .rows_affected(),
        )
    }

    /// Cache failure is observable but does not suppress the source operation.
    pub async fn fetch<F, Fut>(&self, namespace: &str, key: &str, ttl: Duration, source: F) -> Value
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Value>,
    {
        match self.get(namespace, key).await {
            Ok(Some(value)) => return value,
            Err(error) => tracing::warn!(?error, "cache read failed"),
            Ok(None) => {}
        }
        let value = source().await;
        if let Err(error) = self.set(namespace, key, &value, ttl).await {
            tracing::warn!(?error, "cache write failed");
        }
        value
    }
}

#[derive(Clone)]
pub struct RateLimiter {
    db: Db,
    namespace: String,
    limit: i64,
    window: i64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RateDecision {
    Allowed,
    Denied { retry_after: u64 },
}

impl RateLimiter {
    pub fn new(
        db: Db,
        namespace: impl Into<String>,
        limit: u64,
        window: Duration,
    ) -> Result<Self, CacheError> {
        let namespace = namespace.into();
        let limit = i64::try_from(limit).map_err(|_| CacheError::InvalidInput)?;
        let window = i64::try_from(window.as_secs()).map_err(|_| CacheError::InvalidInput)?;
        if namespace.is_empty() || limit == 0 || window == 0 {
            return Err(CacheError::InvalidInput);
        }
        Ok(Self {
            db,
            namespace,
            limit,
            window,
        })
    }

    /// The upsert serializes concurrent requests for one identity across processes.
    pub async fn check(&self, key: &str) -> Result<RateDecision, CacheError> {
        if key.is_empty() {
            return Err(CacheError::InvalidInput);
        }
        let (count, retry): (i64, i64) = sqlx::query_as("WITH hit AS (INSERT INTO kouga_rate_limits(namespace, key, window_start, count) VALUES ($1, $2, now(), 1) ON CONFLICT (namespace, key) DO UPDATE SET count = CASE WHEN kouga_rate_limits.window_start + $3 * interval '1 second' <= now() THEN 1 ELSE kouga_rate_limits.count + 1 END, window_start = CASE WHEN kouga_rate_limits.window_start + $3 * interval '1 second' <= now() THEN now() ELSE kouga_rate_limits.window_start END RETURNING count, window_start) SELECT count, GREATEST(1, CEIL(EXTRACT(EPOCH FROM (window_start + $3 * interval '1 second' - now())))::bigint) FROM hit")
            .bind(&self.namespace).bind(key).bind(self.window).fetch_one(&self.db).await?;
        Ok(if count <= self.limit {
            RateDecision::Allowed
        } else {
            RateDecision::Denied {
                retry_after: retry as u64,
            }
        })
    }

    pub async fn clean(&self) -> Result<u64, CacheError> {
        Ok(sqlx::query("DELETE FROM kouga_rate_limits WHERE namespace = $1 AND window_start + $2 * interval '1 second' <= now()")
            .bind(&self.namespace).bind(self.window).execute(&self.db).await?.rows_affected())
    }
}

/// The key selector may read ClientIp or a verified CurrentUser extension.
pub fn rate_limit<S, F>(limiter: RateLimiter, key: F) -> kouga_http::Middleware<S>
where
    S: Clone + Send + Sync + 'static,
    F: Fn(&kouga_http::HttpRequest<S>) -> Option<String> + Send + Sync + 'static,
{
    kouga_http::Middleware::new(move |request, next| {
        let limiter = limiter.clone();
        let identity = key(&request);
        async move {
            use axum::response::IntoResponse;
            let identity = identity.ok_or_else(|| {
                kouga_http::Error(kouga_core::Error::new(
                    kouga_core::ErrorKind::Unauthorized,
                    "rate_identity_missing",
                    "Authentication required",
                ))
            })?;
            match limiter.check(&identity).await {
                Ok(RateDecision::Allowed) => next.run(request).await,
                Ok(RateDecision::Denied { retry_after }) => {
                    let mut response = kouga_http::Error(kouga_core::Error::new(
                        kouga_core::ErrorKind::RateLimited,
                        "rate_limited",
                        "Rate limit exceeded",
                    ))
                    .into_response();
                    response.headers_mut().insert(
                        http::header::RETRY_AFTER,
                        http::HeaderValue::from_str(&retry_after.to_string())
                            .expect("integer header"),
                    );
                    Ok(response)
                }
                Err(error) => {
                    tracing::warn!(?error, "rate limit store failed");
                    Err(kouga_http::Error(kouga_core::Error::new(
                        kouga_core::ErrorKind::Unavailable,
                        "rate_limit_unavailable",
                        "Service unavailable",
                    )))
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn memory_bounds_ttl_and_namespaces() {
        let cache = MemoryCache::new(2).unwrap();
        cache
            .set("a", "k", Value::from(1), Duration::from_secs(1))
            .unwrap();
        cache
            .set("b", "k", Value::from(2), Duration::from_secs(1))
            .unwrap();
        assert_eq!(cache.get("a", "k").unwrap(), Some(Value::from(1)));
        cache
            .set("c", "k", Value::from(3), Duration::from_secs(1))
            .unwrap();
        assert_eq!(cache.get("a", "k").unwrap(), None);
        cache.delete("b", "k").unwrap();
        assert_eq!(cache.get("b", "k").unwrap(), None);
        cache
            .set("x", "k", Value::from(1), Duration::from_millis(1))
            .unwrap();
        tokio::time::sleep(Duration::from_millis(3)).await;
        assert_eq!(cache.clean().unwrap(), 1);
        assert_eq!(
            cache.fetch("miss", "k", Duration::from_secs(1), || Value::from(4)),
            Value::from(4)
        );
        assert_eq!(
            cache.fetch("miss", "k", Duration::from_secs(1), || Value::from(5)),
            Value::from(4)
        );
    }
}
