# RELEASE_NOTES.md

## v0.5.0-rc.1
DevIt v0.5.0-rc.1 — « Expérience dev »
✨ Points forts

TUI (ratatui) : mode headless stable (--journal-path), viewer de diff (--open-diff), replay journal (--open-log), hooks recettes (liste/exécution, preview diff, apply).

Approvals hiérarchiques pour devit.tool_call : clés outer (devit.tool_call) et inner (devit.tool_call:<outil>) avec consommation priorisée (once/session/always).

MCP serveur/cliente : server.approve exposé, server.policy, statut/health, et routage JSON propre vers la CLI DevIt.

Recipes : runner YAML + 3 recettes starter (add-ci, rust-upgrade-1.81, migrate-jest-vitest).

Extension VS Code : panel timeline (journal), Approve Last Request, Run Recipe…, premières Code Actions (Rust/JS/CI). Packaging .vsix.

CI packaging : jobs pour builder devit-tui, packer l’extension VS Code, et linter les recettes.

🔒 Sécurité & discipline

Sandbox (profil safe) avec réseau off et limites CPU/RAM configurables.

Journal d’audit JSONL signé (HMAC).

Redaction centrale des secrets dans toutes les réponses MCP (placeholders configurables).

Attestations & SBOM déjà présentes (v0.4) – inchangées dans cette RC.

🧠 Approvals hiérarchiques (nouveau)

Lors d’un devit.tool_call, DevIt accepte une approbation si elle correspond :

à la clé inner devit.tool_call:<outil> (ex. devit.tool_call:shell_exec) ;

à la clé outer devit.tool_call.
Priorité de consommation : inner.once > outer.once > inner.session > outer.session > inner.always > outer.always.
Chaque consommation est auditée (champ approval_key: inner|outer).

Exemples rapides :

Autoriser une exécution unique du shell :

devit-mcp --cmd 'devit-mcpd --yes' \
  --call server.approve --json '{"name":"devit.tool_call:shell_exec","scope":"once"}'


Autoriser la session entière pour tous les devit.tool_call :

devit-mcp --cmd 'devit-mcpd --yes' \
  --call server.approve --json '{"name":"devit.tool_call","scope":"session"}'

🧩 Interop CLI (stabilisée)

devit-mcpd transmet désormais à devit tool call - un JSON propre au format :

{"name":"<outil>", "args":{...}, "yes":true}


(entrée via stdin, une seule valeur JSON sur stdout, logs → stderr.)

🧪 Sanity rapide (locaux)
# TUI headless
DEVIT_TUI_HEADLESS=1 devit-tui --journal-path .devit/journal.jsonl

# Diff & replay
DEVIT_TUI_HEADLESS=1 devit-tui --open-diff .devit/reports/sample.diff
DEVIT_TUI_HEADLESS=1 devit-tui --open-log .devit/journal.jsonl --seek-last 10

# Approvals (outer)
devit-mcp --cmd 'devit-mcpd --yes --profile safe' \
  --call server.approve --json '{"name":"devit.tool_call","scope":"once"}'
devit-mcp --cmd 'devit-mcpd --yes --profile safe' \
  --call devit.tool_call --json '{"name":"shell_exec","args":{"cmd":"printf hi\n"}}'

🧰 VS Code

Panel “DevIt” (timeline du journal), Approve Last Request, Run Recipe…

Code Actions : Rust (add-ci), JS (Jest→Vitest), CI (absence workflow).

Packaging .vsix via vsce.

⚠️ Notes

En environnements contraints, il peut être utile d’augmenter --mem-mb (ex. 2048) côté devit-mcpd.

Les policies peuvent différer selon profil (safe|std|danger) ; la demande d’approval peut donc varier.

DevIt v0.5.0-rc.1 — “Developer Experience”
✨ Highlights

TUI (ratatui): stable headless mode (--journal-path), diff viewer (--open-diff), log replay (--open-log), recipe hooks (list/run, diff preview, apply).

Hierarchical approvals for devit.tool_call: outer and inner keys with prioritized consumption (once/session/always).

MCP server/client: server.approve, server.policy, health/stats, robust JSON handoff to DevIt CLI.

Recipes: YAML runner + 3 starters (add-ci, rust-upgrade-1.81, migrate-jest-vitest).

VS Code extension: timeline panel, Approve Last Request, Run Recipe…, initial Code Actions (Rust/JS/CI). Packaged .vsix.

CI packaging: jobs to build devit-tui, package VS Code extension, and lint recipes.

🔒 Security & hygiene

Sandbox (safe profile) with network off and configurable CPU/RAM quotas.

Signed audit log (HMAC) as JSONL.

Central secret redaction in all MCP responses (configurable placeholder).

Attestations & SBOM (from v0.4) remain available.

🧠 Hierarchical approvals (new)

On devit.tool_call, DevIt accepts approval if it matches:

inner key devit.tool_call:<tool> (e.g., devit.tool_call:shell_exec);

outer key devit.tool_call.
Consumption priority: inner.once > outer.once > inner.session > outer.session > inner.always > outer.always.
Each consumption is audited (approval_key: inner|outer).

Quick examples:

Single run for shell:

devit-mcp --cmd 'devit-mcpd --yes' \
  --call server.approve --json '{"name":"devit.tool_call:shell_exec","scope":"once"}'


Whole session for all devit.tool_call:

devit-mcp --cmd 'devit-mcpd --yes' \
  --call server.approve --json '{"name":"devit.tool_call","scope":"session"}'

🧩 CLI interop (stabilized)

devit-mcpd now forwards clean JSON to devit tool call -:

{"name":"<tool>", "args":{...}, "yes":true}


(stdin input, single JSON value on stdout, logs on stderr.)

🧪 Quick sanity
DEVIT_TUI_HEADLESS=1 devit-tui --journal-path .devit/journal.jsonl
DEVIT_TUI_HEADLESS=1 devit-tui --open-diff .devit/reports/sample.diff
DEVIT_TUI_HEADLESS=1 devit-tui --open-log .devit/journal.jsonl --seek-last 10

devit-mcp --cmd 'devit-mcpd --yes --profile safe' \
  --call server.approve --json '{"name":"devit.tool_call","scope":"once"}'
devit-mcp --cmd 'devit-mcpd --yes --profile safe' \
  --call devit.tool_call --json '{"name":"shell_exec","args":{"cmd":"printf hi\n"}}'

🧰 VS Code

“DevIt” panel (audit timeline), Approve Last Request, Run Recipe…

Code Actions: Rust (add-ci), JS (Jest→Vitest), CI (no workflow).

.vsix packaging via vsce.

⚠️ Notes

On constrained systems, consider raising --mem-mb (e.g., 2048) on devit-mcpd.

Policies differ by profile (safe|std|danger); approval prompts vary accordingly.

## v0.4.0-rc.1

Sécurité & Observabilité
- Redaction centrale des secrets (MCP): `--secrets-scan`, `--redact-placeholder`; patterns configurables via `.devit/devit.toml`.
- Sandbox: `--sandbox bwrap|none`, `--net off|full`, limites `--cpu-secs`/`--mem-mb`; erreurs structurées (`sandbox_unavailable`, `bwrap_exec_failed`, `rlimit_set_failed`).

Supply chain
- SBOM CycloneDX: `devit sbom gen --out .devit/sbom.cdx.json` + audit SHA256 dans `.devit/journal.jsonl`.
- Attestation diff (SLSA‑lite): JSONL signé sous `.devit/attestations/YYYYMMDD/attest.jsonl`; CLI `--attest-diff|--no-attest-diff`.

I/O JSON robustes
- DevIt CLI: `devit tool call - --json-only` (stdout strictement JSON, logs → stderr).
- MCPD: input via stdin, parse “dernière valeur JSON valide”, `child_invalid_json` sinon; option debug `--child-dump-dir`.

## v0.3.0

Highlights
- Pre-commit gate (fs_patch_apply): lint/format checks (Rust/JS/Python) before apply; bypass policy; normalized errors.
- Impacted tests runner: `devit test impacted` (cargo/npm/pytest/ctest heuristics) + JUnit minimal.
- Commit messages (Conventional Commits): generator + integration in fs_patch_apply and run; scopes alias; provenance footer.
- Reports: SARIF/JUnit exporters; Quality gate aggregation + thresholds; PR annotation and artifacts.
- Mini pipeline: fs_patch_apply → precommit → apply → impacted tests (+ optional revert) → commit message.
- PR summary: enriched summary.md (proposed commit subject + SHA).
- Merge-assist: explain/apply + one-shot resolve (auto plan) for simple conflicts.

Details
- fs_patch_apply flags: commit(auto|on|off), commit_type, commit_scope, commit_body_template, commit_dry_run, signoff, no_provenance_footer.
- Quality gate config `[quality]`: max_test_failures, max_lint_errors, allow_lint_warnings, fail_on_missing_reports.
- Commit config `[commit]`: max_subject, scopes_alias, default_type, template_body.
- Summary: `.devit/reports/summary.md` now includes proposed commit and SHA if available.

CI
- Build/test matrix + fmt/clippy (blocking).
- MCP E2E smoke job (non-blocking).
- Reports generation + quality gate aggregation; uploads JUnit/SARIF artifacts.
- PR comment: Quality summary; summary.md consumed by CI step.

## v0.2-rc.2
- MCP server.* tools: policy, health, stats, context_head
- Audit HMAC signé (.devit/journal.jsonl)
- Quotas/rate-limit: max-calls-per-min, cooldown-ms, max-json-kb
- Dry-run global: server.* autorisés uniquement, erreurs normalisées
- Context v1: index.json et `server.context_head`
- Flags kebab-case partout (`--timeout-secs`, etc.)
- Version embarquée: SemVer + git describe/sha exposée par devit-mcpd

## v0.2-rc — Confiance & interop (pre-release)
- Tools JSON I/O: `devit tool list` et `devit tool call -` (stdin JSON → stdout JSON)
- Sandboxed `shell_exec`: safe‑list + best‑effort `net=off`, sortie capturée en JSON
- `fs_patch_apply`: `check_only` et `mode: index|worktree` (JSON args), journalisation d'attestation
- Context map: `devit context map .` → `.devit/index.json` (respect .gitignore; ignore `.devit/`, `target/`, `bench/`)
- Journal JSONL signé (HMAC) sous `.devit/journal.jsonl`; option `git.use_notes` pour `git notes`
- CI stricte: fmt/clippy/tests avec timeout; validation Conventional Commits; politique nommage de branches
- Expérimental (feature-gated): binaire `devit-mcp` (client MCP stdio)
  - Build/run: `cargo run -p devit-cli --features experimental --bin devit-mcp -- --help`

## v0.1.0-alpha1
- CLI patch-only : `suggest`, `apply`, `run`, `test`
- Approvals : untrusted/on-request/on-failure/never
- Sandbox (MVP) : read-only/workspace-write
- Backend local : OpenAI-like + fallback Ollama `/api/chat`
- Timeouts tests + sortie structurée
- Bench dossier `bench/` (Lite – expérimental)
