//! LLM processing of transcripts (F1, WP-A4).
//!
//! One client for all providers: OpenAI-compatible chat-completions covers
//! Ollama (`http://localhost:11434/v1`), OpenAI and OpenRouter — only
//! `base_url` / `api_key` / `model` change (decision from `docs/feature-registry.md`).
//! Processing does not mutate the sources: the result is written as new files
//! alongside (`processed.md`, `summary.md`) — principle P2.

pub mod glossary;
pub mod grounding;
pub mod pipeline;
pub mod routing;
pub mod templates;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// Provider profile. V1 — from flags/env; `llm.toml` with several profiles is a follow-up.
#[derive(Clone, Debug)]
pub struct LlmProfile {
    /// `http://localhost:11434/v1` (Ollama) / `https://api.openai.com/v1` /
    /// `https://openrouter.ai/api/v1`.
    pub base_url: String,
    pub model: String,
    /// Key (not needed for Ollama).
    pub api_key: Option<String>,
    pub temperature: f32,
    /// Timeout of a single request, sec (local models are loaded on the first call).
    pub timeout_sec: u64,
    /// Ceiling on answer tokens (Ollama cuts thinking+answer together — thinking models
    /// with reasoning left on return empty content).
    pub max_tokens: u32,
}

impl Default for LlmProfile {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:11434/v1".into(),
            model: "qwen3.5:9b".into(),
            api_key: None,
            temperature: 0.2,
            timeout_sec: 600,
            max_tokens: 8192,
        }
    }
}

/// API dialect. Ollama's OpenAI-compat endpoint cannot turn thinking off (verified:
/// `think:false` and `/no_think` are ignored, reasoning burns through max_tokens and
/// content comes back empty) — so with Ollama we speak its native `/api/chat`
/// with an honest `think: false`. For everyone else — OpenAI chat-completions.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ApiFlavor {
    OpenAiCompat,
    OllamaNative,
}

fn detect_flavor(base_url: &str) -> ApiFlavor {
    if base_url.contains(":11434") {
        ApiFlavor::OllamaNative
    } else {
        ApiFlavor::OpenAiCompat
    }
}

#[derive(Serialize, Clone)]
pub struct ChatMessage {
    pub role: &'static str,
    pub content: String,
}

pub fn system(content: impl Into<String>) -> ChatMessage {
    ChatMessage {
        role: "system",
        content: content.into(),
    }
}
pub fn user(content: impl Into<String>) -> ChatMessage {
    ChatMessage {
        role: "user",
        content: content.into(),
    }
}

/// The model's previous answer — needed to ask it again while confronting it with
/// what it invented («here is what you made up, rewrite without it»).
pub fn assistant(content: impl Into<String>) -> ChatMessage {
    ChatMessage {
        role: "assistant",
        content: content.into(),
    }
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    temperature: f32,
    max_tokens: u32,
    stream: bool,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}
#[derive(Deserialize)]
struct Choice {
    message: AssistantMessage,
}
#[derive(Deserialize)]
struct AssistantMessage {
    content: String,
}

pub struct LlmClient {
    profile: LlmProfile,
    agent: ureq::Agent,
}

impl LlmClient {
    pub fn new(profile: LlmProfile) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(profile.timeout_sec))
            .build();
        Self { profile, agent }
    }

    pub fn model(&self) -> &str {
        &self.profile.model
    }

    /// A single chat request; reasoning is stripped/disabled depending on the dialect.
    pub fn chat(&self, messages: &[ChatMessage]) -> Result<String> {
        let raw = match detect_flavor(&self.profile.base_url) {
            ApiFlavor::OllamaNative => self.chat_ollama_native(messages)?,
            ApiFlavor::OpenAiCompat => self.chat_openai(messages)?,
        };
        let content = strip_think_blocks(&raw).trim().to_string();
        if content.is_empty() {
            bail!("the LLM returned an empty answer (for thinking models — raise max_tokens)");
        }
        Ok(content)
    }

    fn chat_openai(&self, messages: &[ChatMessage]) -> Result<String> {
        let url = format!(
            "{}/chat/completions",
            self.profile.base_url.trim_end_matches('/')
        );
        let body = ChatRequest {
            model: &self.profile.model,
            messages,
            temperature: self.profile.temperature,
            max_tokens: self.profile.max_tokens,
            stream: false,
        };
        let mut req = self
            .agent
            .post(&url)
            .set("Content-Type", "application/json");
        if let Some(key) = &self.profile.api_key {
            req = req.set("Authorization", &format!("Bearer {key}"));
        }
        let resp = req
            .send_json(serde_json::to_value(&body)?)
            .map_err(flatten_ureq_error)
            .with_context(|| format!("LLM request to {url} (model {})", self.profile.model))?;
        let parsed: ChatResponse = resp.into_json().context("parsing the LLM answer")?;
        let Some(choice) = parsed.choices.into_iter().next() else {
            bail!("the LLM returned an empty choices list");
        };
        Ok(choice.message.content)
    }

    fn chat_ollama_native(&self, messages: &[ChatMessage]) -> Result<String> {
        // base_url is stored in OpenAI form (`…:11434/v1`) — the native root has no /v1
        let base = self
            .profile
            .base_url
            .trim_end_matches('/')
            .trim_end_matches("/v1")
            .to_string();
        let url = format!("{base}/api/chat");
        let body = serde_json::json!({
            "model": self.profile.model,
            "messages": messages,
            "stream": false,
            "think": false,
            "options": {
                "num_predict": self.profile.max_tokens,
                "temperature": self.profile.temperature,
            },
        });
        let resp = self
            .agent
            .post(&url)
            .set("Content-Type", "application/json")
            .send_json(body)
            .map_err(flatten_ureq_error)
            .with_context(|| format!("Ollama request to {url} (model {})", self.profile.model))?;
        #[derive(Deserialize)]
        struct NativeResponse {
            message: NativeMessage,
        }
        #[derive(Deserialize)]
        struct NativeMessage {
            #[serde(default)]
            content: String,
        }
        let parsed: NativeResponse = resp.into_json().context("parsing the Ollama answer")?;
        Ok(parsed.message.content)
    }
}

/// The body of a ureq error (e.g. a 404 from Ollama about a model that is not pulled)
/// holds the most valuable part.
fn flatten_ureq_error(e: ureq::Error) -> anyhow::Error {
    match e {
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            anyhow::anyhow!(
                "HTTP {code}: {}",
                body.chars().take(400).collect::<String>()
            )
        }
        other => anyhow::anyhow!(other),
    }
}

/// Removes `<think>…</think>` (the reasoning mode of Qwen3/DeepSeek in Ollama).
fn strip_think_blocks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<think>") {
        out.push_str(&rest[..start]);
        match rest[start..].find("</think>") {
            Some(end_rel) => rest = &rest[start + end_rel + "</think>".len()..],
            None => {
                // unclosed block — drop the tail
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_think_removes_blocks_and_keeps_text() {
        assert_eq!(strip_think_blocks("<think>шум</think>Ответ"), "Ответ");
        assert_eq!(
            strip_think_blocks("до <think>a</think>середина<think>b</think> после"),
            "до середина после"
        );
        assert_eq!(strip_think_blocks("без блоков"), "без блоков");
        assert_eq!(strip_think_blocks("хвост <think>не закрыт"), "хвост ");
    }
}
