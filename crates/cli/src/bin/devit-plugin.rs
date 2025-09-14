//! CLI expérimental: gestion/invocation de plugins WASI (JSON I/O).
//! Construit uniquement avec: --features experimental
//! Usage:
//!   devit-plugin list [--dir <registry>]
//!   devit-plugin invoke --id <id> [--dir <registry>] < input.json > output.json
//!   devit-plugin invoke --manifest <file> < input.json > output.json

use anyhow::{anyhow, Context, Result};
use clap::{Args, Parser, Subcommand};
use serde_json::Value;
use std::io::{self, Read};
use std::path::PathBuf;
use std::time::Duration;

// Import du module loader
#[path = "../plugins.rs"]
mod plugins;

#[derive(Parser, Debug)]
#[command(name = "devit-plugin")]
#[command(about = "Plugins WASM/WASI (expérimental) — JSON stdin→stdout")]
struct Cli {
    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Liste les plugins présents dans le registry (JSON)
    List(ListArgs),
    /// Invoque un plugin (lit JSON sur stdin, écrit JSON sur stdout)
    Invoke(InvokeArgs),
}

#[derive(Args, Debug)]
struct ListArgs {
    /// Racine du registry (défaut: .devit/plugins ou $DEVIT_PLUGINS_DIR)
    #[arg(long)]
    dir: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct InvokeArgs {
    /// Identifiant du plugin (dossier .devit/plugins/<id>)
    #[arg(long)]
    id: Option<String>,
    /// Chemin explicite d'un manifeste (bypass l'ID)
    #[arg(long)]
    manifest: Option<PathBuf>,
    /// Timeout en secondes (fallback DEVIT_TIMEOUT_SECS, sinon 30)
    #[arg(long)]
    timeout_secs: Option<u64>,
    /// Racine du registry (si --id)
    #[arg(long)]
    dir: Option<PathBuf>,
}

fn main() {
    match real_main() {
        Ok(()) => {}
        Err(e) => {
            if e.downcast_ref::<plugins::TimeoutErr>().is_some() {
                eprintln!("error: plugin timeout");
                std::process::exit(124);
            }
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    }
}

fn real_main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Commands::List(a) => do_list(a),
        Commands::Invoke(a) => do_invoke(a),
    }
}

fn do_list(a: ListArgs) -> Result<()> {
    let list = plugins::discover_plugins(a.dir.as_deref().map(|p| p as &std::path::Path))?;
    println!("{}", serde_json::to_string_pretty(&list)?);
    Ok(())
}

fn do_invoke(a: InvokeArgs) -> Result<()> {
    let timeout = a.timeout_secs.map(Duration::from_secs);
    let mut buf = String::new();
    io::stdin().read_to_string(&mut buf)?;
    if buf.trim().is_empty() {
        return Err(anyhow!("stdin is empty; expected JSON request"));
    }
    let _req_json: Value = serde_json::from_str(buf.trim()).context("stdin must be valid JSON")?;

    let out = if let Some(manifest) = a.manifest {
        plugins::invoke_manifest(&manifest, buf.trim(), timeout)?
    } else if let Some(id) = a.id {
        plugins::invoke_by_id(&id, buf.trim(), timeout, a.dir.as_deref().map(|p| p as &std::path::Path))?
    } else {
        return Err(anyhow!("provide either --id <id> or --manifest <file>"));
    };
    println!("{}", serde_json::to_string(&out)?);
    Ok(())
}
