use anyhow::Result;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct Options {
    pub from_staged: bool,
    pub change_from: Option<String>,
    pub typ: Option<String>, // feat|fix|refactor|docs|test|chore|perf|ci
    pub scope: Option<String>,
    pub with_template: bool,
}

pub fn generate(opts: &Options) -> Result<String> {
    let files = changed_files(opts.from_staged, opts.change_from.as_deref());
    let scope = opts
        .scope
        .clone()
        .unwrap_or_else(|| infer_scope(&files));
    let typ = opts
        .typ
        .clone()
        .unwrap_or_else(|| infer_type(&files));
    let subject = infer_subject(&files, &typ, &scope);
    let head = format!("{}({}): {}", typ, scope, subject);
    let truncated = truncate_72(&head);
    if opts.with_template {
        Ok(format!("{}\n\n- Impact: \n- Risk: \n- Tests: \n", truncated))
    } else {
        Ok(truncated)
    }
}

fn truncate_72(s: &str) -> String {
    if s.chars().count() <= 72 { s.to_string() } else { s.chars().take(72).collect() }
}

fn changed_files(staged: bool, from: Option<&str>) -> Vec<String> {
    if staged {
        let out = Command::new("git").args(["diff", "--name-only", "--cached"]).output();
        return to_lines(out);
    }
    let base = from.unwrap_or("HEAD~1");
    let out = Command::new("git").args(["diff", "--name-only", &format!("{}..HEAD", base)]).output();
    to_lines(out)
}

fn to_lines(out: std::io::Result<std::process::Output>) -> Vec<String> {
    if let Ok(o) = out { if o.status.success() { return String::from_utf8_lossy(&o.stdout).lines().map(|x| x.to_string()).collect(); } }
    Vec::new()
}

fn infer_scope(files: &[String]) -> String {
    // deepest common directory name
    let parts: Vec<Vec<&str>> = files.iter().map(|f| f.split('/').collect()).collect();
    if parts.is_empty() { return "repo".into(); }
    let mut i = 0usize;
    loop {
        let mut seg: Option<&str> = None;
        for p in &parts {
            if i >= p.len() { seg = None; break; }
            seg = match seg { None => Some(p[i]), Some(s) if s==p[i] => Some(s), _ => None };
            if seg.is_none() { break; }
        }
        if seg.is_some() { i += 1; } else { break; }
    }
    if i == 0 { return "repo".into(); }
    // prefer last fixed segment (e.g., crates/cli -> cli)
    parts[0].get(i.saturating_sub(1)).copied().unwrap_or("repo").to_string()
}

fn infer_type(files: &[String]) -> String {
    let mut saw_tests = false; let mut saw_docs = false;
    for f in files {
        if f.contains("test") || f.contains("tests/") { saw_tests = true; }
        if f.ends_with(".md") || f.starts_with("docs/") { saw_docs = true; }
    }
    if saw_tests { return "test".into(); }
    if saw_docs { return "docs".into(); }
    // default code change → refactor
    "refactor".into()
}

fn infer_subject(files: &[String], typ: &str, scope: &str) -> String {
    if !files.is_empty() {
        let first = files[0].as_str();
        let name = Path::new(first).file_name().and_then(|s| s.to_str()).unwrap_or(first);
        return match typ {
            "docs" => format!("update {} docs", scope),
            "test" => format!("update tests for {}", scope),
            _ => format!("touch {}", name),
        };
    }
    "update".into()
}

