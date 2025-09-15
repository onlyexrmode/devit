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

Hands‑on demo (Rust repo)

Create a throwaway repo with a Clippy warning, then exercise the gate.

```
# 1) Create temp repo with Rust + config
TMP=$(mktemp -d) && cd "$TMP"
git init -q
cat > devit.toml <<'CFG'
[backend]
kind = "openai_like"
base_url = ""
model = ""
api_key = ""

[policy]
approval = "on_request"
# Uncomment to allow bypass demo later:
# profile = "danger"

[sandbox]
cpu_limit = 1
mem_limit_mb = 64
net = "off"

[precommit]
rust = true
javascript = false
python = false
fail_on = ["rust"]
allow_bypass_profiles = ["danger"]
CFG

cargo init --lib -q
cat > src/lib.rs <<'RS'
pub fn demo() {
    let _unused = 42; // triggers clippy (unused variable) with -D warnings
}
RS
git add -A && git commit -qm init

# 2) Run precommit-only: should FAIL with precommit_failed
devit tool call fs_patch_apply - --precommit-only || true

# Or via cargo if devit not installed:
# cargo run -p devit-cli -- tool call fs_patch_apply - --precommit-only || true

# 3) Bypass attempt (blocked by default profile=on_request/std)
devit tool call fs_patch_apply - --yes --no-precommit || true

# 4) Allow bypass: set profile=danger, then retry
sed -i 's/^# profile = "danger"/profile = "danger"/' devit.toml
devit tool call fs_patch_apply - --yes --no-precommit

echo "Demo done in $TMP"
```

Expected
- Step 2 prints an error shaped like `{ "precommit_failed": true, ... }` and exits non‑zero.
- Step 3 prints `{ "approval_required": true, "reason": "precommit_bypass" }`.
- Step 4 succeeds (bypass allowed for `danger`).

Hands‑on demo (JS/TS repo)

Create a repo with ESLint configured and a quick lint error.

```
# 1) Create temp repo with JS + config
TMP=$(mktemp -d) && cd "$TMP"
git init -q
cat > devit.toml <<'CFG'
[backend]
kind = "openai_like"
base_url = ""
model = ""
api_key = ""

[policy]
approval = "on_request"
# profile = "danger"   # uncomment later to allow bypass

[sandbox]
cpu_limit = 1
mem_limit_mb = 64
net = "off"

[precommit]
rust = false
javascript = true
python = false
fail_on = ["javascript"]
allow_bypass_profiles = ["danger"]
CFG

npm init -y >/dev/null
npm i -D eslint >/dev/null
./node_modules/.bin/eslint --init || true  # optional wizard; we will add a minimal config below
cat > .eslintrc.json <<'ESL'
{
  "env": { "es2021": true, "node": true },
  "extends": ["eslint:recommended"],
  "rules": { "no-unused-vars": "error" }
}
ESL
cat > index.js <<'JS'
function demo() {
  const unused = 42; // triggers eslint no-unused-vars
}
demo();
JS
git add -A && git commit -qm init

# 2) Run precommit-only: should FAIL (eslint)
devit tool call fs_patch_apply - --precommit-only || true

# 3) Bypass attempt (blocked by default)
devit tool call fs_patch_apply - --yes --no-precommit || true

# 4) Allow bypass: enable profile=danger
sed -i 's/^# profile = \"danger\"/profile = \"danger\"/' devit.toml
devit tool call fs_patch_apply - --yes --no-precommit

echo "JS demo done in $TMP"
```

Hands‑on demo (Python repo)

Create a repo with Ruff and a quick lint error.

```
# 1) Create temp repo with Python + config
TMP=$(mktemp -d) && cd "$TMP"
git init -q
cat > devit.toml <<'CFG'
[backend]
kind = "openai_like"
base_url = ""
model = ""
api_key = ""

[policy]
approval = "on_request"
# profile = "danger"   # uncomment later to allow bypass

[sandbox]
cpu_limit = 1
mem_limit_mb = 64
net = "off"

[precommit]
rust = false
javascript = false
python = true
fail_on = ["python"]
allow_bypass_profiles = ["danger"]
CFG

python3 -m venv .venv && . .venv/bin/activate
python -m pip install -U pip >/dev/null
python -m pip install -U ruff >/dev/null
cat > pyproject.toml <<'PY'
[tool.ruff]
line-length = 88
target-version = "py38"
select = ["E", "F"]
ignore = []
PY
mkdir -p pkg && cat > pkg/mod.py <<'PY'
def demo():
    unused = 42  # F841 (local variable assigned but never used)
    print("hi")
PY
git add -A && git commit -qm init

# 2) Run precommit-only: should FAIL (ruff)
DEVIT_TIMEOUT_SECS=120 devit tool call fs_patch_apply - --precommit-only || true

# 3) Bypass attempt (blocked by default)
devit tool call fs_patch_apply - --yes --no-precommit || true

# 4) Allow bypass
sed -i 's/^# profile = \"danger\"/profile = \"danger\"/' devit.toml
devit tool call fs_patch_apply - --yes --no-precommit

echo "Python demo done in $TMP"
```
