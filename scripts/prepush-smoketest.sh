#!/usr/bin/env bash
set -euo pipefail

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[0;33m'; NC='\033[0m'

echo -e "${YELLOW}DevIt pre-push smoke tests${NC}"

# Locate devit binary
if [[ -n "${DEVIT_BIN:-}" ]]; then
  BIN="${DEVIT_BIN}"
else
  if [[ -x target/debug/devit ]]; then BIN="$(pwd)/target/debug/devit"; else
    echo "Building devit..."; cargo build -p devit >/dev/null
    BIN="$(pwd)/target/debug/devit"
  fi
fi
echo "Using devit binary: $BIN"

TMP_ROOT="$(mktemp -d)"
echo "Using temp root: ${TMP_ROOT}"

pass() { echo -e "${GREEN}✔${NC} $*"; }
fail() { echo -e "${RED}✖${NC} $*"; exit 1; }

# Helper to write a devit.toml
write_cfg() {
  local dir="$1"; local approval="$2"; local sandbox="$3";
  cat >"$dir/devit.toml" <<TOML
[backend]
kind = "openai_like"
base_url = ""
model = ""
api_key = ""

[policy]
approval = "${approval}"
sandbox = "${sandbox}"

[sandbox]
cpu_limit = 1
mem_limit_mb = 64
net = "off"

[git]
conventional = true
max_staged_files = 10
TOML
}

# 1) read-only refusal (apply/test)
RO_DIR="${TMP_ROOT}/ro"
mkdir -p "$RO_DIR" && write_cfg "$RO_DIR" "untrusted" "read-only"
(
  cd "$RO_DIR"
  git init -q
  git config user.email test@example.com
  git config user.name "DevIt Test"
  echo root > README.md
  git add . && git commit -q -m "chore: init"
  echo "hello" > demo.txt
  git diff --no-index -- /dev/null demo.txt > /tmp/devit_demo.diff || true
  rm demo.txt
  set +e
  "$BIN" apply /tmp/devit_demo.diff --yes >out.txt 2>err.txt; code=$?
  set -e
  grep -q "read-only" err.txt && [[ $code -ne 0 ]] && pass "read-only apply refused" || fail "read-only apply did not refuse"
  set +e
  "$BIN" test >out.txt 2>err.txt; code=$?
  set -e
  grep -q "read-only" err.txt && [[ $code -ne 0 ]] && pass "read-only test refused" || fail "read-only test did not refuse"
)

# 2) on-request requires --yes (run)
OR_DIR="${TMP_ROOT}/onreq"
mkdir -p "$OR_DIR" && write_cfg "$OR_DIR" "on-request" "workspace-write"
(
  cd "$OR_DIR"
  set +e
  "$BIN" run --goal demo >out.txt 2>err.txt; code=$?
  set -e
  grep -q "nécessite --yes" err.txt && [[ $code -ne 0 ]] && pass "on-request run refused without --yes" || fail "on-request run did not refuse"
)

# 3) workspace-write apply path (init git repo; approve with 'y')
WW_DIR="${TMP_ROOT}/ww"
mkdir -p "$WW_DIR" && write_cfg "$WW_DIR" "untrusted" "workspace-write"
(
  cd "$WW_DIR"
  git init -q
  git config user.email test@example.com
  git config user.name "DevIt Test"
  echo "root" > README.md
  git add . && git commit -q -m "chore: init"
  echo "Hello from DevIt" > demo_added.txt
  git diff --no-index -- /dev/null demo_added.txt > /tmp/devit_demo.diff || true
  rm demo_added.txt
  printf "y\n" | "$BIN" apply /tmp/devit_demo.diff --force >out.txt 2>err.txt || fail "workspace-write apply failed"
  git log --oneline -1 | grep -q "apply patch" && pass "workspace-write apply committed" || fail "commit not found"
)

# 4) no-sandbox structured test on a minimal cargo crate
NS_DIR="${TMP_ROOT}/ns"
mkdir -p "$NS_DIR" && write_cfg "$NS_DIR" "untrusted" "workspace-write"
(
  cd "$NS_DIR"
  cargo init -q --lib --name devit_smk
  "$BIN" test >out.txt 2>err.txt || true
  grep -q "Tests PASS" out.txt && pass "no-sandbox structured tests ran" || fail "no-sandbox tests did not pass"
)

echo -e "${GREEN}All smoke tests passed.${NC}"
echo "Temp root preserved for inspection: ${TMP_ROOT}"
echo
echo "Suggested public commit (adjust emails):"
cat <<MSG
  git checkout --orphan public-main
  git reset --hard
  git add -A
  git commit -m "feat: initial public release (sprints 1–4)" \\
    -m "Co-authored-by: GPT 5 Thinking <gpt5@example.com>" \\
    -m "Co-authored-by: DevIt Agent <devit@example.com>"
  git branch -M main
  git tag v0.1.0 -m "Initial public release"
  git push -u origin main --tags
MSG
