# RELEASE_NOTES.md

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
