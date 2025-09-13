// # -----------------------------
// # crates/tools/src/fs.rs
// # -----------------------------
use anyhow::{Context, Result};
use std::path::Path;

pub fn read_to_string(path: &str) -> Result<String> {
    let p = Path::new(path);
    let s = std::fs::read_to_string(p).with_context(|| format!("read {path}"))?;
    Ok(s)
}

pub fn write_from_string(path: &str, content: &str) -> Result<()> {
    let p = Path::new(path);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(p, content)?;
    Ok(())
}
