use localvox_light_client::{AskRequest, Client};
use serde_json::json;
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};
use tiny_http::{Header, Response, Server};

#[test]
fn mutation_scope_encoding_and_authorization_are_preserved() {
    let server = Server::http("127.0.0.1:0").unwrap();
    let address = format!("http://{}", server.server_addr());
    let thread = std::thread::spawn(move || {
        let mut r = server
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap();
        assert_eq!(r.method().as_str(), "POST");
        assert_eq!(r.url(), "/api/sessions/demo/recook?scope=text");
        assert!(r
            .headers()
            .iter()
            .any(|h| h.field.equiv("Authorization") && h.value.as_str() == "Bearer synthetic"));
        let mut body = String::new();
        r.as_reader().read_to_string(&mut body).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap(),
            json!({})
        );
        r.respond(
            Response::from_string("{\"msg\":\"queued\"}")
                .with_header(Header::from_bytes("Content-Type", "application/json").unwrap()),
        )
        .unwrap();
    });
    let c = Client::new(&address, "synthetic".into()).unwrap();
    assert_eq!(
        c.recook("demo", localvox_light_client::Scope::Text)
            .unwrap()
            .msg
            .as_deref(),
        Some("queued")
    );
    thread.join().unwrap();
}

#[test]
fn lost_ack_never_replays_a_mutation() {
    assert_lost_ack(|c| c.note("text", "ideas").map(|_| ()));
}

#[test]
fn shutdown_lost_ack_never_retries_or_bootstraps() {
    assert_lost_ack(Client::shutdown_application);
}

fn assert_lost_ack(call: fn(&Client) -> anyhow::Result<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let (done, finished) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = [0; 4096];
        let n = stream.read(&mut request).unwrap();
        assert!(std::str::from_utf8(&request[..n])
            .unwrap()
            .starts_with("POST "));
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n{")
            .unwrap();
        drop(stream);
        // Keep listening until the client has actually returned. Checking only
        // immediately after disconnect could miss a delayed transport retry.
        finished.recv_timeout(Duration::from_secs(20)).unwrap();
        listener.set_nonblocking(true).unwrap();
        assert!(listener.accept().is_err());
    });
    let c = Client::new(&address, String::new()).unwrap();
    assert!(call(&c).is_err());
    done.send(()).unwrap();
    thread.join().unwrap();
}

#[test]
fn shutdown_requires_completed_ack_and_sends_explicit_authorized_post() {
    for (reply, expected) in [
        ("{\"completed\":true}", true),
        ("{\"completed\":false}", false),
        ("{}", false),
        ("{\"completed\":\"true\"}", false),
    ] {
        let server = Server::http("127.0.0.1:0").unwrap();
        let client = Client::new(
            &format!("http://{}", server.server_addr()),
            "synthetic".into(),
        )
        .unwrap();
        let thread = std::thread::spawn(move || {
            let mut request = server
                .recv_timeout(Duration::from_secs(3))
                .unwrap()
                .unwrap();
            assert_eq!(request.method().as_str(), "POST");
            assert_eq!(request.url(), "/api/application/shutdown");
            assert!(request
                .headers()
                .iter()
                .any(|h| h.field.equiv("Authorization") && h.value.as_str() == "Bearer synthetic"));
            let mut body = String::new();
            request.as_reader().read_to_string(&mut body).unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&body).unwrap(),
                json!({"confirm":true})
            );
            request.respond(Response::from_string(reply)).unwrap();
        });
        assert_eq!(client.shutdown_application().is_ok(), expected);
        thread.join().unwrap();
    }
}

#[test]
fn provider_is_required_before_network() {
    let c = Client::new("http://127.0.0.1:1", String::new()).unwrap();
    assert!(c
        .create_ask(&AskRequest {
            text: "x".into(),
            provider: "".into(),
            prompt: String::new(),
            input_name: None
        })
        .unwrap_err()
        .to_string()
        .contains("явно"));
}

#[test]
fn encoded_body_limit_is_checked_before_network() {
    let client = Client::new("http://127.0.0.1:1", String::new()).unwrap();
    let error = client
        .note(&"я".repeat(600_000), "идеи")
        .unwrap_err()
        .to_string();
    assert!(error.contains("HTTP-лимит 1 MiB"));
}

#[test]
fn shared_fixture_decodes_documents_and_hides_unknown_settings() {
    let v: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/workspace.json")).unwrap();
    let sessions: Vec<localvox_light_client::Session> =
        serde_json::from_value(v["sessions"].clone()).unwrap();
    assert_eq!(sessions[0].name, "20261031_demo");
    let text: localvox_light_client::ReadableDoc =
        serde_json::from_value(v["readable"].clone()).unwrap();
    assert_eq!(text.version_id, 1);
    assert!(text.lines[0].original.is_some());
    let record: localvox_light_client::RecordState =
        serde_json::from_value(v["record"].clone()).unwrap();
    assert!(record.engine);
    assert!(!record.recording);
    let journal: localvox_light_client::JournalPage =
        serde_json::from_str(include_str!("../../../tests/fixtures/journal.json")).unwrap();
    assert_eq!(journal.events.len(), 36);
    assert_eq!(
        journal.events[0].command.as_ref().unwrap().args[0]
            .value
            .as_deref(),
        Some("summary")
    );
}

#[test]
fn status_failures_are_visible_and_never_echo_the_access_token() {
    let server = Server::http("127.0.0.1:0").unwrap();
    let address = format!("http://{}", server.server_addr());
    let thread = std::thread::spawn(move || {
        for code in [401, 403, 503] {
            let r = server
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .unwrap();
            r.respond(
                Response::from_string("{\"error\":\"bad synthetic-token\"}").with_status_code(code),
            )
            .unwrap();
        }
    });
    let c = Client::new(&address, "synthetic-token".into()).unwrap();
    for code in [401, 403, 503] {
        let e = c.record().unwrap_err().to_string();
        assert!(e.contains(&code.to_string()));
        assert!(!e.contains("synthetic-token"));
    }
    thread.join().unwrap();
}

#[test]
fn timeout_does_not_turn_into_an_empty_record() {
    let server = Server::http("127.0.0.1:0").unwrap();
    let address = format!("http://{}", server.server_addr());
    let (done, release) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        let r = server
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        release.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(r);
    });
    let c = Client::new(&address, String::new()).unwrap();
    let result = c.request::<localvox_light_client::RecordState>(
        "GET",
        &["api", "record"],
        &[],
        None,
        Duration::from_millis(50),
    );
    assert!(result.is_err());
    done.send(()).unwrap();
    thread.join().unwrap();
}

#[test]
fn redirects_cannot_forward_authorization_to_another_origin() {
    let target = Server::http("127.0.0.1:0").unwrap();
    let server = Server::http("127.0.0.1:0").unwrap();
    let address = format!("http://{}", server.server_addr());
    let location = format!("http://{}/api/record", target.server_addr());
    let thread = std::thread::spawn(move || {
        let r = server
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        r.respond(
            Response::empty(302).with_header(Header::from_bytes("Location", location).unwrap()),
        )
        .unwrap();
    });
    let c = Client::new(&address, "synthetic-token".into()).unwrap();
    assert!(c.record().is_err());
    thread.join().unwrap();
    assert!(target.try_recv().unwrap().is_none());
}
