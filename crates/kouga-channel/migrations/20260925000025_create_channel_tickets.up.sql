CREATE TABLE kouga_channel_tickets (
    ticket_hash bytea PRIMARY KEY CHECK (octet_length(ticket_hash) = 32),
    bearer_hash bytea NOT NULL REFERENCES kouga_auth_tokens(token_hash) ON DELETE CASCADE,
    expires_at timestamptz NOT NULL
);
CREATE INDEX kouga_channel_tickets_expires_at_idx ON kouga_channel_tickets (expires_at);
CREATE INDEX kouga_channel_tickets_bearer_hash_idx ON kouga_channel_tickets (bearer_hash);
