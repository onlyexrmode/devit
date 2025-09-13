// # -----------------------------
// # crates/tools/src/shell.rs
// # -----------------------------
use anyhow::Result;
use tokio::process::Command;

pub async fn run(cmd: &str) -> Result<i32> {
    let status = if cfg!(target_os = "windows") {
        Command::new("cmd").arg("/C").arg(cmd).status().await?
    } else {
        Command::new("bash").arg("-lc").arg(cmd).status().await?
    };

    Ok(status.code().unwrap_or(-1))
}
