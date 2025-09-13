**v0.2 — “Confiance & interop”**
v0.2-rc (pre-release) livré:
- shell_exec sandboxé (safe‑list + no‑net best‑effort), I/O JSON
- fs_patch_apply (check-only + index/worktree) via I/O JSON
- context map `.devit/index.json` (respect .gitignore; ignore `.devit/`, `target/`, `bench/`)
- journal JSONL signé (HMAC) + option `git notes`
- binaire expérimental `devit-mcp` (client MCP stdio), feature-gated `--features experimental`
MCP bi-directionnel : DevIt consomme ET expose des outils (fs patch-only, shell sandboxé).
Plugins WASM/WASI : outils (grep, formatter, linter) isolés, chargeables à chaud.
Contexte intelligent : map du repo (ripgrep + tree-sitter), sélection de fichiers pertinents, cache d’index local.
Approvals granulaires : règles par outil (“git yes, shell ask”), profils safe|std|danger.
Provenance minimale : commit footer DevIt-Attest: <hash> + journal JSONL signé, git notes optionnel.

**v0.3 — “Qualité des patchs”**
Test-aware : sélection de tests impactés (heuristique par diff), relance ciblée.
Pre-commit intégré : format/linters avant apply, refus si violations critiques.
Merge 3-way assisté : explication et mini-UI pour résoudre conflits, toujours patch-first.
Génération de messages de commit : LLM + règles Conventional Commits + scope auto (déduit du path).
Rapports : export SARIF (lint/tests) + JUnit (résumé) pour CI.

**v0.4 — “Sécurité & supply-chain”**
Sandbox sérieuse : bwrap/firejail + quotas CPU/RAM, net policy off|egress|full.
SBOM CycloneDX des deps touchées + attestation SLSA-lite sur le diff (hash, horodatage).
Gestion des secrets : lecture par env/agent interdites par défaut (allow-list explicite).
Mode lecture seule total : simulation de patch (dry-run) avec métriques.

**v0.5 — “Expérience dev”**
TUI avancée : timeline d’actions, approbations in-line, diff viewer, replays.
Intégrations IDE (VS Code/JetBrains) : DevIt comme “tool server” (MCP/LSP bridge).
Recettes : commandes prêtes “ajoute CI”, “porte en Rust 1.81”, “migre Jest→Vitest”, etc.

**v0.6 — “Bench & crédibilité”**
SWE-bench Lite-50 automatisé (cron GitHub Actions self-hosté ou doc pipeline local).
SWE-bench Verified/Live en option (doc pas-à-pas, scripts fournis).
Dashboard de scores : taux de résolution, temps moyen, taille de patch, régressions.

**v1.0 — “Prod-ready”**
Stabilité : compat multi-OS, jeux d’essais end-to-end.
Packaging : bins signés, Homebrew/Apt, release notes carrées.
Gouvernance : CONTRIBUTING, RFCs, tri des issues, “good first issue”.

**2 démos “qui font tilt”**
Patch-only sur un bug réel (lib Python/TS populaire) : propose le diff, tests impactés, message de commit propre, attestation.
Migration ciblée (format/CI/linter) sur un repo moyen : DevIt produit une série de petits commits atomiques + plan YAML.

**OKRs (simples, mesurables)**
Qualité patch : ≥80 % des diffs compilent sans retouche sur un set interne (20 issues).
Bench : SWE-bench Lite-50 ≥X % (on fixe X après premier run).
Sécurité : 100 % des actions sensibles loggées + sandboxées par défaut.

**What Codex fait après l’alpha (ordre)**
v0.2 : MCP client/serveur minimal + loader plugins WASM + contexte intelligent (ripgrep/tree-sitter).
v0.3 : pre-commit + test selection + merge assisté + SARIF/JUnit.
v0.4 : sandbox hardening + SBOM/attestation + secrets policy.
v0.5 : TUI avancée + adaptateur VS Code (MCP/LSP) + recettes.
v0.6 : scripts SWE-bench (Lite-50, Verified), dashboard CSV/MD.
