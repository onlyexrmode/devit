# CONTRIBUTING.md
Merci de contribuer à DevIt 💚

## Setup
- Rust stable, `cargo build --workspace`.
- Backend local (LM Studio / Ollama).
- `DEVIT_CONFIG` pour pointer un `devit.toml`.

## Workflow
- Issues → Discussions → PR.
- Commits Conventional: `feat: …`, `fix: …` (≤72 chars).
- PR = patchs minimaux, tests, description claire, reproduction.

## Code style
- `cargo fmt`, `cargo clippy -D warnings`.
- Pas de side-effects implicites : **diff-first**.

## Tests
- `cargo test --workspace`.
- Pour le bench: voir `bench/README.md` (optionnel).

