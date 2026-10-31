use super::*;
use serde_json::json;
use std::{
    fs,
    net::TcpListener,
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};
use tiny_http::{Response, Server};

fn health() -> String {
    json!({"ok":true,"service":"localvox-light","version":"fixture","api_protocol":1,
        "capabilities":["technical-journal-v1","terminal-workspace-v1"]})
    .to_string()
}
fn fixture(status: u16, body: String) -> (Client, std::thread::JoinHandle<()>) {
    let server = Server::http("127.0.0.1:0").unwrap();
    let client = Client::new(
        &format!("http://{}", server.server_addr()),
        "synthetic-token".into(),
    )
    .unwrap();
    let thread = std::thread::spawn(move || {
        let request = server
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap();
        assert_eq!(
            request.url(),
            "/api/health",
            "no workspace material before compatibility check"
        );
        assert_eq!(request.method().as_str(), "GET");
        request
            .respond(Response::from_string(body).with_status_code(status))
            .unwrap();
    });
    (client, thread)
}

#[test]
fn existing_daemon_is_verified_without_starting_another_process() {
    let (client, server) = fixture(200, health());
    ensure(&client, true, Duration::from_secs(2), || {
        panic!("already running")
    })
    .unwrap();
    server.join().unwrap();
}

#[test]
fn foreign_old_unauthorized_and_forbidden_endpoints_never_start_a_daemon() {
    for (status, body) in [
        (200, "{\"ok\":true,\"version\":\"old\"}".into()),
        (200, health().replace("terminal-workspace-v1", "unknown")),
        (200, health().replace("localvox-light", "another-service")),
        (
            200,
            health().replace("\"api_protocol\":1", "\"api_protocol\":99"),
        ),
        (401, "{\"error\":\"unauthorized\"}".into()),
        (403, "{\"error\":\"forbidden\"}".into()),
    ] {
        let (client, server) = fixture(status, body);
        assert!(ensure(&client, true, Duration::from_secs(2), || panic!(
            "do not start over a response"
        ))
        .is_err());
        server.join().unwrap();
    }
}

#[test]
fn remote_addresses_and_https_never_authorize_local_startup() {
    for url in [
        "http://192.0.2.1:9",
        "http://example.invalid",
        "https://127.0.0.1:9",
    ] {
        let client = Client::new(url, String::new()).unwrap();
        assert!(client.local_http_bind().is_none());
    }
    let client = unused_client();
    assert!(ensure(&client, false, Duration::from_millis(20), || panic!(
        "connect only"
    ))
    .is_err());
}

fn unused_client() -> Client {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    Client::new(&format!("http://{address}"), String::new()).unwrap()
}
fn helper(client: &Client, marker: &Path, mode: &str) -> Child {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "launcher::tests::daemon_fixture",
            "--nocapture",
        ])
        .env(
            "LOCALVOX_LAUNCHER_FIXTURE",
            json!([client.address(), marker, mode]).to_string(),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command.spawn().unwrap()
}
fn wait_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.is_file() {
        assert!(Instant::now() < deadline, "owned fixture failed to exit");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_single_owned_start_becomes_ready_and_outlives_the_client() {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("alive");
    fs::write(&marker, "keep fixture alive").unwrap();
    let client = unused_client();
    let starts = AtomicUsize::new(0);
    // Windows connection refusal alone can consume over a second; exercise
    // successful startup with the same finite budget as the real launcher.
    ensure(&client, true, START_TIMEOUT, || {
        starts.fetch_add(1, Ordering::Relaxed);
        Ok(helper(&client, &marker, "ready"))
    })
    .unwrap();
    assert_eq!(starts.load(Ordering::Relaxed), 1);
    drop(client);
    assert!(
        !marker.with_extension("exited").exists(),
        "closing the client must not kill its daemon"
    );
    fs::remove_file(&marker).unwrap();
    wait_file(&marker.with_extension("exited"));
}

#[test]
fn failed_and_timed_out_starts_are_bounded_and_never_retried() {
    for mode in ["failure", "timeout"] {
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("fixture");
        let client = unused_client();
        let starts = AtomicUsize::new(0);
        let start = Instant::now();
        let error = ensure(&client, true, Duration::from_millis(3500), || {
            starts.fetch_add(1, Ordering::Relaxed);
            Ok(helper(&client, &marker, mode))
        })
        .unwrap_err();
        assert_eq!(starts.load(Ordering::Relaxed), 1);
        assert!(start.elapsed() < Duration::from_secs(5));
        assert!(error.to_string().contains(if mode == "failure" {
            "завершился"
        } else {
            "не стал готов"
        }));
        wait_file(&marker.with_extension("exited"));
    }
}

#[test]
#[ignore = "bounded daemon child fixture invoked by launcher process tests"]
fn daemon_fixture() {
    let Ok(config) = std::env::var("LOCALVOX_LAUNCHER_FIXTURE") else {
        return;
    };
    let [address, marker, mode]: [String; 3] = serde_json::from_str(&config).unwrap();
    let marker = Path::new(&marker);
    match mode.as_str() {
        "failure" => {
            fs::write(marker.with_extension("exited"), "exit 7").unwrap();
            std::process::exit(7);
        }
        "timeout" => std::thread::sleep(Duration::from_secs(4)),
        "ready" => {
            let server =
                Server::http(address.trim_start_matches("http://").trim_end_matches('/')).unwrap();
            let deadline = Instant::now() + START_TIMEOUT + Duration::from_secs(2);
            while marker.exists() && Instant::now() < deadline {
                if let Some(request) = server.recv_timeout(Duration::from_millis(20)).unwrap() {
                    assert_eq!(request.url(), "/api/health");
                    request.respond(Response::from_string(health())).unwrap();
                }
            }
        }
        _ => panic!("unknown fixture"),
    }
    fs::write(marker.with_extension("exited"), "clean exit").unwrap();
}
