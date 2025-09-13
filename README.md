# DevIt
Rust CLI dev agent — patch-only, sandboxed, with local LLMs (Ollama/LM Studio).

![Status](https://img.shields.io/badge/status-alpha-orange)
![License](https://img.shields.io/badge/license-Apache--2.0-blue)
![CI](https://github.com/n-engine/devit/actions/workflows/ci.yml/badge.svg)

Authors: naskel and GPT‑5 Thinking (ChatGPT)

Experimental
- The optional binary `devit-mcp` (stdio MCP client) is feature-gated and not included in release archives.
- Build/run locally with:
  - `cargo run -p devit-cli --features experimental --bin devit-mcp -- --help`
- Status: prototype for tooling interop; API and behavior may change.

v0.2‑rc highlights (Confiance & interop)
- Tools JSON I/O: `devit tool list` and `echo '{"name":...,"args":{...}}' | devit tool call -`
- Sandboxed `shell_exec`: safe‑list + best‑effort `net=off`, output returned as JSON
- `fs_patch_apply`: `check_only` and `mode: index|worktree` via JSON args
- Context map: `devit context map .` → `.devit/index.json` (respects .gitignore; ignores `.devit/`, `target/`, `bench/`)
- Journal JSONL signé (HMAC) sous `.devit/journal.jsonl`; option `git.use_notes`
- Experimental (feature-gated): `devit-mcp` (MCP stdio client). Build/run with:
  - `cargo run -p devit-cli --features experimental --bin devit-mcp -- --help`

English (EN)
- Quickstart
  - Start a local OpenAI‑compatible LLM (LM Studio endpoint, or Ollama /v1).
  - Keep `devit.toml` (defaults: approval=untrusted, sandbox=read-only, net off).
  - Three commands:
    1. `devit suggest --goal "add a smoke test" . > PATCH.diff`
    2. `devit apply PATCH.diff --yes` (read‑only defaults will refuse; switch to workspace‑write to allow)
    3. `devit run --goal "..." --yes` (OnRequest requires `--yes`)
- Installation
  - Requirements: Rust stable, `git`
  - Build: `cargo build --workspace`
  - Makefile shortcuts: `make build`, `make test`, `make fmt-check`, `make smoke`
- Configuration (`devit.toml`)
  - `[backend]`: `kind`, `base_url`, `model`, `api_key`
  - `[policy]`: `approval = untrusted|on-request|on-failure|never`, `sandbox = read-only|workspace-write|danger-full-access`
  - `[sandbox]`: limits (MVP informational)
  - `[git]`: conventions
- Useful global flags
  - `--backend-url` / `--model` to override backend on the fly
  - `--no-sandbox` disables isolation (danger)
  - `--tui` enables TUI (preview/approval)
- Commands
  - `devit suggest --goal "..." [PATH]` → print a unified diff
  - `devit apply [-|PATCH.diff] [--yes] [--force]` → apply + commit (respects policy)
  - `devit run --goal "..." [PATH] [--yes] [--force]` → suggest→apply→commit→test
  - `devit test` → run tests (auto‑detected stack)
  - `devit tool list` → JSON description of tools
  - `echo '{"name":"shell_exec","args":{"cmd":"ls -1 | head"}}' | devit tool call -` → sandboxed shell (JSON I/O)
  - `echo '{"name":"fs_patch_apply","args":{"patch":"<DIFF>","check_only":true}}' | devit tool call -` → dry‑run patch
  - `devit context map .` → writes `.devit/index.json`
  - Experimental: `devit-mcp` (stdio MCP client)
    - `cargo run -p devit-cli --features experimental --bin devit-mcp -- --cmd '<server cmd>' --handshake-only`
    - `cargo run -p devit-cli --features experimental --bin devit-mcp -- --cmd '<server cmd>' --echo "hello"`
  - `devit plan` → list `update_plan.yaml`
  - `devit watch [--diff PATCH.diff]` → continuous TUI (Plan | Diff | Logs)
- Approval policies
  - untrusted: always prompt (ignores `--yes`)
  - on-request: `run` fails without `--yes`; otherwise prompts unless `--yes`
  - on-failure: prompts unless `--yes`; tests allowed
  - never: never prompt
- Sandbox
  - Modes: read‑only (refuses apply/run/test), workspace‑write (OK), danger‑full‑access
  - Safe‑list in read‑only: `git`, `cargo`, `npm`, `ctest`
  - Timeouts via `DEVIT_TIMEOUT_SECS` (kill + message)
  - If `bwrap` is available: network off (`--unshare-net`)
- Logs & plan
  - JSONL: `~/.devit/logs/log.jsonl`: ToolCall, Diff, AskApproval, Info
  - `update_plan.yaml` maintained by `run` (done/failed + JUnit summary + tail)
- TUI
  - `--tui` for run/apply: interactive approval (y/n/q), live logs
    - Navigation: arrows or h/j/k/l; PgUp/PgDn; 1/2/3 to select column
    - Diff colors: + green, − red
  - `devit watch`: continuous TUI (plan yaml / optional diff / JSONL logs)
- MVP limitations
  - OpenAI‑like backend (configurable URL)
  - Non‑streaming diff generation (one‑shot preview)

Français (FR)
- Quickstart
  - Démarrez un LLM local compatible OpenAI (LM Studio, ou Ollama /v1).
  - Gardez `devit.toml` (défauts: approval=untrusted, sandbox=read-only, net off).
  - Trois commandes:
    1. `devit suggest --goal "add a smoke test" . > PATCH.diff`
    2. `devit apply PATCH.diff --yes` (en read‑only, refusera sans assouplir la policy)
    3. `devit run --goal "..." --yes` (en OnRequest, `--yes` requis)
- Installation
  - Prérequis: Rust stable, `git`
  - Build: `cargo build --workspace`
  - Raccourcis Makefile: `make build`, `make test`, `make fmt-check`, `make smoke`
- Configuration (`devit.toml`)
  - `[backend]`: `kind`, `base_url`, `model`, `api_key`
  - `[policy]`: `approval = untrusted|on-request|on-failure|never`, `sandbox = read-only|workspace-write|danger-full-access`
  - `[sandbox]`: limites (MVP informatif)
  - `[git]`: conventions
- Flags globaux utiles
  - `--backend-url` / `--model` pour override ponctuel
  - `--no-sandbox` désactive l’isolation (danger)
  - `--tui` active les TUI (aperçu/approbation)
- Commandes
  - `devit suggest --goal "..." [PATH]` → imprime un diff
  - `devit apply [-|PATCH.diff] [--yes] [--force]` → applique + commit (respecte policy)
  - `devit run --goal "..." [PATH] [--yes] [--force]` → suggest→apply→commit→test
  - `devit test` → exécute les tests (stack auto)
  - `devit tool list` → description JSON des outils
  - `echo '{"name":"shell_exec","args":{"cmd":"ls -1 | head"}}' | devit tool call -` → shell sandboxé (I/O JSON)
  - `echo '{"name":"fs_patch_apply","args":{"patch":"<DIFF>","check_only":true}}' | devit tool call -` → dry‑run du patch
  - `devit context map .` → écrit `.devit/index.json`
  - Expérimental: `devit-mcp` (client MCP stdio)
    - `cargo run -p devit-cli --features experimental --bin devit-mcp -- --cmd '<serveur MCP>' --handshake-only`
    - `cargo run -p devit-cli --features experimental --bin devit-mcp -- --cmd '<serveur MCP>' --echo "hello"`
  - `devit plan` → liste `update_plan.yaml`
  - `devit watch [--diff PATCH.diff]` → TUI continu (Plan | Diff | Logs)
- Policies d’approbation
  - untrusted: demande toujours (ignore `--yes`)
  - on-request: `run` échoue sans `--yes`; sinon demande sauf `--yes`
  - on-failure: demande sauf `--yes`; tests libres
  - never: ne demande jamais
- Sandbox
  - Modes: read-only (refuse apply/run/test), workspace-write (OK), danger-full-access
  - Safe‑list en read‑only: `git`, `cargo`, `npm`, `ctest`
  - Timeouts via `DEVIT_TIMEOUT_SECS` (kill + message)
  - Si `bwrap` disponible: réseau coupé (`--unshare-net`)
- Journal & plan
  - JSONL: `~/.devit/logs/log.jsonl`: ToolCall, Diff, AskApproval, Info
  - `update_plan.yaml` maintenu par `run` (status done/failed + résumé JUnit + tail)
- TUI
  - `--tui` pendant run/apply: approbation interactive (y/n/q), logs en live
    - Navigation: flèches ou h/j/k/l; PgUp/PgDn; 1/2/3 sélection de colonne
    - Diff colorisé: lignes + en vert, − en rouge
  - `devit watch`: TUI continu (plan yaml / diff optionnel / logs JSONL)
- Limitations MVP
  - Backend OpenAI‑like (URL configurable)
  - TUI non‑streaming pour la génération de diff (aperçu ponctuel)
