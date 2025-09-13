Scenarios (WIP)

This folder will host end-to-end scenario descriptions for DevIt.

Goals
- Capture realistic multi-step tasks: plan → tool calls → approvals → apply → test.
- Keep inputs small and portable (no large data, no network reliance).

Format ideas
- JSON Lines: one event per line (ToolCall, AskApproval, Diff, Attest, Info).
- Or minimal YAML scripts resolved by a thin runner.

Notes
- The current v0.2-rc focuses on tooling and provenance; scenarios will grow later.
- `bench/` is intentionally ignored for now.

