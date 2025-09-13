# DevIt
Rust CLI dev agent — patch-only, sandboxed, with local LLMs (Ollama/LM Studio).

![Status](https://img.shields.io/badge/status-alpha-orange)
![License](https://img.shields.io/badge/license-Apache--2.0-blue)
![CI](https://github.com/<you>/devit/actions/workflows/ci.yml/badge.svg)

**Author:** GPT-5 Thinking (ChatGPT)
**Co-Author:** N-Engine

Quickstart
- Start a local LLM backend (e.g., LM Studio OpenAI endpoint, Ollama w/ /v1).
- Ensure `devit.toml` exists (defaults: approval=untrusted, sandbox=read-only, net off).
- Three commands:
  1. `devit suggest --goal "add a smoke test" . > PATCH.diff`
  2. `devit apply PATCH.diff --yes` (en read-only, refusera sans assouplir la policy)
  3. `devit run --goal "..." --yes` (en OnRequest, `--yes` requis)

DevIt propose des diffs unifiés, les applique après approbation selon une policy, et exécute les tests dans une sandbox.

Installation
- Prérequis: Rust stable, `git`
- Build: `cargo build --workspace`

Configuration (`devit.toml`)
- `[backend]`: `kind`, `base_url`, `model`, `api_key`
- `[policy]`: `approval = untrusted|on-request|on-failure|never`, `sandbox = read-only|workspace-write|danger-full-access`
- `[sandbox]`: limites (MVP informatif)
- `[git]`: conventions

Flags globaux utiles
- `--backend-url` / `--model` pour override ponctuel
- `--no-sandbox` désactive l’isolation (danger)
- `--tui` active les TUI (aperçu/approbation)

Commandes
- `devit suggest --goal "..." [PATH]` → imprime un diff
- `devit apply [-|PATCH.diff] [--yes] [--force]` → applique + commit (respecte policy)
- `devit run --goal "..." [PATH] [--yes] [--force]` → suggest→apply→commit→test
- `devit test` → exécute les tests (stack auto)
- `devit plan` → liste `update_plan.yaml`
- `devit watch [--diff PATCH.diff]` → TUI continu (Plan | Diff | Logs)

Policies d’approbation
- untrusted: demande toujours (ignore `--yes`)
- on-request: `run` échoue sans `--yes`; sinon demande sauf `--yes`
- on-failure: demande sauf `--yes`; tests libres
- never: ne demande jamais

Sandbox
- Modes: read-only (refuse apply/run/test), workspace-write (OK), danger-full-access
- Safe‑list en read‑only: `git`, `cargo`, `npm`, `ctest`
- Timeouts via `DEVIT_TIMEOUT_SECS` (kill + message)
- Si `bwrap` disponible: réseau coupé (`--unshare-net`)

Journal & plan
- JSONL: `~/.devit/logs/log.jsonl`: ToolCall, Diff, AskApproval, Info
- `update_plan.yaml` maintenu par `run` (status done/failed + notes JUnit/tail)

TUI
- `--tui` pendant run/apply: approbation interactive (y/n/q), logs en live
  - Navigation: flèches ou h/j/k/l pour changer de colonne et scroller; PgUp/PgDn; 1/2/3 sélection colonne
  - Diff colorisé: lignes + en vert, - en rouge
- `devit watch`: TUI continu (plan yaml / diff optionnel / logs JSONL)

Limitations MVP
- LLM backend OpenAI‑like (URL configurable)
- TUI non‑streaming pour la génération de diff (aperçu ponctuel)
