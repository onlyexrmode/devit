// # -----------------------------
// # crates/agent/src/lib.rs
// # -----------------------------
use anyhow::Result;
use devit_backend_openai::{LlmBackend, OpenAiLike};
use devit_common::Config;

pub struct Agent
{
    llm: Box<dyn LlmBackend>,
}


impl Agent
{
    pub fn new(cfg: Config) -> Self
    {
        let llm = OpenAiLike::new(cfg.clone());
        Self { llm: Box::new(llm) }
    }

    pub async fn suggest_patch(&self, goal: &str, ctx: &str) -> Result<String>
    {
        let sys = "You are a code assistant that outputs unified diffs only.";
        let prompt = format!("Goal: {goal}\nContext:\n{ctx}\nOutput a unified diff.");
        let answer = self.llm.chat(sys, &prompt).await?;
        Ok(answer)
    }

    /// Génère un message de commit (Conventional Commits) à partir du goal, d'un résumé et d'un extrait de diff.
    /// Retourne une ligne courte (≤ 72 chars) ; body optionnel non inclus (MVP).
    pub async fn commit_message(&self, goal: &str, summary: &str, diff_head: &str) -> Result<String>
    {
        let sys = "You write Conventional Commit messages.\n\
                   Output a SINGLE LINE: <type>: <short message>.\n\
                   Types: feat, fix, chore, docs, test, refactor.";
        let prompt = format!(
            "Goal: {goal}\nSummary: {summary}\nDiff (first lines):\n{}\n\
             Rules: 1 line only, max 72 chars, no trailing dot.",
            diff_head
        );
        let msg = self.llm.chat(sys, &prompt).await?;
        Ok(msg.lines().next().unwrap_or(&msg).trim().to_string())
    }
}
