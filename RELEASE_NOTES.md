# RELEASE_NOTES.md
# RELEASE_NOTES.md
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
