#!/usr/bin/env bash
set -euo pipefail

PRED=${1:-predictions.jsonl}
RUN_ID=${2:-devit_lite_smoke}
WORKERS=${3:-${WORKERS:-1}}
LOG_DIR=${LOG_DIR:-bench_logs}
TESTBED=${TESTBED:-bench/testbed}
SWE_TASKS=${SWE_TASKS:-princeton-nlp/SWE-bench_Lite}
TIMEOUT=${TIMEOUT:-600}
IMAGE=${IMAGE:-devit-swebench:1.1.2}
REBUILD=${REBUILD:-0}

if ! command -v docker >/dev/null; then
  echo "docker is required" >&2; exit 1
fi

cd "$(dirname "$0")"

if ! docker image inspect "$IMAGE" >/dev/null 2>&1 || [ "$REBUILD" = "1" ]; then
  echo "[eval_docker] Building image $IMAGE (REBUILD=$REBUILD) ..."
  docker build --pull $([ "$REBUILD" = "1" ] && echo "--no-cache") -t "$IMAGE" -f Dockerfile.swebench .
fi

mkdir -p "$LOG_DIR" "$TESTBED"

echo "[eval_docker] Running harness in Docker: image=$IMAGE, workers=$WORKERS, pred=$PRED, run_id=$RUN_ID"
docker run --rm \
  -v "$(pwd)":/work/bench \
  "$IMAGE" \
  --swe_bench_tasks "$SWE_TASKS" \
  --predictions_path "/work/bench/$PRED" \
  --testbed "/work/bench/$TESTBED" \
  --log_dir "/work/bench/$LOG_DIR" \
  --num_processes "$WORKERS" \
  --timeout "$TIMEOUT" \
  --log_suffix "$RUN_ID"
