#!/usr/bin/env bash
set -euo pipefail

# ensure_sarif.sh
# Garantit la présence d'un SARIF valide (non vide) à l'emplacement donné.
# Usage:
#   .devit/scripts/ensure_sarif.sh .devit/reports/sarif.json
#
# Comportement:
# - Si le fichier n'existe pas ou est invalide (pas de runs[]/driver.name),
#   écrit un SARIF minimal avec un run vide et un driver.name.

OUT="${1:-.devit/reports/sarif.json}"
mkdir -p "$(dirname "$OUT")"

is_valid() {
  command -v jq >/dev/null 2>&1 || return 1
  jq -e '.version=="2.1.0"
         and (.runs|length)>0
         and (.runs[0].tool.driver.name|type=="string" and length>0)' \
    "$OUT" >/dev/null 2>&1
}

if [[ ! -s "$OUT" ]] || ! is_valid; then
  cat >"$OUT" <<'JSON'
{
  "version": "2.1.0",
  "$schema": "https://schemastore.azurewebsites.net/schemas/json/sarif-2.1.0.json",
  "runs": [{
    "tool": {
      "driver": {
        "name": "DevIt Quality Gate",
        "informationUri": "https://github.com/n-engine/devit"
      }
    },
    "results": []
  }]
}
JSON
fi

echo "SARIF ready at: $OUT"

