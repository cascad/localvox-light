//! Real main -> sibling client, with a private HTTP fixture and ConPTY.
#![cfg(all(windows, feature = "tui"))]
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
use tiny_http::{Response, Server};

#[test]
#[ignore = "requires built daemon/TUI bundle; no capture or real archive is used"]
fn explicit_and_interactive_launch_delegate_before_models_and_never_stop_the_daemon() {
    let main = Path::new(env!("CARGO_BIN_EXE_localvox-light"));
    let binary_dir = main.parent().unwrap();
    let tui = binary_dir.join("localvox-tui.exe");
    assert!(
        tui.is_file(),
        "build localvox-light-terminal before the bundle test"
    );
    for explicit in [true, false] {
        let directory = tempfile::tempdir().unwrap();
        fs::copy(main, directory.path().join("localvox-light.exe")).unwrap();
        fs::copy(&tui, directory.path().join("localvox-tui.exe")).unwrap();
        for name in [
            "libvosk.dll",
            "libgcc_s_seh-1.dll",
            "libstdc++-6.dll",
            "libwinpthread-1.dll",
        ] {
            let file = binary_dir.join(name);
            if file.is_file() {
                fs::copy(file, directory.path().join(name)).unwrap();
            }
        }
        let server = Server::http("127.0.0.1:0").unwrap();
        let address = format!("http://{}", server.server_addr());
        fs::write(
            directory.path().join(".env"),
            format!("LOCALVOX_TUI_URL={address}\nLOCALVOX_API_TOKEN=synthetic-launch-token\n"),
        )
        .unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let mutations = Arc::new(AtomicUsize::new(0));
        let mutated = mutations.clone();
        let thread = std::thread::spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                if let Some(request) = server.recv_timeout(Duration::from_millis(20)).unwrap() {
                    if request.method().as_str() != "GET" {
                        mutated.fetch_add(1, Ordering::Relaxed);
                    }
                    let payload = match request.url().split('?').next().unwrap() {
                        "/api/health" => {
                            serde_json::json!({"ok":true,"service":"localvox-light","version":"fixture","api_protocol":1,"capabilities":["technical-journal-v1","terminal-workspace-v1"]})
                        }
                        "/api/sessions" => {
                            serde_json::json!([{"name":"private","title":"CLIENT_ONLY_ARCHIVE","duration_sec":0,"cooked":true}])
                        }
                        "/api/record" => {
                            serde_json::json!({"engine":true,"recording":true,"session":"private"})
                        }
                        "/api/jobs" => serde_json::json!({"running":0,"pending":0,"failed":0}),
                        p if p.ends_with("/summary") => {
                            serde_json::json!({"blocks":[{"kind":"heading","text":"CLIENT_ONLY_ARCHIVE","sources":[]}]})
                        }
                        _ => serde_json::json!({}),
                    };
                    let _ = request.respond(Response::from_string(payload.to_string()));
                }
            }
        });
        struct Api(Arc<AtomicBool>, Option<std::thread::JoinHandle<()>>);
        impl Drop for Api {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Relaxed);
                let _ = self.1.take().unwrap().join();
            }
        }
        let _api = Api(stop, Some(thread));
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 36,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = CommandBuilder::new(directory.path().join("localvox-light.exe"));
        command.cwd(directory.path());
        command.args([
            "--model",
            "this-model-must-not-be-opened",
            "--audio-dir",
            "must-not-be-created",
        ]);
        command.env_remove("LOCALVOX_LIGHT_TUI");
        command.env_remove("LOCALVOX_LIGHT_NO_TUI");
        if explicit {
            command.arg("--tui");
        }
        struct Child(Box<dyn portable_pty::Child + Send + Sync>);
        impl Drop for Child {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut child = Child(pair.slave.spawn_command(command).unwrap());
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let mut writer = pair.master.take_writer().unwrap();
        let (tx, rx) = mpsc::sync_channel(32);
        let output = std::thread::spawn(move || {
            let mut buffer = [0; 8192];
            while let Ok(n) = reader.read(&mut buffer) {
                if n == 0 || tx.send(buffer[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        let mut text = String::new();
        let mut queries = 0;
        let mut quitting = false;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let bytes = rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("launcher must reach the client and restore the terminal");
            text.push_str(&String::from_utf8_lossy(&bytes));
            assert!(text.len() < 1024 * 1024);
            let next = text.matches("\x1b[6n").count();
            while queries < next {
                writer.write_all(b"\x1b[1;1R").unwrap();
                writer.flush().unwrap();
                queries += 1;
            }
            if !quitting && text.contains("CLIENT_ONLY_ARCHIVE") {
                writer.write_all(b"\x11").unwrap();
                writer.flush().unwrap();
                quitting = true;
            }
            if quitting && text.contains("\x1b[?1049l") {
                break;
            }
        }
        assert!(child.0.wait().unwrap().success());
        drop(writer);
        drop(pair.master);
        drop(rx);
        output.join().unwrap();
        assert_eq!(
            mutations.load(Ordering::Relaxed),
            0,
            "closing either launcher must not stop a recording"
        );
        assert!(!directory.path().join("must-not-be-created").exists());
        assert!(!text.contains("synthetic-launch-token"));
    }
}
