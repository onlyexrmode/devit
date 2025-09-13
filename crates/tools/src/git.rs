use anyhow::{anyhow, Result};
use std::io::Write;
use std::process::{Command, Stdio};

pub fn is_git_available() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

pub fn in_repo() -> bool {
    Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn status_porcelain() -> Result<String> {
    let out = Command::new("git")
        .args(["status", "--porcelain"])
        .output()?;
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Représentation d'une ligne `git apply --numstat`
#[derive(Debug, Clone)]
pub struct NumstatEntry {
    pub added: u64,
    pub deleted: u64,
    pub path: String,
}

fn run_git_with_patch(args: &[&str], patch: &str) -> Result<(bool, String)> {
    let mut child = Command::new("git")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(patch.as_bytes())?;
    }

    let out = child.wait_with_output()?;
    let ok = out.status.success();
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let txt = if ok {
        stdout
    } else {
        format!("{stdout}{stderr}")
    };
    Ok((ok, txt))
}

/// Vérifie que le patch s'applique proprement (dry-run)
pub fn apply_check(patch: &str) -> Result<bool> {
    let (ok, out) = run_git_with_patch(&["apply", "--check", "-"], patch)?;
    if !ok {
        return Err(anyhow!("git apply --check a échoué:\n{out}"));
    }
    Ok(true)
}

/// Retourne le détail des fichiers touchés par le patch
pub fn numstat(patch: &str) -> Result<Vec<NumstatEntry>> {
    let (ok, out) = run_git_with_patch(&["apply", "--numstat", "-"], patch)?;
    if !ok {
        return Err(anyhow!("git apply --numstat a échoué:\n{out}"));
    }
    let mut v = Vec::new();
    for line in out.lines() {
        // format: "<added>\t<deleted>\t<path>"
        let mut parts = line.splitn(3, '\t');
        let a = parts.next().unwrap_or("0").parse::<u64>().unwrap_or(0);
        let d = parts.next().unwrap_or("0").parse::<u64>().unwrap_or(0);
        let p = parts.next().unwrap_or("").to_string();
        if !p.is_empty() {
            v.push(NumstatEntry {
                added: a,
                deleted: d,
                path: p,
            });
        }
    }
    Ok(v)
}

/// Applique et stage le patch. Tente un fallback --3way si --index échoue.
pub fn apply_index(patch: &str) -> Result<bool> {
    let (ok, out) = run_git_with_patch(&["apply", "--index", "-"], patch)?;
    if ok {
        return Ok(true);
    }
    // Fallback 3-way (utile si index/worktree ne matchent pas parfaitement)
    let (ok2, out2) = run_git_with_patch(&["apply", "--3way", "--index", "-"], patch)?;
    if ok2 {
        return Ok(true);
    }
    Err(anyhow!(format!(
        "git apply --index a échoué:\n{out}\n--- 3-way fallback ---\n{out2}"
    )))
}

pub fn commit(message: &str) -> Result<bool> {
    let status = Command::new("git")
        .args(["commit", "-m", message])
        .status()?;
    Ok(status.success())
}

pub fn head_short() -> Option<String> {
    Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
}

pub fn is_worktree_clean() -> bool {
    let wt = Command::new("git")
        .args(["diff", "--quiet"]) // worktree
        .status()
        .map(|s| s.success())
        .unwrap_or(true);
    let idx = Command::new("git")
        .args(["diff", "--cached", "--quiet"]) // index
        .status()
        .map(|s| s.success())
        .unwrap_or(true);
    wt && idx
}
