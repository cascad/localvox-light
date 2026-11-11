use super::{ChatBackend, GenerationOptions};
use crate::{claude_cli, ChatMessage};
use anyhow::Result;

pub(super) struct ClaudeBackend;
impl ChatBackend for ClaudeBackend {
    /// The Claude subscription path: the official `claude` CLI as a subprocess (see [`claude_cli`]).
    ///
    /// Our summary/refine prompts carry their whole instruction in the message text, and one part of
    /// a summary can be tens of KB — past Windows' command-line limit — so the material ALWAYS rides
    /// stdin (`content`) and the `-p` slot gets only the system text, or a short directive when there
    /// is none. A multi-turn re-ask (user → assistant → user) is flattened into one stdin document,
    /// because `claude -p` is single-shot; the model still sees the full confrontation as text.
    fn chat(&self, messages: &[ChatMessage], _options: &GenerationOptions<'_>) -> Result<String> {
        let sys: String = messages
            .iter()
            .filter(|m| m.role == "system")
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let body: String = messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| {
                if m.role == "assistant" {
                    format!("[Твой предыдущий ответ]\n{}", m.content)
                } else {
                    m.content.clone()
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let prompt = if sys.trim().is_empty() {
            "Выполни задачу, описанную во входных данных ниже, и верни ТОЛЬКО результат — без \
             вступлений, без пояснений, без markdown-обёртки вокруг всего ответа."
        } else {
            sys.as_str()
        };
        let cfg = claude_cli::ClaudeCliConfig::from_env();
        Ok(claude_cli::run(&cfg, prompt, Some(&body))?.text)
    }
}
