# Taskboard integration fixture

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

The same generated workspace also contains `taskboard-grpc` and a Protobuf contract at `proto/taskboard.proto`. Start its separate process with `DATABASE_URL` set and `cargo run -p taskboard-grpc --bin server-grpc`; `KOUGA_GRPC_BIND` defaults to `127.0.0.1:50051`. Register over HTTP and send the token as gRPC `authorization: Bearer …` metadata. The unary Board service supports creating projects and tasks, reading and completing tasks, and reading project counts. It uses the same owner-scoped Board source as HTTP. The `taskboard-board` package compiles that source without HTTP-only endpoints, so the gRPC runtime does not depend on `kouga-http`; the HTTP package does not depend on tonic or prost. Build each binary independently with `cargo build -p taskboard --bin server` and `cargo build -p taskboard-grpc --bin server-grpc`.

The gRPC adapter authenticates metadata before field validation, then calls Board. An invalid field or UUID maps to `INVALID_ARGUMENT`, another owner's record to `NOT_FOUND`, and duplicate project slug to `FAILED_PRECONDITION`; RPC work has a five-second server deadline and 128 in-flight call cap. The client must use its own shorter deadline when needed. Protobuf decoding has a 4 MiB limit. This example does not enable gRPC reflection or grpc-web. Its gRPC server is a separate port/process, not HTTP route multiplexing.

This fixture covers specification §6 scenarios 2–4 and the T38 transport integration. Queue notifications, attachment and WebSocket are added by T36–T37; their cross-transport verification is pending until those branches are integrated. The checked-in overlay is the source of truth; do not edit a generated temporary app expecting changes to persist.
