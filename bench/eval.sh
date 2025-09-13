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

python - <<'PY'
import sys, subprocess
pred = sys.argv[1]
run_id = sys.argv[2]
cmd = [
  'python','-m','swebench.harness.run_evaluation',
  '--dataset_name','princeton-nlp/SWE-bench_Lite',
  '--predictions_path', pred,
  '--max_workers', str(workers),
  '--run_id', run_id,
]
print('Running:', ' '.join(cmd))
subprocess.run(cmd, check=True)
PY
"$PRED" "$RUN_ID" "$WORKERS"
