# Taskboard attachments and live events

Generate the Taskboard application as described in [README](README.md), migrate the database, and configure a private directory before starting HTTP:

```sh
mkdir -p /var/lib/taskboard/files
export BOARD_STORAGE_ROOT=/var/lib/taskboard/files
kouga server
```

`POST /tasks/{id}/attachments` accepts one multipart field named `file` (PNG, JPEG, or PDF, up to 10 MiB). The server checks the authenticated owner's Task before streaming to storage, verifies the file signature, generates an opaque storage key, and links the metadata to that Task. `GET /tasks/{id}/attachments/{file_id}` streams the private file through the authenticated API with `Content-Disposition: attachment` and `X-Content-Type-Options: nosniff`. `DELETE` at the same URL hides it immediately and removes the object. Other users receive 404. Project/Task deletion marks attached objects for cleanup in the same DB transaction.

Storage and PostgreSQL cannot commit atomically. A failed object removal leaves a `delete_pending` row, hidden from download. Schedule the one-shot `taskboard-storage-cleanup` binary with `DATABASE_URL` and the same `BOARD_STORAGE_ROOT`; it retries up to 100 objects each run and reaps abandoned uploads older than one hour. A failed attachment after upload is also hidden and cleaned. Do not share a local directory between servers without a shared filesystem. `kouga-storage` also has an S3-compatible adapter and short-lived signed URLs, but this Taskboard fixture intentionally uses the local adapter and app-proxied download; switching it to S3 requires configuration/credentials and an S3 integration check. Local filesystem failure tests do not prove S3 semantics.

Run the independent `taskboard-channel` binary with the same `DATABASE_URL`, `BOARD_CHANNEL_ORIGIN` (for example `https://app.example.com`), and optionally `BOARD_CHANNEL_BIND` (default `127.0.0.1:3001`). `BOARD_CHANNEL_AUTH_CHECK_MS` sets revoked/expired token checks, from 100 to 60000 ms, default 1000 ms. The browser obtains a single-use 30-second ticket via authenticated `POST /_kouga/ws-ticket`, then upgrades `/_kouga/ws` using `Sec-WebSocket-Protocol: kouga, kouga-ticket.<ticket>` and the configured Origin. Never put the bearer or ticket in a URL. Subscribe to `taskboard.owner.<your-user-uuid>`; the server rejects another user's channel and all client publications. Task create/update/delete publishes `task_changed` with `action` and `task_id` inside the Task transaction. PostgreSQL NOTIFY relays it to separate Channel servers. Events are best-effort; after reconnect, load current state through HTTP. A password reset revokes old bearers, blocks unconsumed old tickets, and closes open sockets on the next configured auth check.

The Taskboard HTTP image needs the local storage root, but the Channel process does not. The storage cleanup binary needs DB and the same storage root; it can run as a one-shot task.
