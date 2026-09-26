#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: generate.sh DESTINATION" >&2
  exit 2
fi

fixture_dir="$(cd "$(dirname "$0")" && pwd)"
repo_dir="$(cd "$fixture_dir/../.." && pwd)"
destination="$1"
if [[ -e "$destination" ]]; then
  echo "destination already exists: $destination" >&2
  exit 2
fi

if [[ -n "${KOUGA_BIN:-}" ]]; then
  cli="$(command -v "$KOUGA_BIN")"
else
  cargo +1.94.0 build --manifest-path "$repo_dir/Cargo.toml" -p kouga-cli --locked --offline
  cli="${CARGO_TARGET_DIR:-$repo_dir/target}/debug/kouga"
fi
cli="$(cd "$(dirname "$cli")" && pwd)/$(basename "$cli")"

"$cli" new taskboard --path "$destination"
destination="$(cd "$destination" && pwd)"
(cd "$destination" && "$cli" generate auth)
cp "$fixture_dir/overlay/crates/contracts/src/task_notice.rs" "$destination/crates/contracts/src/task_notice.rs"
cp "$fixture_dir/overlay/apps/worker/src/bin/task-notice-worker.rs" "$destination/apps/worker/src/bin/task-notice-worker.rs"
cp "$fixture_dir/overlay/migrations/20990101000001_task_notices.up.sql" "$destination/migrations/20990101000001_task_notices.up.sql"
cp "$fixture_dir/overlay/migrations/20990101000001_task_notices.down.sql" "$destination/migrations/20990101000001_task_notices.down.sql"
cp "$fixture_dir/overlay/src/board.rs" "$destination/src/board.rs"
cp "$fixture_dir/overlay/src/attachments.rs" "$destination/src/attachments.rs"
cp "$fixture_dir/overlay/src/realtime.rs" "$destination/src/realtime.rs"
cp "$fixture_dir/overlay/src/bin/task-complete.rs" "$destination/src/bin/task-complete.rs"
cp "$fixture_dir/overlay/src/bin/taskboard-channel.rs" "$destination/src/bin/taskboard-channel.rs"
cp "$fixture_dir/overlay/src/bin/taskboard-storage-cleanup.rs" "$destination/src/bin/taskboard-storage-cleanup.rs"
cp "$fixture_dir/overlay/tests/taskboard.rs" "$destination/tests/taskboard.rs"
cp "$fixture_dir/overlay/tests/attachment_channel.rs" "$destination/tests/attachment_channel.rs"
cp "$fixture_dir/overlay/tests/task_notice.rs" "$destination/tests/task_notice.rs"
mkdir -p "$destination/apps/worker/tests"
cp "$fixture_dir/overlay/apps/worker/tests/task_notice.rs" "$destination/apps/worker/tests/task_notice.rs"
cp "$fixture_dir/overlay/migrations/20990101000000_taskboard.up.sql" "$destination/migrations/20990101000000_taskboard.up.sql"
cp "$fixture_dir/overlay/migrations/20990101000000_taskboard.down.sql" "$destination/migrations/20990101000000_taskboard.down.sql"
cp "$fixture_dir/overlay/migrations/20990101000002_taskboard_attachments.up.sql" "$destination/migrations/20990101000002_taskboard_attachments.up.sql"
cp "$fixture_dir/overlay/migrations/20990101000002_taskboard_attachments.down.sql" "$destination/migrations/20990101000002_taskboard_attachments.down.sql"
bash "$fixture_dir/enable-grpc.sh" "$destination" "$repo_dir"
sed -i.bak '1i\
pub mod board;\
pub mod attachments;\
pub mod realtime;\
' "$destination/src/lib.rs"
sed -i.bak '$a\
pub mod task_notice;\
' "$destination/crates/contracts/src/lib.rs"
sed -i.bak 's/^    router$/    attachments::routes(board::routes(router))/' "$destination/src/lib.rs"
sed -i.bak 's/let router = Router::new()/let router = Router::new().configure(kouga_http::HttpOptions { max_body_bytes: 11 * 1024 * 1024, ..Default::default() }).expect("taskboard limits")/' "$destination/src/lib.rs"
sed -i.bak '/\[dependencies\]/a\
tracing = "=0.1.44"\
futures-util = "=0.3.34"\
kouga-storage = { path = "'"$repo_dir"'/crates/kouga-storage" }\
kouga-channel = { path = "'"$repo_dir"'/crates/kouga-channel" }\
' "$destination/Cargo.toml"
rm "$destination/src/lib.rs.bak" "$destination/crates/contracts/src/lib.rs.bak" "$destination/Cargo.toml.bak"
sed -i.bak '/\[dev-dependencies\]/a\
tokio-tungstenite = "=0.29.0"\
' "$destination/Cargo.toml"
rm "$destination/Cargo.toml.bak"
cargo +1.94.0 fmt --manifest-path "$destination/Cargo.toml" --all
cargo +1.94.0 generate-lockfile --manifest-path "$destination/Cargo.toml" --offline
(cd "$destination" && "$cli" dockerfile)
echo "Generated $destination"
