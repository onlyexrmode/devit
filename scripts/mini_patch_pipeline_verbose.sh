#!/usr/bin/env bash
set -euo pipefail

# -----------------------------------------------------------------------------
# mini_patch_pipeline_verbose.sh
#   - Branche jetable
#   - Micro-modif .rs "safe"
#   - Pré-commit (devit fs_patch_apply --precommit-only | cargo fmt/clippy)
#   - Tests impactés (devit test impacted | cargo test)
#   - Génère .devit/reports/{precommit.out,impacted.out,junit.xml,sarif.json,quality.json,summary.md}
#   - Nettoyage (reset + delete branch)
#   - Résumé console final
# -----------------------------------------------------------------------------

DEVIT_BIN="${DEVIT_BIN:-}"
if [[ -z "${DEVIT_BIN}" ]]; then
  if [[ -x target/debug/devit ]]; then DEVIT_BIN="target/debug/devit"; else DEVIT_BIN="devit"; fi
fi
TIMEOUT_SECS="${TIMEOUT_SECS:-300}"
REPORT_DIR=".devit/reports"
mkdir -p "$REPORT_DIR"

# --- Préconditions Git -------------------------------------------------------
if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "error: pas un repo git" >&2; exit 2
fi
if ! git diff --quiet || [[ -n "$(git status --porcelain)" ]]; then
  echo "error: working tree non clean. Commit/Stash d'abord." >&2; exit 2
fi

CURR_BRANCH="$(git rev-parse --abbrev-ref HEAD)"
STAMP="$(date -u +%Y%m%d-%H%M%S)"
BRANCH="test/mini-pipeline-verbose-${STAMP}"

# --- Sélection d'un fichier Rust à modifier ----------------------------------
pick_rs() {
  local f
  f="$(git ls-files '*/src/*.rs' | grep -vE '(^|/)(target|.devit)(/|$)' | head -n1 || true)"
  if [[ -z "$f" ]]; then
    f="$(git ls-files '*.rs' | grep -vE '(^|/)(target|.devit)(/|$)' | head -n1 || true)"
  fi
  echo "$f"
}
FILE="$(pick_rs)"
if [[ -z "$FILE" ]]; then
  echo "error: aucun fichier .rs trouvé." >&2; exit 2
fi

# --- Branche jetable et modif ------------------------------------------------
git switch -c "$BRANCH"
TS="$(date -u +%FT%TZ)"
printf "\n// devit mini pipeline verbose %s\n" "$TS" >> "$FILE"
git add "$FILE"

# --- Pré-commit --------------------------------------------------------------
echo "==> Pré-commit…"
FAIL_PRECOMMIT=0
if "$DEVIT_BIN" fs_patch_apply --help 2>/dev/null | grep -q -- '--precommit-only'; then
  if ! "$DEVIT_BIN" fs_patch_apply --precommit-only | tee "$REPORT_DIR/precommit.out"; then
    FAIL_PRECOMMIT=1
  fi
else
  # Fallback Rust
  { cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings; } | tee "$REPORT_DIR/precommit.out" || FAIL_PRECOMMIT=1
fi

# --- Tests impactés ----------------------------------------------------------
echo "==> Tests impactés…"
FAIL_TESTS=0
if "$DEVIT_BIN" test impacted --help 2>/dev/null | grep -q impacted; then
  if ! DEVIT_TIMEOUT_SECS="$TIMEOUT_SECS" "$DEVIT_BIN" test impacted --changed-from HEAD --timeout-secs "$TIMEOUT_SECS" | tee "$REPORT_DIR/impacted.out"; then
    FAIL_TESTS=1
  fi
else
  # Fallback cargo test
  if ! DEVIT_TIMEOUT_SECS="$TIMEOUT_SECS" cargo test --workspace --all-targets --no-fail-fast -- --nocapture | tee "$REPORT_DIR/impacted.out"; then
    FAIL_TESTS=1
  fi
fi

# -- Artefacts min si manquants -----------------------------------------------
[ -f "$REPORT_DIR/sarif.json" ] || echo '{"runs":[]}' > "$REPORT_DIR/sarif.json"
if [ ! -f "$REPORT_DIR/junit.xml" ]; then
  printf '%s\n' \
    '<?xml version="1.0" encoding="UTF-8"?>' \
    '<testsuites><testsuite name="mini" tests="0" failures="0"/></testsuites>' > "$REPORT_DIR/junit.xml"
fi

# --- Gate qualité + summary --------------------------------------------------
echo "==> Quality gate…"
if "$DEVIT_BIN" quality gate --help >/dev/null 2>&1; then
  "$DEVIT_BIN" quality gate --junit "$REPORT_DIR/junit.xml" --sarif "$REPORT_DIR/sarif.json" --json \
  | jq 'if has("payload") then .payload else . end' \
  | tee "$REPORT_DIR/quality.json" >/dev/null
else
  # fallback: synthèse minimaliste
  echo '{"pass":true,"summary":{"tests_total":0,"tests_failed":0,"lint_errors":0,"lint_warnings":0}}' > "$REPORT_DIR/quality.json"
fi

if "$DEVIT_BIN" report summary --help >/dev/null 2>&1; then
  "$DEVIT_BIN" report summary --junit "$REPORT_DIR/junit.xml" --sarif "$REPORT_DIR/sarif.json" --out "$REPORT_DIR/summary.md"
else
  printf "DevIt Summary\n\n- Pré-commit: %s\n- Tests impactés: %s\n" \
    "$( [ $FAIL_PRECOMMIT -eq 0 ] && echo OK || echo FAIL )" \
    "$( [ $FAIL_TESTS -eq 0 ] && echo OK || echo FAIL )" > "$REPORT_DIR/summary.md"
fi

# --- Nettoyage ---------------------------------------------------------------
git reset --hard HEAD
git switch "$CURR_BRANCH"
git branch -D "$BRANCH" >/dev/null 2>&1 || true

# --- Résumé console ----------------------------------------------------------
PASS_OVERALL=0
if command -v jq >/dev/null 2>&1; then
  jq -e '.pass == true' "$REPORT_DIR/quality.json" >/dev/null 2>&1 || PASS_OVERALL=1
else
  # Si jq absent, on considère pass si pré-commit & tests OK
  if [[ $FAIL_PRECOMMIT -ne 0 || $FAIL_TESTS -ne 0 ]]; then PASS_OVERALL=1; fi
fi

echo "------------------------------------------------------------"
echo "Pré-commit : $([[ $FAIL_PRECOMMIT -eq 0 ]] && echo OK || echo FAIL)"
echo "Tests       : $([[ $FAIL_TESTS -eq 0 ]] && echo OK || echo FAIL)"
echo "Gate        : $([[ $PASS_OVERALL -eq 0 ]] && echo PASS || echo FAIL)"
echo "Artefacts   : $REPORT_DIR/{precommit.out,impacted.out,junit.xml,sarif.json,quality.json,summary.md}"
if [[ $FAIL_PRECOMMIT -eq 0 && $FAIL_TESTS -eq 0 && $PASS_OVERALL -eq 0 ]]; then
  echo "✅ mini pipeline verbose: OK"
  exit 0
else
  echo "❌ mini pipeline verbose: échec (voir artefacts)."
  exit 1
fi
