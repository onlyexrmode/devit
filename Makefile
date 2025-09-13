.PHONY: fmt fmt-check fmt-fix clippy lint test test-cli build build-release smoke ci check verify help

help:
	@echo "Targets: fmt | fmt-check | fmt-fix | clippy | lint | test | test-cli | build | build-release | smoke | check | verify | ci"

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

fmt-fix: fmt

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

lint: clippy fmt-check

test:
	cargo test -p devit-common
	cargo test -p devit --tests

test-cli:
	cargo test -p devit --tests

build:
	cargo build --workspace

build-release:
	cargo build --workspace --release

smoke:
	./scripts/prepush-smoketest.sh

plan:
	cargo run -p devit -- plan

watch:
	cargo run -p devit -- watch

bench-smoke:
	set -e
	cargo build -p devit --release
	export DEVIT_BIN="$(PWD)/target/release/devit"
	export DEVIT_CONFIG="$(PWD)/bench/devit.bench.toml"
	export DEVIT_BACKEND_URL="http://localhost:11434/v1"
	export DEVIT_TIMEOUT_SECS=120
	python - <<'PY'
from datasets import load_dataset
ds = load_dataset('princeton-nlp/SWE-bench_Lite', split='test')
import os; os.makedirs('bench', exist_ok=True)
with open('bench/instances_auto_5.txt','w') as f:
    for iid in ds.select(range(5))['instance_id']:
        print(iid, file=f)
PY
	cd bench
	python generate_predictions.py \
	  --instances instances_auto_5.txt \
	  --output predictions.jsonl \
	  --workdir ./workspaces \
	  --devit-bin "$$DEVIT_BIN" \
	  --devit-config "$$DEVIT_CONFIG" \
	  --dataset princeton-nlp/SWE-bench_Lite \
	  --split test \
	  --limit 5 \
	  --allow-empty
	bash eval.sh predictions.jsonl devit_lite_smoke 1

check: fmt-check clippy

verify: check build test

.ONESHELL:
SHELL := bash
ci: verify
