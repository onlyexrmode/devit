use anyhow::{Context, Result};
use devit_common::{Config, PrecommitCfg};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct PrecommitFailure {
    pub tool: String,
    pub exit_code: i32,
    pub stderr: String,
}

fn timeout() -> Duration {
    let secs = std::env::var("DEVIT_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(120);
    Duration::from_secs(secs)
}

fn exists(p: &str) -> bool { Path::new(p).exists() }

fn has_prettier_config() -> bool {
    let candidates = [
        ".prettierrc",
        ".prettierrc.json",
        ".prettierrc.js",
        ".prettierrc.cjs",
        ".prettierrc.yaml",
        ".prettierrc.yml",
        "prettier.config.js",
        "prettier.config.cjs",
        "package.json",
    ];
    for c in candidates { if exists(c) { return true; } }
    false
}

fn run_with_timeout(cmd: &str, tool_label: &str) -> std::result::Result<(), PrecommitFailure> {
    let mut child = Command::new("bash")
        .arg("-lc")
        .arg(cmd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| PrecommitFailure { tool: tool_label.into(), exit_code: 127, stderr: e.to_string() })?;
    let t0 = Instant::now();
    let to = timeout();
    while t0.elapsed() < to {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    return Ok(());
                } else {
                    let code = status.code().unwrap_or(1);
                    let mut stderr = String::new();
                    if let Some(mut s) = child.stderr.take() {
                        use std::io::Read; let _ = s.read_to_string(&mut stderr);
                    }
                    return Err(PrecommitFailure { tool: tool_label.into(), exit_code: code, stderr });
                }
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => break,
        }
    }
    let _ = child.kill();
    Err(PrecommitFailure { tool: tool_label.into(), exit_code: 124, stderr: "timeout".into() })
}

fn cfg_or_default(cfg: &Config) -> PrecommitCfg {
    cfg.precommit.clone().unwrap_or(PrecommitCfg {
        rust: true,
        javascript: true,
        python: true,
        additional: vec![],
        fail_on: vec!["rust".into(), "javascript".into(), "python".into()],
        allow_bypass_profiles: vec!["danger".into()],
    })
}

pub fn run(cfg: &Config) -> std::result::Result<(), PrecommitFailure> {
    let pc = cfg_or_default(cfg);
    // Rust
    if pc.rust && exists("Cargo.toml") {
        run_with_timeout("cargo fmt --all -- --check", "fmt").map_err(|e| if pc.fail_on.contains(&"rust".into()) { e } else { PrecommitFailure { tool: e.tool, exit_code: 0, stderr: e.stderr } })?;
        run_with_timeout("cargo clippy --all-targets -- -D warnings", "clippy").map_err(|e| if pc.fail_on.contains(&"rust".into()) { e } else { PrecommitFailure { tool: e.tool, exit_code: 0, stderr: e.stderr } })?;
    }
    // JS/TS
    if pc.javascript && exists("package.json") {
        // Prefer npm run lint; fallback to npx eslint .
        let r = run_with_timeout("npm run -s lint || npx eslint .", "eslint");
        if let Err(e) = r {
            if pc.fail_on.contains(&"javascript".into()) {
                return Err(e);
            }
        }
        if has_prettier_config() {
            let r = run_with_timeout("npx prettier -c .", "prettier");
            if let Err(e) = r {
                if pc.fail_on.contains(&"javascript".into()) {
                    return Err(e);
                }
            }
        }
    }
    // Python
    if pc.python && (exists("pyproject.toml") || exists("tox.ini") || exists("pytest.ini")) {
        // Prefer ruff check
        let r = if exists("pyproject.toml") {
            run_with_timeout("ruff check", "ruff")
        } else {
            run_with_timeout("ruff -q .", "ruff")
        };
        if let Err(e) = r {
            if pc.fail_on.contains(&"python".into()) {
                return Err(e);
            }
        }
    }
    // C/C++
    if exists("CMakeLists.txt") {
        // best-effort, non-blocking by default
        let _ = run_with_timeout("command -v cmake-lint >/dev/null 2>&1 && cmake-lint || true", "cmake-lint");
    }
    // Additional
    for (i, cmd) in pc.additional.iter().enumerate() {
        let label = format!("additional[{}]", i);
        let r = run_with_timeout(cmd, &label);
        if let Err(e) = r {
            // treat additional as blocking if listed in fail_on as "additional"
            if pc.fail_on.iter().any(|s| s == "additional") {
                return Err(e);
            }
        }
    }
    Ok(())
}

pub fn bypass_allowed(cfg: &Config) -> bool {
    let pc = cfg_or_default(cfg);
    let profile = cfg.policy.profile.clone().unwrap_or_else(|| "std".into()).to_lowercase();
    pc.allow_bypass_profiles.iter().any(|p| p.to_lowercase() == profile)
}

