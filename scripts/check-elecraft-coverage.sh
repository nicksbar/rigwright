#!/usr/bin/env bash
set -euo pipefail

summary_file="${1:-coverage-summary.txt}"

if [[ ! -f "$summary_file" ]]; then
  echo "coverage summary not found: $summary_file" >&2
  exit 2
fi

read -r total_lines missed_lines < <(
  awk '
    $1 ~ /^elecraft\// && $8 ~ /^[0-9]+$/ && $9 ~ /^[0-9]+$/ {
      total += $8
      missed += $9
    }
    END { printf "%d %d\n", total, missed }
  ' "$summary_file"
)

if (( total_lines == 0 )); then
  echo "no Elecraft source rows found in $summary_file" >&2
  exit 2
fi

covered_lines=$((total_lines - missed_lines))
threshold_percent=84
printf 'Elecraft line coverage: %d/%d (%d.%02d%%), required: %d%%\n' \
  "$covered_lines" "$total_lines" \
  "$((covered_lines * 100 / total_lines))" \
  "$(((covered_lines * 10000 / total_lines) % 100))" \
  "$threshold_percent"

(( covered_lines * 100 >= total_lines * threshold_percent ))
