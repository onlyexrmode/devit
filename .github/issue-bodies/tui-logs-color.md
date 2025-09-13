### Feature
TUI: Colorize JSONL logs by event type (ToolCall, AskApproval, Diff, Error, Info).

### Rationale
Improve readability; quickly spot errors and approval prompts.

### Tasks
- Parse JSONL lines (lenient) and map to types
- Color scheme: Error=Red, AskApproval=Yellow, Diff=Cyan, ToolCall=Magenta, Info=Gray
- Fallback to plain text on parse errors

### DoD
- Logs pane shows colorized lines with graceful degradation on malformed lines.

