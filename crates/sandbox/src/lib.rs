// # -----------------------------
// # crates/sandbox/src/lib.rs
// # -----------------------------
// MVP sandboxing helpers for shell execution.
// - Safe-list of binaries
// - Optional "no-net" policy (best-effort)

use anyhow::{anyhow, Result};
use devit_common::{PolicyCfg, SandboxCfg};
use std::process::{Command, Stdio};

fn tokenize_commands(cmd: &str) -> Vec<String> {
    // Split on shell operators to extract leading binaries of sub-commands
    let seps = ['|', ';', '&', '\n'];
    cmd.split(|c| seps.contains(&c))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn first_word(s: &str) -> &str {
    s.split_whitespace().next().unwrap_or("")
}

fn allowed_binaries() -> &'static [&'static str] {
    // Conservative default allow-list for read/inspect operations
    &[
        "true", "false", "printf", "echo", "cat", "ls", "stat", "head", "tail", "wc",
        "cut", "sort", "uniq", "tr", "sed", "awk", "grep", "rg", "find", "xargs",
        "dirname", "basename", "pwd",
    ]
}

fn net_sensitive_binaries() -> &'static [&'static str] {
    &["curl", "wget", "pip", "npm", "apt", "git", "ssh", "scp"]
}

fn enforce_policy(cmd: &str, policy: &PolicyCfg, sb: &SandboxCfg) -> Result<()> {
    let parts = tokenize_commands(cmd);
    let allow = allowed_binaries();
    let netblk = net_sensitive_binaries();
    for p in parts {
        let bin = first_word(&p);
        if !allow.contains(&bin) {
            return Err(anyhow!(format!(
                "sandbox: binaire non autorisé: {bin}")));
        }
        if sb.net.eq_ignore_ascii_case("off") && netblk.contains(&bin) {
            return Err(anyhow!(format!(
                "sandbox: réseau interdit, commande bloquée: {bin}")));
        }
    }
    // Approval profile may further restrict execution later at CLI layer
    let eff = policy
        .profile
        .as_ref()
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    if eff == "read-only" {
        // Best-effort: block common mutating commands
        let mutating = ["rm", "mv", "cp", "chmod", "chown", "tee", "dd", ">", ">>"];
        if tokenize_commands(cmd)
            .iter()
            .any(|c| mutating.contains(&first_word(c)))
        {
            return Err(anyhow!("sandbox: profil read-only: écriture interdite"));
        }
    }
    Ok(())
}

pub fn run_shell_sandboxed(cmd: &str, policy: &PolicyCfg, sb: &SandboxCfg) -> Result<i32> {
    enforce_policy(cmd, policy, sb)?;
    // Execute via /bin/bash -lc in a minimized env
    let mut command = if cfg!(target_os = "windows") {
        let mut c = Command::new("cmd");
        c.args(["/C", cmd]);
        c
    } else {
        let mut c = Command::new("bash");
        c.args(["-lc", cmd]);
        c
    };
    // Best-effort disable proxies when net=off
    if sb.net.eq_ignore_ascii_case("off") {
        command.env_remove("http_proxy");
        command.env_remove("https_proxy");
        command.env_remove("HTTP_PROXY");
        command.env_remove("HTTPS_PROXY");
        command.env_remove("ALL_PROXY");
    }
    let status = command.status()?;
    Ok(status.code().unwrap_or(-1))
}

pub fn run_shell_sandboxed_capture(cmd: &str, policy: &PolicyCfg, sb: &SandboxCfg) -> Result<(i32, String)> {
    enforce_policy(cmd, policy, sb)?;
    let mut command = if cfg!(target_os = "windows") {
        let mut c = Command::new("cmd");
        c.args(["/C", cmd]);
        c
    } else {
        let mut c = Command::new("bash");
        c.args(["-lc", cmd]);
        c
    };
    if sb.net.eq_ignore_ascii_case("off") {
        command.env_remove("http_proxy");
        command.env_remove("https_proxy");
        command.env_remove("HTTP_PROXY");
        command.env_remove("HTTPS_PROXY");
        command.env_remove("ALL_PROXY");
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let out = command.output()?;
    let code = out.status.code().unwrap_or(-1);
    let txt = String::from_utf8_lossy(&out.stdout).to_string()
        + String::from_utf8_lossy(&out.stderr).as_ref();
    Ok((code, txt))
}
