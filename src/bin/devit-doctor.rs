//! Binaire de diagnostic (feature-gated).
use clap::{ArgAction, Parser};
use devit::doctor::{exit_code, gather_report, print_human, print_json, DoctorArgs};

/// Diagnostic d'environnement DevIt (isolé, expérimental).
#[derive(Parser, Debug)]
#[command(name = "devit-doctor")]
#[command(about = "Rapport rapide: toolchain, sandbox, WASI target, backends LLM.")]
struct Cli {
    /// Sortie JSON (machine-readable).
    #[arg(long, action = ArgAction::SetTrue)]
    json: bool,
    /// Échec si un check n'est pas OK (exit code != 0).
    #[arg(long, action = ArgAction::SetTrue)]
    strict: bool,
    /// Vérifier les backends LLM (LM Studio/Ollama).
    #[arg(long, action = ArgAction::SetTrue)]
    check_backends: bool,
    /// URL de base LM Studio (ex: http://127.0.0.1:1234/v1)
    #[arg(long)]
    lm_url: Option<String>,
    /// URL de base Ollama (ex: http://127.0.0.1:11434)
    #[arg(long)]
    ollama_url: Option<String>,
    /// Timeout réseau en millisecondes.
    #[arg(long, default_value_t = 600)]
    timeout_ms: u64,
}

fn main() {
    let cli = Cli::parse();
    let report = gather_report(DoctorArgs {
        check_backends: cli.check_backends,
        lm_url: cli.lm_url.as_deref(),
        ollama_url: cli.ollama_url.as_deref(),
        timeout_ms: cli.timeout_ms,
    });

    if cli.json {
        if let Err(e) = print_json(&report) {
            eprintln!("json error: {e}");
            std::process::exit(1);
        }
    } else {
        print_human(&report);
    }

    if cli.strict {
        std::process::exit(exit_code(&report));
    }
}
