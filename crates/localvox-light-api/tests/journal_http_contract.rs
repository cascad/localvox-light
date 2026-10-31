//! Real loopback HTTP + the actual archive/router, no models or user's archive.
use localvox_light_api::{
    archive::Archive,
    http::{respond, HttpConfig},
};
use localvox_light_client::Client;
use localvox_light_core::journal::{self, Entry};
use std::time::Duration;
use tiny_http::{Header, Response, Server};

#[test]
fn client_reads_journal_and_existing_endpoints_through_http() {
    let temp = tempfile::tempdir().unwrap();
    let session = temp.path().join("sessions/demo");
    std::fs::create_dir_all(&session).unwrap();
    journal::append(
        &session,
        Entry {
            kind: "operation_start".into(),
            label: "summary".into(),
            stage: "summary".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let server = Server::http("127.0.0.1:0").unwrap();
    let address = format!("http://{}", server.server_addr());
    let archive = Archive::new(temp.path().into());
    let thread = std::thread::spawn(move || {
        for _ in 0..6 {
            let request = server
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .expect("bounded HTTP test request");
            let token = request
                .headers()
                .iter()
                .find(|h| h.field.equiv("Authorization"))
                .map(|h| h.value.as_str());
            let (status, body) = respond(
                &archive,
                &HttpConfig {
                    bind: "127.0.0.1:0".into(),
                    token: Some("synthetic-token".into()),
                },
                request.method(),
                request.url(),
                token,
                true,
                "",
            );
            request
                .respond(
                    Response::from_string(body.to_string())
                        .with_status_code(status)
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        ),
                )
                .unwrap();
        }
    });
    let client = Client::new(&address, "synthetic-token".into()).unwrap();
    assert!(client.health().unwrap().ok);
    assert!(!client.record().unwrap().engine);
    let page = client.journal("demo", None, None).unwrap();
    assert_eq!(page.events[0].label, "summary");
    assert!(client
        .journal("demo", Some(&page.next_cursor), None)
        .unwrap()
        .events
        .is_empty());
    assert!(client.progress("demo").is_ok());
    assert!(Client::new(&address, String::new())
        .unwrap()
        .journal("demo", None, None)
        .unwrap_err()
        .to_string()
        .contains("401"));
    thread.join().unwrap();
}
