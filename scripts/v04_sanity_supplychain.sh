#!/usr/bin/env bash
set -euo pipefail

# v04_sanity_supplychain.sh
# Vérifie 4 points clés v0.4 :
#  1) Secrets policy & redaction (MCP server)
#  2) Sandbox réseau (--net off) via bwrap si dispo
#  3) SBOM CycloneDX agrégé (.devit/sbom.cdx.json)
#  4) Attestation SLSA-lite du diff (JSONL signé)
#
# Pré-requis : build local des bins (target/debug/devit, devit-mcp, devit-mcpd)
# Nettoyage : branche jetable + reset hard à la fin.

DEVIT_BIN="${DEVIT_BIN:-target/debug/devit}"
MCP_BIN="${MCP_BIN:-target/debug/devit-mcp}"
MCPD_BIN="${MCPD_BIN:-target/debug/devit-mcpd}"
REPORT_DIR=".devit/reports"
mkdir -p "$REPORT_DIR"

pass() { printf "\033[32mPASS\033[0m %s\n" "$*"; }
fail() { printf "\033[31mFAIL\033[0m %s\n" "$*"; exit 1; }
info() { printf "\033[36m-- %s\033[0m\n" "$*"; }

# --- préconditions git ---
git rev-parse --is-inside-work-tree >/dev/null 2>&1 || fail "pas un repo git"
if ! git diff --quiet || [[ -n "$(git status --porcelain)" ]]; then
  fail "working tree non clean (commit/stash d'abord)"
fi

# --- 1) Secrets policy & redaction ---
info "Secrets redaction (MCP)"
# Detect supported flags on devit-mcpd
MCPD_HELP="$($MCPD_BIN --help 2>&1 || true)"
FLAGS_SECRETS=""
if echo "$MCPD_HELP" | grep -q -- "--env-allow"; then FLAGS_SECRETS+=" --env-allow PATH,HOME"; fi
if echo "$MCPD_HELP" | grep -q -- "--secrets-scan"; then FLAGS_SECRETS+=" --secrets-scan"; fi
if echo "$MCPD_HELP" | grep -q -- "--redact-placeholder"; then FLAGS_SECRETS+=" --redact-placeholder '***REDACTED***'"; fi
# Use shell_exec to produce a token-like string for redaction
TOK="ghp_$(printf 'a%.0s' $(seq 1 36))"
PAY_RED=$(jq -cn --arg t "$TOK" '{tool:"shell_exec",args:{cmd:("printf " + $t)}}')
RED_OUT="$("$MCP_BIN" --cmd "$MCPD_BIN --yes$FLAGS_SECRETS" --call devit.tool_call --json "$PAY_RED" || true)"
echo "$RED_OUT" > "$REPORT_DIR/mcp_redaction.json"
echo "$RED_OUT" | grep -q 'REDACTED' && pass "redaction active" || fail "redaction manquante"

# --- 2) Sandbox réseau (net off) ---
info "Sandbox réseau (--net off)"
BWRAP_OK=1
if command -v bwrap >/dev/null 2>&1; then
  FLAGS_NET=""
  if echo "$MCPD_HELP" | grep -q -- "--sandbox"; then FLAGS_NET+=" --sandbox bwrap"; fi
  if echo "$MCPD_HELP" | grep -q -- "--net"; then FLAGS_NET+=" --net off"; fi
  OUT=$("$MCP_BIN" --cmd "$MCPD_BIN --yes$FLAGS_NET" \
    --call devit.tool_call --json '{"tool":"shell_exec","args":{"cmd":"curl -s https://example.com || echo curl_failed"}}' || true)
  echo "$OUT" > "$REPORT_DIR/mcp_netoff.json"
  echo "$OUT" | grep -qi 'curl_failed\|error\|refused' && pass "réseau coupé (attendu)" || fail "réseau semble actif (unexpected)"
else
  info "bwrap absent — skip (considéré PASS best-effort)"
  BWRAP_OK=0
fi

# --- 3) SBOM CycloneDX ---
info "Génération SBOM (.devit/sbom.cdx.json)"
"$DEVIT_BIN" sbom gen --out .devit/sbom.cdx.json || fail "devit sbom gen a échoué"
grep -q '"components"' .devit/sbom.cdx.json && pass "SBOM contient des components" || fail "SBOM vide/invalide"

# --- 4) Attestation SLSA-lite ---
info "Attestation du diff (branche jetable)"
CURR_BRANCH="$(git rev-parse --abbrev-ref HEAD)"
STAMP="$(date -u +%Y%m%d-%H%M%S)"
TMP_BR="test/v04-supplychain-${STAMP}"
git switch -c "$TMP_BR" >/dev/null

# créer un fichier temporaire et appliquer via fs_patch_apply
TMPF="DEVIT_TMP_SANITY.txt"
PATCH=$(cat <<EOF
diff --git a/${TMPF} b/${TMPF}
new file mode 100644
--- /dev/null
+++ b/${TMPF}
@@
+DevIt v0.4 sanity $(date -u +%FT%TZ)
EOF
)
printf '%s\n' "$PATCH" > "$REPORT_DIR/tmp.patch"

# Appliquer le patch (mode index), désactiver étapes coûteuses
printf '%s\n' "$PATCH" | "$DEVIT_BIN" fs_patch_apply --json @- --commit off --precommit off --tests-impacted off --attest-diff || fail "fs_patch_apply a échoué"
# Vérifier l’attestation
ATTEST_LINE="$(tail -n1 .devit/attestations/*/attest.jsonl 2>/dev/null || true)"
[ -n "$ATTEST_LINE" ] || fail "aucune attestation trouvée"
echo "$ATTEST_LINE" | jq . >/dev/null 2>&1 || fail "attestation JSON invalide"
echo "$ATTEST_LINE" | jq -r '{ts,diff_sha256,sbom_sha256,provenance}' > "$REPORT_DIR/attest_summary.json"
pass "attestation présente"

# Cleanup branche
git reset --hard >/dev/null
git switch "$CURR_BRANCH" >/dev/null
git branch -D "$TMP_BR" >/dev/null 2>&1 || true
rm -f "$TMPF" || true

# --- Récap ---
echo "---------------------------------------------"
echo "Rapports :"
echo "  - $REPORT_DIR/mcp_redaction.json"
echo "  - $REPORT_DIR/mcp_netoff.json   (si bwrap présent)"
echo "  - .devit/sbom.cdx.json"
echo "  - $REPORT_DIR/attest_summary.json"
echo "✅ v0.4 sanity: OK"
