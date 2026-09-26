CREATE TABLE kouga_files (
    id uuid PRIMARY KEY,
    owner_id uuid NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    storage_key text NOT NULL UNIQUE,
    original_name text NOT NULL,
    content_type text NOT NULL,
    byte_size bigint CHECK (byte_size > 0),
    record_type text,
    record_id uuid,
    state text NOT NULL DEFAULT 'uploading',
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT kouga_files_state_check CHECK (state IN ('uploading','pending','attached','delete_pending')),
    CONSTRAINT kouga_files_record_pair_check CHECK ((record_type IS NULL) = (record_id IS NULL)),
    CONSTRAINT kouga_files_attached_record_check CHECK (state <> 'attached' OR record_id IS NOT NULL),
    CONSTRAINT kouga_files_pending_size_check CHECK (state NOT IN ('pending','attached') OR byte_size IS NOT NULL)
);
CREATE INDEX kouga_files_cleanup_idx ON kouga_files (created_at) WHERE state IN ('uploading','pending','delete_pending');
CREATE INDEX kouga_files_record_idx ON kouga_files (record_type, record_id) WHERE state='attached';

CREATE FUNCTION taskboard_attachment_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.state = 'attached' AND NEW.record_type = 'tasks' AND NOT EXISTS (
        SELECT 1 FROM tasks WHERE id = NEW.record_id AND owner_id = NEW.owner_id FOR KEY SHARE
    ) THEN
        RAISE EXCEPTION 'task attachment owner mismatch' USING ERRCODE = '23503';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER taskboard_attachment_guard BEFORE INSERT OR UPDATE ON kouga_files
    FOR EACH ROW EXECUTE FUNCTION taskboard_attachment_guard();

CREATE TABLE kouga_channel_tickets (
    ticket_hash bytea PRIMARY KEY CHECK (octet_length(ticket_hash) = 32),
    bearer_hash bytea NOT NULL REFERENCES kouga_auth_tokens(token_hash) ON DELETE CASCADE,
    expires_at timestamptz NOT NULL
);
CREATE INDEX kouga_channel_tickets_expires_at_idx ON kouga_channel_tickets (expires_at);
CREATE INDEX kouga_channel_tickets_bearer_hash_idx ON kouga_channel_tickets (bearer_hash);
