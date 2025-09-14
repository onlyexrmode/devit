//! CLI expérimentale MCP (stdio).
//! Construite uniquement avec: --features experimental
//! Usage:
//!   devit-mcp --cmd '<serveur MCP>' [--handshake-only]
//!   devit-mcp --cmd '<serveur MCP>' --echo "hello"

use clap::{ArgAction, Parser};
use devit_cli_mcp as mcp_mod;

// Import du module `src/mcp.rs` depuis un binaire secondaire.
#[path = "../mcp.rs"]
mod devit_cli_mcp;

use anyhow::{anyhow, Context, Result};
use serde_json::json;
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

    /// Dry-run: n'exécute pas la commande, affiche juste le plan
    #[arg(long, action = ArgAction::SetTrue)]
    dry_run: bool,

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
        println!(
            "{{\"dry_run\":true,\"cmd\":{cmd},\"timeout-secs\":{t},\
             \"handshake_only\":{h},\"echo\":{echo}}}",
            cmd = serde_json::to_string(&cli.cmd)?,
            t = timeout.as_secs(),
            h = cli.handshake_only,
            echo = serde_json::to_string(&cli.echo)?,
        );
        return Ok(());
    }

    let mut client = mcp_mod::McpClient::spawn_cmd(&cli.cmd, timeout)
        .with_context(|| "spawn MCP server failed")?;

    let caps = client
        .handshake(&cli.client_version)
        .with_context(|| "handshake failed")?;
    println!(
        "{}",
        serde_json::to_string(&json!({
            "type": "handshake.ok",
            "payload": { "tools": caps.tools }
        }))?
    );

    if cli.handshake_only {
        return Ok(());
    }

    if let Some(text) = cli.echo.as_deref() {
        let r = client.tool_echo(text).with_context(|| "echo call failed")?;
        println!("{}", serde_json::to_string(&r)?);
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
