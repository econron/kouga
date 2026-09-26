#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: package-source.sh APP_DIRECTORY KOUGA_CHECKOUT" >&2
  exit 2
fi

app="$(cd "$1" && pwd)"
source="$(cd "$2" && pwd)"
if [[ ! -f "$app/Cargo.toml" || ! -f "$app/Dockerfile" ]]; then
  echo "expected a generated Kouga application with Dockerfile" >&2
  exit 2
fi
if [[ -e "$app/vendor/kouga" ]]; then
  echo "vendor/kouga already exists; preserve the existing snapshot" >&2
  exit 2
fi
if [[ -n "$(git -C "$source" status --porcelain --untracked-files=no)" ]]; then
  echo "commit Kouga changes before packaging a reproducible source revision" >&2
  exit 2
fi
if ! grep -q '^\[workspace\]$' "$app/Cargo.toml" || grep -q '^exclude[[:space:]]*=' "$app/Cargo.toml"; then
  echo "expected a generated workspace without a custom exclude list" >&2
  exit 2
fi

revision="$(git -C "$source" rev-parse HEAD)"
mkdir -p "$app/vendor/kouga"
git -C "$source" archive "$revision" Cargo.toml Cargo.lock crates | tar -x -C "$app/vendor/kouga"
printf '%s\n' "$revision" > "$app/vendor/kouga-revision.txt"

# Rewrite only generated Kouga path dependencies. The vendored workspace itself
# retains its original relative dependencies and is not changed.
for manifest in "$app/Cargo.toml" "$app"/apps/*/Cargo.toml "$app"/crates/*/Cargo.toml; do
  [[ -f "$manifest" ]] || continue
  if [[ "$manifest" == "$app/Cargo.toml" ]]; then
    prefix='vendor/kouga'
  else
    prefix='../../vendor/kouga'
  fi
  sed -i.bak -E "s|path = \"[^\"]*/crates/(kouga-[^\"]+)\"|path = \"$prefix/crates/\\1\"|g" "$manifest"
  rm "$manifest.bak"
done
sed -i.bak '/^\[workspace\]$/a\
exclude = ["vendor/kouga"]
' "$app/Cargo.toml"
rm "$app/Cargo.toml.bak"

echo "Packaged Kouga $revision in $app/vendor/kouga"
echo "Build images with: docker build --build-context kouga=./vendor/kouga --target http ."
