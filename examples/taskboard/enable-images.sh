#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: enable-images.sh APP_DESTINATION KOUGA_SOURCE" >&2
  exit 2
fi

destination="$1"
repo_dir="$2"
fixture_dir="$(cd "$(dirname "$0")" && pwd)"
cp -R "$fixture_dir/overlay/apps/channel" "$destination/apps/channel"
cp -R "$fixture_dir/overlay/apps/storage-cleanup" "$destination/apps/storage-cleanup"
sed -i.bak "s|KOUGA_SOURCE|$repo_dir|g" "$destination/apps/channel/Cargo.toml" "$destination/apps/storage-cleanup/Cargo.toml"
sed -i.bak '/^members = /s/]$/, "apps\/channel", "apps\/storage-cleanup"]/' "$destination/Cargo.toml"
rm "$destination/apps/channel/Cargo.toml.bak" "$destination/apps/storage-cleanup/Cargo.toml.bak" "$destination/Cargo.toml.bak"
