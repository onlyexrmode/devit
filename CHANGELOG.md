Changelog (from FEATURES_TRACK)

Sprint 1
- Approval policy enum; config validation; decision table.
- CLI enforcements for `apply/run` with `--yes` semantics.

Sprint 2
- Sandbox modes (read-only/workspace-write/danger) + safe-list.
- Bwrap detection (`--unshare-net`), timeouts via env.
- Route apply/run/test through Sandbox. JSONL logs.

Sprint 3
- `update_plan.yaml` (done/failed + JUnit summary + tail).
- Structured codeexec API (ExecResult, async, timeouts).

Sprint 4 (start)
- Overrides `--backend-url`, `--model`.
- TUI: preview, approval interactive, watch; diff colorized; navigation.
- CI/Release workflows.

