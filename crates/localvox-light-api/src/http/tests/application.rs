use super::*;
use crate::application::ApplicationControl;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

fn request(
    root: &Path,
    control: Option<Arc<ApplicationControl>>,
    token: Option<&str>,
    authorization: Option<&str>,
    origin: Option<&str>,
    path: &str,
    body: &str,
) -> (u16, Value) {
    let server = bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}{}", server.server_addr(), path);
    let archive = Archive::new(root.to_path_buf());
    let cfg = HttpConfig {
        bind: "127.0.0.1:0".into(),
        token: token.map(str::to_owned),
    };
    let thread = std::thread::spawn(move || {
        let r = server
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap();
        handle_request(&archive, &cfg, control.as_ref(), r);
    });
    let mut req = ureq::post(&address).timeout(Duration::from_secs(4));
    if let Some(auth) = authorization {
        req = req.set("Authorization", auth);
    }
    if let Some(origin) = origin {
        req = req.set("Origin", origin);
    }
    let response = match req.send_string(body) {
        Ok(r) | Err(ureq::Error::Status(_, r)) => r,
        Err(e) => panic!("{e}"),
    };
    let code = response.status();
    let json = response.into_json().unwrap();
    thread.join().unwrap();
    (code, json)
}

#[test]
fn shutdown_requires_normal_auth_origin_confirmation_and_runtime_owner() {
    let root = tempfile::tempdir().unwrap();
    let running = Arc::new(AtomicBool::new(true));
    let control = ApplicationControl::new(running.clone());
    let path = "/api/application/shutdown";
    for (token, auth, origin, body, expected) in [
        (Some("secret"), None, None, "{\"confirm\":true}", 401),
        (
            None,
            None,
            Some("https://foreign.test"),
            "{\"confirm\":true}",
            403,
        ),
        (None, None, None, "{}", 400),
        (None, None, None, "{\"confirm\":\"true\"}", 400),
        (None, None, None, "{broken", 400),
    ] {
        assert_eq!(
            request(
                root.path(),
                Some(control.clone()),
                token,
                auth,
                origin,
                path,
                body
            )
            .0,
            expected
        );
        assert!(running.load(Ordering::SeqCst));
    }
    assert_eq!(
        request(
            root.path(),
            None,
            None,
            None,
            None,
            path,
            "{\"confirm\":true}"
        )
        .0,
        501
    );
    assert_eq!(
        request(
            root.path(),
            Some(control),
            None,
            None,
            None,
            path,
            &"x".repeat(MAX_BODY_BYTES + 1)
        )
        .0,
        413
    );
    assert!(running.load(Ordering::SeqCst));
    assert!(std::fs::read_dir(root.path()).unwrap().next().is_none());
}

#[test]
fn shutdown_http_waits_for_owner_result_and_rejects_new_mutations() {
    for outcome in [Ok(()), Err("engine: final chunk write failed".into())] {
        let root = tempfile::tempdir().unwrap();
        let running = Arc::new(AtomicBool::new(true));
        let control = ApplicationControl::new(running.clone());
        let owner = control.clone();
        let recording = root.path().join("final-chunk");
        let saved = recording.clone();
        let expected = outcome.clone();
        let thread = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            while running.load(Ordering::SeqCst) {
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
            // An isolated owner fixture, not a real capture engine.
            std::fs::write(saved, b"closed").unwrap();
            owner.complete_and_drain(outcome, Duration::from_secs(2));
        });
        let (code, result) = request(
            root.path(),
            Some(control.clone()),
            Some("secret"),
            Some("Bearer secret"),
            None,
            "/api/application/shutdown",
            "{\"confirm\":true}",
        );
        assert_eq!(std::fs::read(recording).unwrap(), b"closed");
        if expected.is_ok() {
            assert_eq!(code, 200);
            assert_eq!(result, json!({"completed":true}));
        } else {
            assert_eq!(code, 500);
            assert!(result["error"].as_str().unwrap().contains("final chunk"));
        }
        thread.join().unwrap();
        assert_eq!(
            request(
                root.path(),
                Some(control.clone()),
                None,
                None,
                None,
                "/api/application/shutdown",
                "{\"confirm\":true}"
            )
            .0,
            409
        );
        assert_eq!(
            request(
                root.path(),
                Some(control),
                None,
                None,
                None,
                "/api/record/start",
                "{\"title\":\"must not start\"}"
            )
            .0,
            503
        );
        let files: Vec<_> = std::fs::read_dir(root.path())
            .unwrap()
            .map(|p| p.unwrap().file_name())
            .collect();
        assert_eq!(files, [std::ffi::OsString::from("final-chunk")]);
    }
}
