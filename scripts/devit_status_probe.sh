#!/usr/bin/env bash
set -euo pipefail
REPORT_DIR=".devit/reports"; mkdir -p "$REPORT_DIR"; JSON_OUT="$REPORT_DIR/status_v05.json"
color(){ case "$1" in GREEN) printf "\033[32mGREEN\033[0m";; YELLOW) printf "\033[33mYELLOW\033[0m";; RED) printf "\033[31mRED\033[0m";; *) printf "%s" "$1";; esac;}
have(){ command -v "$1" >/dev/null 2>&1; } ; file_exists(){ [[ -f "$1" ]]; } ; dir_exists(){ [[ -d "$1" ]]; } ; try(){ bash -ceu 'set -o pipefail; '"$*" >/dev/null 2>&1; }
rt(){ local s="$1"; shift; if have timeout; then timeout "${s}s" "$@"; else "$@"; fi }
touch .devit/journal.jsonl || true
declare -A status reason
if grep -Rqs --exclude-dir=target -- 'name\s*=\s*"devit-tui"' **/Cargo.toml 2>/dev/null || grep -Rqs --exclude-dir=target 'devit-tui' crates 2>/dev/null; then
  if try "cargo test -p devit-tui --no-run"; then
    if have cargo && rt 10 bash -lc 'DEVIT_TUI_HEADLESS=1 cargo run -p devit-tui -- --journal-path .devit/journal.jsonl'; then status[T1]="GREEN"; reason[T1]="binaire présent + headless OK"; else status[T1]="YELLOW"; reason[T1]="build OK, headless non vérifié"; fi
  else status[T1]="RED"; reason[T1]="échec cargo test --no-run"; fi
else status[T1]="RED"; reason[T1]="crate/binaire devit-tui introuvable"; fi
HANDSHAKE_JSON="$REPORT_DIR/mcp_handshake.json"
TOOLS_JSON="$REPORT_DIR/tool_list.json"
if have target/debug/devit-mcp && have target/debug/devit-mcpd; then
  rt 10 bash -lc "target/debug/devit-mcp --cmd 'target/debug/devit-mcpd --yes --devit-bin target/debug/devit' --handshake-only >'$HANDSHAKE_JSON'" || true
  rt 10 bash -lc "target/debug/devit-mcp --cmd 'target/debug/devit-mcpd --yes --devit-bin target/debug/devit' --call devit.tool_list --json '{}' >'$TOOLS_JSON'" || true
fi
if file_exists "$HANDSHAKE_JSON" && head -n1 "$HANDSHAKE_JSON" | grep -q '"server.approve"'; then
  status[T2]="GREEN"; reason[T2]="handshake expose server.approve";
elif file_exists "$TOOLS_JSON" && grep -q '"server.approve"' "$TOOLS_JSON"; then
  status[T2]="YELLOW"; reason[T2]="tool_list JSON contient server.approve";
else
  status[T2]="RED"; reason[T2]="server.approve absent (handshake/tool_list)";
fi
PATCH_TEST="$REPORT_DIR/sample.diff"; cat > "$PATCH_TEST" <<'EOF'
diff --git a/README.md b/README.md
index 111..222 100644
--- a/README.md
+++ b/README.md
@@
-Old
+New
EOF
if have cargo; then
  if rt 10 bash -lc 'DEVIT_TUI_HEADLESS=1 cargo run -p devit-tui -- --open-diff '"$PATCH_TEST" ; then
    status[T3]="GREEN"; reason[T3]="devit-tui --open-diff OK";
  elif have target/debug/devit-tui && rt 10 bash -lc 'DEVIT_TUI_HEADLESS=1 target/debug/devit-tui --open-diff '"$PATCH_TEST" ; then
    status[T3]="GREEN"; reason[T3]="devit-tui (binaire) --open-diff OK";
  else
    status[T3]="RED"; reason[T3]="commande --open-diff indisponible/KO";
  fi
else status[T3]="RED"; reason[T3]="cargo indisponible"; fi
HELP_TUI="$REPORT_DIR/devit_tui_help.txt"; rt 10 bash -lc 'cargo run -p devit-tui -- --help' >"$HELP_TUI" 2>/dev/null || true
if have cargo; then
  if rt 10 bash -lc 'DEVIT_TUI_HEADLESS=1 cargo run -p devit-tui -- --open-log .devit/journal.jsonl'; then
    status[T4]="GREEN"; reason[T4]="devit-tui --open-log OK";
  elif have target/debug/devit-tui && rt 10 bash -lc 'DEVIT_TUI_HEADLESS=1 target/debug/devit-tui --open-log .devit/journal.jsonl'; then
    status[T4]="GREEN"; reason[T4]="devit-tui (binaire) --open-log OK";
  else
    status[T4]="RED"; reason[T4]="commande --open-log indisponible/KO";
  fi
else
  status[T4]="RED"; reason[T4]="cargo indisponible";
fi
if dir_exists editors/vscode/devit-vscode && file_exists editors/vscode/devit-vscode/package.json; then
  if grep -q '"activationEvents"' editors/vscode/devit-vscode/package.json; then status[T5]="YELLOW"; reason[T5]="extension présente (build .vsix non vérifié)"; else status[T5]="RED"; reason[T5]="package.json incomplet (activationEvents manquant)"; fi
else status[T5]="RED"; reason[T5]="extension VS Code absente"; fi
if file_exists editors/vscode/devit-vscode/package.json && grep -Rqi 'codeAction' editors/vscode/devit-vscode 2>/dev/null; then status[T6]="YELLOW"; reason[T6]="traces CodeActions détectées"; else status[T6]="RED"; reason[T6]="pas de provider CodeActions détecté"; fi
if have target/debug/devit; then
  if rt 10 target/debug/devit recipe list >"$REPORT_DIR/recipes_list.json" 2>/dev/null; then
    if grep -qE '"add-ci"|"rust-upgrade-1.81"|"migrate-jest-vitest"' "$REPORT_DIR/recipes_list.json"; then status[T7]="GREEN"; reason[T7]="runner + 3 recettes détectées"; else status[T7]="YELLOW"; reason[T7]="runner OK, starters incomplètes"; fi
  else status[T7]="RED"; reason[T7]="devit recipe list indisponible"; fi
else status[T7]="RED"; reason[T7]="binaire devit absent"; fi
if grep -qiE 'recipes|Run Recipe|touche R' "$HELP_TUI" 2>/dev/null || grep -Rqi 'recipes' crates/tui 2>/dev/null; then status[T8]="YELLOW"; reason[T8]="indices d’intégration présents"; else status[T8]="RED"; reason[T8]="pas d’indice TUI<->CLI"; fi
if file_exists README.md && grep -qi 'DevIt TUI' README.md && file_exists docs/recipes.md; then status[T9]="GREEN"; reason[T9]="docs TUI + recipes présentes"; else status[T9]="YELLOW"; reason[T9]="docs partielles/manquantes"; fi
if file_exists .github/workflows/ci.yml; then
  if grep -qE 'tui_build|vscode_pack|recipes_lint' .github/workflows/ci.yml; then status[T10]="YELLOW"; reason[T10]="jobs packaging détectés"; else status[T10]="RED"; reason[T10]="jobs packaging manquants"; fi
else status[T10]="RED"; reason[T10]="workflow CI absent"; fi
{
  echo '{'
  for k in T1 T2 T3 T4 T5 T6 T7 T8 T9 T10; do
    printf '  "%s": {"status":"%s","reason":%s}' "$k" "${status[$k]}" "$(printf '%s' "${reason[$k]}" | sed 's/"/\\"/g' | awk '{printf "\"%s\"", $0}')"
    [[ "$k" != "T10" ]] && echo ','
  done
  echo '}'
} > "$JSON_OUT"
printf "\nStatus v0.5 tickets (heuristiques)\n-----------------------------------\n"
for k in T1 T2 T3 T4 T5 T6 T7 T8 T9 T10; do printf "%-4s %-8s  %s\n" "$k" "$(color "${status[$k]}")" "${reason[$k]}"; done
printf "\n→ Détail JSON: %s\n" "$JSON_OUT"
