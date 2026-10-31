use localvox_light_client::Client;
use localvox_light_terminal::io::{Io, Resource, Task};
use std::{
    collections::{BTreeMap, HashSet},
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};
use tiny_http::{Header, Response, Server};

fn exercise_contention(block_document: bool) {
    let server = Server::http("127.0.0.1:0").unwrap();
    let address = format!("http://{}", server.server_addr());
    let (started_tx, started) = mpsc::channel();
    let (release_tx, release) = mpsc::channel();
    let release = Arc::new(Mutex::new(release));
    let server_thread = std::thread::spawn(move || {
        let mut requests = vec![];
        for _ in 0..if block_document { 5 } else { 4 } {
            let mut request = server
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap();
            let mut body = Vec::new();
            request.as_reader().read_to_end(&mut body).unwrap();
            let started = started_tx.clone();
            let release = release.clone();
            requests.push(std::thread::spawn(move || {
                let slow = matches!(request.url(), "/api/ask" | "/api/route")
                    || (block_document && request.url().ends_with("/summary"));
                if slow {
                    started.send(()).unwrap();
                    release
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(6))
                        .unwrap();
                }
                let body = match request.url() {
                    "/api/record/stop" => "{\"session\":\"demo\"}",
                    "/api/settings" => "{\"saved\":0}",
                    "/api/ask" => "{\"text\":\"answer\",\"sources\":[]}",
                    "/api/route" => "{\"suggestions\":[]}",
                    _ => "{\"blocks\":[]}",
                };
                request
                    .respond(Response::from_string(body).with_header(
                        Header::from_bytes("Content-Type", "application/json").unwrap(),
                    ))
                    .unwrap();
            }));
        }
        for request in requests {
            request.join().unwrap();
        }
    });
    let mut io = Io::new(Client::new(&address, String::new()).unwrap()).unwrap();
    assert!(io.submit(1, Resource::Chat, Task::Chat("question".into())));
    assert!(io.submit(0, Resource::Route, Task::Route("note".into())));
    if block_document {
        assert!(io.submit(
            1,
            Resource::Summary,
            Task::Read(Resource::Summary, "demo".into())
        ));
    }
    for _ in 0..if block_document { 3 } else { 2 } {
        started.recv_timeout(Duration::from_secs(4)).unwrap();
    }
    assert!(io.submit(0, Resource::Control, Task::Stop));
    let other = if block_document {
        assert!(io.submit(0, Resource::Action, Task::SaveSettings(BTreeMap::new())));
        Resource::Action
    } else {
        assert!(io.submit(
            1,
            Resource::Summary,
            Task::Read(Resource::Summary, "demo".into())
        ));
        Resource::Summary
    };
    let mut completed = HashSet::new();
    for _ in 0..2 {
        let response = io.rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(response.result.is_ok());
        completed.insert(response.resource);
    }
    assert_eq!(
        completed,
        HashSet::from([Resource::Control, other]),
        "independent lanes finish before long requests are released"
    );
    for _ in 0..if block_document { 3 } else { 2 } {
        release_tx.send(()).unwrap();
    }
    for _ in 0..if block_document { 3 } else { 2 } {
        assert!(io
            .rx
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .result
            .is_ok());
    }
    server_thread.join().unwrap();
}
#[test]
fn blocked_models_do_not_block_documents_or_stop() {
    exercise_contention(false);
}
#[test]
fn blocked_documents_and_models_do_not_block_control_or_save() {
    exercise_contention(true);
}
