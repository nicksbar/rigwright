#!/usr/bin/env bash
set -euo pipefail

summary_file="${1:-coverage-summary.txt}"
minimum_percent="${2:-90}"
target_percent="${RIGWRIGHT_COVERAGE_TARGET:-90}"

if [[ ! -f "$summary_file" ]]; then
  echo "coverage summary not found: $summary_file" >&2
  exit 2
fi

read -r total_lines missed_lines < <(
  awk '
    /^TOTAL/ {
      # llvm-cov TOTAL row is: lines total/missed/percent, functions,
      # regions. The aggregate gate is intentionally based on executable
      # lines, not the final region columns.
      print $2, $3
      found = 1
      exit
    }
    END {
      if (!found) print 0, 0
    }
  ' "$summary_file"
)

if (( total_lines == 0 )); then
  echo "TOTAL line coverage row not found in $summary_file" >&2
  exit 2
fi

covered_lines=$((total_lines - missed_lines))
printf 'Overall line coverage: %d/%d (%d.%02d%%), required floor: %d%%, target: %d%%\n' \
  "$covered_lines" "$total_lines" \
  "$((covered_lines * 100 / total_lines))" \
  "$(((covered_lines * 10000 / total_lines) % 100))" \
  "$minimum_percent" "$target_percent"

(( covered_lines * 100 >= total_lines * minimum_percent ))