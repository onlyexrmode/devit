use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictHunk {
    pub start_line: usize,
    pub end_line: usize,
    pub ours: String,
    pub base: Option<String>,
    pub theirs: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileConflicts {
    pub path: String,
    pub hunks: Vec<ConflictHunk>,
}

#[allow(dead_code)]
pub fn detect_merge_state() -> bool {
    Path::new(".git/MERGE_HEAD").exists()
}

pub fn explain(paths: &[String]) -> Result<Vec<FileConflicts>> {
    let targets = if paths.is_empty() {
        // scan git status for unmerged
        let out = std::process::Command::new("git")
            .args(["diff", "--name-only", "--diff-filter=U"])
            .output()
            .ok();
        if let Some(o) = out {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|s| s.to_string())
                .collect()
        } else {
            vec![]
        }
    } else {
        paths.to_vec()
    };
    let mut out = Vec::new();
    for p in targets {
        let s = fs::read_to_string(&p).with_context(|| format!("read {}", p))?;
        let mut hunks = Vec::new();
        let mut i = 0usize;
        let lines: Vec<&str> = s.lines().collect();
        while i < lines.len() {
            if lines[i].starts_with("<<<<<<<") {
                let start = i + 1;
                let mut sep = None;
                let mut end = None;
                for (j, line) in lines.iter().enumerate().skip(start) {
                    if sep.is_none() && line.starts_with("=======") {
                        sep = Some(j);
                    }
                    if line.starts_with(">>>>>>>") {
                        end = Some(j);
                        break;
                    }
                }
                let (sep, end) = match (sep, end) {
                    (Some(a), Some(b)) if a < b => (a, b),
                    _ => return Err(anyhow!("unbalanced_markers")),
                };
                let ours = lines[start..sep].join("\n");
                let theirs = lines[sep + 1..end].join("\n");
                hunks.push(ConflictHunk {
                    start_line: start,
                    end_line: end,
                    ours,
                    base: None,
                    theirs,
                });
                i = end + 1;
                continue;
            }
            i += 1;
        }
        if !hunks.is_empty() {
            out.push(FileConflicts { path: p, hunks });
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolutionItem {
    pub hunk_index: usize,
    pub resolution: String,
}
pub type Plan = std::collections::HashMap<String, Vec<ResolutionItem>>; // path -> items

#[allow(dead_code)]
pub fn propose_minimal(conflicts: &[FileConflicts]) -> Plan {
    let mut plan = Plan::new();
    for fc in conflicts {
        let items = (0..fc.hunks.len())
            .map(|idx| ResolutionItem {
                hunk_index: idx,
                resolution: "keep_both".into(),
            })
            .collect();
        plan.insert(fc.path.clone(), items);
    }
    plan
}

pub fn propose_auto(conflicts: &[FileConflicts]) -> Plan {
    let mut plan = Plan::new();
    for fc in conflicts {
        let mut items = Vec::new();
        for (idx, h) in fc.hunks.iter().enumerate() {
            let ours_n = h.ours.trim();
            let theirs_n = h.theirs.trim();
            let resolution = if ours_n == theirs_n {
                "ours"
            } else {
                "keep_both"
            };
            items.push(ResolutionItem {
                hunk_index: idx,
                resolution: resolution.into(),
            });
        }
        plan.insert(fc.path.clone(), items);
    }
    plan
}

pub fn apply_plan(plan: &Plan) -> Result<()> {
    for (path, items) in plan.iter() {
        let s = fs::read_to_string(path)?;
        let mut out = String::new();
        let mut i = 0usize;
        let lines: Vec<&str> = s.lines().collect();
        let mut hunk_idx = 0usize;
        while i < lines.len() {
            if lines[i].starts_with("<<<<<<<") {
                let start = i + 1;
                let mut sep = None;
                let mut end = None;
                for (j, line) in lines.iter().enumerate().skip(start) {
                    if sep.is_none() && line.starts_with("=======") {
                        sep = Some(j);
                    }
                    if line.starts_with(">>>>>>>") {
                        end = Some(j);
                        break;
                    }
                }
                let (sep, end) = match (sep, end) {
                    (Some(a), Some(b)) if a < b => (a, b),
                    _ => return Err(anyhow!("merge_conflict_parse_error")),
                };
                let ours = lines[start..sep].join("\n");
                let theirs = lines[sep + 1..end].join("\n");
                let choice = items
                    .iter()
                    .find(|it| it.hunk_index == hunk_idx)
                    .map(|it| it.resolution.as_str())
                    .unwrap_or("keep_both");
                match choice {
                    "ours" => {
                        if !out.is_empty() {
                            out.push('\n');
                        }
                        out.push_str(&ours);
                    }
                    "theirs" => {
                        if !out.is_empty() {
                            out.push('\n');
                        }
                        out.push_str(&theirs);
                    }
                    _ => {
                        if !out.is_empty() {
                            out.push('\n');
                        }
                        // keep both with a simple separator for clarity
                        out.push_str(&format!("{}\n// --- theirs ---\n{}", ours, theirs));
                    }
                }
                i = end + 1;
                hunk_idx += 1;
                continue;
            } else {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(lines[i]);
                i += 1;
            }
        }
        // backup
        let bak = Path::new(".devit/merge_backups");
        let _ = fs::create_dir_all(bak);
        let bak_path = bak.join(Path::new(path).file_name().unwrap());
        fs::write(&bak_path, s).map_err(|_| anyhow!("merge_apply_failed"))?;
        fs::write(path, out).map_err(|_| anyhow!("merge_apply_failed"))?;
    }
    Ok(())
}
