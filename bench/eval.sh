#!/usr/bin/env bash
set -euo pipefail

PRED=${1:-predictions.jsonl}
RUN_ID=${2:-devit_lite_trial}
WORKERS=${3:-${WORKERS:-1}}
LOG_DIR=${LOG_DIR:-bench_logs}
SWE_TASKS=${SWE_TASKS:-princeton-nlp/SWE-bench_Lite}
TESTBED=${TESTBED:-local}
TIMEOUT=${TIMEOUT:-600}

if ! command -v python >/dev/null; then
  echo "python is required" >&2; exit 1
fi

# Active venv si présent
if [ -f .venv/bin/activate ]; then
  source .venv/bin/activate
fi

echo "Running SWE-bench eval: tasks=$SWE_TASKS testbed=$TESTBED workers=$WORKERS pred=$PRED run_id=$RUN_ID"
mkdir -p "$LOG_DIR"
python -m swebench.harness.run_evaluation \
  --swe_bench_tasks "$SWE_TASKS" \
  --predictions_path "$PRED" \
  --testbed "$TESTBED" \
  --log_dir "$LOG_DIR" \
  --num_processes "$WORKERS" \
  --timeout "$TIMEOUT" \
  --log_suffix "$RUN_ID"
