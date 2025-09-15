use anyhow::Result;
use chrono::Utc;
use hmac::{Hmac, Mac};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use rand::RngCore;
use base64::Engine;

type HmacSha256 = Hmac<Sha256>;

fn ensure_dir(p: &Path) { let _ = fs::create_dir_all(p); }

fn load_or_create_hmac(path: &Path) -> Vec<u8> {
    if let Ok(k) = fs::read(path) { if k.len() >= 32 { return k; } }
    let mut key = vec![0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    let _ = fs::write(path, &key);
    key
}

fn sha256_hex(data: &[u8]) -> String { let mut h = Sha256::new(); h.update(data); hex::encode(h.finalize()) }

pub fn attest_diff(patch: &str, cfg: &devit_common::Config) -> Result<()> {
    let ts = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let head = devit_tools::git::head_short().unwrap_or_default();
    let dirty = !devit_tools::git::is_worktree_clean();
    let diff_sha256 = sha256_hex(patch.as_bytes());
    let sbom_path = Path::new(".devit/sbom.cdx.json");
    let sbom_sha256 = if sbom_path.exists() { Some(sha256_hex(&fs::read(sbom_path).unwrap_or_default())) } else { None };
    let tools = json!({
        "devit": env!("CARGO_PKG_VERSION"),
        "mcpd": serde_json::Value::Null
    });
    let sandbox = json!({
        "kind": "none",
        "net": cfg.sandbox.net,
        "cpu_secs": 0,
        "mem_mb": cfg.sandbox.mem_limit_mb,
    });
    let profile = cfg.policy.profile.clone().unwrap_or_else(|| "std".into());
    let base = json!({
        "ts": ts,
        "git": {"head": head, "dirty": dirty},
        "diff_sha256": diff_sha256,
        "sbom_sha256": sbom_sha256,
        "tools": tools,
        "sandbox": sandbox,
        "profile": profile,
        "provenance": { "key_id": "local-hmac" }
    });
    let line = serde_json::to_string(&base)?;
    let key_path = Path::new(".devit/hmac.key");
    ensure_dir(Path::new(".devit"));
    let key = load_or_create_hmac(key_path);
    let mut mac = HmacSha256::new_from_slice(&key).expect("HMAC key");
    mac.update(line.as_bytes());
    let sig = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
    let mut obj: serde_json::Value = serde_json::from_str(&line)?;
    obj["provenance"]["sig"] = json!(sig);
    let ymd = Utc::now().format("%Y%m%d").to_string();
    let dir = PathBuf::from(format!(".devit/attestations/{ymd}"));
    ensure_dir(&dir);
    let path = dir.join("attest.jsonl");
    let mut f = fs::OpenOptions::new().create(true).append(true).open(path)?;
    use std::io::Write as _;
    writeln!(f, "{}", serde_json::to_string(&obj)?)?;
    Ok(())
}
