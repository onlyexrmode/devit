// # -----------------------------
// # crates/backends/openai_like/src/lib.rs
// # -----------------------------
use anyhow::Result;
use async_trait::async_trait;
use devit_common::Config;
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[async_trait]
pub trait LlmBackend: Send + Sync {
    async fn chat(&self, sys: &str, user: &str) -> Result<String>;
}

pub struct OpenAiLike {
    cfg: Config,
    http: Client,
}

impl OpenAiLike {
    pub fn new(cfg: Config) -> Self {
        Self {
            cfg,
            http: Client::new(),
        }
    }
}

#[derive(Serialize)]
struct ChatReq<'a> {
    model: &'a str,
    messages: Vec<Msg<'a>>,
    stream: bool,
}

#[derive(Serialize)]
struct Msg<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatResp {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMsg,
}

#[derive(Deserialize)]
struct ChoiceMsg {
    content: String,
}

#[async_trait]
impl LlmBackend for OpenAiLike {
    async fn chat(&self, sys: &str, user: &str) -> Result<String> {
        let url = format!("{}/chat/completions", self.cfg.backend.base_url);
        let req = ChatReq {
            model: &self.cfg.backend.model,
            messages: vec![
                Msg {
                    role: "system",
                    content: sys,
                },
                Msg {
                    role: "user",
                    content: user,
                },
            ],
            stream: false,
        };

        let mut rb = self.http.post(&url).json(&req);
        if !self.cfg.backend.api_key.is_empty() {
            rb = rb.bearer_auth(&self.cfg.backend.api_key);
        }

        let resp: ChatResp = rb.send().await?.error_for_status()?.json().await?;
        Ok(resp
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default())
    }
}
