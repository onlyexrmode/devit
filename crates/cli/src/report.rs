use anyhow::{Context, Result};
use devit_common::QualityCfg;
use std::fs;
use std::path::{Path, PathBuf};

pub fn sarif_latest() -> Result<PathBuf> {
    let p = Path::new(".devit/reports/sarif.json");
    if !p.exists() {
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        let skeleton = serde_json::json!({
            "version": "2.1.0",
            "runs": []
        });
        fs::write(p, serde_json::to_vec(&skeleton)?)?;
    }
    Ok(p.to_path_buf())
}

pub fn junit_latest() -> Result<PathBuf> {
    let p = Path::new(".devit/reports/junit.xml");
    if !p.exists() {
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        let content = r#"<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<testsuites><testsuite name=\"empty\" tests=\"0\" failures=\"0\" time=\"0\"/></testsuites>
"#;
        fs::write(p, content)?;
    }
    Ok(p.to_path_buf())
}

#[derive(Debug, Clone, serde::Serialize, Default)]
pub struct QualitySummary {
    pub tests_total: u32,
    pub tests_failed: u32,
    pub lint_errors: u32,
    pub lint_warnings: u32,
    pub sarif_rules: u32,
    pub duration_ms: u64,
    pub notes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flaky_failed: Option<u32>,
}

pub fn read_junit<P: AsRef<Path>>(
    p: P,
    flaky_list: Option<&[String]>,
) -> Result<(u32, u32, Option<u32>)> {
    let s = fs::read_to_string(&p)
        .with_context(|| format!("read junit at {}", p.as_ref().display()))?;
    // naive: look for attributes on testsuite
    let mut total = 0u32;
    let mut failed = 0u32;
    let mut flaky_failed = 0u32;
    for line in s.lines() {
        if line.contains("<testsuite") {
            total = attr_num(line, "tests").unwrap_or(total);
            failed = attr_num(line, "failures").unwrap_or(failed);
        }
    }
    if total == 0 {
        // fallback: count testcase and failures
        for line in s.lines() {
            if line.contains("<testcase") {
                total += 1;
            }
            if line.contains("<failure") {
                failed += 1;
            }
        }
    }
    if let Some(flaky) = flaky_list {
        // Estimate flaky by matching test names in the XML; naive: search strings
        for name in flaky {
            if s.contains(name) && s.contains("<failure") {
                flaky_failed += 1;
            }
        }
    }
    Ok((
        total,
        failed,
        if flaky_list.is_some() {
            Some(flaky_failed)
        } else {
            None
        },
    ))
}

fn attr_num(line: &str, key: &str) -> Option<u32> {
    let pat = format!("{}=\"", key);
    if let Some(i) = line.find(&pat) {
        let rest = &line[i + pat.len()..];
        let j = rest.find('"')?;
        return rest[..j].parse::<u32>().ok();
    }
    None
}

pub fn read_sarif<P: AsRef<Path>>(p: P) -> Result<(u32, u32, u32)> {
    let v: serde_json::Value = serde_json::from_slice(&fs::read(&p)?)?;
    let mut errors = 0u32;
    let mut warnings = 0u32;
    let mut rules = 0u32;
    let runs = v
        .get("runs")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    for run in runs.iter() {
        if let Some(tool) = run.get("tool").and_then(|t| t.get("driver")) {
            if let Some(rs) = tool.get("rules").and_then(|r| r.as_array()) {
                rules += rs.len() as u32;
            }
        }
        if let Some(results) = run.get("results").and_then(|r| r.as_array()) {
            for res in results {
                let lvl = res.get("level").and_then(|l| l.as_str()).unwrap_or("");
                match lvl {
                    "error" => errors += 1,
                    "warning" => warnings += 1,
                    _ => {}
                }
            }
        }
    }
    Ok((errors, warnings, rules))
}

pub fn summarize(
    junit_path: &Path,
    sarif_path: &Path,
    cfg: &QualityCfg,
    flaky: Option<&[String]>,
) -> Result<QualitySummary> {
    let mut sum = QualitySummary::default();
    let dur = std::time::Instant::now();
    match read_junit(junit_path, flaky) {
        Ok((t, f, ff)) => {
            sum.tests_total = t;
            if let Some(x) = ff {
                let strict = f.saturating_sub(x);
                sum.tests_failed = strict;
                sum.flaky_failed = Some(x);
                if x > 0 {
                    sum.notes.push(format!("{} flaky failures ignored", x));
                }
            } else {
                sum.tests_failed = f;
            }
        }
        Err(e) => {
            if cfg.fail_on_missing_reports {
                return Err(e);
            }
            sum.notes.push(format!("junit missing: {}", e));
        }
    }
    match read_sarif(sarif_path) {
        Ok((e, w, r)) => {
            sum.lint_errors = e;
            sum.lint_warnings = w;
            sum.sarif_rules = r;
        }
        Err(e) => {
            if cfg.fail_on_missing_reports {
                return Err(e);
            }
            sum.notes.push(format!("sarif missing: {}", e));
        }
    }
    sum.duration_ms = dur.elapsed().as_millis() as u64;
    Ok(sum)
}

pub fn check_thresholds(sum: &QualitySummary, cfg: &QualityCfg) -> bool {
    if sum.tests_failed > cfg.max_test_failures {
        return false;
    }
    if sum.lint_errors > cfg.max_lint_errors {
        return false;
    }
    if !cfg.allow_lint_warnings && sum.lint_warnings > 0 {
        return false;
    }
    true
}

pub fn summary_markdown(junit: &Path, sarif: &Path, out: &Path) -> Result<()> {
    let q = QualityCfg::default();
    let sum = summarize(junit, sarif, &q, None)?;
    let mut md = String::new();
    md.push_str("# DevIt Summary\n\n");
    // Commit proposed (if available)
    if let Ok(s) = std::fs::read_to_string(".devit/reports/commit_meta.json") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
            let ctype = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
            let subject = v.get("subject").and_then(|x| x.as_str()).unwrap_or("");
            let scope = v.get("scope").and_then(|x| x.as_str());
            let committed = v
                .get("committed")
                .and_then(|x| x.as_bool())
                .unwrap_or(false);
            let sha = v.get("sha").and_then(|x| x.as_str()).unwrap_or("");
            let line = match scope {
                Some(sc) if !sc.is_empty() => format!("{}({}): {}", ctype, sc, subject),
                _ => format!("{}: {}", ctype, subject),
            };
            md.push_str(&format!("Commit proposé: {}\n", line));
            md.push_str(&format!(
                "SHA: {}\n\n",
                if committed && !sha.is_empty() {
                    sha
                } else {
                    "pending"
                }
            ));
        }
    }
    // Pre-commit not tracked here; keep placeholder
    md.push_str("- Pre-commit: n/a\n");
    md.push_str(&format!(
        "- Tests: {}/{} failed\n",
        sum.tests_failed, sum.tests_total
    ));
    md.push_str(&format!(
        "- Lint: {} errors, {} warnings\n\n",
        sum.lint_errors, sum.lint_warnings
    ));
    // Top files from .devit/index.json if present
    if let Ok(s) = std::fs::read_to_string(".devit/index.json") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
            if let Some(files) = v.get("files").and_then(|x| x.as_array()) {
                let mut rows: Vec<(i64, String)> = Vec::new();
                for f in files {
                    let p = f
                        .get("path")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string();
                    let score = f.get("score").and_then(|x| x.as_i64()).unwrap_or(0);
                    rows.push((score, p));
                }
                rows.sort_by(|a, b| b.0.cmp(&a.0));
                md.push_str("## Top impacted files\n");
                for (_s, p) in rows.into_iter().take(10) {
                    md.push_str(&format!("- {}\n", p));
                }
                md.push('\n');
            }
        }
    }
    if let Some(dir) = out.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(out, md)?;
    Ok(())
}
