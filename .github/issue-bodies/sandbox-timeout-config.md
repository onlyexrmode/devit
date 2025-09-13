### Feature
Sandbox: Timeouts configurable via `devit.toml` (in addition to ENV).

### Rationale
Make timeouts reproducible and versioned per repo.

### Tasks
- Add `[sandbox] timeout_secs = <int>` in config
- Use config value as default; ENV overrides remain supported
- Document precedence (ENV > config > default)

### DoD
- Long-running commands respect configured timeouts across environments.

