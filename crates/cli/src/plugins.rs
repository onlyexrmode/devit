//! Loader minimal pour plugins WASI (JSON stdin → JSON stdout) via `wasmtime` binaire.
//! - Registry: .devit/plugins/<id>/devit-plugin.toml (override: DEVIT_PLUGINS_DIR)
//! - Manifest TOML: id, name, wasm, version?, allowed_dirs?[], env?[]
//! - Sandbox: pas de `--dir` par défaut (zéro accès FS). Ajouts contrôlés via allowed_dirs.
//! - Timeout: DEVIT_TIMEOUT_SECS (fallback 30s). Timeout → exit 124.
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[derive(Debug)]
pub struct TimeoutErr;
impl std::fmt::Display for TimeoutErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "timeout waiting plugin output")
    }
}
impl std::error::Error for TimeoutErr {}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginManifest {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    /// Chemin vers le .wasm (relatif au manifest).
    pub wasm: String,
    #[serde(default)]
    pub version: Option<String>,
    /// Pré-ouvertures de répertoires (`wasmtime run --dir=<path>`).
    #[serde(default)]
    pub allowed_dirs: Vec<String>,
    /// Variables d'environnement à propager (`--env key=value`).
    #[serde(default)]
    pub env: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub manifest_path: String,
}

fn timeout_from_env() -> Duration {
    let secs = env::var("DEVIT_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(30);
    Duration::from_secs(secs)
}

fn default_registry_dir() -> PathBuf {
    if let Ok(p) = env::var("DEVIT_PLUGINS_DIR") {
        return PathBuf::from(p);
    }
    PathBuf::from(".devit/plugins")
}

pub fn load_manifest(path: &Path) -> Result<PluginManifest> {
    let s = fs::read_to_string(path)
        .with_context(|| format!("read manifest {}", path.display()))?;
    let m: PluginManifest = toml::from_str(&s)
        .with_context(|| format!("parse TOML {}", path.display()))?;
    Ok(m)
}

pub fn discover_plugins(root: Option<&Path>) -> Result<Vec<PluginInfo>> {
    let root = root.map(PathBuf::from).unwrap_or_else(default_registry_dir);
    if !root.exists() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let manifest = entry.path().join("devit-plugin.toml");
        if !manifest.exists() {
            continue;
        }
        let m = match load_manifest(&manifest) {
            Ok(x) => x,
            Err(e) => {
                eprintln!("warn: invalid plugin manifest {}: {e}", manifest.display());
                continue;
            }
        };
        out.push(PluginInfo {
            id: m.id.clone(),
            name: m.name.clone().unwrap_or_else(|| m.id.clone()),
            version: m.version.clone(),
            manifest_path: manifest.display().to_string(),
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn ensure_bin_exists<S: AsRef<OsStr>>(bin: S) -> Result<()> {
    let which = if cfg!(target_os = "windows") { "where" } else { "which" };
    let status = Command::new(which).arg(&bin).stdout(Stdio::null()).stderr(Stdio::null()).status();
    match status {
        Ok(s) if s.success() => Ok(()),
        _ => Err(anyhow!("required binary {:?} not found in PATH", bin.as_ref())),
    }
}

/// Invoque un plugin par manifeste, lit JSON depuis `stdin_json` et renvoie JSON stdout.
pub fn invoke_manifest(manifest_path: &Path, stdin_json: &str, per_msg_timeout: Option<Duration>) -> Result<Value> {
    ensure_bin_exists("wasmtime")?;

    let manifest = load_manifest(manifest_path)?;
    // Résoudre le chemin du .wasm relativement au manifest
    let wasm_path = manifest_path.parent().unwrap_or_else(|| Path::new(".")).join(&manifest.wasm);
    if !wasm_path.exists() {
        return Err(anyhow!("wasm file not found: {}", wasm_path.display()));
    }
    let timeout = per_msg_timeout.unwrap_or_else(timeout_from_env);

    // Préparer la commande wasmtime
    let mut cmd = Command::new("wasmtime");
    cmd.arg("run");
    // Ajout de pré-ouvertures explicites (sandbox FS fermée sinon).
    for d in &manifest.allowed_dirs {
        cmd.arg(format!("--dir={d}"));
    }
    // Variables d'env limitées
    for kv in &manifest.env {
        cmd.arg(format!("--env={kv}"));
    }
    cmd.arg(wasm_path.as_os_str());
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit());

    let mut child = cmd.spawn().with_context(|| "spawn wasmtime failed")?;
    // Feed JSON to stdin
    {
        let mut sin = child.stdin.take().ok_or_else(|| anyhow!("child stdin missing"))?;
        sin.write_all(stdin_json.as_bytes())?;
        sin.flush()?;
        drop(sin);
    }
    // Read stdout with timeout
    let mut sout = child.stdout.take().ok_or_else(|| anyhow!("child stdout missing"))?;
    let (tx, rx) = mpsc::sync_channel::<Result<String>>(1);
    thread::spawn(move || {
        let mut buf = String::new();
        let res = sout.read_to_string(&mut buf).map_err(|e| anyhow!(e)).map(|_| buf);
        let _ = tx.send(res);
    });
    match rx.recv_timeout(timeout) {
        Ok(res) => {
            let out = res?;
            // Option: garder première ligne JSON si plugin log avant.
            let first_json = out
                .lines()
                .find(|l| l.trim_start().starts_with('{') || l.trim_start().starts_with('['))
                .ok_or_else(|| anyhow!("no JSON found on plugin stdout"))?;
            let v: Value = serde_json::from_str(first_json).with_context(|| format!("invalid JSON: {first_json}"))?;
            Ok(v)
        }
        Err(_timeout) => {
            // Tue le processus plugin si toujours en cours
            let _ = child.kill();
            Err(TimeoutErr.into())
        }
    }
}

/// Résout un plugin par ID dans le registry (DEVIT_PLUGINS_DIR) et l'invoque.
pub fn invoke_by_id(id: &str, stdin_json: &str, per_msg_timeout: Option<Duration>, root: Option<&Path>) -> Result<Value> {
    let root = root.map(PathBuf::from).unwrap_or_else(default_registry_dir);
    let manifest = root.join(id).join("devit-plugin.toml");
    if !manifest.exists() {
        return Err(anyhow!("plugin id {:?} not found at {}", id, manifest.display()));
    }
    invoke_manifest(&manifest, stdin_json, per_msg_timeout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use tempfile::tempdir;

    #[test]
    fn parse_manifest_ok() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("devit-plugin.toml");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"
id = "echo_sum"
name = "Echo Sum"
wasm = "echo_sum.wasm"
version = "0.1.0"
allowed_dirs = []
env = []
"#
        )
        .unwrap();
        let m = load_manifest(&path).unwrap();
        assert_eq!(m.id, "echo_sum");
        assert_eq!(m.wasm, "echo_sum.wasm");
    }

    #[test]
    fn discover_empty_ok() {
        let dir = tempdir().unwrap();
        let res = discover_plugins(Some(dir.path())).unwrap();
        assert!(res.is_empty());
    }

    #[test]
    fn discover_lists_manifest_dirs() {
        let dir = tempdir().unwrap();
        let one = dir.path().join("one");
        let two = dir.path().join("two");
        fs::create_dir_all(&one).unwrap();
        fs::create_dir_all(&two).unwrap();
        fs::write(
            one.join("devit-plugin.toml"),
            "id = \"a\"\nwasm = \"a.wasm\"\n",
        )
        .unwrap();
        fs::write(
            two.join("devit-plugin.toml"),
            "id = \"b\"\nwasm = \"b.wasm\"\n",
        )
        .unwrap();
        let mut list = discover_plugins(Some(dir.path())).unwrap();
        list.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "a");
        assert_eq!(list[1].id, "b");
    }
}
