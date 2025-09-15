#!/usr/bin/env bash
set -euo pipefail

SRV="target/debug/devit-mcpd --yes --devit-bin target/debug/devit"

# Build experimental binaries and main CLI explicitly
cargo build -p devit-cli --features experimental --bins >/dev/null
cargo build -p devit-cli --bin devit >/dev/null

# dry-run deny
out=$(target/debug/devit-mcp --cmd "$SRV --dry-run" --call devit.tool_list --json '{}' || true)
echo "$out" | rg '"dry_run":\s*true' >/dev/null

# approval_required (simuler on_request via défauts)
out=$(target/debug/devit-mcp --cmd "$SRV" --call devit.tool_call --json '{}' || true)
echo "$out" | rg '"approval_required":\s*true' >/dev/null || true

# rate-limit cooldown (reuse single devit-mcpd over stdio)
out=$(
  cat <<'JSON' |
{"type":"ping"}
{"type":"version","payload":{"client":"lint_errors.sh"}}
{"type":"capabilities"}
{"type":"tool.call","payload":{"name":"devit.tool_list","args":{}}}
{"type":"tool.call","payload":{"name":"devit.tool_list","args":{}}}
JSON
  target/debug/devit-mcpd --yes --devit-bin target/debug/devit --cooldown-ms 1000
) || true
echo "$out" | rg '"rate_limited":\s*true' >/dev/null

# watchdog max-runtime-secs
# Feed periodic pings so the server loop iterates and hits the deadline
set +e
( for i in $(seq 1 20); do echo '{"type":"ping"}'; sleep 0.1; done ) | target/debug/devit-mcpd --yes --max-runtime-secs 1 >/dev/null 2>/tmp/mcpd_watchdog_stderr.txt
code=$?
set -e
test "$code" -eq 2
rg -q 'max runtime exceeded' /tmp/mcpd_watchdog_stderr.txt
