//! MCP server (stdio) minimal — expérimental.
//! Protocol JSON line-based: handles ping/version/capabilities and a demo tool `echo`.

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use serde_json::{json, Value};
use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(name = "devit-mcpd")]
#[command(about = "MCP server stdio (expérimental)")]
struct Cli {
    /// Announce server version string
    #[arg(long, default_value = "devit-mcpd/0.1.0")]
    server_version: String,
    /// Path to `devit` binary (default: devit in PATH)
    #[arg(long = "devit-bin")]
    devit_bin: Option<PathBuf>,
    /// Per-message timeout in seconds (fallback DEVIT_TIMEOUT_SECS, else 30)
    #[arg(long = "timeout-secs")]
    timeout_secs: Option<u64>,
}

fn main() {
    if let Err(e) = real_main() {
        eprintln!("error: {e}");
        std::process::exit(2);
    }
}

fn real_main() -> Result<()> {
    let cli = Cli::parse();
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut lines = stdin.lock().lines();
    let timeout = timeout_from_cli_env(cli.timeout_secs);

    while let Some(line) = lines.next() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = serde_json::from_str(&line)
            .with_context(|| format!("invalid json: {}", truncate(&line)))?;
        let typ = msg
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("missing type"))?;
        match typ {
            "ping" => {
                writeln!(stdout, "{}", json!({"type":"pong"}))?;
            }
            "version" => {
                writeln!(
                    stdout,
                    "{}",
                    json!({"type":"version","payload":{"server": cli.server_version }})
                )?;
            }
            "capabilities" => {
                // Expose demo tool `echo` and DevIt passthrough tools
                writeln!(
                    stdout,
                    "{}",
                    json!({"type":"capabilities","payload":{"tools":[
                        "echo",
                        "devit.tool_list",
                        "devit.tool_call"
                    ]}})
                )?;
            }
            "tool.call" => {
                let payload = msg
                    .get("payload")
                    .ok_or_else(|| anyhow!("missing payload"))?;
                let name = payload
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("missing tool name"))?;
                match name {
                    "echo" => {
                        let text = payload
                            .get("args")
                            .and_then(|a| a.get("text"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        writeln!(
                            stdout,
                            "{}",
                            json!({
                                "type": "tool.result",
                                "payload": {"name": "echo", "result": {"text": text}}
                            })
                        )?;
                    }
                    "devit.tool_list" => {
                        let bin = cli
                            .devit_bin
                            .clone()
                            .unwrap_or_else(|| PathBuf::from("devit"));
                        let out = run_devit_list(&bin, timeout)?;
                        writeln!(
                            stdout,
                            "{}",
                            json!({
                                "type": "tool.result",
                                "payload": {"name": name, "result": out}
                            })
                        )?;
                    }
                    "devit.tool_call" => {
                        let bin = cli
                            .devit_bin
                            .clone()
                            .unwrap_or_else(|| PathBuf::from("devit"));
                        let args_json = payload.get("args").cloned().unwrap_or(json!({}));
                        let out = run_devit_call(&bin, &args_json, timeout)?;
                        writeln!(
                            stdout,
                            "{}",
                            json!({
                                "type": "tool.result",
                                "payload": {"name": name, "result": out}
                            })
                        )?;
                    }
                    other => {
                        writeln!(
                            stdout,
                            "{}",
                            json!({
                                "type":"error",
                                "payload": {"message": format!("unknown tool: {}", other)}
                            })
                        )?;
                    }
                }
            }
            _ => {
                writeln!(
                    stdout,
                    "{}",
                    json!({"type":"error","payload":{"message": format!("unsupported type: {}", typ)}})
                )?;
            }
        }
        stdout.flush()?;
    }
    Ok(())
}

fn truncate(s: &str) -> String {
    const MAX: usize = 200;
    if s.len() <= MAX {
        s.to_string()
    } else {
        format!("{}…", &s[..MAX])
    }
}

fn timeout_from_cli_env(override_secs: Option<u64>) -> Duration {
    if let Some(s) = override_secs {
        return Duration::from_secs(s);
    }
    if let Ok(v) = std::env::var("DEVIT_TIMEOUT_SECS") {
        if let Ok(s) = v.parse::<u64>() {
            return Duration::from_secs(s);
        }
    }
    Duration::from_secs(30)
}

fn run_devit_list(bin: &PathBuf, timeout: Duration) -> Result<Value> {
    let mut child = Command::new(bin)
        .arg("tool")
        .arg("list")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("spawn {:?} tool list", bin))?;

    let mut out = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("child stdout missing"))?;
    let (tx, rx) = mpsc::sync_channel::<Result<String>>(1);
    std::thread::spawn(move || {
        let mut buf = String::new();
        let res = out
            .read_to_string(&mut buf)
            .map(|_| buf)
            .map_err(|e| anyhow!(e));
        let _ = tx.send(res);
    });
    match rx.recv_timeout(timeout) {
        Ok(s) => {
            let s = s?;
            let v: Value =
                serde_json::from_str(s.trim()).context("devit tool list: invalid JSON")?;
            Ok(v)
        }
        Err(_) => {
            let _ = child.kill();
            eprintln!("error: devit tool list timeout");
            std::process::exit(124);
        }
    }
}

fn run_devit_call(bin: &PathBuf, args_json: &Value, timeout: Duration) -> Result<Value> {
    let mut child = Command::new(bin)
        .arg("tool")
        .arg("call")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("spawn {:?} tool call -", bin))?;

    // write JSON to stdin
    {
        let mut sin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("child stdin missing"))?;
        let s = serde_json::to_string(args_json)?;
        use std::io::Write as _;
        sin.write_all(s.as_bytes())?;
        sin.flush()?;
    }
    let mut out = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("child stdout missing"))?;
    let (tx, rx) = mpsc::sync_channel::<Result<String>>(1);
    std::thread::spawn(move || {
        let mut buf = String::new();
        let res = out
            .read_to_string(&mut buf)
            .map(|_| buf)
            .map_err(|e| anyhow!(e));
        let _ = tx.send(res);
    });
    match rx.recv_timeout(timeout) {
        Ok(s) => {
            let s = s?;
            let v: Value =
                serde_json::from_str(s.trim()).context("devit tool call: invalid JSON")?;
            Ok(v)
        }
        Err(_) => {
            let _ = child.kill();
            eprintln!("error: devit tool call timeout");
            std::process::exit(124);
        }
    }
}
