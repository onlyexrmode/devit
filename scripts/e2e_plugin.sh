#!/usr/bin/env bash
set -euo pipefail

# Skip if wasmtime runtime is not available (typical in CI)
if ! command -v wasmtime >/dev/null 2>&1; then
  echo "SKIP: wasmtime not found; skipping plugin E2E"
  exit 0
fi

# Build required binaries (devit, devit-plugin, devit-mcp, devit-mcpd)
cargo build -p devit-cli --features experimental --bins >/dev/null

# Build example plugin and register manifest
make -s plugin-echo-sum >/dev/null

# Invoke plugin via MCP server (plugin.invoke)
out=$(echo '{"id":"echo_sum","payload":{"a":2,"b":40}}' | \
  target/debug/devit-mcp --cmd 'target/debug/devit-mcpd --yes --devit-bin target/debug/devit' \
  --call plugin.invoke --json @- || true)

echo "$out" | rg '"sum"\s*:\s*42' >/dev/null || {
  echo "error: plugin E2E failed (expected sum=42)"
  echo "$out"
  exit 2
}

echo "E2E Plugin: OK"

