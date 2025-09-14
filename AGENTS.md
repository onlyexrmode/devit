# Agent Notes (Resumption Guide)

Purpose
- Snapshot of current state and how to resume smoothly next time.

State Summary (v0.2.0-rc.2)
- Flags: kebab-case enforced; Makefile `lint-flags` added.
- CI: added non-blocking `lint_meta` (flags + JSON errors); ensured scripts work in GH runners.
- MCP (server/client):
  - Server: policy/health/stats/context_head/echo/devit.tool_* implemented.
  - Watchdog: `--max-runtime-secs` exits with code 2 and stderr message.
  - Stats reset: `server.stats.reset` tool + client `--stats-reset`.
  - Approval profiles: `safe|std|danger` + overrides, merged into effective policies; `policy_dump` reports `profile`.
  - Schema validation: normalized `schema_error` for `devit.tool_call` and `plugin.invoke` (path + reason).
  - Version: embedded from build.rs (SemVer + git describe/sha), surfaced in `version`, `server.policy`, `server.health`.
  - Plugins: `plugin.invoke` validates manifest (`id`, semver-ish `version`, rel-safe `wasm`/`allowed_dirs`, presence of wasm) and optional `args_schema`; executes via `devit-plugin`.
- Provenance:
  - `[provenance] footer=true` → adds "DevIt-Attest: <hash>" trailer to commits.
  - `[git] use_notes=true` → adds git notes with attestation (non-blocking on failure).
- Sample config: `examples/devit.sample.toml` (includes provenance + MCP profiles).

Open Items / Next Steps
- lint_errors.sh: rate-limit cooldown check currently launches fresh servers per call. If needed, adapt to reuse a single `devit-mcpd` process for the cooldown test (stdio loop), otherwise keep as-is.
- Plugin E2E: CI typically lacks `wasmtime`; keep plugin E2E optional/skipped unless runtime present.
- Optional: extend attestation (HMAC tool+args+ts) to other actions if desired.

Local Test/Build Cheatsheet
- Quick checks: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace --all-targets --no-fail-fast -- --nocapture`
- MCP E2E: `make e2e-mcp`
- Lints: `make lint-flags` and `scripts/lint_errors.sh`
- Build experimental bins: `cargo build -p devit-cli --features experimental --bins`

Release/Tags (already used for rc.2)
- Tag env: `TAG=v0.2.0-rc.2` in Makefile.
- Draft/publish: `make release-draft` / `make release-publish` (requires `gh`).

Interaction Guidelines (for future sessions)
- Defaults: keep outputs concise; prefer listing changed files over large diffs/logs.
- Before running commands: 1–2 line preamble; group related actions.
- Plans: use `update_plan` for multi-step tasks.
- Approvals: ask 1 clear question when a decision is needed (push/non-push, destructive ops).
- Commits: Conventional Commits ≤72 chars; do not push until requested (unless explicitly allowed).
- CI ergonomics: prefer non-blocking metadata jobs; tests must be green locally before proposing push.

Notes for Repo Owner
- Config reference: `examples/devit.sample.toml`; copy to `devit.toml` and adjust.
- Footer vs notes: enable both (recommended) for visibility + non-intrusive provenance.

