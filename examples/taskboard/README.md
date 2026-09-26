# Taskboard integration fixture

Attachment and live-event usage is described in [ATTACHMENTS_AND_CHANNELS.md](ATTACHMENTS_AND_CHANNELS.md).

This is one regenerable Kouga application, not a second framework implementation. It extends `kouga new taskboard` and `kouga generate auth` with owner-scoped Project/Task operations. No generated absolute local dependency path is checked in.

```sh
# From the Kouga checkout; choose a new directory each time.
CARGO_TARGET_DIR="$PWD/target" bash examples/taskboard/generate.sh /tmp/my-taskboard
cd /tmp/my-taskboard
export DATABASE_URL='postgres://postgres:password@localhost:5432/taskboard_dev'
kouga db create
kouga db migrate
kouga server
```

`KOUGA_BIN` can point to an already-built Kouga executable. The generator refuses an existing destination. Generated `Cargo.toml` uses local paths to the checkout, so keep that checkout available; re-run the script from another checkout to relocate the app. Set a separate `TEST_DATABASE_URL` and run `cargo +1.94.0 test --workspace --locked`; `kouga-test` creates and removes a schema per test. Do not use a production database for tests.

Register via `POST /auth/register` and pass the returned token as `Authorization: Bearer …`. The protected API is `GET/POST /projects`, `GET/PATCH/DELETE /projects/{id}`, `GET /projects/{id}/count`, `GET/POST /tasks`, and `GET/PATCH/DELETE /tasks/{id}`. Project creation accepts `slug` and `name`; task creation accepts `project_id` and `title`. Listings accept `page` and `per_page`, defaulting to 1 and 20. The task list preloads its project name and returns `meta.has_next`.

The domain `taskboard::board::Board` receives the authenticated actor ID on every operation. Generated low-level model CRUD is private to that module. The Board is also used by the `task-complete` one-shot runner: set `BOARD_ACTOR_ID`, `BOARD_TASK_ID`, and `DATABASE_URL`, then run `kouga runner complete`. A different owner's task is not changed. Completion cannot be reversed. The request layer rejects unknown properties and invalid values before handlers; DB foreign keys, owner-matching composite FK, and unique indexes remain the final guards under concurrency. Cache invalidation for project counts is in the same database transaction as task mutation.

Task creation also persists a `taskboard.task_created` version 2 notification job in the same transaction, on the dedicated `task-mail` queue so the auth mail worker cannot claim it. Run the separate `task-notice-worker` binary with `DATABASE_URL`, `KOUGA_SMTP_HOST`, `KOUGA_SMTP_PORT`, and `KOUGA_MAIL_FROM`; production SMTP uses a verified TLS relay, while `KOUGA_ENV=test KOUGA_SMTP_LOCAL=1` permits a loopback plaintext sink without credentials. This worker and the HTTP server are separate Cargo packages. The HTTP package only depends on the typed job contract and queue enqueue API, not SMTP or worker execution.

The worker retains a version 1 handler for persisted jobs containing only `task_id`; version 2 additionally verifies `owner_id`. Deploy the new worker before or together with the producer switch, and keep version 1 registered until those jobs have drained or been deliberately migrated. A version 1 payload is not rewritten in place. Queue delivery is at least once: `task_notice_effects` uses the stable job ID as its primary key so the database effect is not repeated after a crash, but SMTP delivery can still duplicate if the process dies after SMTP accepts the message and before queue acknowledgement. Consumers must not assume exactly-once email.

The fixture tests kill a real worker after the durable effect but before acknowledgement, wait for lease expiry, and start a new worker. They also exercise SMTP retry, permanent owner mismatch, and a version 1 payload. `TASKBOARD_TEST_PAUSE_AFTER_EFFECT_MS` only takes effect in `KOUGA_ENV=test` and exists to make the crash window deterministic. Do not set it in deployment.

This fixture covers specification §6 scenarios 2–9 and the queue portion of scenario 12. [Attachment and live-event usage](ATTACHMENTS_AND_CHANNELS.md) covers scenarios 7–9. gRPC integration belongs to T38 and is not claimed here. The checked-in overlay is the source of truth; do not edit a generated temporary app expecting changes to persist.
