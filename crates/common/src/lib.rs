// # -----------------------------
// # crates/common/src/lib.rs
// # -----------------------------
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub backend: BackendCfg,
    pub policy: PolicyCfg,
    pub sandbox: SandboxCfg,
    pub git: GitCfg,
    #[serde(default)]
    pub provenance: ProvenanceCfg,
    #[serde(default)]
    pub precommit: Option<PrecommitCfg>,
    #[serde(default)]
    pub commit: Option<CommitCfg>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendCfg {
    pub kind: String,
    pub base_url: String,
    pub model: String,
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyCfg {
    pub approval: String,
    pub sandbox: String,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub approvals: Option<HashMap<String, String>>, // per-tool overrides: git|shell|test
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxCfg {
    pub cpu_limit: u32,
    pub mem_limit_mb: u32,
    pub net: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCfg {
    pub conventional: bool,
    pub max_staged_files: u32,
    #[serde(default)]
    pub use_notes: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProvenanceCfg {
    #[serde(default)]
    pub footer: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QualityCfg {
    #[serde(default)]
    pub max_test_failures: u32,
    #[serde(default)]
    pub max_lint_errors: u32,
    #[serde(default = "default_true")]
    pub allow_lint_warnings: bool,
    #[serde(default)]
    pub fail_on_missing_reports: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CommitCfg {
    #[serde(default = "default_max_subject")]
    pub max_subject: usize,
    #[serde(default)]
    pub scopes_alias: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub default_type: Option<String>,
    #[serde(default)]
    pub template_body: Option<String>,
}

fn default_max_subject() -> usize {
    72
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrecommitCfg {
    #[serde(default = "default_true")]
    pub rust: bool,
    #[serde(default = "default_true")]
    pub javascript: bool,
    #[serde(default = "default_true")]
    pub python: bool,
    #[serde(default)]
    pub additional: Vec<String>,
    #[serde(default = "default_fail_on")]
    pub fail_on: Vec<String>,
    #[serde(default)]
    pub allow_bypass_profiles: Vec<String>,
}

fn default_true() -> bool {
    true
}
fn default_fail_on() -> Vec<String> {
    vec!["rust".into(), "javascript".into(), "python".into()]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    ToolCall {
        name: String,
        args: serde_json::Value,
    },
    CommandOut {
        line: String,
    },
    Diff {
        unified: String,
    },
    AskApproval {
        summary: String,
    },
    Error {
        message: String,
    },
    Info {
        message: String,
    },
    Attest {
        hash: String,
    },
}
