### Feature
TUI: Live event stream during `devit run` (Plan, Diff head, Logs).

### Rationale
Provide continuous feedback without waiting for step boundaries.

### Tasks
- Add internal channel to publish events from run path
- Update TUI to subscribe and render incrementally
- Integrate approval prompt within TUI flow (single surface)

### DoD
- `devit --tui run` renders stream updates (ToolCall/Diff/AskApproval/Test output) until completion.

