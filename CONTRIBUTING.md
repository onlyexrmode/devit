# CONTRIBUTING.md
Merci de contribuer à DevIt 💚

## Setup
- Rust stable, `cargo build --workspace`.
- Backend local (LM Studio / Ollama).
- `DEVIT_CONFIG` pour pointer un `devit.toml`.

## Workflow
- Branches: travail sur `feat/*` ou `fix/*` (CI bloque sinon). Exceptions permises: `chore/*`, `docs/*`, `refactor/*`, `test/*`, `ci/*`, `release/*`, `dependabot/*`.
- Commits Conventional (≤72 chars): `feat: …`, `fix: …`, `chore: …`, `docs: …`, `test: …`, `refactor: …`. Scope optionnel: `feat(cli): …`. Pour des nouveautés risquées, utilisez un scope `(experimental)` si pertinent.
- PR = patchs minimaux, tests, description claire (voir template). Mentionner approvals/sandbox/timeouts.

## Code style
- `cargo fmt`, `cargo clippy -D warnings`.
- Pas de side-effects implicites : **diff-first**.

## Tests
- `cargo test --workspace --all-targets --no-fail-fast`.
- Timeout conseillé via `DEVIT_TIMEOUT_SECS` dans CI.
- Pour le bench: voir `bench/README.md` (optionnel).
