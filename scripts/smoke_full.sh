#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
BIN_DEVIT="$ROOT_DIR/target/debug/devit"
BIN_MCPD="$ROOT_DIR/target/debug/devit-mcpd"
BIN_MCP="$ROOT_DIR/target/debug/devit-mcp"

echo "[1/6] Build binaries (devit, devit-mcp, devit-mcpd)…" >&2
cargo build -q -p devit-cli --features experimental

echo "[2/6] SBOM generation (CycloneDX)…" >&2
"$BIN_DEVIT" sbom gen --out .devit/sbom.cdx.json >/dev/null
rg -n '"components"' .devit/sbom.cdx.json >/dev/null && echo "SBOM OK"

echo "[3/6] MCP redaction (schema+transform)…" >&2
# Token assez long (36) pour matcher les patterns
TOK="ghp_$(printf 'a%.0s' $(seq 1 36))"
PAY=$(jq -cn --arg t "$TOK" '{tool:"shell_exec",args:{cmd:("printf " + $t)}}')
"$BIN_MCP" --cmd "'$BIN_MCPD' --yes --devit-bin '$BIN_DEVIT'" \
  --call devit.tool_call --json "$PAY" | tee /tmp/mcp_redaction_full.out >/dev/null || true
if rg -q 'REDACTED|"redacted"\s*:\s*true' /tmp/mcp_redaction_full.out; then
  echo "MCP redaction OK"
else
  echo "WARN: MCP redaction markers not found (ensure [secrets].scan=true or patterns match)" >&2
fi

echo "[4/6] Attestation via fs_patch_apply (new file, safe) …" >&2
FNAME="SMOKE_DEVIT__$(date +%s).txt"
git add -N "$FNAME" 2>/dev/null || true
printf 'hello %s\n' "$(date -Iseconds)" > "$FNAME"
git diff -- "$FNAME" > /tmp/SMOKE.patch || true
git restore --staged "$FNAME" 2>/dev/null || true
rm -f "$FNAME"
jq -Rs '{name:"fs_patch_apply",args:{patch:.,mode:"index",check_only:false,commit:"off",tests_impacted:"off",precommit:"off"}}' /tmp/SMOKE.patch \
  | "$BIN_DEVIT" tool call - --yes | tee /tmp/fs_apply_full.json >/dev/null || true
LAST_ATTEST=$(ls -t .devit/attestations/*/attest.jsonl 2>/dev/null | head -n1 || true)
if [ -n "$LAST_ATTEST" ]; then
  tail -n1 "$LAST_ATTEST" | jq '{ok:true,has_sig:(.provenance.sig|length>0),diff_sha256:.diff_sha256,sbom_sha256:.sbom_sha256}'
else
  echo "WARN: no attestation file found (patch may not have applied)" >&2
fi

echo "[5/6] MCP + bwrap quick (tool list)…" >&2
if command -v bwrap >/dev/null 2>&1; then
  "$BIN_MCP" --cmd "'$BIN_MCPD' --yes --devit-bin '$BIN_DEVIT'" --call devit.tool_list --json '{}' | rg -n 'tool.result|tool.error' || true
else
  echo "bubblewrap not installed — skipping bwrap smoke" >&2
fi

echo "[6/6] Commit message (LLM if available)…" >&2
touch SMOKE_DEVIT_COMMIT.txt && git add SMOKE_DEVIT_COMMIT.txt || true
"$BIN_DEVIT" commit-msg --from-staged --with-template | sed -n '1,10p' || true
# Cleanup staged file (no commit)
git reset -q HEAD SMOKE_DEVIT_COMMIT.txt && rm -f SMOKE_DEVIT_COMMIT.txt || true

echo "Smoke completed." >&2
