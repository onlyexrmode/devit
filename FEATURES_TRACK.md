DevIt — Suivi des features / changelog (résumé)

Sprint 1
- Policy d’approbation: enum `Approval { untrusted, on-failure, on-request, never }`
- Validation config `devit.toml` (erreurs claires sur approval)
- Table de décision `requires_approval`
- Enforcements CLI:
  - `apply`: Untrusted ignore `--yes`; OnRequest/OnFailure demandent sauf `--yes`; Never ne demande pas
  - `run`: OnRequest échoue sans `--yes`; Untrusted ignore `--yes` (toujours demande)
- Aide `--help` mise à jour

Sprint 2
- Sandbox (`devit-sandbox`): `Mode { read-only, workspace-write, danger-full-access }`
- Trait `Sandbox` + impl `NoopSandbox` et `BwrapSandbox` auto si dispo (`--unshare-net`)
- Flag global `--no-sandbox` (logué comme danger)
- Enforcement read-only pour `apply/run/test` + safe-list en read-only (`git`, `cargo`, `npm`, `ctest`)
- Timeouts via `DEVIT_TIMEOUT_SECS` (kill process + message)
- Routage tests et opérations git critiques via le Sandbox
- Journal JSONL minimal `~/.devit/logs/log.jsonl` (ToolCall, Diff, AskApproval, Info)

Sprint 3 (en cours)
- `update_plan.yaml`: `step,status,commit,notes` append depuis `run`
- Sur échec de tests: résumé JUnit si trouvé + tail du log, statut `failed`
- Sur succès: entrée `done`
- `run` route les tests via le Sandbox (cargo/npm/ctest) avec timeouts respectés (DEVIT_TIMEOUT_SECS)
- Commande `devit plan` pour afficher les entrées du plan (aperçu notes)

Sprint 4 — Interop + TUI + packaging (début)
- `--backend-url` et `--model` pour override ponctuel de la config
- TUI minimal (one-shot) via `--tui` pour Suggest: colonnes “Plan | Diff | Logs”
- (à venir) TUI interactif + packaging
- CI GitHub Actions: fmt, clippy, build, tests (Linux/macOS) + artefacts release sur tags `v*`
- TUI d'approbation interactive intégrée à `apply`/`run` (y/n/q) avec logs JSONL en live
- Commande `devit watch` pour un TUI continu (plan yaml / diff optionnel / logs JSONL)
 - Navigation TUI: flèches ou h/j/k/l pour focus/scroll, PgUp/PgDn, 1/2/3 pour sélectionner une colonne; diff colorisé (+ vert, - rouge)

Notes d’usage
- Configurer `devit.toml` → `[policy] approval` et `sandbox`
- Env `DEVIT_TIMEOUT_SECS` pour limiter la durée d’exécution des commandes
- `--no-sandbox` désactive l’isolation (à éviter hors debug)
