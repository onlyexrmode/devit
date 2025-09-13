Contributing to DevIt

Setup
- Install Rust stable and `git`.
- Build: `cargo build --workspace`

Guidelines
- Small, focused PRs. Keep changes minimal and adherent to existing style.
- Preserve policies: approval + sandbox must be enforced in the CLI paths.
- Add/update docs when changing behavior (README/FEATURES_TRACK/ROADMAP).

Testing
- Prefer unit tests for library code (`devit-common`, `devit-tools`).
- Integration tests (`crates/cli/tests`) should avoid network; use temp dirs.

Code Style
- Rust 2021, `cargo fmt`, `cargo clippy -D warnings`.

Security
- Never bypass the sandbox in CLI paths unless `--no-sandbox` is explicitly set.

Release
- Tags `v*` trigger CI release artefacts for the `devit` binary.

