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
use std::{collections::HashMap, fs};

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
    /// Auto-approve actions gated by policy
    #[arg(long, action = clap::ArgAction::SetTrue)]
    yes: bool,
    /// Config path for approval policies (default: .devit/devit.toml)
    #[arg(long = "config")]
    config_path: Option<PathBuf>,
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
    let policies = load_policies(cli.config_path.as_ref()).unwrap_or_default();

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
                    json!({
                        "type":"version",
                        "payload":{
                            "server": cli.server_version,
                            "server_name": "devit-mcpd"
                        }
                    })
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
                let policy = policies
                    .0
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| default_policy_for(name));
                // on_request/untrusted: require approval before running
                if (policy == "on_request" || policy == "untrusted") && !cli.yes {
                    writeln!(
                        stdout,
                        "{}",
                        json!({
                            "type": "tool.error",
                            "payload": {"approval_required": true, "policy": policy, "phase": "pre"}
                        })
                    )?;
                    stdout.flush()?;
                    continue;
                }
                match name {
                    "echo" => {
                        // echo allowed unless explicitly restricted
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
                        match run_devit_list(&bin, timeout) {
                            Ok(out) => {
                                writeln!(
                                    stdout,
                                    "{}",
                                    json!({
                                        "type": "tool.result",
                                        "payload": {"name": name, "result": out}
                                    })
                                )?;
                            }
                            Err(e) => {
                                if policy == "on_failure" && !cli.yes {
                                    writeln!(
                                        stdout,
                                        "{}",
                                        json!({
                                            "type": "tool.error",
                                            "payload": {"approval_required": true, "policy": policy, "phase": "post"}
                                        })
                                    )?;
                                } else {
                                    writeln!(
                                        stdout,
                                        "{}",
                                        json!({
                                            "type": "tool.error",
                                            "payload": {"message": e.to_string()}
                                        })
                                    )?;
                                }
                            }
                        }
                    }
                    "devit.tool_call" => {
                        let bin = cli
                            .devit_bin
                            .clone()
                            .unwrap_or_else(|| PathBuf::from("devit"));
                        let args_json = payload.get("args").cloned().unwrap_or(json!({}));
                        match run_devit_call(&bin, &args_json, timeout) {
                            Ok(out) => {
                                // on_failure: if DevIt reports ok=false, require approval (post)
                                let is_fail = out
                                    .get("ok")
                                    .and_then(|v| v.as_bool())
                                    .map(|b| !b)
                                    .unwrap_or(false);
                                if policy == "on_failure" && is_fail && !cli.yes {
                                    writeln!(
                                        stdout,
                                        "{}",
                                        json!({
                                            "type": "tool.error",
                                            "payload": {"approval_required": true, "policy": policy, "phase": "post"}
                                        })
                                    )?;
                                } else {
                                    writeln!(
                                        stdout,
                                        "{}",
                                        json!({
                                            "type": "tool.result",
                                            "payload": {"name": name, "result": out}
                                        })
                                    )?;
                                }
                            }
                            Err(e) => {
                                if policy == "on_failure" && !cli.yes {
                                    writeln!(
                                        stdout,
                                        "{}",
                                        json!({
                                            "type": "tool.error",
                                            "payload": {"approval_required": true, "policy": policy, "phase": "post"}
                                        })
                                    )?;
                                } else {
                                    writeln!(
                                        stdout,
                                        "{}",
                                        json!({
                                            "type": "tool.error",
                                            "payload": {"message": e.to_string()}
                                        })
                                    )?;
                                }
                            }
                        }
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

#[derive(Default)]
struct Policies(HashMap<String, String>);

fn load_policies(path: Option<&PathBuf>) -> Result<Policies> {
    let path = if let Some(p) = path {
        p.clone()
    } else {
        PathBuf::from(".devit/devit.toml")
    };
    if !path.exists() {
        return Ok(default_policies());
    }
    let s = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    // Try format: [mcp.approvals]\n<tool> = "policy"
    #[derive(serde::Deserialize, Default)]
    struct Root {
        mcp: Option<Mcp>,
    }
    #[derive(serde::Deserialize, Default)]
    struct Mcp {
        approvals: Option<HashMap<String, String>>,
    }
    let r: Root = toml::from_str(&s).context("parse TOML")?;
    let mut out = default_policies();
    if let Some(map) = r.mcp.and_then(|m| m.approvals) {
        for (k, v) in map.into_iter() {
            out.0.insert(k, v.to_ascii_lowercase());
        }
    }
    Ok(out)
}

fn default_policies() -> Policies {
    let mut m = HashMap::new();
    m.insert("devit.tool_list".to_string(), "never".to_string());
    m.insert("devit.tool_call".to_string(), "on_request".to_string());
    m.insert("echo".to_string(), "never".to_string());
    Policies(m)
}

fn default_policy_for(tool: &str) -> String {
    match tool {
        "devit.tool_list" => "never".to_string(),
        "devit.tool_call" => "on_request".to_string(),
        "echo" => "never".to_string(),
        _ => "on_request".to_string(),
    }
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
