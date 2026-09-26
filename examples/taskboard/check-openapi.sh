#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: check-openapi.sh GENERATED_APP KOUGA_BIN" >&2
  exit 2
fi
app="$(cd "$1" && pwd)"
cli="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"
cd "$app"

"$cli" openapi generate
"$cli" openapi check
jq -e '.openapi == "3.1.1" and
  (.paths["/projects"].post != null) and
  (.paths["/tasks/{id}"].patch != null) and
  (.paths["/auth/register"].post != null) and
  (.paths["/auth/logout"].post.security[0].bearerAuth != null) and
  (.paths["/tasks/{id}/attachments"].post.requestBody.content["multipart/form-data"] != null)' openapi.yml >/dev/null

cp src/board.rs src/board.rs.t42-original
trap 'mv -f src/board.rs.t42-original src/board.rs' EXIT
sed -i.bak 's/min = 1, max = 80/min = 2, max = 80/' src/board.rs
rm src/board.rs.bak
if "$cli" openapi check; then
  echo "Request constraint change was not detected" >&2
  exit 1
fi
mv -f src/board.rs.t42-original src/board.rs
trap - EXIT

cp src/auth.rs src/auth.rs.t42-original
trap 'mv -f src/auth.rs.t42-original src/auth.rs' EXIT
sed -i.bak 's/logout_endpoint().middleware(protected.clone())/logout_endpoint()/' src/auth.rs
rm src/auth.rs.bak
if "$cli" openapi check; then
  echo "auth security change was not detected" >&2
  exit 1
fi
mv -f src/auth.rs.t42-original src/auth.rs
trap - EXIT

"$cli" openapi generate
"$cli" openapi check
