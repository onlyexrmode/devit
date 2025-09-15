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
#[cfg(unix)]
use std::os::unix::process::CommandExt;
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

    /// Watchdog: stop server after N seconds (exit 2)
    #[arg(long, value_name = "SECS")]
    max_runtime_secs: Option<u64>,

    /// Limite: appels par minute
    #[arg(long = "max-calls-per-min", default_value_t = 60)]
    max_calls_per_min: u32,
    /// Limite: taille JSON max (kB)
    #[arg(long = "max-json-kb", default_value_t = 256)]
    max_json_kb: usize,
    /// Limite: cooldown entre appels (ms)
    #[arg(long = "cooldown-ms", default_value_t = 250)]
    cooldown_ms: u64,

    /// Sandbox kind: bwrap|none (default: none)
    #[arg(long = "sandbox", default_value = "none")]
    sandbox: String,
    /// Network policy: off|full (default: off)
    #[arg(long = "net", default_value = "off")]
    net: String,
    /// CPU seconds limit for child processes
    #[arg(long = "cpu-secs", default_value_t = 30)]
    cpu_secs: u64,
    /// Memory limit (MB) for child processes
    #[arg(long = "mem-mb", default_value_t = 512)]
    mem_mb: u64,
}

fn main() {
    if let Err(e) = real_main() {
        eprintln!("error: {e}");
        std::process::exit(2);
    }
}

fn real_main() -> Result<()> {
    let cli = Cli::parse();
    let max_runtime = cli.max_runtime_secs.map(std::time::Duration::from_secs);
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
    if cli.sandbox.to_ascii_lowercase() == "bwrap" && which("bwrap").is_none() {
        // Do not exit; mark unavailable (will return structured error later)
        state.sandbox_unavailable = true;
    }
    let secrets = load_secrets_allow(cli.config_path.as_ref());

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
    let started = Instant::now();
    loop {
        if let Some(deadline) = max_runtime {
            if started.elapsed() > deadline {
                eprintln!("error: max runtime exceeded ({}s)", deadline.as_secs());
                return Err(anyhow::anyhow!("max runtime exceeded"));
            }
        }
        let line = match lines.next() {
            Some(x) => x?,
            None => break,
        };
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
                        "plugin.invoke",
                        "server.context_head",
                        "server.health",
                        "server.stats",
                        "server.stats.reset",
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
                    || name == "server.context_head"
                    || name == "server.stats.reset";
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
                    "plugin.invoke" => {
                        let tool_key = "plugin.invoke";
                        state.bump_call(tool_key);
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
                        // Schema check: id:string
                        let id = match args_json.get("id") {
                            Some(v) if v.is_string() => v.as_str().unwrap(),
                            Some(_) => {
                                writeln!(
                                    stdout,
                                    "{}",
                                    json!({"type":"tool.error","payload":{ "schema_error": true, "path": "payload.id", "reason": "type_mismatch" }})
                                )?;
                                continue;
                            }
                            None => {
                                writeln!(
                                    stdout,
                                    "{}",
                                    json!({"type":"tool.error","payload":{ "schema_error": true, "path": "payload.id", "reason": "missing" }})
                                )?;
                                continue;
                            }
                        };
                        // Schema check: payload:object
                        match args_json.get("payload") {
                            Some(v) if v.is_object() => {}
                            Some(_) => {
                                writeln!(
                                    stdout,
                                    "{}",
                                    json!({"type":"tool.error","payload":{ "schema_error": true, "path": "payload.payload", "reason": "type_mismatch" }})
                                )?;
                                continue;
                            }
                            None => {
                                writeln!(
                                    stdout,
                                    "{}",
                                    json!({"type":"tool.error","payload":{ "schema_error": true, "path": "payload.payload", "reason": "missing" }})
                                )?;
                                continue;
                            }
                        }
                        let plugin_root = std::env::var("DEVIT_PLUGINS_DIR")
                            .map(PathBuf::from)
                            .unwrap_or_else(|_| PathBuf::from(".devit/plugins"));
                        let manifest_path = plugin_root.join(id).join("devit-plugin.toml");
                        if !manifest_path.exists() {
                            writeln!(
                                stdout,
                                "{}",
                                json!({"type":"tool.error","payload":{ "plugin_error": true, "reason": "manifest_missing", "id": id }})
                            )?;
                            continue;
                        }
                        match validate_manifest_for(&manifest_path, id) {
                            Ok(info) => {
                                if let Some(schema) = info.args_schema.as_ref() {
                                    if let Some(req) = args_json.get("payload") {
                                        if let Err((path, why)) =
                                            validate_payload_types(req, schema)
                                        {
                                            writeln!(
                                                stdout,
                                                "{}",
                                                json!({"type":"tool.error","payload":{ "plugin_error": true, "reason": "invalid", "schema_error": true, "path": path, "why": why }})
                                            )?;
                                            continue;
                                        }
                                    }
                                }
                                let bin = which("devit-plugin")
                                    .map(PathBuf::from)
                                    .or_else(|| {
                                        let p = PathBuf::from("target/debug/devit-plugin");
                                        if p.exists() {
                                            Some(p)
                                        } else {
                                            None
                                        }
                                    })
                                    .unwrap_or_else(|| PathBuf::from("devit-plugin"));
                                let start = Instant::now();
                                match run_devit_plugin_manifest(
                                    &bin,
                                    &manifest_path,
                                    args_json.get("payload").cloned().unwrap_or(json!({})),
                                    timeout,
                                ) {
                                    Ok(out) => {
                                        let dur = start.elapsed().as_millis();
                                        audit_done(&audit, tool_key, true, dur, None);
                                        // on_failure handling for plugin.invoke
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
                                            state.bump_ok(tool_key);
                                            writeln!(
                                                stdout,
                                                "{}",
                                                json!({"type":"tool.result","payload": {"name": tool_key, "result": out}})
                                            )?;
                                        }
                                    }
                                    Err(e) => {
                                        let dur = start.elapsed().as_millis();
                                        audit_done(
                                            &audit,
                                            tool_key,
                                            false,
                                            dur,
                                            Some(&e.to_string()),
                                        );
                                        writeln!(
                                            stdout,
                                            "{}",
                                            json!({"type":"tool.error","payload":{ "plugin_error": true, "reason": "exec_failed", "message": e.to_string() }})
                                        )?;
                                    }
                                }
                            }
                            Err((reason, msg)) => {
                                let mut m = serde_json::Map::new();
                                m.insert("plugin_error".into(), json!(true));
                                m.insert("reason".into(), json!(reason));
                                if let Some(s) = msg {
                                    m.insert("message".into(), json!(s));
                                }
                                writeln!(
                                    stdout,
                                    "{}",
                                    json!({"type":"tool.error","payload": serde_json::Value::Object(m)})
                                )?;
                            }
                        }
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
                    "server.stats.reset" => {
                        let tool_key = "server.stats.reset";
                        state.bump_call(tool_key);
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
                        let start = Instant::now();
                        state.reset();
                        let dur = start.elapsed().as_millis();
                        audit_done(&audit, tool_key, true, dur, None);
                        state.bump_ok(tool_key);
                        writeln!(
                            stdout,
                            "{}",
                            json!({"type":"tool.result","payload":{"ok":true,"name": tool_key}})
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
                            .and_then(|a| a.get("text").or_else(|| a.get("msg")))
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
                        if state.sandbox_unavailable && cli.sandbox.to_ascii_lowercase() == "bwrap" {
                            writeln!(stdout, "{}", json!({"type":"tool.error","payload":{"sandbox_unavailable": true, "reason":"bwrap_not_found"}}))?;
                            continue;
                        }
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
                        match run_devit_list_sandboxed(&bin, timeout, &cli) {
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
                        if state.sandbox_unavailable && cli.sandbox.to_ascii_lowercase() == "bwrap" {
                            writeln!(stdout, "{}", json!({"type":"tool.error","payload":{"sandbox_unavailable": true, "reason":"bwrap_not_found"}}))?;
                            continue;
                        }
                        let bin = cli
                            .devit_bin
                            .clone()
                            .unwrap_or_else(|| PathBuf::from("devit"));
                        let args_json = payload.get("args").cloned().unwrap_or(json!({}));
                        // PR1: explicit env request denial
                        if let Some(args_obj) = args_json.get("args").and_then(|v| v.as_object()) {
                            if let Some(env_obj) = args_obj.get("env").and_then(|v| v.as_object()) {
                                if let Some(denied) = first_env_denied(env_obj, &secrets) {
                                    writeln!(
                                        stdout,
                                        "{}",
                                        json!({"type":"tool.error","payload":{ "secrets_env_denied": true, "var": denied }})
                                    )?;
                                    stdout.flush()?;
                                    continue;
                                }
                            }
                        }
                        // Schema check: tool:string and args:object
                        match args_json.get("tool") {
                            Some(v) if v.is_string() => {}
                            Some(_) => {
                                writeln!(
                                    stdout,
                                    "{}",
                                    json!({"type":"tool.error","payload":{ "schema_error": true, "path": "payload.tool", "reason": "type_mismatch" }})
                                )?;
                                continue;
                            }
                            None => {
                                writeln!(
                                    stdout,
                                    "{}",
                                    json!({"type":"tool.error","payload":{ "schema_error": true, "path": "payload.tool", "reason": "missing" }})
                                )?;
                                continue;
                            }
                        }
                        match args_json.get("args") {
                            Some(v) if v.is_object() => {}
                            Some(_) => {
                                writeln!(
                                    stdout,
                                    "{}",
                                    json!({"type":"tool.error","payload":{ "schema_error": true, "path": "payload.args", "reason": "type_mismatch" }})
                                )?;
                                continue;
                            }
                            None => {
                                writeln!(
                                    stdout,
                                    "{}",
                                    json!({"type":"tool.error","payload":{ "schema_error": true, "path": "payload.args", "reason": "missing" }})
                                )?;
                                continue;
                            }
                        }
                        let start = Instant::now();
                        match run_devit_call_sandboxed(&bin, &args_json, timeout, &cli) {
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

// ---- PR1: simple secrets env allowlist loader ----
fn load_secrets_allow(path: Option<&PathBuf>) -> Vec<String> {
    let mut allow = vec!["PATH".to_string(), "HOME".to_string(), "RUST_BACKTRACE".to_string()];
    let path = path.cloned().unwrap_or_else(|| PathBuf::from(".devit/devit.toml"));
    if let Ok(s) = fs::read_to_string(&path) {
        #[derive(serde::Deserialize, Default)]
        struct Root { secrets: Option<SecretsSect> }
        #[derive(serde::Deserialize, Default)]
        struct SecretsSect { env_allow: Option<Vec<String>> }
        if let Ok(r) = toml::from_str::<Root>(&s) {
            if let Some(sec) = r.secrets { if let Some(v) = sec.env_allow { allow = v; } }
        }
    }
    allow
}

fn first_env_denied(env_map: &serde_json::Map<String, Value>, allow: &[String]) -> Option<String> {
    let set: std::collections::HashSet<String> = allow.iter().map(|s| s.to_ascii_uppercase()).collect();
    for (k, _v) in env_map.iter() {
        if !set.contains(&k.to_ascii_uppercase()) {
            return Some(k.clone());
        }
    }
    None
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
        profile: Option<String>,
        approvals: Option<HashMap<String, String>>,
    }
    let r: Root = toml::from_str(&s).context("parse TOML")?;
    let mut out = default_policies();
    if let Some(mcp) = r.mcp {
        // Apply profile presets first
        if let Some(p) = mcp.profile.as_deref() {
            match p {
                "safe" => {
                    out.0.insert("devit.tool_call".into(), "on_request".into());
                    out.0.insert("plugin.invoke".into(), "on_request".into());
                    // server.* already "never" by defaults
                }
                "std" => {
                    out.0.insert("devit.tool_call".into(), "on_failure".into());
                    out.0.insert("plugin.invoke".into(), "on_request".into());
                }
                "danger" => {
                    out.0.insert("devit.tool_call".into(), "never".into());
                    out.0.insert("plugin.invoke".into(), "on_failure".into());
                }
                _ => {}
            }
        }
        // Then explicit overrides
        if let Some(map) = mcp.approvals {
            for (k, v) in map.into_iter() {
                out.0.insert(k, v.to_ascii_lowercase());
            }
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
    m.insert("server.stats.reset".to_string(), "on_request".to_string());
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

    // parse raw config to extract profile + approvals
    #[derive(serde::Deserialize, Default)]
    struct Root {
        mcp: Option<Mcp>,
    }
    #[derive(serde::Deserialize, Default)]
    struct Mcp {
        profile: Option<String>,
        approvals: Option<HashMap<String, String>>,
    }

    let mut eff = default_policies();
    let mut profile: Option<String> = None;
    if let Some(p) = config_path {
        if let Ok(s) = fs::read_to_string(p) {
            if let Ok(root) = toml::from_str::<Root>(&s) {
                if let Some(m) = root.mcp {
                    if let Some(pr) = m.profile {
                        profile = Some(pr.clone());
                        match pr.as_str() {
                            "safe" => {
                                eff.0.insert("devit.tool_call".into(), "on_request".into());
                                eff.0.insert("plugin.invoke".into(), "on_request".into());
                            }
                            "std" => {
                                eff.0.insert("devit.tool_call".into(), "on_failure".into());
                                eff.0.insert("plugin.invoke".into(), "on_request".into());
                            }
                            "danger" => {
                                eff.0.insert("devit.tool_call".into(), "never".into());
                                eff.0.insert("plugin.invoke".into(), "on_failure".into());
                            }
                            _ => {}
                        }
                    }
                    if let Some(map) = m.approvals {
                        for (k, v) in map.into_iter() {
                            eff.0.insert(k, v.to_ascii_lowercase());
                        }
                    }
                }
            }
        }
    }

    let mut tools: BTreeMap<String, String> = BTreeMap::new();
    for k in [
        "devit.tool_list",
        "devit.tool_call",
        "plugin.invoke",
        "server.policy",
        "server.context_head",
        "server.health",
        "server.stats",
        "server.stats.reset",
        "echo",
    ] {
        let v = eff
            .0
            .get(k)
            .cloned()
            .unwrap_or_else(|| default_policy_for(k));
        tools.insert(k.to_string(), v);
    }

    serde_json::json!({
        "profile": profile.unwrap_or_else(|| "none".to_string()),
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
        "server.stats.reset",
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
    sandbox_unavailable: bool,
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
            sandbox_unavailable: false,
        }
    }
    fn reset(&mut self) {
        self.per_key_calls.clear();
        self.per_key_ok.clear();
        self.per_key_err.clear();
        self.total_calls = 0;
        self.total_ok = 0;
        self.total_err = 0;
        self.start = Instant::now();
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

// ----- Plugin manifest validation and invocation helpers -----
#[derive(serde::Deserialize)]
struct ManifestCheck {
    id: String,
    #[serde(default)]
    version: Option<String>,
    wasm: String,
    #[serde(default)]
    allowed_dirs: Vec<String>,
    #[serde(default)]
    env: Vec<String>,
    #[serde(default)]
    args_schema: Option<HashMap<String, String>>,
}

struct ValidatedManifest {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    wasm_abs: PathBuf,
    args_schema: Option<HashMap<String, String>>,
}

fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.chars().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_' || c == '-'
        })
}

fn is_rel_safe(p: &str) -> bool {
    let path = Path::new(p);
    if path.is_absolute() {
        return false;
    }
    for comp in path.components() {
        if matches!(comp, std::path::Component::ParentDir) {
            return false;
        }
    }
    true
}

fn validate_manifest_for(
    path: &Path,
    expected_id: &str,
) -> Result<ValidatedManifest, (&'static str, Option<String>)> {
    if !is_valid_id(expected_id) {
        return Err(("invalid", Some("invalid id".to_string())));
    }
    let s = match fs::read_to_string(path) {
        Ok(x) => x,
        Err(_) => return Err(("manifest_missing", None)),
    };
    let m: ManifestCheck = match toml::from_str(&s) {
        Ok(v) => v,
        Err(e) => return Err(("invalid", Some(e.to_string()))),
    };
    if m.id != expected_id {
        return Err(("invalid", Some("id mismatch".to_string())));
    }
    if let Some(ver) = &m.version {
        // minimal semver check: a.b.c prefix numeric
        let parts: Vec<&str> = ver.split('.').collect();
        if parts.len() < 3
            || parts[0].parse::<u64>().is_err()
            || parts[1].parse::<u64>().is_err()
            || parts[2]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .is_empty()
        {
            return Err(("invalid", Some("version not semver-like".to_string())));
        }
    }
    if !is_rel_safe(&m.wasm) {
        return Err((
            "path_outside_root",
            Some("wasm path escapes root".to_string()),
        ));
    }
    let root = path.parent().unwrap_or_else(|| Path::new("."));
    let wasm_abs = root.join(&m.wasm);
    if !wasm_abs.exists() {
        return Err(("wasm_missing", None));
    }
    for d in &m.allowed_dirs {
        if !is_rel_safe(d) {
            return Err(("path_outside_root", Some(format!("bad allowed_dir: {d}"))));
        }
    }
    Ok(ValidatedManifest {
        id: m.id,
        wasm_abs,
        args_schema: m.args_schema,
    })
}

fn validate_payload_types(
    req: &serde_json::Value,
    schema: &HashMap<String, String>,
) -> Result<(), (String, &'static str)> {
    let obj = match req.as_object() {
        Some(m) => m,
        None => return Err(("payload".to_string(), "type_mismatch")),
    };
    for (k, t) in schema.iter() {
        if let Some(v) = obj.get(k) {
            let ok = match t.as_str() {
                "string" => v.is_string(),
                "number" => v.is_number(),
                "boolean" => v.is_boolean(),
                "object" => v.is_object(),
                _ => true,
            };
            if !ok {
                return Err((format!("payload.{k}"), "type_mismatch"));
            }
        }
    }
    Ok(())
}

fn run_devit_plugin_manifest(
    bin: &PathBuf,
    manifest: &Path,
    payload: serde_json::Value,
    timeout: Duration,
) -> Result<Value> {
    let mut child = Command::new(bin)
        .arg("invoke")
        .arg("--manifest")
        .arg(manifest)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("spawn {:?} devit-plugin invoke", bin))?;

    // write JSON to stdin
    {
        let mut sin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("child stdin missing"))?;
        let s = serde_json::to_string(&payload)?;
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
                serde_json::from_str(s.trim()).context("devit-plugin invoke: invalid JSON")?;
            Ok(v)
        }
        Err(_) => {
            let _ = child.kill();
            eprintln!("error: devit-plugin invoke timeout");
            std::process::exit(124);
        }
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

#[cfg(test)]
mod profile_tests {
    use super::*;
    #[test]
    fn policy_dump_includes_profile_and_merge() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("devit.toml");
        std::fs::write(
            &cfg,
            r#"
[mcp]
profile = "std"
[mcp.approvals]
"server.stats.reset" = "never"
"#,
        )
        .unwrap();
        let v = policy_dump_json(Some(&cfg));
        assert_eq!(v["profile"].as_str().unwrap(), "std");
        // std preset => devit.tool_call on_failure
        assert_eq!(
            v["tools"]["devit.tool_call"].as_str().unwrap(),
            "on_failure"
        );
        // explicit override applied
        assert_eq!(v["tools"]["server.stats.reset"].as_str().unwrap(), "never");
    }
}
fn run_devit_list_sandboxed(bin: &PathBuf, timeout: Duration, cli: &Cli) -> Result<Value> {
    // Build command possibly under bwrap; apply rlimits
    let mut cmd = if cli.sandbox.to_ascii_lowercase() == "bwrap" {
        let mut c = Command::new("bwrap");
        c.arg("--unshare-user");
        if cli.net.to_ascii_lowercase() == "off" { c.arg("--unshare-net"); }
        c.args(["--dev","/dev"]).args(["--proc","/proc"]).arg("--die-with-parent");
        for p in ["/usr","/bin","/sbin","/lib","/lib64","/etc"].iter() {
            if std::path::Path::new(p).exists() { c.args(["--ro-bind", p, p]); }
        }
        if let Ok(cwd) = std::env::current_dir() {
            let p = cwd.to_string_lossy().to_string();
            c.args(["--bind", &p, &p]).args(["--chdir", &p]);
        }
        // Run devit directly inside bwrap
        c.arg("--").arg(bin.as_os_str()).arg("tool").arg("list");
        c
    } else {
        let mut c = Command::new(bin);
        c.arg("tool").arg("list");
        c
    };
    // stdout/stderr
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::inherit());
    // Apply rlimits for sandbox=none using pre_exec
    #[cfg(unix)]
    if cli.sandbox.to_ascii_lowercase() == "none" {
        use libc::{rlimit, RLIMIT_AS, RLIMIT_CPU};
        let cpu = cli.cpu_secs as u64;
        let mem = (cli.mem_mb as u64) * 1024 * 1024;
        unsafe {
            cmd.pre_exec(move || {
                let r_cpu = rlimit { rlim_cur: cpu, rlim_max: cpu };
                let r_mem = rlimit { rlim_cur: mem, rlim_max: mem };
                if libc::setrlimit(RLIMIT_CPU, &r_cpu as *const _) != 0 { return Err(std::io::Error::new(std::io::ErrorKind::Other, "sandbox_error:rlimit_set_failed")); }
                if libc::setrlimit(RLIMIT_AS, &r_mem as *const _) != 0 { return Err(std::io::Error::new(std::io::ErrorKind::Other, "sandbox_error:rlimit_set_failed")); }
                Ok(())
            });
        }
    }
    let mut child = cmd.spawn().map_err(|_e| anyhow!("sandbox_error:bwrap_exec_failed")).with_context(|| format!("spawn {:?} tool list", bin))?;

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

fn run_devit_call_sandboxed(bin: &PathBuf, args_json: &Value, timeout: Duration, cli: &Cli) -> Result<Value> {
    let mut cmd = if cli.sandbox.to_ascii_lowercase() == "bwrap" {
        let mut c = Command::new("bwrap");
        c.arg("--unshare-user");
        if cli.net.to_ascii_lowercase() == "off" { c.arg("--unshare-net"); }
        c.args(["--dev","/dev"]).args(["--proc","/proc"]).arg("--die-with-parent");
        for p in ["/usr","/bin","/sbin","/lib","/lib64","/etc"].iter() {
            if std::path::Path::new(p).exists() { c.args(["--ro-bind", p, p]); }
        }
        if let Ok(cwd) = std::env::current_dir() {
            let p = cwd.to_string_lossy().to_string();
            c.args(["--bind", &p, &p]).args(["--chdir", &p]);
        }
        c.arg("--").arg(bin.as_os_str()).arg("tool").arg("call").arg("-");
        c
    } else {
        let mut c = Command::new(bin);
        c.arg("tool").arg("call").arg("-");
        c
    };
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit());
    #[cfg(unix)]
    if cli.sandbox.to_ascii_lowercase() == "none" {
        use libc::{rlimit, RLIMIT_AS, RLIMIT_CPU};
        let cpu = cli.cpu_secs as u64;
        let mem = (cli.mem_mb as u64) * 1024 * 1024;
        unsafe {
            cmd.pre_exec(move || {
                let r_cpu = rlimit { rlim_cur: cpu, rlim_max: cpu };
                let r_mem = rlimit { rlim_cur: mem, rlim_max: mem };
                if libc::setrlimit(RLIMIT_CPU, &r_cpu as *const _) != 0 { return Err(std::io::Error::new(std::io::ErrorKind::Other, "sandbox_error:rlimit_set_failed")); }
                if libc::setrlimit(RLIMIT_AS, &r_mem as *const _) != 0 { return Err(std::io::Error::new(std::io::ErrorKind::Other, "sandbox_error:rlimit_set_failed")); }
                Ok(())
            });
        }
    }
    let mut child = cmd.spawn().map_err(|_e| anyhow!("sandbox_error:bwrap_exec_failed")).with_context(|| format!("spawn {:?} tool call -", bin))?;

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
