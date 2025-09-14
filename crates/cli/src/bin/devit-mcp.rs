//! CLI expérimentale MCP (stdio).
//! Construite uniquement avec: --features experimental
//! Usage:
//! devit-mcp --cmd '<serveur MCP>' --handshake-only
//! devit-mcp --cmd '<serveur MCP>' --echo "hello"
//! devit-mcp --cmd '<serveur MCP>' --call devit.tool_list --json '{}'
//! devit-mcp --cmd '<serveur MCP>' --policy
//! devit-mcp --cmd '<serveur MCP>' --health
//! devit-mcp --cmd '<serveur MCP>' --stats
//!   devit-mcp --cmd '<serveur MCP>' --policy

use clap::{ArgAction, Parser};
use devit_cli_mcp as mcp_mod;

// Import du module `src/mcp.rs` depuis un binaire secondaire.
#[path = "../mcp.rs"]
mod devit_cli_mcp;

use anyhow::{anyhow, Context, Result};
use serde_json::json;
use std::io::{self, Write};
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(name = "devit-mcp")]
#[command(about = "Client MCP stdio (expérimental)")]
struct Cli {
    /// Commande à lancer (serveur MCP, via bash -lc)
    #[arg(long)]
    cmd: String,

    /// Handshake seulement (ping/version/capabilities)
    #[arg(long, action = ArgAction::SetTrue)]
    handshake_only: bool,

    /// Tool echo: envoie un appel avec {text}
    #[arg(long)]
    echo: Option<String>,

    /// Appel générique: nom d'outil MCP (ex: devit.tool_list)
    #[arg(long = "call")]
    call_name: Option<String>,
    /// Arguments JSON pour l'appel générique (--call)
    #[arg(long = "json")]
    call_args_json: Option<String>,

    /// Dry-run: n'exécute pas la commande, affiche juste le plan
    #[arg(long, action = ArgAction::SetTrue)]
    dry_run: bool,

    /// Affiche la politique serveur via MCP (server.policy)
    #[arg(long, action = ArgAction::SetTrue)]
    policy: bool,
    /// Affiche la santé serveur via MCP (server.health)
    #[arg(long, action = ArgAction::SetTrue)]
    health: bool,
    /// Affiche les stats serveur via MCP (server.stats)
    #[arg(long, action = ArgAction::SetTrue)]
    stats: bool,

    /// Timeout par message (secs). Par défaut: DEVIT_TIMEOUT_SECS ou 30
    #[arg(long = "timeout-secs")]
    timeout_secs: Option<u64>,

    /// Version client à annoncer
    #[arg(long, default_value = "0.2.0-rc.1")]
    client_version: String,
}

fn main() {
    match real_main() {
        Ok(()) => {}
        Err(e) => {
            if e.downcast_ref::<mcp_mod::TimeoutErr>().is_some() {
                eprintln!("error: timeout (no response within per-message deadline)");
                std::process::exit(124);
            }
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    }
}

fn real_main() -> Result<()> {
    let cli = Cli::parse();
    let timeout = timeout(cli.timeout_secs);

    if cli.dry_run {
        safe_print_line(&format!(
            "{{\"dry_run\":true,\"cmd\":{cmd},\"timeout-secs\":{t},\
             \"handshake_only\":{h},\"echo\":{echo}}}",
            cmd = serde_json::to_string(&cli.cmd)?,
            t = timeout.as_secs(),
            h = cli.handshake_only,
            echo = serde_json::to_string(&cli.echo)?,
        ));
        return Ok(());
    }

    let mut client = mcp_mod::McpClient::spawn_cmd(&cli.cmd, timeout)
        .with_context(|| "spawn MCP server failed")?;

    let caps = client
        .handshake(&cli.client_version)
        .with_context(|| "handshake failed")?;
    safe_print_json(&json!({
        "type": "handshake.ok",
        "payload": { "tools": caps.tools }
    }))?;

    if cli.handshake_only {
        return Ok(());
    }

    if cli.policy {
        let v = client.tool_call("server.policy", serde_json::json!({}))?;
        safe_print_json(&v)?;
        return Ok(());
    }

    if cli.health {
        let v = client.tool_call("server.health", serde_json::json!({}))?;
        safe_print_json(&v)?;
        return Ok(());
    }

    if cli.stats {
        let v = client.tool_call("server.stats", serde_json::json!({}))?;
        safe_print_json(&v)?;
        return Ok(());
    }

    if let Some(text) = cli.echo.as_deref() {
        let r = client.tool_echo(text).with_context(|| "echo call failed")?;
        safe_print_json(&r)?;
        return Ok(());
    }

    if let Some(name) = cli.call_name.as_deref() {
        let args_v: serde_json::Value = if let Some(js) = cli.call_args_json.as_deref() {
            serde_json::from_str(js).context("--json must be valid JSON")?
        } else {
            serde_json::json!({})
        };
        let v = client.tool_call(name, args_v)?;
        safe_print_json(&v)?;
        return Ok(());
    }

    Err(anyhow!("nothing to do: pass --handshake-only or --echo"))
}

fn timeout(override_secs: Option<u64>) -> Duration {
    if let Some(s) = override_secs {
        return Duration::from_secs(s);
    }
    mcp_mod::timeout_from_env()
}

fn safe_print_json(v: &serde_json::Value) -> Result<()> {
    let s = serde_json::to_string(v)?;
    safe_print_line(&s);
    Ok(())
}

fn safe_print_line(s: &str) {
    let mut out = io::stdout();
    if let Err(e) = (|| -> io::Result<()> {
        out.write_all(s.as_bytes())?;
        out.write_all(b"\n")?;
        out.flush()?;
        Ok(())
    })() {
        if e.kind() == io::ErrorKind::BrokenPipe {
            std::process::exit(0);
        } else {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    }
}
