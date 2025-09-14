//! MCP server (stdio) minimal — expérimental.
//! Protocol JSON line-based: handles ping/version/capabilities and a demo tool `echo`.

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

#[derive(Parser, Debug)]
#[command(name = "devit-mcpd")]
#[command(about = "MCP server stdio (expérimental)")]
struct Cli {
    /// Announce server version string
    #[arg(long, default_value = "devit-mcpd/0.1.0")]
    server_version: String,
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
                // For now we expose a single demo tool `echo`.
                writeln!(
                    stdout,
                    "{}",
                    json!({"type":"capabilities","payload":{"tools":["echo"]}})
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
