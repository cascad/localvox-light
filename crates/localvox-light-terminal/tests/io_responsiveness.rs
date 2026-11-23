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

#[test]
fn a_delayed_delete_keeps_local_navigation_and_recording_control_responsive() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use localvox_light_client::{Session, SlotNotes, StoredNote};
    use localvox_light_terminal::app::{App, View};
    for note in [false, true] {
        let server = Server::http("127.0.0.1:0").unwrap();
        let address = format!("http://{}", server.server_addr());
        let (started_tx, started) = mpsc::channel();
        let (release_tx, release) = mpsc::channel();
        let host = std::thread::spawn(move || {
            let mut held = Some(release);
            let mut workers = vec![];
            for _ in 0..2 {
                let mut request = server
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
                    .unwrap();
                let deleting = request.method().as_str() == "DELETE";
                let release = if deleting { held.take() } else { None };
                let started = started_tx.clone();
                workers.push(std::thread::spawn(move|| {
                    let body=if deleting {
                        let mut body=String::new();request.as_reader().read_to_string(&mut body).unwrap();
                        let body:serde_json::Value=serde_json::from_str(&body).unwrap();
                        if note {
                            assert_eq!(request.url(),"/api/slots/notes");
                            assert_eq!(body,serde_json::json!({"slot":"идеи","raw":"- selected","source":"selected.md"}));
                        } else {
                            assert_eq!(request.url(),"/api/sessions/selected");
                            assert_eq!(body,serde_json::json!({"confirm":"selected"}));
                        }
                        started.send(()).unwrap();release.unwrap().recv_timeout(Duration::from_secs(6)).unwrap();
                        serde_json::json!({"ok":true,"deleted":if note {"- selected"} else {"selected"}}).to_string()
                    } else {
                        assert_eq!(request.url(),"/api/record/stop");"{\"ok\":true}".into()
                    };
                    request.respond(Response::from_string(body).with_header(Header::from_bytes("Content-Type","application/json").unwrap())).unwrap();
                }));
            }
            for worker in workers {
                worker.join().unwrap();
            }
        });
        let mut app = App::default();
        app.focus = 1;
        app.record.recording = true;
        if note {
            app.view = View::Notes;
            app.notes = vec![SlotNotes {
                name: "идеи".into(),
                readable: true,
                notes: vec![StoredNote {
                    text: "selected".into(),
                    raw: "- selected".into(),
                    source: "selected.md".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }];
        } else {
            app.sessions = vec![Session {
                name: "selected".into(),
                title: Some("Small recording".into()),
                ..Default::default()
            }];
        }
        app.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(app.form.is_some() && app.effects.is_empty());
        app.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        let mut io = Io::new(Client::new(&address, String::new()).unwrap()).unwrap();
        for (resource, task) in app.effects.drain(..) {
            assert!(io.submit(app.epoch, resource, task));
        }
        started.recv_timeout(Duration::from_secs(3)).unwrap();
        let focus = app.focus;
        app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_ne!(app.focus, focus);
        // The runtime handles reserved F8 before App::key and dispatches this action.
        app.command(localvox_light_terminal::app::Action::StopRecording);
        for (resource, task) in app.effects.drain(..) {
            assert!(io.submit(app.epoch, resource, task));
        }
        let response = io.rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(response.resource, Resource::Control);
        assert!(response.result.is_ok());
        release_tx.send(()).unwrap();
        let response = io.rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(response.resource, Resource::Action);
        app.apply(response);
        if note {
            assert!(app.notes[0].notes.is_empty());
        } else {
            assert!(app.sessions.is_empty());
        }
        host.join().unwrap();
    }
}
