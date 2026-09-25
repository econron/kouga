CREATE TABLE kouga_files (
    id uuid PRIMARY KEY,
    owner_id uuid NOT NULL,
    storage_key text NOT NULL UNIQUE,
    original_name text NOT NULL,
    content_type text NOT NULL,
    byte_size bigint CHECK (byte_size > 0),
    record_type text,
    record_id uuid,
    state text NOT NULL DEFAULT 'uploading' CHECK (state IN ('uploading', 'pending', 'attached', 'delete_pending')),
    created_at timestamptz NOT NULL DEFAULT now(),
    CHECK ((record_type IS NULL) = (record_id IS NULL)),
    CHECK (state <> 'attached' OR record_id IS NOT NULL),
    CHECK (state NOT IN ('pending', 'attached') OR byte_size IS NOT NULL)
);
CREATE INDEX kouga_files_cleanup_idx ON kouga_files (created_at) WHERE state IN ('uploading', 'pending', 'delete_pending');
