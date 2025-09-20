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
- `fs_patch_apply`: precommit gate (lint/format) + `check_only` and `mode: index|worktree`
  - Integrated pipeline: optional impacted tests after apply; auto revert on fail (configurable)
  - Commit stage: Conventional Commits auto-message and commit (profile/flags)
- Context map: `devit context map .` → `.devit/index.json` (respects .gitignore; ignores `.devit/`, `target/`, `bench/`)
- Journal JSONL signé (HMAC) sous `.devit/journal.jsonl`; option `git.use_notes`
  - Provenance (footer/notes): activer le footer via `[provenance] footer=true`; ajouter des notes via `[git] use_notes=true`.
- Experimental (feature-gated): `devit-mcp` (MCP stdio client). Build/run with:
  - `cargo run -p devit-cli --features experimental --bin devit-mcp -- --help`

Plugins (WASM/WASI)
- Experimental, feature-gated. Run with `--features experimental`.
- Registry: `.devit/plugins/<id>/devit-plugin.toml` (or `DEVIT_PLUGINS_DIR`).
- Manifest example (`devit-plugin.toml`):
  - `id = "echo_sum"`
  - `name = "Echo Sum"`
  - `wasm = "echo_sum.wasm"`
  - `version = "0.1.0"`
  - `allowed_dirs = []` (optional preopened dirs)
  - `env = []` (optional `KEY=VALUE` entries)
- Build example plugin:
  - Install WASI target (new naming): `rustup target add wasm32-wasip1` (or `wasm32-wasi` on older toolchains)
  - `cargo build -p devit-plugin-echo-sum --target wasm32-wasip1 --release` (from `examples/plugins/echo_sum`)
  - Copy to registry: `mkdir -p .devit/plugins/echo_sum && cp examples/plugins/echo_sum/target/wasm32-wasip1/release/echo_sum.wasm .devit/plugins/echo_sum/`
  - Write manifest per above.
- CLI usage (JSON I/O):
  - List: `cargo run -p devit-cli --features experimental --bin devit-plugin -- list`
  - Invoke by id: `echo '{"a":1,"b":2}' | cargo run -p devit-cli --features experimental --bin devit-plugin -- invoke --id echo_sum`
  - Or by manifest: `echo '{"a":1,"b":2}' | cargo run -p devit-cli --features experimental --bin devit-plugin -- invoke --manifest .devit/plugins/echo_sum/devit-plugin.toml`
  - Timeouts: `DEVIT_TIMEOUT_SECS` (default 30s). Timeout exit code: 124.

English (EN)
Security & Supply‑chain (v0.4)
- Secrets redaction (MCP): enable `--secrets-scan` (or `[secrets].scan=true`), configure `placeholder` and `patterns` in `.devit/devit.toml`.
- Sandbox: `--sandbox bwrap|none`, `--net off|full`, `--cpu-secs`, `--mem-mb` (recommended defaults: bwrap + net off when available).
- SBOM CycloneDX: `devit sbom gen --out .devit/sbom.cdx.json` (audit sha256 in `.devit/journal.jsonl`).
- Attestation (SLSA‑lite): JSONL under `.devit/attestations/YYYYMMDD/attest.jsonl`; CLI `--attest-diff|--no-attest-diff`.
- Robust JSON I/O: `devit tool call - --json-only`; MCPD parses the last valid JSON and exposes `child_invalid_json` when needed; raw dumps via `--child-dump-dir`.
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
  - `[precommit]`: pre‑apply checks (Rust/JS/Python/extra) and bypass policy
  - `[quality]`: thresholds for tests/lint in CI; `max_test_failures`, `max_lint_errors`, `allow_lint_warnings`, `fail_on_missing_reports`
  - `[commit]`: Conventional Commits (max_subject, scopes_alias, default_type, template_body)
- Useful global flags
  - `--backend-url` / `--model` to override backend on the fly
  - `--no-sandbox` disables isolation (danger)
  - `--tui` enables TUI (preview/approval)
- Commands
  - `devit suggest --goal "..." [PATH]` → print a unified diff
  - `devit apply [-|PATCH.diff] [--yes] [--force]` → apply + commit (respects policy)
  - `devit run --goal "..." [PATH] [--yes] [--force]` → suggest→apply→commit→test
  - `devit test` → run tests (auto‑detected stack)
  - `devit test impacted [--changed-from <ref>] [--framework auto|cargo|npm|pytest|ctest]` → run only impacted tests
  - `devit commit-msg [--from-staged|--from-ref <ref>] [--type <t>] [--scope <s>] [--with-template] [--write]` → Conventional Commit subject
  - `devit commit-msg [--from-staged|--from-ref <ref>] [--type <t>] [--scope <s>] [--with-template] [--write]` → Conventional Commit subject
  - `devit report sarif|junit|summary` → ensure/export reports; `summary` writes `.devit/reports/summary.md`
  - `devit quality gate --junit .devit/reports/junit.xml --sarif .devit/reports/sarif.json --json` → aggregate + thresholds
  
Fs Patch Apply — integrated commit

- JSON flags via `devit tool call -` (fs_patch_apply):
  - `commit`: `auto|on|off` (default auto; safe/std=on, danger=auto)
  - `commit_type`, `commit_scope`, `commit_body_template`, `commit_dry_run`, `signoff`, `no_provenance_footer`
- Outputs:
  - Success with commit: `{ ok:true, committed:true, commit_sha, type, scope, subject, msg_path }`
  - Success without commit (off/dry-run): `{ ok:true, committed:false, type, scope, subject, msg_path }`
  - Errors: `approval_required` (commit stage) or `git_commit_failed`
- Provenance: adds “DevIt-Attest: …” footer if enabled (can be disabled per-call).

Run — commit message

- `devit run` uses the same generator (auto scope + alias, heuristic type) and preserves provenance footer when enabled.
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
 - Quality gate
   - Aggregates `.devit/reports/junit.xml` and `.devit/reports/sarif.json` with thresholds from `[quality]`
   - CLI: `devit quality gate --json`; Summary: `devit report summary`
   - Flaky tests: list patterns in `.devit/flaky_tests.txt` to ignore in threshold (reported separately)
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
  - `[precommit]`: vérifs pré‑apply (Rust/JS/Python/extra) et bypass
  - `[quality]`: seuils tests/lint pour CI; `max_test_failures`, `max_lint_errors`, `allow_lint_warnings`, `fail_on_missing_reports`
- Flags globaux utiles
  - `--backend-url` / `--model` pour override ponctuel
  - `--no-sandbox` désactive l’isolation (danger)
  - `--tui` active les TUI (aperçu/approbation)
- Commandes
  - `devit suggest --goal "..." [PATH]` → imprime un diff
  - `devit apply [-|PATCH.diff] [--yes] [--force]` → applique + commit (respecte policy)
  - `devit run --goal "..." [PATH] [--yes] [--force]` → suggest→apply→commit→test
  - `devit test` → exécute les tests (stack auto)
  - `devit test impacted [--changed-from <ref>] [--framework auto|cargo|npm|pytest|ctest]` → tests impactés uniquement
  - `devit commit-msg [--from-staged|--from-ref <ref>] [--type <t>] [--scope <s>] [--with-template] [--write]` → Conventional Commits
  - `devit report sarif|junit|summary` → export; `summary` écrit `.devit/reports/summary.md`
  - `devit quality gate --junit .devit/reports/junit.xml --sarif .devit/reports/sarif.json --json` → agrégat + seuils
  - `devit tool list` → description JSON des outils
  - `echo '{"name":"shell_exec","args":{"cmd":"ls -1 | head"}}' | devit tool call -` → shell sandboxé (I/O JSON)
  - `echo '{"name":"fs_patch_apply","args":{"patch":"<DIFF>","check_only":true}}' | devit tool call -` → dry‑run du patch
  - Porte pré‑commit: DevIt exécute des checks (Rust/JS/Python) avant d’appliquer; échec → apply refusé.
  - `devit context map .` → écrit `.devit/index.json`
  - Expérimental: `devit-mcp` (client MCP stdio)
    - `cargo run -p devit-cli --features experimental --bin devit-mcp -- --cmd '<serveur MCP>' --handshake-only`
    - `cargo run -p devit-cli --features experimental --bin devit-mcp -- --cmd '<serveur MCP>' --echo "hello"`

### DevIt TUI (ratatui) — démarrage rapide

- Préparer un journal : `devit run --goal "..." --yes` produit `.devit/journal.jsonl` (ou utilisez les rapports générés par la CI).
- Lancer l’interface : `cargo run -p devit-tui -- --open-log .devit/journal.jsonl`.
- Navigation principale :
  - `↑/↓` pour parcourir la timeline, `F` active/désactive le suivi des nouveaux events.
  - `R` ouvre le panneau “Recipes” (sélection `↑/↓`, `Enter` pour dry-run, `O` pour afficher le diff, `A` pour appliquer, `Esc` pour revenir).
  - Si un diff est ouvert : `j/k` changent de hunk, `h/H` changent de fichier, `Esc` ferme la vue diff.
  - `F1` affiche l’aide contextuelle, `q` quitte.
- Mode headless : `DEVIT_TUI_HEADLESS=1 devit-tui --open-log .devit/journal.jsonl` imprime l’event sélectionné (compatible CI/scripts).

Recettes (TUI ↔ CLI)
- Lister les recettes (headless‑friendly):
  - `DEVIT_TUI_HEADLESS=1 cargo run -p devit-tui -- --list-recipes | jq`
- Exécuter une recette en dry‑run (headless):
  - `DEVIT_TUI_HEADLESS=1 cargo run -p devit-tui -- --run-recipe add-ci --dry-run`
  - Codes de sortie: 0 = succès, 2 = `approval_required` (rejouer après approbation)
  - Erreurs normalisées sur stderr: `{ error: { recipe_integration_failed:true, reason:"list_failed|run_failed|no_patch" } }`
- Interactif:
  - `R` → liste des recettes → `Enter` lance un dry‑run
  - Si un patch est généré, le viewer diff s’ouvre (puis `A` pour appliquer)
  - `--run-recipe <id> --dry-run` ouvre directement la preview du diff si disponible

Plugins (WASM/WASI)
- Expérimental (feature-gated). Utiliser `--features experimental`.
- Registry: `.devit/plugins/<id>/devit-plugin.toml` (ou `DEVIT_PLUGINS_DIR`).
- Exemple de manifeste (`devit-plugin.toml`) :
  - `id = "echo_sum"`, `name = "Echo Sum"`, `wasm = "echo_sum.wasm"`, `version = "0.1.0"`
  - `allowed_dirs = []` (répertoires pré-ouverts facultatifs), `env = []` (variables `KEY=VALUE`).
- Construire l’exemple:
  - Installer la cible WASI (nouvelle dénomination): `rustup target add wasm32-wasip1` (ou `wasm32-wasi`)
  - `cargo build -p devit-plugin-echo-sum --target wasm32-wasip1 --release` (depuis `examples/plugins/echo_sum`)
  - Copier: `mkdir -p .devit/plugins/echo_sum && cp examples/plugins/echo_sum/target/wasm32-wasip1/release/echo_sum.wasm .devit/plugins/echo_sum/`
  - Écrire le manifeste comme ci-dessus.
- CLI (I/O JSON):
  - Lister: `cargo run -p devit-cli --features experimental --bin devit-plugin -- list`
  - Invoquer par id: `echo '{"a":1,"b":2}' | cargo run -p devit-cli --features experimental --bin devit-plugin -- invoke --id echo_sum`
  - Ou par manifeste: `echo '{"a":1,"b":2}' | cargo run -p devit-cli --features experimental --bin devit-plugin -- invoke --manifest .devit/plugins/echo_sum/devit-plugin.toml`
  - Timeout: `DEVIT_TIMEOUT_SECS` (défaut 30s). Code sortie timeout: 124.
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
## MCP (expérimental)

Binaire client : `devit-mcp`

Binaire serveur : `devit-mcpd`

Outils exposés (server):

- `server.policy` — état effectif (approvals, limites, audit)
- `server.health` — uptime + dépendances (devit, devit-plugin, wasmtime)
- `server.stats` — compteurs d’appels par outil
- `devit.tool_list` — proxy de `devit tool list`
- `devit.tool_call` — proxy de `devit tool call -` (JSON stdin → JSON stdout)
- `plugin.invoke` — proxy de `devit-plugin invoke --id <id>` (JSON stdin → JSON stdout)
- `echo` — outil de test

Profils d'approbation (server)

- Config (`.devit/devit.toml`):

```
[mcp]
profile = "safe" # ou "std" | "danger"
[mcp.approvals]
# overrides spécifiques par outil (facultatif)
"server.stats.reset" = "never"
```

- Presets:
  - safe: `devit.tool_call=on_request`, `plugin.invoke=on_request`, `server.*=never`
  - std: `devit.tool_call=on_failure`, `plugin.invoke=on_request`, `server.*=never`
  - danger: `devit.tool_call=never`, `plugin.invoke=on_failure`, `server.*=never`
- Inspecter la politique effective:

```
devit-mcp --cmd 'devit-mcpd --yes' --policy | jq
# JSON inclut: { "profile": "safe|std|danger|none", "tools": { ... } }
```

Flags utiles (client) :

- `--policy`, `--health`, `--stats`, `--call <name> --json '<payload>'`

Flags utiles (serveur) :

- `--yes` (auto-approve), `--policy-dump`, `--no-audit`
- `--max-calls-per-min`, `--max-json-kb`, `--cooldown-ms`
- `--devit-bin`, `--devit-plugin-bin`, `--timeout-secs`
- `--max-runtime-secs` (watchdog global: arrêt propre au bout de N secondes)

Exemples :

Handshake :

```
devit-mcp --cmd 'devit-mcpd --yes' --handshake-only
```

Politique côté serveur :

```
devit-mcp --cmd 'devit-mcpd --yes' --policy | jq
```

Lancer mcpd avec des flags typiques (profil/réseau/limites):

```
devit-mcpd --yes --profile safe --sandbox bwrap --net off --cpu-secs 30 --mem-mb 1024
```

Approvals rapides (outer/inner) — voir `docs/approvals.md` pour les détails hiérarchiques:

```
# Accorder une fois pour shell_exec (inner)
devit-mcp --cmd 'devit-mcpd --yes' --call server.approve --json '{"name":"devit.tool_call:shell_exec","scope":"once"}'

# Accorder pour la session entière (outer)
devit-mcp --cmd 'devit-mcpd --yes' --call server.approve --json '{"name":"devit.tool_call","scope":"session"}'
```

Santé et stats :

```
devit-mcp --cmd 'devit-mcpd --yes' --health | jq
devit-mcp --cmd 'devit-mcpd --yes' --stats | jq

Réinitialiser les compteurs (server.stats.reset) :

```
# Après quelques appels, remets les compteurs à zéro
devit-mcp --cmd 'devit-mcpd --yes' --stats-reset | jq
# Vérifier
devit-mcp --cmd 'devit-mcpd --yes' --stats | jq '.payload.stats.totals'
```

Watchdog global (arrêt après N secondes) :

```
# Le serveur s'arrête proprement après 1s (exit 2), message clair sur stderr
devit-mcp --cmd 'devit-mcpd --yes --max-runtime-secs 1' --policy || echo "exit=$?"
```

## Dépannage mcpd (rapide)

- Mémoire insuffisante ("Cannot allocate memory" / "memory allocation ... failed")
  - Augmenter la limite: `devit-mcpd --yes --mem-mb 2048` (ou plus selon l’environnement)
- Délai trop court
  - Allonger: `devit-mcpd --yes --timeout-secs 60` ou `DEVIT_TIMEOUT_SECS=60 devit-mcpd --yes`
- bwrap absent (sandbox_unavailable)
  - Installer bubblewrap, ou lancer sans bwrap: `--sandbox none` (les limites CPU/Mémoire restent actives via rlimits)
- child_invalid_json (sortie enfant non JSON)
  - Activer les dumps: `--child-dump-dir .devit/reports` puis inspecter `child_*.stdout.log` / `child_*.stderr.log`
- Approvals trop fréquents
  - Accorder côté outer/inner: `server.approve` (ex.: `devit.tool_call:shell_exec` ou `devit.tool_call`) — voir `docs/approvals.md`
- Réseau bloqué en sandbox bwrap
  - Par défaut `--net off` (isolé). Activer: `--net full` si nécessaire
- Proxy server.* refusé depuis devit.tool_call
  - Message `server_tool_proxy_denied`: appelez directement l’outil `server.*` souhaité
- Variables d’environnement refusées
  - `secrets_env_denied`: variable non autorisée. Utiliser l’allowlist adéquate dans la config, ou éviter `args.env`

Config d'exemple

- Un fichier complet d'exemple est disponible: `examples/devit.sample.toml`.
- Copiez-le à la racine sous le nom `devit.toml` et adaptez:
  - `[provenance] footer=true` pour ajouter un trailer "DevIt-Attest" dans les commits
  - `[git] use_notes=true` pour ajouter des `git notes` d'attestation
  - `[mcp] profile = "safe|std|danger"` et éventuels overrides `[mcp.approvals]`

Appel de tool :

```
devit-mcp --cmd 'devit-mcpd --yes' --call devit.tool_list --json '{}'
```

Plugin WASI (si echo_sum.wasm installé) :

```
echo '{"id":"echo_sum","payload":{"a":2,"b":40}}' | devit-mcp --cmd 'devit-mcpd --yes' --call plugin.invoke --json @-
```
smoke llm 2025-09-15T16:19:26+02:00
smoke llm 2025-09-15T16:41:50+02:00
