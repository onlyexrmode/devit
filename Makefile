
.ONESHELL:
SHELL := bash

# Package/binary names for CLI (override via env if needed)
# Keep binary name `devit`; package is the CLI crate name.
TAG ?= v0.2.0-rc.1
DEVIT_PKG ?= devit-cli
DEVIT_BIN ?= devit
# Ensure cargo gets a binary NAME, not a path possibly set in env
DEVIT_BIN_NAME := $(notdir $(DEVIT_BIN))

.PHONY: fmt fmt-check fmt-fix clippy lint test test-cli build build-release smoke ci check verify help \
        build-cli run-cli release-cli check-cli ci-cli help-cli

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
	cargo run -p $(DEVIT_PKG) -- plan

watch:
	cargo run -p $(DEVIT_PKG) -- watch

bench-ids50:
	# Ensure Python deps
	python3 - <<-'PY' || (python3 -m venv bench/.venv && bench/.venv/bin/pip install -U pip && bench/.venv/bin/pip install -r bench/requirements.txt datasets gitpython tqdm)
	from datasets import load_dataset
	PY
	# Generate IDs
	bench/.venv/bin/python - <<-'PY'
	from datasets import load_dataset
	ds = load_dataset('princeton-nlp/SWE-bench_Lite', split='test')
	ids = ds.select(range(50))['instance_id']
	open('bench/instances_lite_50.txt','w').write('\n'.join(ids)+'\n')
	print('OK -> bench/instances_lite_50.txt:', len(ids), 'ids')
	PY

bench-smoke:
	set -e
	cargo build -p devit --release
	export DEVIT_BIN="$(PWD)/target/release/devit"
	export DEVIT_CONFIG="$(PWD)/bench/devit.bench.toml"
	export DEVIT_BACKEND_URL="http://localhost:11434/v1"
	export DEVIT_TIMEOUT_SECS=120
	python3 - <<-'PY'
	from datasets import load_dataset
	import os
	ds = load_dataset('princeton-nlp/SWE-bench_Lite', split='test')
	os.makedirs('bench', exist_ok=True)
	ids = ds.select(range(5))['instance_id']
	open('bench/instances_auto_5.txt','w').write('\n'.join(ids)+"\n")
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
	@echo "[bench-smoke] predictions.jsonl generated. To run harness: make bench-eval"

bench-eval:
	set -e
	cd bench
	LOG_DIR=${LOG_DIR:-bench_logs} TESTBED=${TESTBED:-bench/testbed} WORKERS=${WORKERS:-1} TIMEOUT=${TIMEOUT:-600} \
	  bash eval.sh predictions.jsonl ${RUN_ID:-devit_lite_smoke} $$WORKERS
	./summarize.sh predictions.jsonl $$LOG_DIR

bench-eval-docker:
	set -e
	cd bench
	LOG_DIR=${LOG_DIR:-bench_logs} TESTBED=${TESTBED:-bench/testbed} WORKERS=${WORKERS:-1} TIMEOUT=${TIMEOUT:-600} \
	  IMAGE=${IMAGE:-devit-swebench:1.1.2} \
	  bash eval_docker.sh predictions.jsonl ${RUN_ID:-devit_lite_smoke} $$WORKERS
	./summarize.sh predictions.jsonl $$LOG_DIR

check: fmt-check clippy

verify: check build test

ci: verify

.PHONY: release-draft release-publish
release-draft:
	@if ! command -v gh >/dev/null 2>&1; then \
	  echo "error: GitHub CLI 'gh' non trouvé. Installe-le puis authentifie-toi (gh auth login)"; exit 2; \
	fi
	chmod +x scripts/extract_release_notes.sh
	scripts/extract_release_notes.sh "$(TAG)" > /tmp/devit_release_notes.md
	gh release create "$(TAG)" --draft -F /tmp/devit_release_notes.md || \
	  gh release edit   "$(TAG)" --draft -F /tmp/devit_release_notes.md
	@echo "Draft créée/mise à jour pour $(TAG)"

release-publish:
	@if ! command -v gh >/dev/null 2>&1; then \
	  echo "error: GitHub CLI 'gh' non trouvé. Installe-le puis authentifie-toi (gh auth login)"; exit 2; \
	fi
	gh release edit "$(TAG)" --draft=false
	@echo "Release publiée pour $(TAG)"

# ===== CLI-focused targets (safe, no side effects) =====
build-cli:
	cargo build -p $(DEVIT_PKG) --bin $(DEVIT_BIN_NAME) --verbose

run-cli:
	cargo run -p $(DEVIT_PKG) --bin $(DEVIT_BIN_NAME) -- --help

release-cli:
	cargo build -p $(DEVIT_PKG) --bin $(DEVIT_BIN_NAME) --release --verbose

## Crée un tar.gz + sha256 en local (même format que la CI)
.PHONY: dist
dist: release-cli
	mkdir -p dist/pkg
	cp target/release/$(DEVIT_BIN_NAME) dist/pkg/
	[ -f LICENSE ] && cp LICENSE dist/pkg/ || true
	[ -f README.md ] && cp README.md dist/pkg/ || true
	tar -czf dist/$(DEVIT_BIN_NAME)-$(TAG)-linux-x86_64.tar.gz -C dist pkg
	( cd dist && sha256sum $(DEVIT_BIN_NAME)-$(TAG)-linux-x86_64.tar.gz > $(DEVIT_BIN_NAME)-$(TAG)-linux-x86_64.sha256 )
	@ls -lah dist && echo "SHA256:" && cat dist/$(DEVIT_BIN_NAME)-$(TAG)-linux-x86_64.sha256

check-cli:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets -- -D warnings
	cargo test --workspace --all-targets --no-fail-fast -- --nocapture

ci-cli: check-cli build-cli

help-cli:
	@echo "build-cli      : build $(DEVIT_BIN) from $(DEVIT_PKG)"
	@echo "release-cli    : build release of $(DEVIT_BIN)"
	@echo "run-cli        : run $(DEVIT_BIN) --help"
	@echo "check-cli      : fmt + clippy -D warnings + tests"
	@echo "ci-cli         : check-cli + build-cli"
	@echo "dist           : package tar.gz + sha256 (local)"

# Generic IDs generator: N defaults to 50 (usage: make bench-ids N=50)
bench-ids:
	set -e
	N=${N:-50}
	# ensure venv & deps
	if [ ! -x bench/.venv/bin/python ]; then \
	  python3 -m venv bench/.venv; \
	  bench/.venv/bin/pip install -U pip; \
	  bench/.venv/bin/pip install -r bench/requirements.txt datasets gitpython tqdm; \
	fi
	# generate ids
	bench/.venv/bin/python - <<-'PY'
	import os
	from datasets import load_dataset
	N = int(os.environ.get('N','50'))
	ds = load_dataset('princeton-nlp/SWE-bench_Lite', split='test')
	ids = ds.select(range(min(N, len(ds))))['instance_id']
	path = f'bench/instances_lite_{N}.txt'
	open(path,'w').write('\n'.join(ids)+'\n')
	print('OK ->', path, ':', len(ids), 'ids')
	PY
