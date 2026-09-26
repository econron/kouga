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
cp "$fixture_dir/overlay/src/bin/task-complete.rs" "$destination/src/bin/task-complete.rs"
cp "$fixture_dir/overlay/tests/taskboard.rs" "$destination/tests/taskboard.rs"
cp "$fixture_dir/overlay/tests/task_notice.rs" "$destination/tests/task_notice.rs"
mkdir -p "$destination/apps/worker/tests"
cp "$fixture_dir/overlay/apps/worker/tests/task_notice.rs" "$destination/apps/worker/tests/task_notice.rs"
cp "$fixture_dir/overlay/migrations/20990101000000_taskboard.up.sql" "$destination/migrations/20990101000000_taskboard.up.sql"
cp "$fixture_dir/overlay/migrations/20990101000000_taskboard.down.sql" "$destination/migrations/20990101000000_taskboard.down.sql"
bash "$fixture_dir/enable-grpc.sh" "$destination" "$repo_dir"
sed -i.bak '1i\
pub mod board;\
' "$destination/src/lib.rs"
sed -i.bak '$a\
pub mod task_notice;\
' "$destination/crates/contracts/src/lib.rs"
sed -i.bak 's/^    router$/    board::routes(router)/' "$destination/src/lib.rs"
sed -i.bak '/\[dependencies\]/a\
tracing = "=0.1.44"\
' "$destination/Cargo.toml"
rm "$destination/src/lib.rs.bak" "$destination/crates/contracts/src/lib.rs.bak" "$destination/Cargo.toml.bak"
cargo +1.94.0 fmt --manifest-path "$destination/Cargo.toml" --all
cargo +1.94.0 generate-lockfile --manifest-path "$destination/Cargo.toml" --offline
echo "Generated $destination"
