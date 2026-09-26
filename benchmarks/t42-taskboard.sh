#!/usr/bin/env bash
set -euo pipefail

# Disposable, migrated Taskboard database only. Four VUs, 15 seconds, two runs per mode.
base_url="${BASE_URL:-http://127.0.0.1:18093}"
for mode in json read crud; do
  for repetition in 1 2; do
    email="t42-bench-$(date +%s)-$$-$mode-$repetition@example.invalid"
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
    summary="/private/tmp/kouga-t42-$mode-$repetition-summary.json"
    BASE_URL="$base_url" MODE="$mode" TOKEN="$token" PROJECT_ID="$project_id" TASK_ID="$task_id" \
      k6 run --vus 4 --duration 15s \
        --summary-trend-stats 'avg,min,med,max,p(90),p(95),p(99)' \
        --summary-export "$summary" \
        "$(dirname "$0")/t40-taskboard-http.js"
    echo "$mode repetition=$repetition summary=$summary"
  done
done
