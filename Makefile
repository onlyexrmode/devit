
.ONESHELL:
SHELL := bash

# Package/binary names for CLI (override via env if needed)
# Keep binary name `devit`; package is the CLI crate name.
TAG ?= v0.2.0-rc.2
DEVIT_PKG ?= devit-cli
DEVIT_BIN ?= devit
# Ensure cargo gets a binary NAME, not a path possibly set in env
DEVIT_BIN_NAME := $(notdir $(DEVIT_BIN))
PLUGINS_DIR ?= .devit/plugins

.PHONY: fmt fmt-check fmt-fix clippy lint test test-cli build build-release smoke ci check verify help \
        build-cli run-cli release-cli check-cli ci-cli help-cli plugin-echo-sum plugin-echo-sum-run \
        e2e-plugin lint-flags

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
	cargo test -p devit-cli --tests

test-cli:
	cargo test -p devit-cli --tests

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
	cargo build -p $(DEVIT_PKG) --release
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

# Lint flags (kebab-case only + expected flags present)
lint-flags:
	@rg --hidden --glob '!target' --glob '!.prompt_ignore_me' -- '--[a-z]+_[a-z]+' || echo 'OK: aucun flag snake_case'
	# Vérifie la présence d'au moins un des flags attendus (tolère fallback fichier)
	@( rg --hidden --glob '!target' --glob '!.prompt_ignore_me' -- '--timeout-secs|--policy-dump|--no-audit|--max-calls-per-min|--max-json-kb|--cooldown-ms|--context-head|--head-limit|--head-ext' \
	   || rg -- '--timeout-secs|--policy-dump|--no-audit|--max-calls-per-min|--max-json-kb|--cooldown-ms|--context-head|--head-limit|--head-ext' scripts/flags_expected.txt ) >/dev/null \
	  || (echo 'WARN: flags attendus manquants'; exit 1)

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

# ===== MCP helpers =====
.PHONY: build-exp run-mcpd mcp-policy mcp-health mcp-stats e2e-mcp
build-exp:
	@cargo build -p $(DEVIT_PKG) --features experimental --bins

run-mcpd:
	@target/debug/devit-mcpd --yes --devit-bin target/debug/devit

mcp-policy:
	@target/debug/devit-mcp --cmd 'target/debug/devit-mcpd --yes --devit-bin target/debug/devit' --policy | jq

mcp-health:
	@target/debug/devit-mcp --cmd 'target/debug/devit-mcpd --yes --devit-bin target/debug/devit' --call server.health --json '{}' | jq

mcp-stats:
	@target/debug/devit-mcp --cmd 'target/debug/devit-mcpd --yes --devit-bin target/debug/devit' --call server.stats --json '{}' | jq

e2e-mcp:
	@set -e; \
	cargo build -p $(DEVIT_PKG) --features experimental --bins; \
	SRV="target/debug/devit-mcpd --yes --devit-bin target/debug/devit"; \
	( $$SRV & echo $$! > .devit/mcpd.pid ); \
	sleep 0.5; \
		target/debug/devit-mcp --cmd "$$SRV" --policy >/dev/null; \
		target/debug/devit-mcp --cmd "$$SRV" --call server.health --json '{}' >/dev/null || true; \
		target/debug/devit-mcp --cmd "$$SRV" --call server.stats --json '{}' >/dev/null || true; \
	echo '{"tool":"echo","args":{"msg":"ok"}}' | target/debug/devit-mcp --cmd "$$SRV" --call devit.tool_call --json @- >/dev/null || true; \
	kill $$(cat .devit/mcpd.pid) 2>/dev/null || true; \
	rm -f .devit/mcpd.pid; \
	echo "E2E MCP: OK"

e2e-plugin:
	@bash scripts/e2e_plugin.sh

# ===== Plugins (WASM/WASI) helpers =====

plugin-echo-sum:
	@echo "[plugin-echo-sum] ensure wasm32-wasip1 target (WASI Preview 1)"
	rustup target add wasm32-wasip1 >/dev/null 2>&1 || true
	@echo "[plugin-echo-sum] build example plugin (echo_sum)"
	PL_EX=examples/plugins/echo_sum; \
	cargo build --manifest-path $$PL_EX/Cargo.toml --target wasm32-wasip1 --release; \
	ART=$$PL_EX/target/wasm32-wasip1/release/echo_sum.wasm; \
	mkdir -p $(PLUGINS_DIR)/echo_sum; \
	cp $$ART $(PLUGINS_DIR)/echo_sum/
	@printf '%s\n' \
	  'id = "echo_sum"' \
	  'name = "Echo Sum"' \
	  'wasm = "echo_sum.wasm"' \
	  'version = "0.1.0"' \
	  'allowed_dirs = []' \
	  'env = []' \
	  > "$(PLUGINS_DIR)/echo_sum/devit-plugin.toml"
	@echo "[plugin-echo-sum] done"

plugin-echo-sum-run: plugin-echo-sum
	@echo "[plugin-echo-sum-run] invoking echo_sum with {a:1,b:2}"
	@echo '{"a":1,"b":2}' | cargo run -p $(DEVIT_PKG) --features experimental --bin devit-plugin -- invoke --id echo_sum

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
