use super::*;

fn upstream() -> (Server, String) {
    let server = Server::http("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", server.server_addr().to_ip().unwrap());
    (server, url)
}

fn post_url(trace: &Trace) -> String {
    format!("{}/api/chat", trace.base_url().trim_end_matches("/v1"))
}

#[test]
fn forwards_exact_native_json_and_records_only_numeric_telemetry() {
    let (server, url) = upstream();
    let body = json!({"model":"test", "messages":[{"role":"user","content":"PRIVATE PROMPT"}],
        "think":false,"stream":false,"options":{"num_ctx":4096,"num_predict":50,"secret":"HIDDEN"}});
    let expected = body.clone();
    let response = json!({"message":{"content":"PRIVATE ANSWER"},"done":true,
        "load_duration":1000000000u64,"prompt_eval_duration":2000000000u64,"eval_duration":3000000000u64,
        "total_duration":6500000000u64,"prompt_eval_count":30,"eval_count":12,"unknown":"HIDDEN"});
    let returned = response.clone();
    let worker = std::thread::spawn(move || {
        let mut request = server
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap();
        assert_eq!(request.url(), "/api/chat");
        let mut text = String::new();
        request.as_reader().read_to_string(&mut text).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), expected);
        request
            .respond(Response::from_string(response.to_string()))
            .unwrap();
    });
    let trace = Trace::start(&url, Duration::from_secs(3)).unwrap();
    let actual: Value = ureq::post(&post_url(&trace))
        .send_json(body)
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(actual, returned);
    worker.join().unwrap();
    let data = summarize(trace.since(0), Some(7.0));
    assert_eq!(data["model_load_sec"], 1.0);
    assert_eq!(data["generation_sec"], 3.0);
    assert_eq!(data["requests"][0]["ollama"]["eval_count"], 12);
    assert!(chrono::DateTime::parse_from_rfc3339(
        data["requests"][0]["started_at"].as_str().unwrap()
    )
    .is_ok());
    assert!(chrono::DateTime::parse_from_rfc3339(
        data["requests"][0]["ended_at"].as_str().unwrap()
    )
    .is_ok());
    assert_eq!(
        data["requests"][0]["options"],
        json!({"num_ctx":4096,"num_predict":50})
    );
    let text = data.to_string();
    for secret in ["PRIVATE", "HIDDEN", "messages", "message\""] {
        assert!(!text.contains(secret));
    }
}

#[test]
fn missing_stats_stay_unknown_and_real_zero_is_preserved() {
    let missing = response_metrics(&json!({"message":{"content":"text"}}));
    assert!(missing["load_sec"].is_null());
    assert!(missing["eval_count"].is_null());
    let zero = response_metrics(&json!({"load_duration":0,"eval_duration":0,"eval_count":0}));
    assert_eq!(zero["load_sec"], 0.0);
    assert_eq!(zero["eval_count"], 0);
    assert!(response_metrics(&json!({"done_reason":"PRIVATE DATA"}))["done_reason"].is_null());
    assert_eq!(
        response_metrics(&json!({"done_reason":"length"}))["done_reason"],
        "length"
    );
    assert!(summarize(vec![], Some(1.0))["model_load_sec"].is_null());
    let rows = vec![json!({"status":"completed","http_wall_sec":1.0,"ollama":missing})];
    assert!(summarize(rows, Some(2.0))["generation_sec"].is_null());
    let inconsistent = vec![json!({"status":"completed","http_wall_sec":3.0,"ollama":zero})];
    assert!(summarize(inconsistent, Some(1.0))["non_http_worker_wall_sec"].is_null());
}

#[test]
fn rejects_other_routes_and_forwards_http_errors_without_recording_the_body() {
    assert!(Trace::start("https://external.example/v1", Duration::from_secs(1)).is_err());
    let (server, url) = upstream();
    let worker = std::thread::spawn(move || {
        let request = server
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap();
        request
            .respond(Response::from_string("PRIVATE ERROR").with_status_code(503))
            .unwrap();
    });
    let trace = Trace::start(&url, Duration::from_secs(3)).unwrap();
    assert!(matches!(
        ureq::get(&post_url(&trace)).call(),
        Err(ureq::Error::Status(404, _))
    ));
    assert_eq!(trace.count(), 0);
    let error = ureq::post(&post_url(&trace)).send_json(json!({"messages":[]}));
    let Err(ureq::Error::Status(503, response)) = error else {
        panic!("expected upstream 503");
    };
    assert_eq!(response.into_string().unwrap(), "PRIVATE ERROR");
    worker.join().unwrap();
    let data = summarize(trace.since(0), Some(1.0));
    assert!(data["generation_sec"].is_null());
    assert!(!data.to_string().contains("PRIVATE"));
}

#[test]
fn upstream_timeout_is_an_error_with_unknown_generation() {
    let (server, url) = upstream();
    let worker = std::thread::spawn(move || {
        let request = server
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap();
        std::thread::sleep(Duration::from_millis(250));
        let _ = request.respond(Response::empty(200));
    });
    let trace = Trace::start(&url, Duration::from_millis(50)).unwrap();
    assert!(matches!(
        ureq::post(&post_url(&trace)).send_json(json!({"messages":[]})),
        Err(ureq::Error::Status(502, _))
    ));
    let data = summarize(trace.since(0), Some(1.0));
    assert_eq!(data["requests"][0]["status"], "http_error");
    assert!(data["generation_sec"].is_null());
    worker.join().unwrap();
}

#[test]
fn stopping_the_proxy_does_not_wait_for_the_external_provider() {
    let (server, url) = upstream();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let backend = std::thread::spawn(move || {
        let request = server
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap();
        ready_tx.send(()).unwrap();
        let _ = release_rx.recv_timeout(Duration::from_secs(3));
        let _ = request.respond(Response::from_string("{}"));
    });
    let trace = Trace::start(&url, Duration::from_secs(3)).unwrap();
    let request_url = post_url(&trace);
    let caller = std::thread::spawn(move || {
        ureq::post(&request_url)
            .send_json(json!({"messages":[]}))
            .is_ok()
    });
    ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let data = summarize(trace.since(0), Some(0.1));
    assert_eq!(data["requests"][0]["status"], "pending");
    assert!(data["generation_sec"].is_null());
    let started = Instant::now();
    drop(trace);
    assert!(started.elapsed() < Duration::from_secs(1));
    let _ = release_tx.send(());
    backend.join().unwrap();
    assert!(caller.join().unwrap());
}

#[test]
fn redirects_are_not_followed_or_exposed_to_the_worker() {
    let (server, url) = upstream();
    let (destination, destination_url) = upstream();
    let backend = std::thread::spawn(move || {
        let request = server
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap();
        request
            .respond(
                Response::empty(302)
                    .with_header(Header::from_bytes("Location", destination_url).unwrap()),
            )
            .unwrap();
    });
    let trace = Trace::start(&url, Duration::from_secs(3)).unwrap();
    let result = ureq::AgentBuilder::new()
        .redirects(0)
        .build()
        .post(&post_url(&trace))
        .send_json(json!({"messages":[]}));
    let response = match result {
        Ok(response) | Err(ureq::Error::Status(_, response)) => response,
        Err(error) => panic!("{error}"),
    };
    assert_eq!(response.status(), 302);
    assert!(response.header("Location").is_none());
    assert!(destination
        .recv_timeout(Duration::from_millis(100))
        .unwrap()
        .is_none());
    backend.join().unwrap();
}

#[test]
fn trace_mode_redacts_diagnostics_and_error_notes_from_the_entire_report() {
    let mut phase = json!({"status":"failed","diagnostic_tail":"PRIVATE ANSWER",
        "verification_error":"PRIVATE SOURCE","error":"PRIVATE ERROR","llm_trace":{"request_count":1}});
    redact_phase(&mut phase);
    assert!(!phase.to_string().contains("PRIVATE"));
    assert_eq!(phase["status"], "failed");
    let mut timeline = json!({"stages":[{"stage":"summary","note":"PRIVATE PROMPT",
        "artifact":{"reason":"PRIVATE ANSWER"}}],"operations":[
        {"label":"PRIVATE ANSWER","elapsed_sec":10.0},
        {"label":"Загрузка модели проверки имён","elapsed_sec":1.0}]});
    redact_timeline(&mut timeline);
    assert!(!timeline.to_string().contains("PRIVATE"));
    assert_eq!(timeline["operations"][0]["elapsed_sec"], 10.0);
    assert_eq!(timeline["operations"][1]["category"], "ner_model_load");
}
