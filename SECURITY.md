Security Policy

Scope
- DevIt executes code only through a Sandbox abstraction.
- Default policy is strict: approval = untrusted; sandbox = read-only; network off.

Risks
- Disabling the sandbox (`--no-sandbox`) or using `danger-full-access` increases risk of unintended writes or network access.
- Applying patches from untrusted sources can introduce vulnerabilities.

Recommendations
- Keep defaults (read-only, untrusted) and review diffs carefully.
- Use `workspace-write` only for repositories you control; keep `net=off`.
- Avoid `--no-sandbox` except for debugging.

Reporting
- Please open a security advisory on GitHub or contact maintainers privately.

