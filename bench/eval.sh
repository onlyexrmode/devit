#!/usr/bin/env bash
set -euo pipefail

PRED=${1:-predictions.jsonl}
RUN_ID=${2:-devit_lite_trial}
WORKERS=${3:-${WORKERS:-1}}

if ! command -v python >/dev/null; then
  echo "python is required" >&2; exit 1
fi

# Active venv si présent
if [ -f .venv/bin/activate ]; then
  source .venv/bin/activate
fi

echo "Running SWE-bench eval with $WORKERS worker(s) on $PRED (run_id=$RUN_ID)"
python -m swebench.harness.run_evaluation \
  --dataset_name princeton-nlp/SWE-bench_Lite \
  --predictions_path "$PRED" \
  --max_workers "$WORKERS" \
  --run_id "$RUN_ID"
