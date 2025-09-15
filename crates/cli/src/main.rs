// # -----------------------------
// # crates/cli/src/main.rs
// # -----------------------------
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use devit_agent::Agent;
use devit_common::{Config, Event, PolicyCfg};
use devit_sandbox as sandbox;
use devit_tools::{codeexec, git};
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{stdin, Read, Write as _};
use std::path::{Path, PathBuf};
use std::time::Duration;
mod context;

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

    /// Context utilities
    Context {
        #[command(subcommand)]
        action: CtxCmd,
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

#[derive(Subcommand, Debug)]
enum CtxCmd {
    /// Build a file index at .devit/index.json
    Map {
        /// Root path (default: .)
        #[arg(default_value = ".")]
        path: String,
        /// Max bytes per file (default: 262144)
        #[arg(long = "max-bytes-per-file")]
        max_bytes_per_file: Option<usize>,
        /// Max files to index (default: 5000)
        #[arg(long = "max-files")]
        max_files: Option<usize>,
        /// Allowed extensions CSV (e.g., rs,toml,md)
        #[arg(long = "ext-allow")]
        ext_allow: Option<String>,
        /// Output JSON path (default: .devit/index.json)
        #[arg(long = "json-out")]
        json_out: Option<PathBuf>,
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
            let _diff_head = patch.lines().take(60).collect::<Vec<_>>().join(
                "
",
            );
            // Pas de goal ici → fallback générique
            let commit_msg = default_commit_msg(None, &summary);
            let attest = compute_attest_hash(&patch);
            let full_msg = if cfg.provenance.footer {
                format!("{}\n\nDevIt-Attest: {}", commit_msg, attest)
            } else {
                commit_msg.clone()
            };
            if !git::commit(&full_msg)? {
                anyhow::bail!("Échec git commit.");
            }
            if cfg.git.use_notes {
                let _ = git::add_note(&format!("DevIt-Attest: {}", attest));
            }
            journal_event(&Event::Attest {
                hash: attest.clone(),
            })?;
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
            {
                let eff = cfg
                    .policy
                    .approvals
                    .as_ref()
                    .and_then(|m| m.get("git").map(|s| s.to_ascii_lowercase()))
                    .unwrap_or_else(|| cfg.policy.approval.to_ascii_lowercase());
                if eff == "on-request" && !yes {
                    eprintln!("`devit run` nécessite --yes lorsque policy.approval=on-request");
                    anyhow::bail!("nécessite --yes");
                }
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
            let diff_head = patch.lines().take(60).collect::<Vec<_>>().join(
                "
",
            );
            let commit_msg = agent
                .commit_message(&goal, &summary, &diff_head)
                .await
                .ok()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| default_commit_msg(Some(&goal), &summary));
            let attest = compute_attest_hash(&patch);
            let full_msg = if cfg.provenance.footer {
                format!("{}\n\nDevIt-Attest: {}", commit_msg, attest)
            } else {
                commit_msg.clone()
            };
            if !git::commit(&full_msg)? {
                anyhow::bail!("Échec git commit.");
            }
            if cfg.git.use_notes {
                let _ = git::add_note(&format!("DevIt-Attest: {}", attest));
            }
            journal_event(&Event::Attest {
                hash: attest.clone(),
            })?;
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
        Some(Commands::Tool { action }) => match action {
            ToolCmd::List => {
                let tools = serde_json::json!([
                    {"name": "fs_patch_apply", "args": {"patch": "string", "mode": "index|worktree", "check_only": "bool"}, "description": "Apply unified diff (index/worktree), or --check-only"},
                    {"name": "shell_exec", "args": {"cmd": "string"}, "description": "Execute command via sandboxed shell (safe-list)"}
                ]);
                println!("{}", serde_json::to_string_pretty(&tools).unwrap());
            }
            ToolCmd::Call { name, input, yes } => {
                if name == "-" {
                    let mut s = String::new();
                    stdin().lock().read_to_string(&mut s)?;
                    let req: serde_json::Value =
                        serde_json::from_str(&s).context("tool call: JSON invalide sur stdin")?;
                    let tname = req.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let args = req.get("args").cloned().unwrap_or(serde_json::json!({}));
                    let yes_flag = req.get("yes").and_then(|v| v.as_bool()).unwrap_or(yes);
                    let res = tool_call_json(&cfg, tname, args, yes_flag);
                    match res {
                        Ok(v) => println!(
                            "{}",
                            serde_json::to_string(&serde_json::json!({"ok": true, "result": v}))?
                        ),
                        Err(e) => println!(
                            "{}",
                            serde_json::to_string(
                                &serde_json::json!({"ok": false, "error": e.to_string()})
                            )?
                        ),
                    }
                } else {
                    let out = tool_call_legacy(&cfg, &name, &input, yes);
                    if let Err(e) = out {
                        anyhow::bail!(e);
                    }
                }
            }
        },
        Some(Commands::Context { action }) => match action {
            CtxCmd::Map {
                path,
                max_bytes_per_file,
                max_files,
                ext_allow,
                json_out,
            } => {
                let written = build_context_index_adv(
                    &path,
                    max_bytes_per_file,
                    max_files,
                    ext_allow.as_deref(),
                    json_out.as_deref(),
                )?;
                println!("index écrit: {}", written.display());
            }
        },
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
    let eff = policy
        .approvals
        .as_ref()
        .and_then(|m| {
            m.get(&tool.to_ascii_lowercase())
                .map(|s| s.to_ascii_lowercase())
        })
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

fn compute_call_attest(tool: &str, args: &serde_json::Value) -> Result<String> {
    // HMAC(tool_name, sha256(args_json), timestamp_ms)
    let ts_ms: u128 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let args_json = serde_json::to_string(args)?;
    let mut hasher = Sha256::new();
    hasher.update(args_json.as_bytes());
    let args_sha = hex::encode(hasher.finalize());
    let key = hmac_key()?;
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(&key).expect("HMAC key");
    let material = format!("{}:{}:{}", tool, args_sha, ts_ms);
    mac.update(material.as_bytes());
    Ok(hex::encode(mac.finalize().into_bytes()))
}

fn ensure_devit_dir() -> Result<PathBuf> {
    let p = Path::new(".devit");
    if !p.exists() {
        fs::create_dir_all(p)?;
    }
    Ok(p.to_path_buf())
}

fn hmac_key() -> Result<Vec<u8>> {
    let dir = ensure_devit_dir()?;
    let key_path = dir.join("hmac.key");
    if key_path.exists() {
        return Ok(fs::read(key_path)?);
    }
    let mut key = vec![0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    fs::write(&key_path, &key)?;
    Ok(key)
}

fn journal_event(ev: &Event) -> Result<()> {
    let dir = ensure_devit_dir()?;
    let jpath = dir.join("journal.jsonl");
    let key = hmac_key()?;
    let ev_json = serde_json::to_vec(ev)?;
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(&key).expect("HMAC key");
    mac.update(&ev_json);
    let sig = hex::encode(mac.finalize().into_bytes());
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let rec = serde_json::json!({ "ts": ts, "event": ev, "sig": sig });
    let line = serde_json::to_string(&rec)? + "\n";
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(jpath)?
        .write_all(line.as_bytes())?;
    Ok(())
}

fn build_context_index_adv(
    root: &str,
    max_bytes_per_file: Option<usize>,
    max_files: Option<usize>,
    ext_allow: Option<&str>,
    json_out: Option<&Path>,
) -> Result<PathBuf> {
    let dir = ensure_devit_dir()?;
    let out = json_out
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| dir.join("index.json"));
    // Timeout support
    let timeout = std::env::var("DEVIT_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_secs);
    let opts = crate::context::ContextOpts {
        max_bytes_per_file: max_bytes_per_file.unwrap_or(262_144),
        max_files: max_files.unwrap_or(5000),
        ext_allow: ext_allow.map(|s| {
            s.split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect()
        }),
        timeout,
        out_path: out.clone(),
    };
    match crate::context::generate_index(Path::new(root), &opts) {
        Ok(w) => Ok(w),
        Err(e) => {
            if e.to_string().contains("timeout") {
                eprintln!("error: context map timeout");
                std::process::exit(124);
            }
            Err(e)
        }
    }
}

// legacy helper removed; scanning now handled in context module

fn tool_call_json(
    cfg: &Config,
    name: &str,
    args: serde_json::Value,
    yes: bool,
) -> Result<serde_json::Value> {
    match name {
        "fs_patch_apply" => {
            ensure_git_repo()?;
            if cfg.policy.sandbox.to_lowercase() == "read-only" {
                anyhow::bail!("policy.sandbox=read-only: apply refusé (aucune écriture autorisée)");
            }
            let patch = args.get("patch").and_then(|v| v.as_str()).unwrap_or("");
            let mode = args.get("mode").and_then(|v| v.as_str()).unwrap_or("index");
            let check_only = args
                .get("check_only")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if patch.is_empty() {
                anyhow::bail!("fs_patch_apply: champ 'patch' requis (contenu du diff)");
            }
            git::apply_check(patch)?;
            if check_only {
                return Ok(serde_json::json!({"checked": true}));
            }
            let ask = requires_approval_tool(&cfg.policy, "git", yes, "write");
            if ask && !ask_approval()? {
                anyhow::bail!("Annulé par l'utilisateur.");
            }
            let ok = match mode {
                "worktree" => git::apply_worktree(patch)?,
                _ => git::apply_index(patch)?,
            };
            if !ok {
                anyhow::bail!("Échec git apply ({mode})");
            }
            let attest = compute_attest_hash(patch);
            journal_event(&Event::Attest { hash: attest })?;
            Ok(serde_json::json!({"applied": true, "mode": mode}))
        }
        "shell_exec" => {
            let cmd = args.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
            if cmd.is_empty() {
                anyhow::bail!("shell_exec: champ 'cmd' requis");
            }
            let ask = requires_approval_tool(&cfg.policy, "shell", yes, "exec");
            if ask && !ask_approval()? {
                anyhow::bail!("Annulé par l'utilisateur.");
            }
            let (code, out) = sandbox::run_shell_sandboxed_capture(cmd, &cfg.policy, &cfg.sandbox)?;
            // provenance: attest shell_exec call (tool+args+ts)
            if let Ok(hash) = compute_call_attest("shell_exec", &args) {
                let _ = journal_event(&Event::Attest { hash });
            }
            Ok(serde_json::json!({"exit_code": code, "output": out}))
        }
        _ => anyhow::bail!(format!("outil inconnu: {name}")),
    }
}

fn tool_call_legacy(cfg: &Config, name: &str, input: &str, yes: bool) -> Result<()> {
    match name {
        "fs_patch_apply" => {
            ensure_git_repo()?;
            if cfg.policy.sandbox.to_lowercase() == "read-only" {
                anyhow::bail!("policy.sandbox=read-only: apply refusé (aucune écriture autorisée)");
            }
            let patch = read_patch(input)?;
            git::apply_check(&patch)?;
            let ask = requires_approval_tool(&cfg.policy, "git", yes, "write");
            if ask && !ask_approval()? {
                anyhow::bail!("Annulé par l'utilisateur.");
            }
            if !git::apply_index(&patch)? {
                anyhow::bail!("Échec git apply --index (patch-only).");
            }
            let attest = compute_attest_hash(&patch);
            journal_event(&Event::Attest { hash: attest })?;
            println!("ok: patch applied to index (no commit)");
            Ok(())
        }
        "shell_exec" => {
            let ask = requires_approval_tool(&cfg.policy, "shell", yes, "exec");
            if ask && !ask_approval()? {
                anyhow::bail!("Annulé par l'utilisateur.");
            }
            let cmd = if input == "-" {
                anyhow::bail!("shell_exec requires a command string as input");
            } else {
                input.to_string()
            };
            let code = sandbox::run_shell_sandboxed(&cmd, &cfg.policy, &cfg.sandbox)?;
            if code != 0 {
                anyhow::bail!(format!("shell_exec exit code {code}"));
            }
            // provenance: attest shell_exec legacy call
            if let Ok(hash) = compute_call_attest("shell_exec", &serde_json::json!({"cmd": cmd})) {
                let _ = journal_event(&Event::Attest { hash });
            }
            Ok(())
        }
        _ => anyhow::bail!(format!("outil inconnu: {name}")),
    }
}
