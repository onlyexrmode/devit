use anyhow::{Context, Result};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

pub fn generate(out: &Path) -> Result<()> {
    let mut components = Vec::new();

    // Rust via Cargo.lock (fallback minimal)
    if Path::new("Cargo.lock").exists() {
        if let Ok(s) = fs::read_to_string("Cargo.lock") {
            let v: toml::Value =
                toml::from_str(&s).unwrap_or(toml::Value::Table(Default::default()));
            if let Some(pkgs) = v.get("package").and_then(|x| x.as_array()) {
                let mut seen = BTreeSet::new();
                for p in pkgs {
                    let name = p.get("name").and_then(|x| x.as_str()).unwrap_or("");
                    let ver = p.get("version").and_then(|x| x.as_str()).unwrap_or("");
                    if !name.is_empty() && !ver.is_empty() {
                        let key = format!("rust:{}@{}", name, ver);
                        if seen.insert(key) {
                            components.push(json!({
                                "type":"library",
                                "name": name,
                                "version": ver,
                                "group": "rust"
                            }));
                        }
                    }
                }
            }
        }
    }

    // JS via package-lock.json (minimal)
    if Path::new("package-lock.json").exists() {
        if let Ok(s) = fs::read_to_string("package-lock.json") {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                if let Some(deps) = v.get("dependencies").and_then(|x| x.as_object()) {
                    for (name, info) in deps.iter() {
                        let ver = info.get("version").and_then(|x| x.as_str()).unwrap_or("");
                        if !name.is_empty() && !ver.is_empty() {
                            components.push(json!({
                                "type":"library",
                                "name": name,
                                "version": ver,
                                "group": "npm"
                            }));
                        }
                    }
                }
            }
        }
    }

    // Python via requirements.txt (minimal)
    if Path::new("requirements.txt").exists() {
        if let Ok(s) = fs::read_to_string("requirements.txt") {
            for line in s.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let (name, ver) = if let Some((n, v)) = line.split_once("==") {
                    (n, v)
                } else {
                    (line, "")
                };
                let name = name.trim();
                let ver = ver.trim();
                if !name.is_empty() {
                    let mut comp = json!({"type":"library","name": name, "group":"python"});
                    if !ver.is_empty() {
                        comp["version"] = json!(ver);
                    }
                    components.push(comp);
                }
            }
        }
    }

    let bom = json!({
        "$schema": "https://cyclonedx.org/schema/bom-1.5.schema.json",
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "version": 1,
        "metadata": {
            "tools": [{"vendor":"devit","name":"devit-cli","version": env!("CARGO_PKG_VERSION")}]
        },
        "components": components
    });
    if let Some(dir) = out.parent() {
        fs::create_dir_all(dir).ok();
    }
    fs::write(
        out,
        serde_json::to_vec_pretty(&bom).context("serialize SBOM")?,
    )?;
    // Audit: append a journal line with sha256 of the SBOM
    if let Ok(bytes) = fs::read(out) {
        let mut h = Sha256::new();
        h.update(&bytes);
        let sha = hex::encode(h.finalize());
        let _ = fs::create_dir_all(".devit");
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(".devit/journal.jsonl")?;
        let line = json!({"action":"sbom_gen","path": out.display().to_string(), "sha256": sha});
        use std::io::Write as _;
        writeln!(
            f,
            "{}",
            serde_json::to_string(&line).unwrap_or_else(|_| "{}".into())
        )?;
    }
    Ok(())
}
