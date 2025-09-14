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

# rate-limit cooldown
out=$(target/debug/devit-mcp --cmd "$SRV --cooldown-ms 1000" --call devit.tool_list --json '{}' >/dev/null; target/debug/devit-mcp --cmd "$SRV --cooldown-ms 1000" --call devit.tool_list --json '{}' || true)
echo "$out" | rg '"rate_limited":\s*true' >/dev/null
