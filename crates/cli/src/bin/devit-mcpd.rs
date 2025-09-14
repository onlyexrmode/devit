//! Serveur MCP stdio (expérimental) exposant des outils DevIt.
//! Exemples:
//! devit-mcpd --yes --devit-bin devit
//! devit-mcpd --policy-dump
//! devit-mcpd --no-audit --max-calls-per-min 30 --cooldown-ms 500
//! devit-mcpd --max-json-kb 256

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use chrono::Utc;
use clap::Parser;
use hmac::{Hmac, Mac};
use rand::{rngs::OsRng, RngCore};
use serde_json::{json, Value};
use sha2::Sha256;
use std::collections::VecDeque;
use std::io::{self, BufRead, Read, Write};
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use std::{collections::HashMap, fs};
type HmacSha256 = Hmac<Sha256>;

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
    /// Affiche la politique effective (JSON) puis quitte
    #[arg(long, action = clap::ArgAction::SetTrue)]
    policy_dump: bool,
    /// Désactive l'audit JSONL signé
    #[arg(long, action = clap::ArgAction::SetTrue)]
    no_audit: bool,
    /// Chemin du journal JSONL
    #[arg(long, default_value = ".devit/journal.jsonl")]
    audit_path: PathBuf,
    /// Chemin de la clé HMAC
    #[arg(long, default_value = ".devit/hmac.key")]
    hmac_key: PathBuf,
    /// Mode dry-run: n'autorise que server.*; refuse toute exécution
    #[arg(long, action = clap::ArgAction::SetTrue)]
    dry_run: bool,

    /// Limite: appels par minute
    #[arg(long = "max-calls-per-min", default_value_t = 60)]
    max_calls_per_min: u32,
    /// Limite: taille JSON max (kB)
    #[arg(long = "max-json-kb", default_value_t = 256)]
    max_json_kb: usize,
    /// Limite: cooldown entre appels (ms)
    #[arg(long = "cooldown-ms", default_value_t = 250)]
    cooldown_ms: u64,
}

fn main() {
    if let Err(e) = real_main() {
        eprintln!("error: {e}");
        std::process::exit(2);
    }
}

fn real_main() -> Result<()> {
    let cli = Cli::parse();
    // Enrich server version with git metadata provided at build time
    let git_desc = option_env!("DEVIT_GIT_DESCRIBE").unwrap_or("unknown");
    let git_sha = option_env!("DEVIT_GIT_SHA").unwrap_or("unknown");
    let server_version = format!("{} ({} {})", cli.server_version, git_desc, git_sha);
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut lines = stdin.lock().lines();
    let timeout = timeout_from_cli_env(cli.timeout_secs);
    let policies = load_policies(cli.config_path.as_ref()).unwrap_or_default();
    let audit = AuditOpts {
        audit_enabled: !cli.no_audit,
        audit_path: cli.audit_path.clone(),
        hmac_key_path: cli.hmac_key.clone(),
        auto_yes: cli.yes,
    };
    let mut state = ServerState::new();

    // --policy-dump: print effective approvals JSON and exit
    if cli.policy_dump {
        let v = policy_dump_json(cli.config_path.as_deref().map(|p| p as &std::path::Path));
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }

    let mut rl = RateLimiter::new(Limits {
        max_calls_per_min: cli.max_calls_per_min,
        max_json_kb: cli.max_json_kb,
        cooldown: Duration::from_millis(cli.cooldown_ms),
    });
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
                            "server": server_version,
                            "server_name": "devit-mcpd"
                        }
                    })
                )?;
            }
            "capabilities" => {
                // Expose tools, including policy introspection
                writeln!(
                    stdout,
                    "{}",
                    json!({"type":"capabilities","payload":{"tools":[
                        "devit.tool_list",
                        "devit.tool_call",
                        "server.context_head",
                        "server.health",
                        "server.stats",
                        "server.policy",
                        "echo"
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
                // Dry-run guard: only server.* tools allowed
                let is_server_tool = name == "server.policy"
                    || name == "server.health"
                    || name == "server.stats"
                    || name == "server.context_head";
                if cli.dry_run && !is_server_tool {
                    let tool_key = name;
                    audit_pre(&audit, tool_key, "dry-run-deny");
                    state.bump_err(tool_key);
                    writeln!(
                        stdout,
                        "{}",
                        json!({"type":"tool.error","payload":{
                            "name": tool_key,
                            "dry_run": true,
                            "denied": true,
                            "reason": "server in dry-run (server.* only)"
                        }})
                    )?;
                    stdout.flush()?;
                    continue;
                }
                let policy = policies
                    .0
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| default_policy_for(name));
                // on_request/untrusted: require approval before running
                if (policy == "on_request" || policy == "untrusted") && !cli.yes {
                    audit_pre(&audit, name, "pre-deny");
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
                    "server.context_head" => {
                        let tool_key = "server.context_head";
                        state.bump_call(tool_key);
                        // approvals already handled above (pre-deny) if any
                        // ratelimit
                        let now = Instant::now();
                        if let Err(e) = rl.allow(tool_key, now) {
                            audit_pre(&audit, tool_key, "rate-limit");
                            let v = match e {
                                RateLimitErr::TooManyCalls { limit } => json!({
                                    "type":"tool.error","payload":{
                                        "name": tool_key,
                                        "rate_limited": true,
                                        "reason": "too_many_calls",
                                        "limit_per_min": limit
                                    }
                                }),
                                RateLimitErr::Cooldown { ms_left } => json!({
                                    "type":"tool.error","payload":{
                                        "name": tool_key,
                                        "rate_limited": true,
                                        "reason": "cooldown",
                                        "cooldown_ms": ms_left
                                    }
                                }),
                            };
                            writeln!(stdout, "{}", v)?;
                            continue;
                        }
                        let args_json = payload.get("args").cloned().unwrap_or(json!({}));
                        let limit = args_json
                            .get("limit")
                            .and_then(|x| x.as_u64())
                            .unwrap_or(50)
                            .clamp(1, 1000) as usize;
                        let ext_allow =
                            args_json
                                .get("ext_allow")
                                .and_then(|x| x.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                        .collect::<Vec<String>>()
                                });
                        let index_path = args_json
                            .get("index_path")
                            .and_then(|x| x.as_str())
                            .map(|s| std::path::Path::new(s).to_path_buf());
                        let start = Instant::now();
                        let v =
                            context_head_json(index_path.as_deref(), limit, ext_allow.as_deref());
                        let dur = start.elapsed().as_millis();
                        audit_done(&audit, tool_key, true, dur, None);
                        state.bump_ok(tool_key);
                        writeln!(
                            stdout,
                            "{}",
                            json!({
                                "type": "tool.result",
                                "payload": {"ok": true, "name": tool_key, "head": v}
                            })
                        )?;
                    }
                    "server.health" => {
                        let tool_key = "server.health";
                        state.bump_call(tool_key);
                        let now = Instant::now();
                        if let Err(e) = rl.allow(tool_key, now) {
                            audit_pre(&audit, tool_key, "rate-limit");
                            let v = match e {
                                RateLimitErr::TooManyCalls { limit } => {
                                    json!({"type":"tool.error","payload":{ "name": tool_key, "rate_limited": true, "reason": "too_many_calls", "limit_per_min": limit }})
                                }
                                RateLimitErr::Cooldown { ms_left } => {
                                    json!({"type":"tool.error","payload":{ "name": tool_key, "rate_limited": true, "reason": "cooldown", "cooldown_ms": ms_left }})
                                }
                            };
                            writeln!(stdout, "{}", v)?;
                            continue;
                        }
                        let start = Instant::now();
                        let v = health_json(
                            &audit,
                            &policies,
                            &rl.limits,
                            &state,
                            &server_version,
                            cli.devit_bin.as_deref(),
                        );
                        let dur = start.elapsed().as_millis();
                        audit_done(&audit, tool_key, true, dur, None);
                        state.bump_ok(tool_key);
                        writeln!(
                            stdout,
                            "{}",
                            json!({"type":"tool.result","payload":{"ok":true,"name": tool_key, "health": v}})
                        )?;
                    }
                    "server.stats" => {
                        let tool_key = "server.stats";
                        state.bump_call(tool_key);
                        let now = Instant::now();
                        if let Err(e) = rl.allow(tool_key, now) {
                            audit_pre(&audit, tool_key, "rate-limit");
                            let v = match e {
                                RateLimitErr::TooManyCalls { limit } => {
                                    json!({"type":"tool.error","payload":{ "name": tool_key, "rate_limited": true, "reason": "too_many_calls", "limit_per_min": limit }})
                                }
                                RateLimitErr::Cooldown { ms_left } => {
                                    json!({"type":"tool.error","payload":{ "name": tool_key, "rate_limited": true, "reason": "cooldown", "cooldown_ms": ms_left }})
                                }
                            };
                            writeln!(stdout, "{}", v)?;
                            continue;
                        }
                        let start = Instant::now();
                        let v = stats_json(&state);
                        let dur = start.elapsed().as_millis();
                        audit_done(&audit, tool_key, true, dur, None);
                        state.bump_ok(tool_key);
                        writeln!(
                            stdout,
                            "{}",
                            json!({"type":"tool.result","payload":{"ok":true,"name": tool_key, "stats": v}})
                        )?;
                    }
                    "server.policy" => {
                        let pol = policies
                            .0
                            .get("server.policy")
                            .cloned()
                            .unwrap_or_else(|| default_policy_for("server.policy"));
                        if (pol == "on_request" || pol == "untrusted") && !cli.yes {
                            writeln!(
                                stdout,
                                "{}",
                                json!({
                                    "type": "tool.error",
                                    "payload": {"approval_required": true, "policy": pol, "phase": "pre"}
                                })
                            )?;
                            stdout.flush()?;
                            continue;
                        }
                        // rate-limit for server.policy
                        let tool_key = "server.policy";
                        let now = Instant::now();
                        if let Err(e) = rl.allow(tool_key, now) {
                            audit_pre(&audit, tool_key, "rate-limit");
                            match e {
                                RateLimitErr::TooManyCalls { limit } => {
                                    writeln!(
                                        stdout,
                                        "{}",
                                        json!({"type":"tool.error","payload":{
                                            "name": tool_key,
                                            "rate_limited": true,
                                            "reason": "too_many_calls",
                                            "limit_per_min": limit
                                        }})
                                    )?;
                                    continue;
                                }
                                RateLimitErr::Cooldown { ms_left } => {
                                    writeln!(
                                        stdout,
                                        "{}",
                                        json!({"type":"tool.error","payload":{
                                            "name": tool_key,
                                            "rate_limited": true,
                                            "reason": "cooldown",
                                            "cooldown_ms": ms_left
                                        }})
                                    )?;
                                    continue;
                                }
                            }
                        }
                        let start = Instant::now();
                        let v =
                            policy_effective_json(&audit, &policies, &rl.limits, &server_version);
                        let dur = start.elapsed().as_millis();
                        audit_done(&audit, tool_key, true, dur, None);
                        writeln!(
                            stdout,
                            "{}",
                            json!({
                                "type": "tool.result",
                                "payload": {"ok": true, "name": "server.policy", "policy": v}
                            })
                        )?;
                    }
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
                        // rate-limit for devit.tool_list
                        let tool_key = "devit.tool_list";
                        let now = Instant::now();
                        if let Err(e) = rl.allow(tool_key, now) {
                            audit_pre(&audit, tool_key, "rate-limit");
                            match e {
                                RateLimitErr::TooManyCalls { limit } => {
                                    writeln!(
                                        stdout,
                                        "{}",
                                        json!({"type":"tool.error","payload":{
                                            "name": tool_key,
                                            "rate_limited": true,
                                            "reason": "too_many_calls",
                                            "limit_per_min": limit
                                        }})
                                    )?;
                                    continue;
                                }
                                RateLimitErr::Cooldown { ms_left } => {
                                    writeln!(
                                        stdout,
                                        "{}",
                                        json!({"type":"tool.error","payload":{
                                            "name": tool_key,
                                            "rate_limited": true,
                                            "reason": "cooldown",
                                            "cooldown_ms": ms_left
                                        }})
                                    )?;
                                    continue;
                                }
                            }
                        }
                        let start = Instant::now();
                        match run_devit_list(&bin, timeout) {
                            Ok(out) => {
                                let dur = start.elapsed().as_millis();
                                audit_done(&audit, name, true, dur, None);
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
                                let dur = start.elapsed().as_millis();
                                audit_done(&audit, name, false, dur, Some(&e.to_string()));
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
                        let start = Instant::now();
                        match run_devit_call(&bin, &args_json, timeout) {
                            Ok(out) => {
                                // on_failure: if DevIt reports ok=false, require approval (post)
                                let is_fail = out
                                    .get("ok")
                                    .and_then(|v| v.as_bool())
                                    .map(|b| !b)
                                    .unwrap_or(false);
                                let dur = start.elapsed().as_millis();
                                audit_done(&audit, name, !is_fail, dur, None);
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
                                let dur = start.elapsed().as_millis();
                                audit_done(&audit, name, false, dur, Some(&e.to_string()));
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
    m.insert("server.policy".to_string(), "never".to_string());
    m.insert("server.context_head".to_string(), "never".to_string());
    m.insert("server.health".to_string(), "never".to_string());
    m.insert("server.stats".to_string(), "never".to_string());
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

// -------- Quotas & Rate-limiting --------
#[derive(Clone, Debug)]
pub struct Limits {
    pub max_calls_per_min: u32,
    pub max_json_kb: usize,
    pub cooldown: Duration,
}

struct RateLimiter {
    per_key: HashMap<String, VecDeque<Instant>>,
    last_call: HashMap<String, Instant>,
    limits: Limits,
}

impl RateLimiter {
    fn new(limits: Limits) -> Self {
        Self {
            per_key: HashMap::new(),
            last_call: HashMap::new(),
            limits,
        }
    }
    fn allow(&mut self, key: &str, now: Instant) -> Result<(), RateLimitErr> {
        if let Some(prev) = self.last_call.get(key) {
            if now.duration_since(*prev) < self.limits.cooldown {
                let left = (self.limits.cooldown - now.duration_since(*prev)).as_millis() as u64;
                return Err(RateLimitErr::Cooldown { ms_left: left });
            }
        }
        let q = self.per_key.entry(key.to_string()).or_default();
        while let Some(&t) = q.front() {
            if now.duration_since(t) > Duration::from_secs(60) {
                q.pop_front();
            } else {
                break;
            }
        }
        if q.len() as u32 >= self.limits.max_calls_per_min {
            return Err(RateLimitErr::TooManyCalls {
                limit: self.limits.max_calls_per_min,
            });
        }
        q.push_back(now);
        self.last_call.insert(key.to_string(), now);
        Ok(())
    }
}

#[derive(Debug)]
enum RateLimitErr {
    TooManyCalls { limit: u32 },
    Cooldown { ms_left: u64 },
}
// -------- Audit helpers --------
struct AuditOpts {
    audit_enabled: bool,
    audit_path: PathBuf,
    hmac_key_path: PathBuf,
    auto_yes: bool,
}

fn load_or_create_key(path: &Path) -> Vec<u8> {
    if let Ok(k) = fs::read(path) {
        if k.len() >= 32 {
            return k;
        }
    }
    let mut key = vec![0u8; 32];
    OsRng.fill_bytes(&mut key);
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(path, &key);
    key
}

fn append_signed(path: &Path, key_path: &Path, json_line_no_sig: &str) {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let key = load_or_create_key(key_path);
    let mut mac = HmacSha256::new_from_slice(&key).expect("HMAC key");
    mac.update(json_line_no_sig.as_bytes());
    let sig = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
    let full = format!(
        r#"{},"sig":"{}"}}"#,
        json_line_no_sig.trim_end_matches('}'),
        sig
    );
    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| {
            use std::io::Write;
            f.write_all(full.as_bytes())?;
            f.write_all(b"\n")
        })
        .map_err(|e| eprintln!("audit append failed: {e}"));
}

fn audit_pre(opts: &AuditOpts, tool: &str, phase: &str) {
    if !opts.audit_enabled {
        return;
    }
    let ts = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let line = format!(
        r#"{{"ts":"{ts}","tool":"{tool}","phase":"{phase}","policy":"n/a","auto_yes":{}}}"#,
        opts.auto_yes
    );
    append_signed(
        &opts.audit_path.as_path(),
        &opts.hmac_key_path.as_path(),
        &line,
    );
}

fn audit_done(opts: &AuditOpts, tool: &str, ok: bool, dur_ms: u128, err: Option<&str>) {
    if !opts.audit_enabled {
        return;
    }
    let ts = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let base = if let Some(e) = err {
        let error_json = serde_json::to_string(e).unwrap();
        let auto_yes = opts.auto_yes;
        format!(
            r#"{{"ts":"{ts}","tool":"{tool}","phase":"done","ok":{ok},"duration_ms":{dur_ms},"error":{error_json},"policy":"n/a","auto_yes":{auto_yes}}}"#,
        )
    } else {
        let auto_yes = opts.auto_yes;
        format!(
            r#"{{"ts":"{ts}","tool":"{tool}","phase":"done","ok":{ok},"duration_ms":{dur_ms},"policy":"n/a","auto_yes":{auto_yes}}}"#,
        )
    };
    append_signed(
        &opts.audit_path.as_path(),
        &opts.hmac_key_path.as_path(),
        &base,
    );
}

// --- helper de dump de politique (JSON) ---
pub fn policy_dump_json(config_path: Option<&std::path::Path>) -> serde_json::Value {
    use std::collections::BTreeMap;

    // on réutilise la logique existante
    let cfg = match config_path {
        Some(p) => load_policies(Some(&p.to_path_buf())).unwrap_or_else(|_| default_policies()),
        None => default_policies(),
    };

    // defaults visibles par le superviseur
    let mut tools: BTreeMap<String, String> = BTreeMap::from([
        ("devit.tool_list".to_string(), "never".to_string()),
        ("devit.tool_call".to_string(), "on_request".to_string()),
        ("plugin.invoke".to_string(), "on_request".to_string()),
        ("echo".to_string(), "never".to_string()),
    ]);

    // merge avec la map interne
    for k in [
        "devit.tool_list",
        "devit.tool_call",
        "plugin.invoke",
        "echo",
    ] {
        let eff = cfg
            .0
            .get(k)
            .cloned()
            .unwrap_or_else(|| default_policy_for(k));
        tools.insert(k.to_string(), eff);
    }

    // expose aussi un “wildcard” par défaut
    serde_json::json!({
        "default": "on_request",
        "tools": tools
    })
}

// Build effective policy JSON (approvals, limits, audit)
fn policy_effective_json(
    audit: &AuditOpts,
    policies: &Policies,
    limits: &Limits,
    server_version: &str,
) -> serde_json::Value {
    use serde_json::json;
    use std::collections::BTreeMap;

    fn pol_str(s: &str) -> &str {
        s
    }

    let server = json!({
        "name": "devit-mcpd",
        "version": server_version,
    });

    let mut tools: BTreeMap<String, String> = BTreeMap::new();
    for k in [
        "devit.tool_list",
        "devit.tool_call",
        "plugin.invoke",
        "server.policy",
        "echo",
    ] {
        let eff = policies
            .0
            .get(k)
            .cloned()
            .unwrap_or_else(|| default_policy_for(k));
        tools.insert(k.to_string(), pol_str(&eff).to_string());
    }

    let approvals = json!({
        "default": policies.0.get("default").cloned().unwrap_or_else(|| "on_request".to_string()),
        "tools": tools,
    });

    let limits = json!({
        "max_calls_per_min": limits.max_calls_per_min,
        "max_json_kb": limits.max_json_kb,
        "cooldown_ms": limits.cooldown.as_millis(),
    });

    let audit = json!({
        "enabled": audit.audit_enabled,
        "path": audit.audit_path.display().to_string(),
    });

    json!({
        "server": server,
        "approvals": approvals,
        "limits": limits,
        "audit": audit,
    })
}

// -------- Server State / Health / Stats --------
struct ServerState {
    start: Instant,
    per_key_calls: HashMap<String, u64>,
    per_key_ok: HashMap<String, u64>,
    per_key_err: HashMap<String, u64>,
    total_calls: u64,
    total_ok: u64,
    total_err: u64,
}

impl ServerState {
    fn new() -> Self {
        Self {
            start: Instant::now(),
            per_key_calls: HashMap::new(),
            per_key_ok: HashMap::new(),
            per_key_err: HashMap::new(),
            total_calls: 0,
            total_ok: 0,
            total_err: 0,
        }
    }
    fn bump_call(&mut self, key: &str) {
        self.total_calls += 1;
        *self.per_key_calls.entry(key.to_string()).or_insert(0) += 1;
    }
    fn bump_ok(&mut self, key: &str) {
        self.total_ok += 1;
        *self.per_key_ok.entry(key.to_string()).or_insert(0) += 1;
    }
    fn bump_err(&mut self, key: &str) {
        self.total_err += 1;
        *self.per_key_err.entry(key.to_string()).or_insert(0) += 1;
    }
}

fn which(bin: &str) -> Option<String> {
    let probe = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };
    let out = std::process::Command::new(probe).arg(bin).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()?
        .trim()
        .to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn health_json(
    audit: &AuditOpts,
    _policies: &Policies,
    limits: &Limits,
    state: &ServerState,
    server_version: &str,
    devit_bin: Option<&Path>,
) -> serde_json::Value {
    let uptime_ms = state.start.elapsed().as_millis() as u64;
    let devit = if let Some(p) = devit_bin {
        Some(p.display().to_string())
    } else {
        which("devit")
    };
    let devit = devit
        .map(|p| json!({"found": true, "path": p}))
        .unwrap_or(json!({"found": false}));
    let devit_plugin = which("devit-plugin")
        .map(|p| json!({"found": true, "path": p}))
        .unwrap_or(json!({"found": false}));
    let wasmtime = which("wasmtime")
        .map(|p| json!({"found": true, "path": p}))
        .unwrap_or(json!({"found": false}));
    json!({
        "ok": true,
        "server": { "name": "devit-mcpd", "version": server_version },
        "uptime_ms": uptime_ms,
        "bins": { "devit": devit, "devit_plugin": devit_plugin, "wasmtime": wasmtime },
        "limits": {
            "max_calls_per_min": limits.max_calls_per_min,
            "max_json_kb": limits.max_json_kb,
            "cooldown_ms": limits.cooldown.as_millis()
        },
        "audit": { "enabled": audit.audit_enabled, "path": audit.audit_path.display().to_string() }
    })
}

fn stats_json(state: &ServerState) -> serde_json::Value {
    use std::collections::{BTreeMap, HashSet};
    let mut per_tool: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    let keys: HashSet<String> = state
        .per_key_calls
        .keys()
        .chain(state.per_key_ok.keys())
        .chain(state.per_key_err.keys())
        .cloned()
        .collect();
    for key in keys {
        let calls = *state.per_key_calls.get(&key).unwrap_or(&0);
        let ok = *state.per_key_ok.get(&key).unwrap_or(&0);
        let err = *state.per_key_err.get(&key).unwrap_or(&0);
        per_tool.insert(key, json!({"calls":calls,"ok":ok,"errors":err}));
    }
    json!({
        "ok": true,
        "totals": { "calls": state.total_calls, "ok": state.total_ok, "errors": state.total_err },
        "per_tool": per_tool
    })
}

fn context_head_json(
    index_path_opt: Option<&std::path::Path>,
    limit: usize,
    ext_allow: Option<&[String]>,
) -> serde_json::Value {
    use serde_json::json;
    let path = index_path_opt
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".devit/index.json"));
    let data = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => {
            return json!({
                "ok": false,
                "not_indexed": true,
                "path": path.display().to_string(),
                "hint": "run: devit context map .",
            })
        }
    };
    let v: serde_json::Value = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(e) => {
            return json!({
                "ok": false,
                "parse_error": e.to_string(),
                "path": path.display().to_string()
            })
        }
    };
    let files = match v.get("files").and_then(|x| x.as_array()) {
        Some(a) => a,
        None => {
            return json!({
                "ok": false,
                "invalid_index": true,
                "path": path.display().to_string()
            })
        }
    };
    let mut rows: Vec<(i64, serde_json::Value)> = Vec::with_capacity(files.len());
    'outer: for f in files {
        let p = f.get("path").and_then(|x| x.as_str()).unwrap_or("");
        let score = f.get("score").and_then(|x| x.as_i64()).unwrap_or(0);
        if let Some(exts) = ext_allow {
            let allowed = exts.iter().any(|e| p.ends_with(&format!(".{}", e)));
            if !allowed {
                continue 'outer;
            }
        }
        rows.push((score, f.clone()));
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    let take = rows
        .into_iter()
        .take(limit)
        .map(|(_s, f)| {
            let path = f.get("path").cloned().unwrap_or(json!(""));
            let score = f.get("score").cloned().unwrap_or(json!(0));
            let lang = f.get("lang").cloned().unwrap_or(json!(null));
            let size = f.get("size").cloned().unwrap_or(json!(null));
            let symbols_count = f.get("symbols_count").cloned();
            let mut m = serde_json::Map::new();
            m.insert("path".to_string(), path);
            m.insert("score".to_string(), score);
            m.insert("lang".to_string(), lang);
            m.insert("size".to_string(), size);
            if let Some(sc) = symbols_count {
                m.insert("symbols_count".to_string(), sc);
            }
            serde_json::Value::Object(m)
        })
        .collect::<Vec<_>>();
    json!({
        "ok": true,
        "source": {
            "path": path.display().to_string(),
            "generated_at": v.get("generated_at").cloned().unwrap_or(json!(null)),
            "root": v.get("root").cloned().unwrap_or(json!(null))
        },
        "total": files.len(),
        "limit": limit,
        "items": take
    })
}

#[cfg(test)]
mod ctx_tests {
    use super::*;
    use std::io::Write as _;
    #[test]
    fn context_head_reads_index() {
        let dir = tempfile::tempdir().unwrap();
        let devit_dir = dir.path().join(".devit");
        fs::create_dir_all(&devit_dir).unwrap();
        let idx = devit_dir.join("index.json");
        let mut f = fs::File::create(&idx).unwrap();
        write!(
            f,
            "{}",
            r#"{"root": ".", "generated_at":"2025-09-14T00:00:00Z","files":[
            {"path":"src/lib.rs","size":100,"lang":"rust","score":90,"symbols_count":5},
            {"path":"README.md","size":200,"lang":"text","score":10}
        ]}"#
        )
        .unwrap();
        let v = context_head_json(Some(&idx), 1, None);
        assert!(v["ok"].as_bool().unwrap_or(false));
        assert_eq!(v["items"].as_array().unwrap().len(), 1);
        assert_eq!(v["items"][0]["path"].as_str().unwrap(), "src/lib.rs");
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
