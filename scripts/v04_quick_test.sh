#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
DEVIT="$ROOT_DIR/target/debug/devit"
MCPD="$ROOT_DIR/target/debug/devit-mcpd"
MCP="$ROOT_DIR/target/debug/devit-mcp"
REPORT=".devit/reports"
mkdir -p "$REPORT"

if [[ ! -x "$DEVIT" || ! -x "$MCPD" || ! -x "$MCP" ]]; then
  echo "[build] building devit/devit-mcp/devit-mcpd…" >&2
  cargo build -q -p devit-cli --features experimental
fi

TOK="ghp_$(printf 'a%.0s' $(seq 1 36))"
PLACE="REDACTED"

pass(){ printf '\033[32mPASS\033[0m %s\n' "$*"; }
fail(){ printf '\033[31mFAIL\033[0m %s\n' "$*"; exit 1; }

echo "[1/3] MCP redaction via echo" >&2
PAY_ECHO=$(jq -cn --arg t "$TOK" '{tool:"echo",args:{msg:("token " + $t)}}')
OUT1=$("$MCP" --cmd "'$MCPD' --yes --devit-bin '$DEVIT' --secrets-scan --redact-placeholder '$PLACE'" --call devit.tool_call --json "$PAY_ECHO" || true)
echo "$OUT1" > "$REPORT/v04_echo.json"
echo "$OUT1" | rg -q "$PLACE|\"redacted\"\s*:\s*true" && pass "echo redacted" || fail "echo non redacted"

echo "[2/3] MCP redaction via shell_exec (echo token)" >&2
PAY_SH=$(jq -cn --arg t "$TOK" '{tool:"shell_exec",args:{cmd:("echo token " + $t)}}')
OUT2=$("$MCP" --cmd "'$MCPD' --yes --devit-bin '$DEVIT' --secrets-scan --redact-placeholder '$PLACE' --child-dump-dir .devit/reports" --call devit.tool_call --json "$PAY_SH" || true)
echo "$OUT2" > "$REPORT/v04_shell.json"
echo "$OUT2" | rg -q "$PLACE|\"redacted\"\s*:\s*true" && pass "shell_exec redacted" || {
  echo "-- child stdout/stderr (last) --" >&2
  ls -t .devit/reports/child_* 2>/dev/null | head -n2 | xargs -r -I {} sh -c 'echo ========== {}; sed -n "1,80p" {}'
  fail "shell_exec non redacted"
}

echo "[3/3] DevIt CLI direct (json-only)" >&2
OUT3=$(jq -cn --arg t "$TOK" '{name:"shell_exec",args:{cmd:("echo token " + $t)}}' | "$DEVIT" tool call - --json-only || true)
echo "$OUT3" > "$REPORT/v04_cli.json"
echo "$OUT3" | rg -q "token" && pass "cli json-only ok" || fail "cli json-only vide"

echo "OK"

