#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: enable-grpc.sh APP_DESTINATION KOUGA_SOURCE" >&2
  exit 2
fi
destination="$1"
repo_dir="$2"
fixture_dir="$(cd "$(dirname "$0")" && pwd)"

# Keep the Protobuf contract, gRPC transport and Board package outside the
# HTTP package. Only the owner-scoped Board source is shared by path.
mkdir -p "$destination/crates" "$destination/apps" "$destination/proto"
cp -R "$fixture_dir/overlay/crates/board" "$destination/crates/board"
cp -R "$fixture_dir/overlay/crates/rpc" "$destination/crates/rpc"
cp -R "$fixture_dir/overlay/apps/grpc" "$destination/apps/grpc"
cp "$fixture_dir/overlay/proto/taskboard.proto" "$destination/proto/taskboard.proto"
sed -i.bak "s|KOUGA_SOURCE|$repo_dir|g" "$destination/crates/board/Cargo.toml"
sed -i.bak "s|KOUGA_SOURCE|$repo_dir|g" "$destination/apps/grpc/Cargo.toml"
sed -i.bak '/^members = /s/]$/, "crates\/board", "crates\/rpc", "apps\/grpc"]/' "$destination/Cargo.toml"
printf '\n[features]\ndefault = ["http"]\nhttp = []\n' >> "$destination/Cargo.toml"
rm "$destination/crates/board/Cargo.toml.bak" "$destination/apps/grpc/Cargo.toml.bak" "$destination/Cargo.toml.bak"
