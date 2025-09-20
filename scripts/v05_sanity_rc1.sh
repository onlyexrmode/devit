#!/usr/bin/env bash
set -euo pipefail

# v05_sanity_rc1.sh
# Sanity end-to-end pour DevIt v0.5-rc.1
#  - TUI headless (--journal-path)
#  - TUI --open-diff (patch unifié)
#  - TUI --open-log (replay minimal)
#  - server.approve E2E + shell_exec (profil safe)
#  - Recipes: list + dry-run, TUI list/run hooks
#  - VS Code: build/pack optionnels si présents
#  - CI: présence des jobs packaging
#
# Prérequis: bins buildés dans target/debug (devit, devit-mcp, devit-mcpd, devit-tui)
# Non destructif: n'écrit que sous .devit/

PASS() { printf "\033[32mPASS\033[0m %s\n" "$*"; }
WARN() { printf "\033[33mWARN\033[0m %s\n" "$*"; }
FAIL() { printf "\033[31mFAIL\033[0m %s\n" "$*"; exit 1; }
INFO() { printf "\033[36m-- %s\033[0m\n" "$*"; }

export PATH="$PWD/target/debug:$PATH"
DEVIT="${DEVIT_BIN:-target/debug/devit}"
MCP="${MCP_BIN:-target/debug/devit-mcp}"
MCPD="${MCPD_BIN:-target/debug/devit-mcpd}"
TUI="${TUI_BIN:-target/debug/devit-tui}"
REPORT_DIR=".devit/reports"
mkdir -p "$REPORT_DIR" .devit || true

have() { command -v "$1" >/dev/null 2>&1; }

# 0) Présence des binaires
for b in "$DEVIT" "$MCP" "$MCPD" "$TUI"; do
  [[ -x "$b" ]] || FAIL "binaire manquant: $b"
done
PASS "bins présents"

# 1) TUI headless (journal minimal)
INFO "TUI headless --journal-path"
touch .devit/journal.jsonl
DEVIT_TUI_HEADLESS=1 "$TUI" --journal-path .devit/journal.jsonl >/dev/null 2>&1 && PASS "TUI headless OK" || FAIL "TUI headless KO"

# 2) TUI --open-diff
INFO "TUI --open-diff"
DIFF="$REPORT_DIR/sample.diff"
cat >"$DIFF" <<'EOF'
diff --git a/README.md b/README.md
index 111..222 100644
--- a/README.md
+++ b/README.md
@@
-Old
+New
EOF
DEVIT_TUI_HEADLESS=1 "$TUI" --open-diff "$DIFF" >/dev/null 2>&1 && PASS "TUI open-diff OK" || FAIL "TUI open-diff KO"

# 3) TUI --open-log
INFO "TUI --open-log"
echo '{"ts":"2024-01-01T00:00:00Z","action":"test","ok":true}' >> .devit/journal.jsonl
DEVIT_TUI_HEADLESS=1 "$TUI" --open-log .devit/journal.jsonl --seek-last 10 >/dev/null 2>&1 && PASS "TUI open-log OK" || FAIL "TUI open-log KO"

# 4) Approvals E2E (profil safe)
INFO "Approvals E2E (server.approve + shell_exec)"
# Vérifie policy (shell_exec on_request attendu en profil safe)
POLICY_JSON="$("$MCP" --cmd "$MCPD --yes --devit-bin $DEVIT --profile safe" --call server.policy --json '{}' 2>/dev/null | sed -n '2p' || true)"
echo "$POLICY_JSON" | grep -q '"shell_exec":"on_request"' || WARN "policy.shell_exec != on_request (safe) — test peut être permissif"
# Accorde once
"$MCP" --cmd "$MCPD --yes --devit-bin $DEVIT --profile safe" \
  --call server.approve --json '{"name":"devit.tool_call","scope":"once"}' >/dev/null
# 1) doit passer
OUT1="$("$MCP" --cmd "$MCPD --yes --devit-bin $DEVIT --profile safe" \
  --call devit.tool_call --json '{"tool":"shell_exec","args":{"cmd":"printf hi\n"}}' 2>/dev/null | sed -n '2p' || true)"
echo "$OUT1" | grep -q '"ok":true' && PASS "shell_exec #1 autorisé (once consommé)" || FAIL "shell_exec #1 KO"
# 2) doit redemander approval si on_request
OUT2="$("$MCP" --cmd "$MCPD --yes --devit-bin $DEVIT --profile safe" \
  --call devit.tool_call --json '{"tool":"shell_exec","args":{"cmd":"printf hi\n"}}' 2>/dev/null | tail -n1 || true)"
if echo "$OUT2" | grep -q '"approval_required":true'; then
  PASS "shell_exec #2 redemande approval (attendu)"
else
  WARN "shell_exec #2 n'a pas demandé d'approval — policy probablement permissive"
fi
echo "$OUT1" > "$REPORT_DIR/approve_run1.json"
echo "$OUT2" > "$REPORT_DIR/approve_run2.json"

# 5) Recipes
INFO "Recipes list + dry-run"
"$DEVIT" recipe list >"$REPORT_DIR/recipes_list.json" 2>/dev/null || FAIL "recipe list KO"
grep -qE '"add-ci"|"rust-upgrade-1.81"|"migrate-jest-vitest"' "$REPORT_DIR/recipes_list.json" && PASS "recipes starters détectées" || WARN "starters non détectées"
"$DEVIT" recipe run add-ci --dry-run >"$REPORT_DIR/recipe_add_ci.out" 2>/dev/null && PASS "recipe add-ci dry-run OK" || FAIL "recipe add-ci dry-run KO"

# 6) TUI hooks recipes (HEADLESS)
INFO "TUI hooks recipes (HEADLESS)"
DEVIT_TUI_HEADLESS=1 "$TUI" --list-recipes >"$REPORT_DIR/tui_recipes.json" 2>/dev/null && PASS "tui --list-recipes OK" || FAIL "tui --list-recipes KO"
DEVIT_TUI_HEADLESS=1 "$TUI" --run-recipe add-ci --dry-run >/dev/null 2>&1 && PASS "tui --run-recipe add-ci --dry-run OK" || FAIL "tui --run-recipe --dry-run KO"

# 7) VS Code packaging (optionnel)
if [[ -f editors/vscode/devit-vscode/package.json ]]; then
  INFO "VS Code: build/package (optionnel)"
  if have npm && have npx; then
    (cd editors/vscode/devit-vscode && npm ci >/dev/null 2>&1 && npm run -s build >/dev/null 2>&1 && npx -y vsce package >/dev/null 2>&1) \
      && PASS "VS Code .vsix pack OK" || WARN "VS Code pack: skip/KO (npm/vsce manquants ou erreurs)"
  else
    WARN "npm/vsce absents — skip VS Code pack"
  fi
else
  WARN "extension VS Code absente — skip"
fi

# 8) CI packaging jobs (présence)
if [[ -f .github/workflows/ci.yml ]]; then
  if grep -qE 'tui_build|vscode_pack|recipes_lint' .github/workflows/ci.yml; then
    PASS "CI packaging jobs détectés"
  else
    WARN "CI packaging jobs manquants (tui_build/vscode_pack/recipes_lint)"
  fi
else
  WARN "workflow CI absent"
fi

echo "---------------------------------------------"
echo "Artefacts:"
echo "  - $REPORT_DIR/recipes_list.json"
echo "  - $REPORT_DIR/recipe_add_ci.out"
echo "  - $REPORT_DIR/tui_recipes.json"
echo "  - $REPORT_DIR/approve_run1.json"
echo "  - $REPORT_DIR/approve_run2.json"
echo "✅ v0.5-rc.1 sanity: DONE"
