CREATE TABLE kouga_auth_tokens (
    token_hash bytea PRIMARY KEY CHECK (octet_length(token_hash) = 32),
    user_id uuid NOT NULL,
    expires_at timestamptz NOT NULL,
    revoked_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX kouga_auth_tokens_user_id_idx ON kouga_auth_tokens (user_id);
CREATE INDEX kouga_auth_tokens_expires_at_idx ON kouga_auth_tokens (expires_at);
