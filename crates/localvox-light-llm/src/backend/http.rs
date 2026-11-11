use super::{ChatBackend, GenerationOptions};
use crate::{num_ctx_for, ChatMessage, LlmProfile};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
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

pub(super) struct HttpBackend {
    base_url: String,
    api_key: Option<String>,
    agent: ureq::Agent,
}
impl HttpBackend {
    pub(super) fn new(profile: &LlmProfile) -> Self {
        Self {
            base_url: profile.base_url.clone(),
            api_key: profile.api_key.clone(),
            agent: ureq::AgentBuilder::new()
                .timeout(std::time::Duration::from_secs(profile.timeout_sec))
                .build(),
        }
    }

    fn chat_openai(
        &self,
        messages: &[ChatMessage],
        options: &GenerationOptions<'_>,
    ) -> Result<String> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let body = ChatRequest {
            model: options.model,
            messages,
            temperature: options.temperature,
            max_tokens: options.max_tokens,
            stream: false,
        };
        let mut req = self
            .agent
            .post(&url)
            .set("Content-Type", "application/json");
        if let Some(key) = &self.api_key {
            req = req.set("Authorization", &format!("Bearer {key}"));
        }
        let resp = req
            .send_json(serde_json::to_value(&body)?)
            .map_err(flatten_ureq_error)
            .with_context(|| format!("LLM request to {url} (model {})", options.model))?;
        let parsed: ChatResponse = resp.into_json().context("parsing the LLM answer")?;
        let Some(choice) = parsed.choices.into_iter().next() else {
            bail!("the LLM returned an empty choices list");
        };
        Ok(choice.message.content)
    }

    fn chat_ollama_native(
        &self,
        messages: &[ChatMessage],
        options: &GenerationOptions<'_>,
    ) -> Result<String> {
        // base_url is stored in OpenAI form (`…:11434/v1`) — the native root has no /v1
        let base = self
            .base_url
            .trim_end_matches('/')
            .trim_end_matches("/v1")
            .to_string();
        let url = format!("{base}/api/chat");
        let body = serde_json::json!({
            "model": options.model,
            "messages": messages,
            "stream": false,
            "think": false,
            // EVERY sampling parameter is pinned HERE, explicitly.
            //
            // We used to send only `num_predict` and `temperature`, and let the rest come from
            // whatever the model ships in its own params blob. That is not a default — it is
            // hidden entropy in someone else's file, and it changes our behaviour when they
            // re-publish the model.
            //
            // What it cost us, measured: `ollama show --parameters qwen3.5:9b` ships
            // `presence_penalty 1.5`. That penalises tokens which have ALREADY APPEARED — and the
            // correct output of the cleanup is ~90 % the very same tokens as its input, because
            // the task is «repeat this text, tidied». So we were mechanically penalising the model
            // for doing exactly what we asked, and pushing it towards the one thing that earns
            // fresh tokens: writing something of its own. On a live recording (16.07.2026) it did
            // precisely that — the readable text opened with «Вот структурированный анализ…»
            // instead of the conversation.
            //
            // Faithful retelling needs NO repetition penalty. Repeating the source is the job.
            "options": {
                // The one that was missing. See `num_ctx_for`.
                "num_ctx": num_ctx_for(
                    messages.iter().map(|m| m.content.len()).sum(),
                    options.max_ctx,
                ),
                "num_predict": options.max_tokens,
                "temperature": options.temperature,
                "presence_penalty": 0.0,
                "frequency_penalty": 0.0,
                "repeat_penalty": 1.0,
                "top_p": options.top_p,
                "top_k": options.top_k,
            },
        });
        let resp = self
            .agent
            .post(&url)
            .set("Content-Type", "application/json")
            .send_json(body)
            .map_err(flatten_ureq_error)
            .with_context(|| format!("Ollama request to {url} (model {})", options.model))?;
        #[derive(Deserialize)]
        struct NativeResponse {
            message: NativeMessage,
            #[serde(default)]
            load_duration: u64,
            #[serde(default)]
            prompt_eval_duration: u64,
            #[serde(default)]
            eval_duration: u64,
            #[serde(default)]
            eval_count: u64,
        }
        #[derive(Deserialize)]
        struct NativeMessage {
            #[serde(default)]
            content: String,
        }
        let parsed: NativeResponse = resp.into_json().context("parsing the Ollama answer")?;
        tracing::info!(model = %options.model,
            load_sec = parsed.load_duration as f64 / 1e9,
            prompt_sec = parsed.prompt_eval_duration as f64 / 1e9,
            generation_sec = parsed.eval_duration as f64 / 1e9,
            output_tokens = parsed.eval_count, "Ollama request completed");
        Ok(parsed.message.content)
    }
}
impl ChatBackend for HttpBackend {
    fn chat(&self, messages: &[ChatMessage], options: &GenerationOptions<'_>) -> Result<String> {
        match detect_flavor(&self.base_url) {
            ApiFlavor::OllamaNative => self.chat_ollama_native(messages, options),
            ApiFlavor::OpenAiCompat => self.chat_openai(messages, options),
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};

    // An actual bounded local HTTP exchange checks the wire, not just a request builder.
    fn exchange(
        status: &str,
        answer: &str,
    ) -> (String, std::thread::JoinHandle<(String, serde_json::Value)>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/v1/", listener.local_addr().unwrap());
        let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}", answer.len());
        let server = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "client did not connect to the HTTP fixture"
                        );
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(error) => panic!("HTTP fixture accept: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(&stream);
            let mut headers = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).unwrap() > 0);
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse::<usize>().unwrap();
                }
                headers.push_str(&line);
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            stream.write_all(response.as_bytes()).unwrap();
            (headers, serde_json::from_slice(&body).unwrap())
        });
        (url, server)
    }

    #[test]
    fn openai_adapter_preserves_messages_options_auth_and_common_guards() {
        let (url, server) = exchange(
            "200 OK",
            r#"{"choices":[{"message":{"content":"<think>private</think> answer"}}]}"#,
        );
        let mut client = crate::LlmClient::new(LlmProfile {
            base_url: url,
            api_key: Some("fixture-key".into()),
            model: "fixture-model".into(),
            temperature: 0.125,
            timeout_sec: 5,
            ..Default::default()
        });
        client.set_max_tokens(123);
        assert_eq!(
            client
                .chat(&[crate::system("rules"), crate::user("source")])
                .unwrap(),
            "answer"
        );
        let (headers, body) = server.join().unwrap();
        assert!(headers.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"));
        assert!(headers
            .to_ascii_lowercase()
            .contains("authorization: bearer fixture-key\r\n"));
        assert_eq!(
            body,
            serde_json::json!({"model":"fixture-model", "messages":[{"role":"system","content":"rules"},{"role":"user","content":"source"}], "temperature":0.125,"max_tokens":123,"stream":false})
        );
    }

    #[test]
    fn ollama_adapter_pins_native_dialect_context_and_sampling() {
        let (url, server) = exchange("200 OK", r#"{"message":{"content":"answer"}}"#);
        let profile = LlmProfile {
            base_url: url,
            timeout_sec: 5,
            max_tokens: 123,
            ..Default::default()
        };
        let backend = HttpBackend::new(&profile);
        assert_eq!(
            backend
                .chat_ollama_native(&[crate::user("source")], &GenerationOptions::from(&profile))
                .unwrap(),
            "answer"
        );
        let (headers, body) = server.join().unwrap();
        assert!(headers.starts_with("POST /api/chat HTTP/1.1\r\n"));
        assert_eq!(
            body,
            serde_json::json!({"model":profile.model,"messages":[{"role":"user","content":"source"}],"stream":false,"think":false,"options":{"num_ctx":4096,"num_predict":123,"temperature":profile.temperature,"presence_penalty":0.0,"frequency_penalty":0.0,"repeat_penalty":1.0,"top_p":profile.top_p,"top_k":profile.top_k}})
        );
        assert_eq!(
            detect_flavor("http://localhost:11434/v1"),
            ApiFlavor::OllamaNative
        );
        assert_eq!(
            detect_flavor("http://localhost:8080/v1"),
            ApiFlavor::OpenAiCompat
        );
    }

    #[test]
    fn http_failure_retains_bounded_server_detail_and_request_context() {
        let (url, server) = exchange("404 Not Found", r#"{"error":"model not pulled"}"#);
        let client = crate::LlmClient::new(LlmProfile {
            base_url: url,
            timeout_sec: 5,
            ..Default::default()
        });
        let error = format!("{:#}", client.chat(&[crate::user("source")]).unwrap_err());
        server.join().unwrap();
        assert!(error.contains("LLM request to"));
        assert!(error.contains("HTTP 404: {\"error\":\"model not pulled\"}"));
    }
}
