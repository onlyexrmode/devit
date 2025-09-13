## Type / Scope
- Type (Conventional): feat | fix | chore | docs | test | refactor
- Scope (optionnel): ex. `cli`, `sandbox`, `tools`

## Description courte
Brève synthèse: quoi et pourquoi en 2–3 lignes.

## Impact CLI / Sandbox / Approvals / Timeouts
- CLI (flags/commandes ajoutées/modifiées):
- Sandbox (comportement, safe‑list, net):
- Approvals (règles, prompts):
- Timeouts (valeur, où l’appliquer):

## Risques & Plan de rollback
- Risques identifiés:
- Plan de rollback (commande ou revert simples):

## Étapes de test (commandes exactes)
1. …
2. …
3. …

## Checklist
- [ ] cargo fmt --all -- --check
- [ ] cargo clippy --workspace --all-targets -- -D warnings
- [ ] cargo test --workspace --all-targets --no-fail-fast
