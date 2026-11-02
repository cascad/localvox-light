//! Temporary local-only native Ollama telemetry. Never records request/response content.
use super::*;
use std::sync::Mutex;
use tiny_http::{Header, Method, Response, Server};

const MAX_BODY: u64 = 8 * 1024 * 1024;

pub struct Trace {
    server: Arc<Server>,
    stopped: Arc<AtomicBool>,
    rows: Arc<Mutex<Vec<Value>>>,
    thread: Option<std::thread::JoinHandle<()>>,
    base_url: String,
}

impl Trace {
    pub fn start(upstream: &str, timeout: Duration) -> Result<Self> {
        Self::start_mode(upstream, timeout, 0)
    }
    pub fn sparse_experiment(upstream: &str, timeout: Duration) -> Result<Self> {
        Self::start_mode(upstream, timeout, 1)
    }
    pub fn sparse_lines(upstream: &str, timeout: Duration) -> Result<Self> {
        Self::start_mode(upstream, timeout, 2)
    }
    fn start_mode(upstream: &str, timeout: Duration, sparse: u8) -> Result<Self> {
        ensure!(
            local_endpoint(upstream),
            "telemetry upstream must be local HTTP"
        );
        let upstream = format!(
            "{}/api/chat",
            upstream.trim_end_matches('/').trim_end_matches("/v1")
        );
        let server = Arc::new(Server::http("127.0.0.1:0").map_err(|e| anyhow::anyhow!("{e}"))?);
        let port = server
            .server_addr()
            .to_ip()
            .context("telemetry bind address")?
            .port();
        let stopped = Arc::new(AtomicBool::new(false));
        let rows = Arc::new(Mutex::new(Vec::new()));
        let (s, stop, data) = (server.clone(), stopped.clone(), rows.clone());
        let thread = std::thread::spawn(move || {
            let agent = ureq::AgentBuilder::new()
                .timeout(timeout)
                .redirects(0)
                .build();
            while !stop.load(Ordering::Relaxed) {
                let Ok(Some(mut request)) = s.recv_timeout(Duration::from_millis(50)) else {
                    continue;
                };
                if request.method() != &Method::Post || request.url() != "/:11434/api/chat" {
                    let _ = request.respond(Response::empty(404));
                    continue;
                }
                let mut body = Vec::new();
                if request
                    .as_reader()
                    .take(MAX_BODY + 1)
                    .read_to_end(&mut body)
                    .is_err()
                    || body.len() as u64 > MAX_BODY
                {
                    let _ = request.respond(Response::empty(413));
                    continue;
                }
                let Ok(mut value) = serde_json::from_slice::<Value>(&body) else {
                    let _ = request.respond(Response::empty(400));
                    continue;
                };
                let prepared = if sparse != 0 {
                    match if sparse == 2 {
                        crate::sparse::prepare_lines(&value)
                    } else {
                        crate::sparse::prepare(&value)
                    } {
                        Ok(prepared) => Some(prepared),
                        Err(_) => {
                            let _ = request.respond(Response::empty(400));
                            continue;
                        }
                    }
                } else {
                    None
                };
                if let Some(prepared) = &prepared {
                    value = prepared.body.clone();
                    body = serde_json::to_vec(&value).unwrap();
                }
                let mut row = request_metadata(&value, body.len());
                if sparse != 0 {
                    row["experimental_sparse_edits"] = json!(true);
                    row["sparse_protocol"] = json!(if sparse == 2 { "lines" } else { "anchors" });
                }
                let index = {
                    let mut entries = data.lock().unwrap_or_else(|e| e.into_inner());
                    let index = entries.len();
                    entries.push(row);
                    index
                };
                let started = Instant::now();
                let response = agent
                    .post(&upstream)
                    .set("Content-Type", "application/json")
                    .send_bytes(&body);
                let response = match response {
                    Ok(response) | Err(ureq::Error::Status(_, response)) => Some(response),
                    Err(_) => None,
                };
                let mut status = response.as_ref().map_or(502, |response| response.status());
                let mut bytes = Vec::new();
                if let Some(response) = response {
                    if response
                        .into_reader()
                        .take(MAX_BODY + 1)
                        .read_to_end(&mut bytes)
                        .is_err()
                        || bytes.len() as u64 > MAX_BODY
                    {
                        status = 502;
                        bytes.clear();
                    }
                }
                let parsed = serde_json::from_slice::<Value>(&bytes).ok();
                let native_bytes = bytes.len();
                let sparse_stats = prepared.as_ref().filter(|_| status == 200).map(|prepared| {
                    let content = parsed
                        .as_ref()
                        .and_then(|v| v["message"]["content"].as_str())
                        .unwrap_or("");
                    let (expanded, stats) = prepared.adapt(content);
                    let mut adapted = parsed
                        .clone()
                        .filter(|v| v["message"].is_object())
                        .unwrap_or_else(|| json!({"message":{}}));
                    adapted["message"]["content"] = json!(expanded);
                    bytes = serde_json::to_vec(&adapted).unwrap();
                    serde_json::to_value(stats).unwrap()
                });
                {
                    let mut entries = data.lock().unwrap_or_else(|e| e.into_inner());
                    let row = &mut entries[index];
                    row["status"] = json!(if status == 200 {
                        "completed"
                    } else {
                        "http_error"
                    });
                    row["http_status"] = json!(status);
                    row["http_wall_sec"] = json!(started.elapsed().as_secs_f64());
                    row["ended_at"] = json!(localvox_light_core::versions::now_rfc3339());
                    row["response_bytes"] = json!(native_bytes);
                    row["ollama"] = parsed.as_ref().map(response_metrics).unwrap_or(Value::Null);
                    if let Some(stats) = sparse_stats {
                        row["sparse"] = stats;
                        row["locally_expanded_response_bytes"] = json!(bytes.len());
                    }
                }
                // Record telemetry before allowing the worker to observe completion.
                let response = Response::from_data(bytes)
                    .with_status_code(status)
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
            }
        });
        Ok(Self {
            server,
            stopped,
            rows,
            thread: Some(thread),
            base_url: format!("http://127.0.0.1:{port}/:11434/v1"),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }
    pub fn count(&self) -> usize {
        self.rows.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
    pub fn since(&self, index: usize) -> Vec<Value> {
        self.rows
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(index..)
            .unwrap_or_default()
            .to_vec()
    }
}

impl Drop for Trace {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        self.server.unblock();
        if let Some(thread) = self.thread.take() {
            // An external provider may still be computing. Never block cancellation on it.
            if thread.is_finished() {
                let _ = thread.join();
            }
        }
    }
}

fn request_metadata(body: &Value, bytes: usize) -> Value {
    let mut options = serde_json::Map::new();
    for key in [
        "num_ctx",
        "num_predict",
        "temperature",
        "presence_penalty",
        "frequency_penalty",
        "repeat_penalty",
        "top_p",
        "top_k",
    ] {
        if let Some(value) = body["options"][key].as_number() {
            options.insert(key.into(), json!(value));
        }
    }
    json!({"status": "pending", "http_wall_sec": null, "http_status": null,
        "started_at": localvox_light_core::versions::now_rfc3339(),
        "request_bytes": bytes, "message_count": body["messages"].as_array().map(Vec::len),
        "stream": body["stream"].as_bool(), "think": body["think"].as_bool(),
        "options": options, "ollama": null})
}

fn response_metrics(body: &Value) -> Value {
    let mut metrics = serde_json::Map::new();
    for (source, target) in [
        ("load_duration", "load_sec"),
        ("prompt_eval_duration", "prompt_sec"),
        ("eval_duration", "generation_sec"),
        ("total_duration", "total_sec"),
    ] {
        metrics.insert(
            target.into(),
            json!(body[source].as_u64().map(|ns| ns as f64 / 1e9)),
        );
    }
    for key in ["prompt_eval_count", "eval_count"] {
        metrics.insert(key.into(), json!(body[key].as_u64()));
    }
    metrics.insert("done".into(), json!(body["done"].as_bool()));
    metrics.insert(
        "done_reason".into(),
        json!(body["done_reason"]
            .as_str()
            .filter(|reason| matches!(*reason, "stop" | "length" | "load" | "unload"))),
    );
    json!(metrics)
}

pub fn redact_phase(phase: &mut Value) {
    if let Some(fields) = phase.as_object_mut() {
        for field in ["diagnostic_tail", "verification_error", "error"] {
            fields.remove(field);
        }
    }
    phase["diagnostic_content_omitted"] = json!(true);
}

pub fn redact_timeline(timeline: &mut Value) {
    if let Some(stages) = timeline["stages"].as_array_mut() {
        for stage in stages {
            if let Some(fields) = stage.as_object_mut() {
                fields.remove("note");
            }
            if let Some(fields) = stage["artifact"].as_object_mut() {
                fields.remove("reason");
            }
        }
    }
    if let Some(operations) = timeline["operations"].as_array_mut() {
        for operation in operations {
            let label = operation["label"].as_str().unwrap_or("");
            let category = if label == "Загрузка модели проверки имён" {
                "ner_model_load"
            } else if label == "Загрузка ASR-модели и адаптера" {
                "asr_model_load"
            } else if label.contains("повторный запрос после проверки")
            {
                "llm_retry"
            } else if label.contains("ожидание ответа") {
                "llm_request"
            } else if label.contains("Сборка общей сводки") {
                "summary_reduce"
            } else if label.contains("проверка имён и чисел")
                || label.contains("Проверка итогового текста")
            {
                "grounding_check"
            } else {
                "operation"
            };
            operation["category"] = json!(category);
            operation.as_object_mut().unwrap().remove("label");
        }
    }
}

pub fn summarize(rows: Vec<Value>, worker_wall: Option<f64>) -> Value {
    let sum = |field: &str| {
        (!rows.is_empty())
            .then(|| {
                rows.iter()
                    .map(|row| {
                        (row["status"] == "completed")
                            .then(|| row["ollama"][field].as_f64())
                            .flatten()
                    })
                    .collect::<Option<Vec<_>>>()
                    .map(|values| values.iter().sum::<f64>())
            })
            .flatten()
    };
    let http_wall = (!rows.is_empty())
        .then(|| {
            rows.iter()
                .map(|row| row["http_wall_sec"].as_f64())
                .collect::<Option<Vec<_>>>()
                .map(|values| values.iter().sum::<f64>())
        })
        .flatten();
    json!({"source": "native_ollama_response", "request_count": rows.len(),
        "model_load_sec": sum("load_sec"), "prompt_sec": sum("prompt_sec"), "generation_sec": sum("generation_sec"),
        "http_wall_sec": http_wall,
        "non_http_worker_wall_sec": http_wall.zip(worker_wall).and_then(|(http, worker)| (worker >= http).then_some(worker-http)),
        "requests": rows})
}

#[cfg(test)]
#[path = "ollama_tests.rs"]
mod tests;
