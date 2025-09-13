### Feature
TUI: Horizontal scroll for long lines in Diff/Logs panes; smart wrapping toggle.

### Rationale
Long diff/log lines are truncated or wrapped, making review harder.

### Tasks
- Add horizontal scroll state per pane (Diff, Logs, Plan optional)
- Key bindings: Shift+Left/Right or H/L to scroll 10 cols
- Toggle wrap on/off per pane
- Persist last state within session

### DoD
- User can toggle wrapping and scroll horizontally in Diff/Logs.
- No visual artifacts; performance acceptable on large diffs.

