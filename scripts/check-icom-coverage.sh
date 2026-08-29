#!/usr/bin/env bash
set -euo pipefail

summary_file="${1:-coverage-summary.txt}"

if [[ ! -f "$summary_file" ]]; then
  echo "coverage summary not found: $summary_file" >&2
  exit 2
fi

check_target() {
  local label="$1"
  local pattern="$2"
  local threshold_percent="$3"
  local total_lines missed_lines

  read -r total_lines missed_lines < <(
    awk -v pattern="$pattern" '
      $1 ~ pattern && $8 ~ /^[0-9]+$/ && $9 ~ /^[0-9]+$/ {
        total += $8
        missed += $9
      }
      END { printf "%d %d\n", total, missed }
    ' "$summary_file"
  )

  if (( total_lines == 0 )); then
    echo "no $label source rows found in $summary_file" >&2
    return 2
  fi

  local covered_lines=$((total_lines - missed_lines))
  printf '%s line coverage: %d/%d (%d.%02d%%), required: %d%%\n' \
    "$label" "$covered_lines" "$total_lines" \
    "$((covered_lines * 100 / total_lines))" \
    "$(((covered_lines * 10000 / total_lines) % 100))" \
    "$threshold_percent"

  (( covered_lines * 100 >= total_lines * threshold_percent ))
}

check_target "Icom" '^icom/' 85
check_target "HAL" '^hal\.rs$' 90
check_target "Android" '^android\.rs$' 80
check_target "Transport" '^transport\.rs$' 90
check_target "Drivers" '^drivers\.rs$' 85
check_target "IQ" '^iq\.rs$' 95
check_target "rigctld" '^rigctld\.rs$' 90
check_target "DX Lab" '^dxlab\.rs$' 90
check_target "Kenwood profile" '^kenwood/profile\.rs$' 90
