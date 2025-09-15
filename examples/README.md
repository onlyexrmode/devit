# Examples — Precommit Gate (fs_patch_apply)

What it is
- Runs basic format/lint checks before applying a patch.
- Fails fast if checks fail; optional bypass controlled by policy/profile.

Supported checks (auto-detected)
- Rust: `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`
- JS/TS: if `package.json`: `npm run -s lint` or `npx eslint .`; if Prettier config: `npx prettier -c .`
- Python: if `pyproject.toml`: `ruff check`; else if `tox.ini`/`pytest.ini`: `ruff -q .`
- C/C++: if `CMakeLists.txt`: `cmake-lint` when available (best-effort)
- Timeout per family: `DEVIT_TIMEOUT_SECS` (default 120s)

Usage — JSON I/O
- Dry-run apply (no changes):
  - `echo '{"name":"fs_patch_apply","args":{"patch":"<DIFF>","check_only":true}}' | devit tool call -`
- Precommit only (run checks, then exit):
  - `echo '{"name":"fs_patch_apply","args":{"patch":"<DIFF>","precommit_only":true}}' | devit tool call -`
- Bypass precommit (requires allowed profile and `--yes`):
  - `echo '{"name":"fs_patch_apply","args":{"patch":"<DIFF>","no_precommit":true}}' | devit tool call - --yes`

Usage — Legacy CLI flags
- Precommit only:
  - `devit tool call fs_patch_apply - --precommit-only`
- Bypass precommit (requires allowed profile and `--yes`):
  - `devit tool call fs_patch_apply - --yes --no-precommit`

Error shapes (normalized)
- Precommit failure:
  - `{ "precommit_failed": true, "tool": "clippy|eslint|prettier|ruff|fmt", "exit_code": 1, "stderr": "..." }`
- Bypass not allowed (profile or approval):
  - `{ "approval_required": true, "policy": "on_request", "phase": "pre", "reason": "precommit_bypass" }`

Sample config (devit.toml)

```
[precommit]
rust = true
javascript = true
python = true
additional = [
  # "bash -lc 'make lint'"
]
fail_on = ["rust","javascript","python"]
allow_bypass_profiles = ["danger"]
```

Notes
- Families not listed in `fail_on` run as best-effort (non-blocking).
- Bypass requires `--yes` and a profile listed in `allow_bypass_profiles`.
