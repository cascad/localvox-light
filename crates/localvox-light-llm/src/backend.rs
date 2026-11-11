//! Inference boundary. Requests carry model/options; adapters own wire formats,
//! credentials, connections and provider-specific execution limits.
use crate::{ChatMessage, LlmProfile, Provider};
use anyhow::Result;

mod claude;
mod http;

pub struct GenerationOptions<'a> {
    pub model: &'a str,
    pub temperature: f32,
    pub max_tokens: u32,
    pub max_ctx: u32,
    pub top_p: f32,
    pub top_k: u32,
}
impl<'a> From<&'a LlmProfile> for GenerationOptions<'a> {
    fn from(profile: &'a LlmProfile) -> Self {
        Self {
            model: &profile.model,
            temperature: profile.temperature,
            max_tokens: profile.max_tokens,
            max_ctx: profile.max_ctx,
            top_p: profile.top_p,
            top_k: profile.top_k,
        }
    }
}

/// Produces raw content. The client applies the common thinking/empty-answer guard.
/// Provider adapters preserve their own bounded HTTP/owned-process configuration.
pub trait ChatBackend: Send + Sync {
    fn chat(&self, messages: &[ChatMessage], options: &GenerationOptions<'_>) -> Result<String>;
}

pub(super) fn configured(profile: &LlmProfile) -> Box<dyn ChatBackend> {
    match profile.provider {
        Provider::Http => Box::new(http::HttpBackend::new(profile)),
        Provider::Claude => Box::new(claude::ClaudeBackend),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{user, LlmClient};
    use std::sync::{Arc, Mutex};
    struct RecordedBackend(Arc<Mutex<Vec<(String, u32, String)>>>);
    impl ChatBackend for RecordedBackend {
        fn chat(
            &self,
            messages: &[ChatMessage],
            options: &GenerationOptions<'_>,
        ) -> Result<String> {
            self.0.lock().unwrap().push((
                options.model.into(),
                options.max_tokens,
                messages[0].content.clone(),
            ));
            Ok("<think>hidden reasoning</think>\n answer ".into())
        }
    }
    #[test]
    fn a_replaceable_backend_receives_the_current_model_options_and_common_output_guards() {
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let mut client = LlmClient::with_backend(
            LlmProfile {
                model: "fixture-model".into(),
                ..Default::default()
            },
            Box::new(RecordedBackend(recorded.clone())),
        );
        client.set_max_tokens(128);
        assert_eq!(client.chat(&[user("source text")]).unwrap(), "answer");
        assert_eq!(
            *recorded.lock().unwrap(),
            [("fixture-model".into(), 128, "source text".into())]
        );
    }
    struct FailingBackend;
    impl ChatBackend for FailingBackend {
        fn chat(&self, _: &[ChatMessage], _: &GenerationOptions<'_>) -> Result<String> {
            anyhow::bail!("synthetic backend failure")
        }
    }
    #[test]
    fn backend_failure_is_not_replaced_with_empty_or_successful_content() {
        let client = LlmClient::with_backend(LlmProfile::default(), Box::new(FailingBackend));
        assert!(client
            .chat(&[user("source")])
            .unwrap_err()
            .to_string()
            .contains("synthetic backend failure"));
    }
}
