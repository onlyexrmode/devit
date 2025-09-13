### Feature
Corrective run after test FAIL (suggest → approve → apply → retest).

### Rationale
Close the loop automatically while preserving approvals.

### Tasks
- On FAIL, craft a targeted `suggest` using JUnit summary and tail logs
- Show diff in TUI; prompt approval; if yes, apply/commit
- Re-run tests and update `update_plan.yaml` accordingly

### DoD
- After a FAIL in `devit run`, the user can choose a corrective cycle; no auto-apply without approval.

