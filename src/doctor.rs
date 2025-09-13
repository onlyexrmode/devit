#![allow(clippy::uninlined_format_args)]
//! Outil de diagnostic isolé, compilé avec `--features experimental`.
//! Ne modifie pas la CLI principale.
//! Sortie humaine ou JSON, mode strict optionnel.
#[cfg(feature = "experimental")]
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    NotFound,
    Unreachable,
    NotInstalled,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCheck {
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BackendChecks {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lm_studio: Option<ToolCheck>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ollama: Option<ToolCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DoctorReport {
    pub rustc: ToolCheck,
    pub cargo: ToolCheck,
    pub bwrap: ToolCheck,
    pub wasm32_wasi_target: ToolCheck,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backends: Option<BackendChecks>,
}

fn run_cmd_version(cmd: &str, arg: &str) -> ToolCheck {
    match Command::new(cmd).arg(arg).output() {
        Ok(out) => {
            if out.status.success() {
                let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
                ToolCheck { status: Status::Ok, detail: Some(v) }
            } else {
                ToolCheck { status: Status::Unknown, detail: Some(format!("exit: {}", out.status)) }
            }
        }
        Err(_) => ToolCheck { status: Status::NotFound, detail: None },
    }
}

fn run_cmd_exists(cmd: &str, arg: &str) -> ToolCheck {
    match Command::new(cmd).arg(arg).output() {
        Ok(out) => {
            if out.status.success() {
                ToolCheck { status: Status::Ok, detail: None }
            } else {
                ToolCheck { status: Status::Unknown, detail: Some(format!("exit: {}", out.status)) }
            }
        }
        Err(_) => ToolCheck { status: Status::NotFound, detail: None },
    }
}

fn check_wasm32_wasi_target() -> ToolCheck {
    match Command::new("rustup").args(["target", "list", "--installed"]).output() {
        Ok(out) => {
            if !out.status.success() {
                return ToolCheck { status: Status::Unknown, detail: Some(format!("rustup exit: {}", out.status)) };
            }
            let s = String::from_utf8_lossy(&out.stdout);
            if s.lines().any(|l| l.trim() == "wasm32-wasi") {
                ToolCheck { status: Status::Ok, detail: None }
            } else {
                ToolCheck { status: Status::NotInstalled, detail: Some("missing wasm32-wasi".into()) }
            }
        }
        Err(_) => ToolCheck { status: Status::Unknown, detail: Some("rustup not found".into()) },
    }
}

#[cfg(feature = "experimental")]
fn check_http_get(url: &str, timeout_ms: u64) -> ToolCheck {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_millis(timeout_ms))
        .timeout(Duration::from_millis(timeout_ms))
        .build();
    match agent.get(url).call() {
        Ok(resp) => {
            if resp.status() / 100 == 2 {
                ToolCheck { status: Status::Ok, detail: Some(format!("http {}", resp.status())) }
            } else {
                ToolCheck { status: Status::Unreachable, detail: Some(format!("http {}", resp.status())) }
            }
        }
        Err(e) => ToolCheck { status: Status::Unreachable, detail: Some(format!("{e}")) },
    }
}

pub struct DoctorArgs<'a> {
    pub check_backends: bool,
    pub lm_url: Option<&'a str>,
    pub ollama_url: Option<&'a str>,
    pub timeout_ms: u64,
}

pub fn gather_report(args: DoctorArgs) -> DoctorReport {
    let rustc = run_cmd_version("rustc", "--version");
    let cargo = run_cmd_version("cargo", "--version");
    let bwrap = run_cmd_exists("bwrap", "--version");
    let wasm32_wasi_target = check_wasm32_wasi_target();

    let mut report = DoctorReport {
        rustc,
        cargo,
        bwrap,
        wasm32_wasi_target,
        backends: None,
    };

    if args.check_backends {
        let mut backends = BackendChecks::default();
        // LM Studio: /v1/models (OpenAI-compatible)
        if let Some(u) = args.lm_url {
            #[cfg(feature = "experimental")]
            {
                let url = format!("{}/models", u.trim_end_matches('/'));
                backends.lm_studio = Some(check_http_get(&url, args.timeout_ms));
            }
            #[cfg(not(feature = "experimental"))]
            {
                let _ = u;
                backends.lm_studio = Some(ToolCheck { status: Status::Unknown, detail: Some("built without experimental".into()) });
            }
        }
        // Ollama: /api/tags
        if let Some(u) = args.ollama_url {
            #[cfg(feature = "experimental")]
            {
                let url = format!("{}/api/tags", u.trim_end_matches('/'));
                backends.ollama = Some(check_http_get(&url, args.timeout_ms));
            }
            #[cfg(not(feature = "experimental"))]
            {
                let _ = u;
                backends.ollama = Some(ToolCheck { status: Status::Unknown, detail: Some("built without experimental".into()) });
            }
        }
        report.backends = Some(backends);
    }

    report
}

pub fn print_human(report: &DoctorReport) {
    fn icon(s: &Status) -> &'static str {
        match s {
            Status::Ok => "✔",
            Status::NotFound => "✖",
            Status::Unreachable => "⚠",
            Status::NotInstalled => "◌",
            Status::Unknown => "?",
        }
    }
    println!("== DevIt Doctor ==");
    println!("rustc: {} {}", icon(&report.rustc.status), report.rustc.detail.as_deref().unwrap_or(""));
    println!("cargo: {} {}", icon(&report.cargo.status), report.cargo.detail.as_deref().unwrap_or(""));
    println!("bwrap: {} {}", icon(&report.bwrap.status), report.bwrap.detail.as_deref().unwrap_or("not found or no version"));
    println!("wasm32-wasi: {} {}", icon(&report.wasm32_wasi_target.status), report.wasm32_wasi_target.detail.as_deref().unwrap_or(""));
    if let Some(b) = &report.backends {
        if let Some(lm) = &b.lm_studio {
            println!("LM Studio: {} {}", icon(&lm.status), lm.detail.as_deref().unwrap_or(""));
        }
        if let Some(ol) = &b.ollama {
            println!("Ollama: {} {}", icon(&ol.status), ol.detail.as_deref().unwrap_or(""));
        }
    }
}

pub fn print_json(report: &DoctorReport) -> Result<()> {
    let s = serde_json::to_string_pretty(report)?;
    println!("{s}");
    Ok(())
}

pub fn exit_code(report: &DoctorReport) -> i32 {
    let mut bad = false;
    for t in [&report.rustc, &report.cargo, &report.bwrap, &report.wasm32_wasi_target] {
        bad |= !matches!(t.status, Status::Ok);
    }
    if let Some(b) = &report.backends {
        if let Some(lm) = &b.lm_studio {
            bad |= !matches!(lm.status, Status::Ok);
        }
        if let Some(ol) = &b.ollama {
            bad |= !matches!(ol.status, Status::Ok);
        }
    }
    if bad { 2 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_shape_is_stable() {
        // Pas de requêtes réseau dans le test.
        let report = gather_report(DoctorArgs {
            check_backends: false,
            lm_url: None,
            ollama_url: None,
            timeout_ms: 300,
        });
        let s = serde_json::to_string(&report).expect("json");
        assert!(s.contains("rustc"));
        assert!(s.contains("cargo"));
        assert!(s.contains("wasm32_wasi_target"));
    }
}
