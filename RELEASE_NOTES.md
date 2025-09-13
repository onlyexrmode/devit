# RELEASE_NOTES.md
## v0.2-rc — Confiance & interop (pre-release)
- Tools JSON I/O: `devit tool list` et `devit tool call -` (stdin JSON → stdout JSON)
- Sandboxed `shell_exec`: safe‑list + best‑effort `net=off`, sortie capturée en JSON
- `fs_patch_apply`: `check_only` et `mode: index|worktree` (JSON args), journalisation d'attestation
- Context map: `devit context map .` → `.devit/index.json` (respect .gitignore; ignore `.devit/`, `target/`, `bench/`)
- Journal JSONL signé (HMAC) sous `.devit/journal.jsonl`; option `git.use_notes` pour `git notes`
- CI stricte: fmt/clippy/tests avec timeout; validation Conventional Commits; politique nommage de branches

## v0.1.0-alpha1
- CLI patch-only : `suggest`, `apply`, `run`, `test`
- Approvals : untrusted/on-request/on-failure/never
- Sandbox (MVP) : read-only/workspace-write
- Backend local : OpenAI-like + fallback Ollama `/api/chat`
- Timeouts tests + sortie structurée
- Bench dossier `bench/` (Lite – expérimental)
