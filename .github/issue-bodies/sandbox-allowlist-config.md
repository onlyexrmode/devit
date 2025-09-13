### Feature
Sandbox: Configurable allow-list for commands in read-only mode.

### Rationale
Different stacks may need extra safe commands (e.g., `pytest`, `go test`).

### Tasks
- Add `[sandbox] allowlist = ["git", "cargo", ...]` in `devit.toml`
- Enforce allow-list in Sandbox; merge with defaults
- Clear error message on violation

### DoD
- Projects can add commands to run tests without relaxing policy.

