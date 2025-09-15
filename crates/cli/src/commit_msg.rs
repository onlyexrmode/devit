use anyhow::Result;
use std::collections::HashMap;
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
    let scope = opts.scope.clone().unwrap_or_else(|| infer_scope(&files));
    let typ = opts.typ.clone().unwrap_or_else(|| infer_type(&files));
    let subject = infer_subject(&files, &typ, &scope);
    let head = format!("{}({}): {}", typ, scope, subject);
    let truncated = truncate_72(&head);
    if opts.with_template {
        Ok(format!(
            "{}\n\n- Impact: \n- Risk: \n- Tests: \n",
            truncated
        ))
    } else {
        Ok(truncated)
    }
}

fn truncate_72(s: &str) -> String {
    if s.chars().count() <= 72 {
        s.to_string()
    } else {
        s.chars().take(72).collect()
    }
}

fn changed_files(staged: bool, from: Option<&str>) -> Vec<String> {
    if staged {
        let out = Command::new("git")
            .args(["diff", "--name-only", "--cached"])
            .output();
        return to_lines(out);
    }
    let base = from.unwrap_or("HEAD~1");
    let out = Command::new("git")
        .args(["diff", "--name-only", &format!("{}..HEAD", base)])
        .output();
    to_lines(out)
}

fn to_lines(out: std::io::Result<std::process::Output>) -> Vec<String> {
    if let Ok(o) = out {
        if o.status.success() {
            return String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|x| x.to_string())
                .collect();
        }
    }
    Vec::new()
}

fn infer_scope(files: &[String]) -> String {
    // deepest common directory name
    let parts: Vec<Vec<&str>> = files.iter().map(|f| f.split('/').collect()).collect();
    if parts.is_empty() {
        return "repo".into();
    }
    let mut i = 0usize;
    loop {
        let mut seg: Option<&str> = None;
        for p in &parts {
            if i >= p.len() {
                seg = None;
                break;
            }
            seg = match seg {
                None => Some(p[i]),
                Some(s) if s == p[i] => Some(s),
                _ => None,
            };
            if seg.is_none() {
                break;
            }
        }
        if seg.is_some() {
            i += 1;
        } else {
            break;
        }
    }
    if i == 0 {
        return "repo".into();
    }
    // prefer last fixed segment (e.g., crates/cli -> cli)
    parts[0]
        .get(i.saturating_sub(1))
        .copied()
        .unwrap_or("repo")
        .to_string()
}

fn infer_type(files: &[String]) -> String {
    let mut saw_tests = false;
    let mut saw_docs = false;
    for f in files {
        if f.contains("test") || f.contains("tests/") {
            saw_tests = true;
        }
        if f.ends_with(".md") || f.starts_with("docs/") {
            saw_docs = true;
        }
    }
    if saw_tests {
        return "test".into();
    }
    if saw_docs {
        return "docs".into();
    }
    // default code change → refactor
    "refactor".into()
}

fn infer_subject(files: &[String], typ: &str, scope: &str) -> String {
    if !files.is_empty() {
        let first = files[0].as_str();
        let name = Path::new(first)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(first);
        return match typ {
            "docs" => format!("update {} docs", scope),
            "test" => format!("update tests for {}", scope),
            _ => format!("touch {}", name),
        };
    }
    "update".into()
}

// -------- Structured API (v0.3) --------

#[derive(Debug, Clone)]
pub struct MsgInput {
    pub staged_paths: Vec<std::path::PathBuf>,
    #[allow(dead_code)]
    pub diff_summary: Option<String>,
    pub forced_type: Option<String>,
    pub forced_scope: Option<String>,
    pub max_subject: usize,
    pub template_body: Option<String>,
    pub scopes_alias: Option<HashMap<String, String>>, // optional alias mapping
}

#[derive(Debug, Clone)]
pub struct MsgOutput {
    pub ctype: String,
    pub scope: Option<String>,
    pub subject: String,
    pub body: String,
    pub footers: Vec<String>,
}

pub fn generate_struct(input: &MsgInput) -> Result<MsgOutput> {
    let files: Vec<String> = input
        .staged_paths
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    let scope_auto = infer_scope(&files);
    let scope = if let Some(s) = input.forced_scope.as_ref() {
        if s == "auto" {
            Some(scope_auto)
        } else {
            Some(s.clone())
        }
    } else {
        Some(scope_auto)
    };
    let scope = apply_alias(scope, input.scopes_alias.as_ref());
    let ctype = match input.forced_type.as_deref() {
        Some("auto") | None => infer_type(&files),
        Some(s) => s.to_string(),
    };
    let subj_raw = infer_subject(&files, &ctype, scope.as_deref().unwrap_or("repo"));
    let subject = truncate_to(subj_raw.trim_end_matches('.'), input.max_subject);
    let body = input.template_body.clone().unwrap_or_default();
    Ok(MsgOutput {
        ctype,
        scope,
        subject,
        body,
        footers: Vec::new(),
    })
}

fn apply_alias(scope: Option<String>, alias: Option<&HashMap<String, String>>) -> Option<String> {
    let mut s = scope?;
    if let Some(map) = alias {
        for (prefix, name) in map.iter() {
            if s.starts_with(prefix) || s.contains(prefix) {
                s = name.clone();
                break;
            }
        }
    }
    Some(s)
}

fn truncate_to(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect()
    }
}
