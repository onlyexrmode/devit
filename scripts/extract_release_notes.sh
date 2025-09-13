#!/usr/bin/env bash
set -euo pipefail
# Usage: scripts/extract_release_notes.sh v0.2.0-rc.1
# Sort la section de RELEASE_NOTES.md correspondant au tag.
# Stratégie:
#   1) "## <tag>" exact
#   2) "## v<tag>" (si l'input est "0.2.0-rc.1")
#   3) Si tag =~ ^v0\.2, fallback "## v0.2-rc"
#   4) Sinon: tout le fichier + warning

TAG="${1:-}"
if [[ -z "${TAG}" ]]; then
  echo "error: missing tag argument" >&2
  exit 2
fi

NOTES="RELEASE_NOTES.md"
if [[ ! -f "${NOTES}" ]]; then
  echo "error: ${NOTES} not found" >&2
  exit 3
fi

# Normaliser
RAW="${TAG#v}"      # "v0.2.0-rc.1" -> "0.2.0-rc.1"
VRAW="v${RAW}"      # "0.2.0-rc.1"  -> "v0.2.0-rc.1"

extract_section() {
  local header="$1" # e.g. "## v0.2.0-rc.1"
  awk -v H="$header" '
    BEGIN { printing=0 }
    $0 ~ "^"H"[[:space:]]*$" { printing=1; next }
    printing && $0 ~ "^##[[:space:]]" { exit }
    printing { print }
  ' "${NOTES}"
}

try_print() {
  local hdr="$1"
  local out
  out="$(extract_section "$hdr" || true)"
  if [[ -n "${out//[[:space:]]/}" ]]; then
    echo "$hdr"
    echo
    echo "$out"
    return 0
  fi
  return 1
}

# 1) "## <tag>"
if try_print "## ${TAG}"; then exit 0; fi
# 2) "## v<tag>" si l'input ne commence pas par v
if [[ "${TAG}" != "${VRAW}" ]]; then
  if try_print "## ${VRAW}"; then exit 0; fi
fi
# 3) Fallback rc bucket (ex: v0.2-rc) si le tag commence par v0.2
if [[ "${VRAW}" =~ ^v0\.2 ]]; then
  if try_print "## v0.2-rc"; then
    echo -e "\n> note: fallback sur la section v0.2-rc (aucune section spécifique trouvée pour ${TAG})" >&2
    exit 0
  fi
fi

# 4) Fallback global (entier) avec avertissement
echo "> warning: aucune section dédiée au tag ${TAG} — impression complète de ${NOTES}" >&2
cat "${NOTES}"
exit 0

