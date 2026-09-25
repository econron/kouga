CREATE TABLE kouga_cache (
    namespace TEXT NOT NULL,
    key TEXT NOT NULL,
    value JSONB NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (namespace, key)
);
CREATE INDEX kouga_cache_expires_at_idx ON kouga_cache (expires_at);

CREATE TABLE kouga_rate_limits (
    namespace TEXT NOT NULL,
    key TEXT NOT NULL,
    window_start TIMESTAMPTZ NOT NULL,
    count BIGINT NOT NULL,
    PRIMARY KEY (namespace, key)
);
CREATE INDEX kouga_rate_limits_window_start_idx ON kouga_rate_limits (window_start);
