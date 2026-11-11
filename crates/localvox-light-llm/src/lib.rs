//! LLM processing of transcripts (F1, WP-A4).
//!
//! One client for all providers. Backend adapters speak OpenAI-compatible HTTP,
//! Ollama native chat or the owned Claude CLI. Messages and generation options
//! stay independent of wire formats; the client validates the resulting content.
//! Processing does not mutate the sources: the result is written as new files
//! alongside (`processed.md`, `summary.md`) — principle P2.

mod backend;
pub use backend::{ChatBackend, GenerationOptions};
pub mod claude_cli;
pub mod correction_guard;
pub mod glossary;
pub mod grounding;
pub mod pipeline;
pub mod routing;
pub mod templates;

use anyhow::{bail, Result};
use serde::Serialize;

/// Where a chat request is answered. `Http` — the OpenAI-compatible / Ollama-native endpoint at
/// `base_url` (the normal path). `Claude` — the official `claude` CLI as a subprocess, drawing on a
/// Pro/Max SUBSCRIPTION (see [`claude_cli`]); used when the cook is told to make the summary with
/// Claude. It composes cleanly: only the transport differs, everything above `chat()` is unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Provider {
    #[default]
    Http,
    Claude,
}

/// Provider profile. V1 — from flags/env; `llm.toml` with several profiles is a follow-up.
#[derive(Clone, Debug)]
pub struct LlmProfile {
    /// Which transport answers `chat()`. Default [`Provider::Http`] — the endpoint below.
    pub provider: Provider,
    /// `http://localhost:11434/v1` (Ollama) / `https://api.openai.com/v1` /
    /// `https://openrouter.ai/api/v1`. Ignored when `provider` is [`Provider::Claude`].
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
    /// Nucleus sampling. Pinned by US, not taken from the model's shipped params: a default that
    /// lives in someone else's file is hidden entropy — it changes our behaviour when they
    /// re-publish the model, and nothing here would say so.
    pub top_p: f32,
    pub top_k: u32,
    /// Ceiling on the context window we will ask a request to allocate.
    ///
    /// The window itself is COMPUTED per request (`num_ctx_for`); this only bounds it, because
    /// Ollama allocates the KV cache for whatever it is told and a careless number costs real
    /// memory on the owner's machine.
    pub max_ctx: u32,
}

/// How many characters of Russian text one token holds, rounded DOWN so the estimate errs towards
/// asking for a bigger window. Measured 19.07.2026 on the owner's cleanup batch: 9801 characters
/// became 3326 prompt tokens — 2.95 chars per token.
const CHARS_PER_TOKEN: usize = 2;

/// The context window for a request of this size — prompt AND answer together.
///
/// THE BUG THIS EXISTS FOR, measured 19.07.2026. Every sampling parameter was pinned explicitly,
/// and `num_ctx` was not — so it came from Ollama's runtime default, which is small, while the
/// model itself advertises 262144. The cleanup sent a 3326-token batch and asked for a similar
/// answer; the two together did not fit, so the answer was cut off and the model degenerated into
/// loops («Вот. Вот. Вот.» ×200). The same batch, twice:
///
/// ```text
///     num_ctx by default — answered for 11 of 40 lines
///     num_ctx = 16384    — answered for 40 of 40
/// ```
///
/// On the owner's video that showed up as a readable text still full of «э-э» and «ну как бы»: of
/// 76 lines, 13 got no answer and 31 answers were rolled back by the guards. The model was not
/// failing at the task. We never gave it room to do it.
///
/// The cleanup's answer is about the size of its prompt — it is a retelling, not a summary — so
/// the window has to hold both, plus room for the model to finish a sentence.
fn num_ctx_for(prompt_chars: usize, max_ctx: u32) -> u32 {
    let tokens = prompt_chars / CHARS_PER_TOKEN;
    let need = (tokens * 2 + 512) as u32;
    // Powers of two: Ollama reuses a loaded model when the window matches, and a value that
    // wobbles per request would reload it on nearly every call.
    let mut ctx = 4096u32;
    while ctx < need && ctx < max_ctx {
        ctx *= 2;
    }
    ctx.min(max_ctx)
}

impl Default for LlmProfile {
    fn default() -> Self {
        Self {
            provider: Provider::Http,
            base_url: localvox_light_core::presets::baked("LOCALVOX_LLM_BASE_URL").into(),
            model: localvox_light_core::presets::baked("LOCALVOX_LLM_MODEL").into(),
            api_key: None,
            temperature: 0.2,
            timeout_sec: 600,
            max_tokens: 8192,
            top_p: 0.9,
            top_k: 20,
            max_ctx: 32768,
        }
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

pub struct LlmClient {
    profile: LlmProfile,
    backend: Box<dyn ChatBackend>,
}

impl LlmClient {
    pub fn new(profile: LlmProfile) -> Self {
        let backend = backend::configured(&profile);
        Self::with_backend(profile, backend)
    }

    pub fn with_backend(profile: LlmProfile, backend: Box<dyn ChatBackend>) -> Self {
        Self { profile, backend }
    }

    pub fn model(&self) -> &str {
        &self.profile.model
    }

    /// A ceiling on the answer. Some questions deserve a page; a question about your own
    /// recordings does not — and a model that has started looping will run to the timeout unless
    /// something stops it.
    pub fn set_max_tokens(&mut self, max_tokens: u32) {
        self.profile.max_tokens = max_tokens;
    }

    /// A single chat request; reasoning is stripped/disabled depending on the dialect.
    pub fn chat(&self, messages: &[ChatMessage]) -> Result<String> {
        let raw = self
            .backend
            .chat(messages, &GenerationOptions::from(&self.profile))?;
        let content = strip_think_blocks(&raw).trim().to_string();
        if content.is_empty() {
            bail!("the LLM returned an empty answer (for thinking models — raise max_tokens)");
        }
        Ok(content)
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
mod ctx_tests {
    use super::num_ctx_for;

    /// The measured case: a 9801-character cleanup batch. It must not land on a window that only
    /// fits the prompt — the answer is the same size again.
    #[test]
    fn the_window_holds_the_prompt_and_its_answer() {
        let ctx = num_ctx_for(9801, 32768);
        assert!(ctx >= 3326 * 2, "the answer has no room: {ctx}");
        assert_eq!(ctx, 16384);
    }

    /// A small request must not allocate a huge KV cache on the owner's machine.
    #[test]
    fn a_short_request_keeps_a_small_window() {
        assert_eq!(num_ctx_for(200, 32768), 4096);
    }

    /// The ceiling is a ceiling. Asking for more than the machine was told to allow is how a
    /// local model starts swapping instead of answering.
    #[test]
    fn the_ceiling_holds() {
        assert_eq!(num_ctx_for(10_000_000, 32768), 32768);
    }

    /// Powers of two only: Ollama reloads the model when the window changes, and a value that
    /// wobbled per request would reload it on nearly every call.
    #[test]
    fn windows_are_powers_of_two() {
        for chars in [0, 1, 500, 5_000, 20_000, 100_000] {
            let c = num_ctx_for(chars, 32768);
            assert!(c.is_power_of_two(), "{chars} chars gave {c}");
        }
    }
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
