# Taskboard attachments and live events

Generate the Taskboard application as described in [README](README.md), migrate the database, and configure a private directory before starting HTTP:

```sh
mkdir -p /var/lib/taskboard/files
export BOARD_STORAGE_ROOT=/var/lib/taskboard/files
kouga server
```

`POST /tasks/{id}/attachments` accepts one multipart field named `file` (PNG, JPEG, or PDF, up to 10 MiB). The server checks the authenticated owner's Task before streaming to storage, verifies the file signature, generates an opaque storage key, and links the metadata to that Task. `GET /tasks/{id}/attachments/{file_id}` streams the private file through the authenticated API with `Content-Disposition: attachment` and `X-Content-Type-Options: nosniff`. `DELETE` at the same URL hides it immediately and removes the object. Other users receive 404. Project/Task deletion marks attached objects for cleanup in the same DB transaction.

Storage and PostgreSQL cannot commit atomically. A failed object removal leaves a `delete_pending` row, hidden from download. Schedule the one-shot `taskboard-storage-cleanup` binary with `DATABASE_URL` and the same storage configuration; it retries up to 100 objects each run and reaps abandoned uploads older than one hour. A failed attachment after upload is also hidden and cleaned.

`BOARD_STORAGE_BACKEND=local` (the default) requires `BOARD_STORAGE_ROOT` on both HTTP and cleanup processes. A shared volume is required if these processes run on different hosts. For S3-compatible storage, set `BOARD_STORAGE_BACKEND=s3`, `BOARD_S3_BUCKET`, and `BOARD_S3_REGION` on both processes. `BOARD_S3_ENDPOINT` is optional for AWS S3 and required for a custom endpoint. Credentials use the object-store AWS environment or instance-role chain (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN`, and related standard variables); never bake them into an image. `BOARD_S3_ALLOW_HTTP=1` is only for local non-production tests and is rejected in production. S3 downloads still stream through the authenticated Taskboard API. The adapter supports short-lived signed URLs, but this fixture deliberately does not expose them, so a private object URL cannot bypass its Task ownership check. Use a private bucket and TLS in production.

Run the independent `taskboard-channel` binary with the same `DATABASE_URL`, `BOARD_CHANNEL_ORIGIN` (for example `https://app.example.com`), and optionally `BOARD_CHANNEL_BIND` (default `127.0.0.1:3001`). `BOARD_CHANNEL_AUTH_CHECK_MS` sets revoked/expired token checks, from 100 to 60000 ms, default 1000 ms. The browser obtains a single-use 30-second ticket via authenticated `POST /_kouga/ws-ticket`, then upgrades `/_kouga/ws` using `Sec-WebSocket-Protocol: kouga, kouga-ticket.<ticket>` and the configured Origin. Never put the bearer or ticket in a URL. Subscribe to `taskboard.owner.<your-user-uuid>`; the server rejects another user's channel and all client publications. Task create/update/delete publishes `task_changed` with `action` and `task_id` inside the Task transaction. PostgreSQL NOTIFY relays it to separate Channel servers. Events are best-effort; after reconnect, load current state through HTTP. A password reset revokes old bearers, blocks unconsumed old tickets, and closes open sockets on the next configured auth check.

With local storage, the Taskboard HTTP image needs the local storage root, but the Channel process does not. The storage cleanup binary needs DB and the same storage root; it can run as a one-shot task.

The generated Dockerfile provides separate `taskboard-channel` and `taskboard-storage-cleanup` targets. In the Channel image, leave `BOARD_CHANNEL_BIND` unset to listen on `0.0.0.0:$PORT` (default 3001), or set an explicit bind address. Mount the same persistent storage volume read-write into HTTP and cleanup images while keeping their root filesystems read-only. Local storage is not suitable for independent hosts without a shared filesystem. With S3, no storage volume is required; both roles need network access to the same private bucket.
