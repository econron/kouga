#!/usr/bin/env bash
set -euo pipefail

# Run only against an isolated, migrated Taskboard database.
base_url="${BASE_URL:-http://127.0.0.1:18086}"
email="t40-bench-$(date +%s)-$$@example.invalid"
registration="$(curl -fsS -H 'Content-Type: application/json' \
  --data "{\"email\":\"$email\",\"password\":\"correct horse battery\"}" \
  "$base_url/auth/register")"
token="$(printf '%s' "$registration" | jq -er '.data.token')"
project="$(curl -fsS -H "Authorization: Bearer $token" \
  -H 'Content-Type: application/json' \
  --data '{"slug":"bench","name":"Benchmark"}' "$base_url/projects")"
project_id="$(printf '%s' "$project" | jq -er '.data.id')"
task="$(curl -fsS -H "Authorization: Bearer $token" \
  -H 'Content-Type: application/json' \
  --data "{\"project_id\":\"$project_id\",\"title\":\"fixed-read\"}" \
  "$base_url/tasks")"
task_id="$(printf '%s' "$task" | jq -er '.data.id')"

for mode in json read crud; do
  echo "T40 mode=$mode"
  BASE_URL="$base_url" MODE="$mode" TOKEN="$token" PROJECT_ID="$project_id" TASK_ID="$task_id" \
    k6 run --vus 4 --duration 15s \
      --summary-trend-stats 'avg,min,med,max,p(90),p(95),p(99)' \
      --summary-export "/private/tmp/kouga-t40-$mode-summary.json" \
      "$(dirname "$0")/t40-taskboard-http.js"
done
