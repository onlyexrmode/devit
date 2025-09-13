// # -----------------------------
// # crates/common/src/lib.rs
// # -----------------------------
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub backend: BackendCfg,
    pub policy: PolicyCfg,
    pub sandbox: SandboxCfg,
    pub git: GitCfg,
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
}
