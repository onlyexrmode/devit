#!/usr/bin/env bash
set -euo pipefail

PRED=${1:-predictions.jsonl}
LOG_DIR=${2:-bench_logs}

pred_count=0
if [ -f "$PRED" ]; then
  pred_count=$(grep -cve '^\s*$' "$PRED" || true)
fi

apply_failed=0
solved=0
if [ -d "$LOG_DIR" ]; then
  apply_failed=$(grep -R "Apply patch failed" "$LOG_DIR" 2>/dev/null | wc -l | tr -d ' ')
  # Heuristique: certaines versions loguent "Solved" dans les rapports
  solved=$(grep -R "Solved" "$LOG_DIR" 2>/dev/null | wc -l | tr -d ' ')
fi

echo "=== DevIt SWE-bench Summary ==="
echo "Predictions file : $PRED"
echo "Log directory    : $LOG_DIR"
echo "# predictions    : $pred_count"
echo "# apply failed   : $apply_failed"
echo "# solved (grep)  : $solved"
echo "==============================="

