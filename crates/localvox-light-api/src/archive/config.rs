//! Construct application profiles without mixing them into feature operations.

/// LLM client from the `LOCALVOX_LLM_*` env (shared with `localvox-process`).
///
/// The timeout is a PARAMETER, not a constant: "where do I file this?" is one phrase
/// and a short wait, whereas a question to the archive carries thousands of
/// characters of found fragments with it and thinks noticeably longer. A single
/// timeout for both cases either cuts an honest answer off mid-word, or lets a hung
/// LLM hold an HTTP thread for ten minutes.
pub(super) fn llm_client_from_env_with(timeout_sec: u64) -> localvox_light_llm::LlmClient {
    llm_client_tuned(timeout_sec, None)
}

/// The same client, but with the temperature under our control.
///
/// A question to the archive is a RETRIEVAL task, not creative writing: at temperature 0.2
/// the model tosses a coin. Measured (13.07.2026): on one and the same prompt — with
/// fragments that literally talk about a cake — it answered correctly once and refused
/// («В записях этого нет») the next time. An answer that depends on a dice roll is not an
/// answer.
/// The same, with a ceiling on the answer. A bounded output is not a nicety: without it a model
/// that has started looping runs until the timeout, and the person gets a page of noise.
pub(super) fn llm_client_capped(
    timeout_sec: u64,
    temperature: Option<f32>,
    max_tokens: u32,
) -> localvox_light_llm::LlmClient {
    let mut client = llm_client_tuned(timeout_sec, temperature);
    client.set_max_tokens(max_tokens);
    client
}

pub(super) fn llm_client_tuned(
    timeout_sec: u64,
    temperature: Option<f32>,
) -> localvox_light_llm::LlmClient {
    let base_url = std::env::var("LOCALVOX_LLM_BASE_URL")
        .unwrap_or_else(|_| localvox_light_core::presets::baked("LOCALVOX_LLM_BASE_URL").into());
    let model = std::env::var("LOCALVOX_LLM_MODEL")
        .unwrap_or_else(|_| localvox_light_core::presets::baked("LOCALVOX_LLM_MODEL").into());
    let api_key = std::env::var("LOCALVOX_LLM_API_KEY_ENV")
        .ok()
        .and_then(|name| std::env::var(name).ok());
    let default = localvox_light_llm::LlmProfile::default();
    localvox_light_llm::LlmClient::new(localvox_light_llm::LlmProfile {
        base_url,
        model,
        api_key,
        timeout_sec,
        temperature: temperature.unwrap_or(default.temperature),
        ..default
    })
}

pub(super) fn env_secs(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

pub(super) fn env_f64(name: &str, default: f64) -> f64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
