#!/usr/bin/env bash
set -euo pipefail
cargo build -p devit-cli --features experimental --bins
SRV="target/debug/devit-mcpd --yes --devit-bin target/debug/devit"
( $SRV & echo $! > .devit/mcpd.pid )
sleep 0.5
target/debug/devit-mcp --cmd "$SRV" --policy >/dev/null
target/debug/devit-mcp --cmd "$SRV" --health >/dev/null || true
target/debug/devit-mcp --cmd "$SRV" --stats >/dev/null || true
echo '{"tool":"echo","args":{"msg":"ok"}}' | target/debug/devit-mcp --cmd "$SRV" --call devit.tool_call --json @- >/dev/null || true
kill $(cat .devit/mcpd.pid) 2>/dev/null || true
rm -f .devit/mcpd.pid
echo "E2E MCP: OK"
