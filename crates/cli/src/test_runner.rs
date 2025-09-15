use serde::Serialize;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct ImpactedOpts {
    pub changed_from: Option<String>,
    pub changed_paths: Option<Vec<String>>,
    pub max_jobs: Option<usize>,
    pub framework: Option<String>, // auto|cargo|npm|pnpm|pytest|ctest
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImpactedReport {
    pub framework: String,
    pub ran: u32,
    pub passed: u32,
    pub failed: u32,
    pub duration_ms: u128,
    pub logs_path: String,
}

fn timeout(secs: Option<u64>) -> Duration {
    let s = secs
        .or_else(|| std::env::var("DEVIT_TIMEOUT_SECS").ok().and_then(|x| x.parse().ok()))
        .unwrap_or(300);
    Duration::from_secs(s)
}

fn ensure_reports_dir() -> PathBuf {
    let p = Path::new(".devit/reports");
    let _ = fs::create_dir_all(p);
    p.to_path_buf()
}

fn git_changed_paths(from: Option<&str>) -> Vec<String> {
    let range = from.unwrap_or("HEAD");
    let spec = format!("{}..HEAD", range);
    let out = Command::new("git")
        .args(["diff", "--name-only", &spec])
        .output()
        .ok();
    if let Some(o) = out {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout);
            return s.lines().map(|x| x.to_string()).collect();
        }
    }
    Vec::new()
}

fn detect_framework() -> String {
    if Path::new("Cargo.toml").exists() { return "cargo".into(); }
    if Path::new("package.json").exists() { return "npm".into(); }
    if Path::new("pyproject.toml").exists() || Path::new("pytest.ini").exists() || Path::new("tox.ini").exists() { return "pytest".into(); }
    if Path::new("CMakeLists.txt").exists() { return "ctest".into(); }
    "auto".into()
}

fn resolve_rust_packages(changed: &[String]) -> Vec<String> {
    // Use cargo metadata to map files to package names
    let meta = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output();
    if let Ok(o) = meta {
        if o.status.success() {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&o.stdout) {
                let pkgs = v.get("packages").and_then(|x| x.as_array()).cloned().unwrap_or_default();
                let mut out = Vec::new();
                for p in pkgs {
                    let name = p.get("name").and_then(|x| x.as_str()).unwrap_or("");
                    let manifest = p.get("manifest_path").and_then(|x| x.as_str()).unwrap_or("");
                    let dir = Path::new(manifest).parent().map(|p| p.to_path_buf()).unwrap_or_default();
                    for ch in changed {
                        let abs = Path::new(ch);
                        if abs.starts_with(&dir) {
                            if !out.iter().any(|s: &String| s == name) {
                                out.push(name.to_string());
                            }
                        }
                    }
                }
                return out;
            }
        }
    }
    Vec::new()
}

fn write_min_junit(path: &Path, suite_name: &str, ran: u32, failed: u32, dur_ms: u128) {
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites>
  <testsuite name="{}" tests="{}" failures="{}" time="{}">
  </testsuite>
</testsuites>
"#,
        suite_name,
        ran,
        failed,
        (dur_ms as f64) / 1000.0
    );
    if let Some(dir) = path.parent() { let _ = fs::create_dir_all(dir); }
    let _ = fs::write(path, content);
}

pub fn run_impacted(opts: &ImpactedOpts) -> anyhow::Result<ImpactedReport> {
    let framework = opts
        .framework
        .clone()
        .filter(|s| s != "auto")
        .unwrap_or_else(detect_framework);
    let changed = opts
        .changed_paths
        .clone()
        .unwrap_or_else(|| git_changed_paths(opts.changed_from.as_deref()));
    let t0 = Instant::now();
    let to = timeout(opts.timeout_secs);
    let reports_dir = ensure_reports_dir();
    let junit_path = reports_dir.join("junit.xml");

    match framework.as_str() {
        "cargo" => {
            let pkgs = resolve_rust_packages(&changed);
            let mut cmd = Command::new("cargo");
            cmd.arg("test");
            for p in pkgs.iter() { cmd.args(["-p", p]); }
            cmd.stdout(Stdio::piped()).stderr(Stdio::inherit());
            let mut child = cmd.spawn()?;
            let mut ran = 0u32; let mut failed = 0u32; let mut passed = 0u32;
            let mut reader = BufReader::new(child.stdout.take().unwrap());
            let mut line = String::new();
            while t0.elapsed() < to {
                line.clear();
                let n = reader.read_line(&mut line)?;
                if n == 0 { break; }
                // Parse libtest-style lines: "test path::to::name ... ok|FAILED"
                if let Some(rest) = line.strip_prefix("test ") {
                    if rest.contains(" ok") { ran += 1; passed += 1; }
                    else if rest.contains(" FAILED") { ran += 1; failed += 1; }
                }
            }
            if t0.elapsed() >= to {
                let _ = child.kill();
                write_min_junit(&junit_path, "cargo-impacted", ran, failed, t0.elapsed().as_millis());
                anyhow::bail!(serde_json::json!({"timeout": true}).to_string());
            }
            let _ = child.wait();
            write_min_junit(&junit_path, "cargo-impacted", ran, failed, t0.elapsed().as_millis());
            Ok(ImpactedReport { framework, ran, passed, failed, duration_ms: t0.elapsed().as_millis(), logs_path: junit_path.display().to_string() })
        }
        "pytest" => {
            // Prefer native JUnit; counts estimated by exit code
            let status = Command::new("bash")
                .arg("-lc")
                .arg(format!("pytest -q -k {} --disable-warnings --maxfail=1 --junitxml {}",
                    guess_py_pattern(&changed), junit_path.display()))
                .status()?;
            let failed = if status.success() { 0 } else { 1 };
            let ran = 0; let passed = if failed==0 { 0 } else { 0 }; // unknown without parsing
            Ok(ImpactedReport { framework, ran, passed, failed, duration_ms: t0.elapsed().as_millis(), logs_path: junit_path.display().to_string() })
        }
        "npm" | "pnpm" => {
            // Attempt vitest related; else run npm test (best-effort)
            let files = changed.join(" ");
            let cmd = format!("npx vitest related {} --reporter=json || npm test --silent", files);
            let status = Command::new("bash").arg("-lc").arg(&cmd).status()?;
            let failed = if status.success() { 0 } else { 1 };
            write_min_junit(&junit_path, "js-impacted", 0, failed, t0.elapsed().as_millis());
            Ok(ImpactedReport { framework, ran: 0, passed: if failed==0 {0}else{0}, failed, duration_ms: t0.elapsed().as_millis(), logs_path: junit_path.display().to_string() })
        }
        "ctest" => {
            let pat = guess_c_pattern(&changed);
            let status = Command::new("bash").arg("-lc").arg(format!("ctest -R '{}' || true", pat)).status()?;
            let failed = if status.success() { 0 } else { 1 };
            write_min_junit(&junit_path, "ctest-impacted", 0, failed, t0.elapsed().as_millis());
            Ok(ImpactedReport { framework, ran: 0, passed: if failed==0 {0}else{0}, failed, duration_ms: t0.elapsed().as_millis(), logs_path: junit_path.display().to_string() })
        }
        _ => {
            // Unknown: no-op
            write_min_junit(&junit_path, "none", 0, 0, 0);
            Ok(ImpactedReport { framework, ran: 0, passed: 0, failed: 0, duration_ms: 0, logs_path: junit_path.display().to_string() })
        }
    }
}

fn guess_py_pattern(changed: &[String]) -> String {
    for p in changed {
        if let Some(stem) = Path::new(p).file_stem().and_then(|s| s.to_str()) {
            return stem.to_string();
        }
    }
    String::from("")
}

fn guess_c_pattern(changed: &[String]) -> String {
    for p in changed { if let Some(stem) = Path::new(p).file_stem().and_then(|s| s.to_str()) { return stem.to_string(); } }
    String::from("")
}
