#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
BIN_DEVIT="$ROOT_DIR/target/debug/devit"
BIN_MCPD="$ROOT_DIR/target/debug/devit-mcpd"
BIN_MCP="$ROOT_DIR/target/debug/devit-mcp"

if [[ ! -x "$BIN_DEVIT" || ! -x "$BIN_MCPD" || ! -x "$BIN_MCP" ]]; then
  echo "Building binaries…" >&2
  cargo build -q -p devit-cli --features experimental
fi

PAY='{"tool":"shell_exec","args":{"cmd":"printf ghp_ABCDEF1234567890"}}'

echo "Running MCP redaction smoke (sandbox=none)…" >&2
"$BIN_MCP" --cmd "'$BIN_MCPD' --yes --devit-bin '$BIN_DEVIT' --sandbox none --secrets-scan --env-allow PATH,HOME" \
  --call devit.tool_call --json "$PAY" | tee /tmp/mcp_redaction.out

echo "Searching for redaction markers…" >&2
grep -E 'REDACTED|redacted' /tmp/mcp_redaction.out && echo "OK" || { echo "Redaction markers not found" >&2; exit 1; }

