# DevIt VS Code Extension

Minimal VS Code bridge for DevIt: timeline panel, approvals, and recipe launcher.

## Prerequisites

- Node.js 20+
- `npm` (bundled with Node)
- Local DevIt workspace (extension shells out to `target/debug/devit` by default)

## Build and package locally

```bash
npm ci
npm run build
npm run package
```

The VSIX artifact (`devit-vscode-*.vsix`) lands in the current directory. Install it via the VS Code command palette (`Extensions: Install from VSIX...`).

## Commands exposed

- **DevIt: Show Panel** — opens the timeline webview (last 10 `.devit/journal.jsonl` events, buttons for approval/refusal and recipe launcher).
- **DevIt: Approve Last Request** — reads the journal for the latest `approval_required` entry and sends `server.approve` through `devit-mcpd`.
- **DevIt: Run Recipe…** — lists recipes (via `devit recipe list`) and runs the chosen id with `--dry-run`.

Quick tips:
- Configure `devit.devitBin` / `devit.mcpdBin` if you need custom binary paths (fallback is PATH + `target/{debug,release}`).
- The panel refreshes automatically when `.devit/journal.jsonl` changes, and also polls every second.
- `npm run watch` keeps `out/` updated during development; launch the VS Code Extension Host (`Run Extension`) to test commands interactively.
