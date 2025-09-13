// # -----------------------------
// # crates/cli/src/main.rs
// # -----------------------------
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use devit_agent::Agent;
use devit_common::{Config, PolicyCfg};
use devit_tools::{codeexec, git};
use std::fs;
use std::io::{stdin, Read};
use sha2::{Digest, Sha256};

#[derive(Parser, Debug)]
#[command(name = "devit", version, about = "DevIt CLI - patch-only agent", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Propose a patch (unified diff)
    Suggest {
        #[arg(default_value = ".")]
        path: String,
        /// Goal to achieve (e.g., "add websocket support")
        #[arg(short, long)]
        goal: String,
    },

    /// Apply a unified diff to the workspace
    Apply {
        /// Read diff from file, or '-' for stdin (default)
        #[arg(default_value = "-")]
        input: String,
        /// Auto-approve (no prompt)
        #[arg(long)]
        yes: bool,
        /// Continue even if worktree/index is dirty (try 3-way)
        #[arg(long)]
        force: bool,
    },

    /// Chain: suggest -> (approval) -> apply -> commit -> test
    Run {
        /// Workspace path (default: current dir)
        #[arg(default_value = ".")]
        path: String,
        /// Goal to achieve
        #[arg(short, long)]
        goal: String,
        /// Auto-approve write/apply
        #[arg(long)]
        yes: bool,
        /// Continue even if worktree/index is dirty (try 3-way)
        #[arg(long)]
        force: bool,
    },

    /// Run tests according to detected stack (Cargo/npm/CMake)
    Test,

    /// Tools (experimental): list and call
    Tool {
        #[command(subcommand)]
        action: ToolCmd,
    },
}

#[derive(Subcommand, Debug)]
enum ToolCmd {
    /// List available tools (JSON)
    List,
    /// Call a tool
    Call {
        /// Tool name (fs_patch_apply | shell_exec)
        name: String,
        /// Read diff from file, or '-' for stdin (fs_patch_apply), or command for shell_exec after '--'
        #[arg(default_value = "-")]
        input: String,
        /// Auto-approve (no prompt)
        #[arg(long)]
        yes: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let cli = Cli::parse();
    let cfg: Config = load_cfg("devit.toml").context("load config")?;
    let agent = Agent::new(cfg.clone());

    match cli.command {
        Some(Commands::Suggest { path, goal }) => {
            let ctx = collect_context(&path)?;
            let diff = agent.suggest_patch(&goal, &ctx).await?;
            println!("{}", diff);
        }
        Some(Commands::Apply { input, yes, force }) => {
            ensure_git_repo()?;
            if cfg.policy.sandbox.to_lowercase() == "read-only" {
                anyhow::bail!("policy.sandbox=read-only: apply refusé (aucune écriture autorisée)");
            }
            let patch = read_patch(&input)?;
            // 0) index propre ?
            if !git::is_worktree_clean() && !force {
                anyhow::bail!(
                    "Le worktree ou l'index contient des modifications.\n\
                     - Commit/stash tes changements OU relance avec --force (tentative 3-way)."
                );
            }
            // 1) dry-run
            git::apply_check(&patch)?; // renvoie Err(...) avec le message Git détaillé
            let ns = git::numstat(&patch)?;
            let files = ns.len();
            let added: u64 = ns.iter().map(|e| e.added).sum();
            let deleted: u64 = ns.iter().map(|e| e.deleted).sum();
            let summary = format!("{} fichier(s), +{}, -{}", files, added, deleted);
            // 3) approval (sauf policy 'never' ou --yes)
            let must_ask = !yes && cfg.policy.approval.to_lowercase() != "never";
            if must_ask {
                eprintln!("Patch prêt: {summary}");
                for e in ns.iter().take(10) {
                    eprintln!("  - {}", e.path);
                }
                if ns.len() > 10 {
                    eprintln!("  … ({} autres)", ns.len() - 10);
                }
                if !ask_approval()? {
                    anyhow::bail!("Annulé par l'utilisateur.");
                }
            }
            // 4) apply + commit
            if !git::apply_index(&patch)? {
                anyhow::bail!("Échec git apply --index.");
            }
            // Génère un titre de commit (LLM si dispo, sinon fallback)
            let _diff_head = patch.lines().take(60).collect::<Vec<_>>().join("
");
            // Pas de goal ici → fallback générique
            let commit_msg = default_commit_msg(None, &summary);
            let attest = compute_attest_hash(&patch);
            let full_msg = format!("{}\n\nDevIt-Attest: {}", commit_msg, attest);
            if !git::commit(&full_msg)? {
                anyhow::bail!("Échec git commit.");
            }
            let sha = git::head_short().unwrap_or_default();
            println!("✅ Commit {}: {}", sha, commit_msg);
        }
        Some(Commands::Run {
            path,
            goal,
            yes,
            force,
        }) => {
            // OnRequest: aucune action automatique; nécessite --yes
            { let eff = cfg.policy.approvals.as_ref().and_then(|m| m.get("git").map(|s| s.to_ascii_lowercase())).unwrap_or_else(|| cfg.policy.approval.to_ascii_lowercase()); if eff == "on-request" && !yes {
                anyhow::bail!("`devit run` nécessite --yes lorsque policy.approval=on-request"); } }
            }
            if cfg.policy.sandbox.to_lowercase() == "read-only" {
                anyhow::bail!(
                    "policy.sandbox=read-only: run/apply refusé (aucune écriture autorisée)"
                );
            }
            ensure_git_repo()?;
            // 1) suggest
            let ctx = collect_context(&path)?;
            let patch = agent.suggest_patch(&goal, &ctx).await?;
            if patch.trim().is_empty() {
                anyhow::bail!("Le backend n'a pas produit de diff.");
            }
            // 2) index propre ?
            if !git::is_worktree_clean() && !force {
                anyhow::bail!(
                    "Le worktree ou l'index contient des modifications.\n\
                     - Commit/stash tes changements OU relance avec --force (tentative 3-way)."
                );
            }
            // 3) dry-run + résumé
            git::apply_check(&patch)?;
            let ns = git::numstat(&patch)?;
            let files = ns.len();
            let added: u64 = ns.iter().map(|e| e.added).sum();
            let deleted: u64 = ns.iter().map(|e| e.deleted).sum();
            let summary = format!("{} fichier(s), +{}, -{}", files, added, deleted);
            if requires_approval_tool(&cfg.policy, "git", yes, "write") {
                eprintln!("Patch prêt (RUN): {summary}");
                for e in ns.iter().take(10) {
                    eprintln!("  - {}", e.path);
                }
                if ns.len() > 10 {
                    eprintln!("  … ({} autres)", ns.len() - 10);
                }
                if !ask_approval()? {
                    anyhow::bail!("Annulé par l'utilisateur.");
                }
            }
            // 4) apply + commit
            if !git::apply_index(&patch)? {
                anyhow::bail!("Échec git apply --index (et fallback --3way).");
            }
            let diff_head = patch.lines().take(60).collect::<Vec<_>>().join("
");
            let commit_msg = agent
                .commit_message(&goal, &summary, &diff_head)
                .await
                .ok()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| default_commit_msg(Some(&goal), &summary));
            let attest = compute_attest_hash(&patch);
            let full_msg = format!("{}\n\nDevIt-Attest: {}", commit_msg, attest);
            if !git::commit(&full_msg)? {
                anyhow::bail!("Échec git commit.");
            }
            let sha = git::head_short().unwrap_or_default();
            println!("✅ Commit {}: {}", sha, commit_msg);
            // 5) tests
            let (code, out) = codeexec::run_tests_with_output()?;
            println!("{}", out);
            if code == 0 {
                println!("✅ Tests PASS");
            } else {
                anyhow::bail!("❌ Tests FAIL (exit code {code})");
            }
        }
        Some(Commands::Test) => {
            if cfg.policy.sandbox.to_lowercase() == "read-only" {
                anyhow::bail!(
                    "policy.sandbox=read-only: test refusé (exécution/écriture interdites)"
                );
            }
            match codeexec::run_tests_with_output() {
                Ok((code, out)) => {
                    println!("{}", out);
                    if code == 0 {
                        println!("✅ Tests PASS");
                    } else {
                        anyhow::bail!("❌ Tests FAIL (exit code {code})");
                    }
                }
                Err(e) => {
                    anyhow::bail!("Impossible d'exécuter les tests: {e}");
                }
            }
        }
        Some(Commands::Tool { action }) => {
            match action {
                ToolCmd::List => {
                    let tools = serde_json::json!([
                        {"name": "fs_patch_apply", "args": ["input"], "description": "Apply unified diff to index (no commit)"},
                        {"name": "shell_exec", "args": ["cmd..."], "description": "Execute command via shell (experimental)"}
                    ]);
                    println!("{}", serde_json::to_string_pretty(&tools).unwrap());
                }
                ToolCmd::Call { name, input, yes } => {
                    match name.as_str() {
                        "fs_patch_apply" => {
                            ensure_git_repo()?;
                            if cfg.policy.sandbox.to_lowercase() == "read-only" { anyhow::bail!("policy.sandbox=read-only: apply refusé (aucune écriture autorisée)"); }
                            let patch = read_patch(&input)?;
                            git::apply_check(&patch)?;
                            let ask = requires_approval_tool(&cfg.policy, "git", yes, "write");
                            if ask && !ask_approval()? { anyhow::bail!("Annulé par l'utilisateur."); }
                            if !git::apply_index(&patch)? { anyhow::bail!("Échec git apply --index (patch-only)." ); }
                            println!("ok: patch applied to index (no commit)");
                        }
                        "shell_exec" => {
                            // Minimal experimental implementation
                            let ask = requires_approval_tool(&cfg.policy, "shell", yes, "exec");
                            if ask && !ask_approval()? { anyhow::bail!("Annulé par l'utilisateur."); }
                            // Execute using /bin/sh -lc <input>
                            let cmd = if input == "-" { anyhow::bail!("shell_exec requires a command string as input"); } else { input };
                            let status = std::process::Command::new("bash").arg("-lc").arg(&cmd).status()?;
                            let code = status.code().unwrap_or(-1);
                            if code != 0 { anyhow::bail!("shell_exec exit code {code}"); }
                        }
                        _ => anyhow::bail!("outil inconnu: {}", name),
                    }
                }
            }
        }
        _ => {
            eprintln!(
                "Usage:\n  devit suggest --goal \"...\" [PATH]\n  devit apply [-|PATCH.diff] [--yes] [--force]\n  devit run --goal \"...\" [PATH] [--yes] [--force]\n  devit test"
            );
        }
    }

    Ok(())
}

fn load_cfg(path: &str) -> Result<Config> {
    // Permettre un override via variable d'environnement
    let cfg_path = std::env::var("DEVIT_CONFIG").unwrap_or_else(|_| path.to_string());
    let s = fs::read_to_string(&cfg_path)
        .with_context(|| format!("unable to read config at {}", cfg_path))?;
    let cfg: Config = toml::from_str(&s)?;
    Ok(cfg)
}

fn collect_context(path: &str) -> Result<String> {
    // MVP: naive — list a few files with content; later: git-aware, size limits
    let mut out = String::new();
    for entry in walkdir::WalkDir::new(path).max_depth(2) {
        let entry = entry?;
        if entry.file_type().is_file() {
            let p = entry.path().display().to_string();
            if p.ends_with(".rs") || p.ends_with("Cargo.toml") {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    out.push_str(&format!("\n>>> FILE: {}\n{}\n", p, content));
                }
            }
        }
    }
    Ok(out)
}

fn read_patch(input: &str) -> Result<String> {
    if input == "-" {
        let mut s = String::new();
        stdin().lock().read_to_string(&mut s)?;
        Ok(s)
    } else {
        Ok(fs::read_to_string(input)?)
    }
}

fn ensure_git_repo() -> Result<()> {
    if !git::is_git_available() {
        anyhow::bail!("git n'est pas disponible dans le PATH.");
    }
    if !git::in_repo() {
        anyhow::bail!("pas dans un dépôt git (git rev-parse --is-inside-work-tree).");
    }
    Ok(())
}

fn ask_approval() -> Result<bool> {
    use std::io::{self, Write};
    eprint!("Appliquer le patch et committer ? [y/N] ");
    io::stderr().flush().ok();
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    let ans = buf.trim().to_lowercase();
    Ok(ans == "y" || ans == "yes")
}

fn default_commit_msg(goal: Option<&str>, summary: &str) -> String {
    match goal {
        Some(g) if !g.trim().is_empty() => format!("feat: {} ({})", g.trim(), summary),
        _ => format!("chore: apply patch ({})", summary),
    }
}

fn requires_approval_tool(policy: &PolicyCfg, tool: &str, yes_flag: bool, action: &str) -> bool {
    let eff = policy.approvals.as_ref()
        .and_then(|m| m.get(&tool.to_ascii_lowercase()).map(|s| s.to_ascii_lowercase()))
        .unwrap_or_else(|| policy.approval.to_ascii_lowercase());
    match (eff.as_str(), action) {
        ("never", _) => false,
        ("untrusted", _) => true,
        ("on-request", _) => !yes_flag,
        ("on-failure", "write") => !yes_flag,
        ("on-failure", _) => false,
        _ => !yes_flag,
    }
}

fn compute_attest_hash(patch: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(patch.as_bytes());
    let out = hasher.finalize();
    hex::encode(out)
}
