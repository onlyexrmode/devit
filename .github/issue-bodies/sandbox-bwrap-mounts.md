### Feature
Sandbox: Bubblewrap mounts (workspace RW/RO per policy, HOME RO, tmp private).

### Rationale
Predictable filesystem access and isolation.

### Tasks
- Detect repo root and mount as RW (workspace-write) or RO (read-only)
- Mount HOME as RO; provide a tmp dir private for processes
- Propagate env/argv safely; block network (`--unshare-net` already)

### DoD
- `apply/run/test` behave correctly with mounts; writes blocked in read-only.

