//! The desktop shell: a window onto the local daemon.
//!
//! It owns NO state. The recording, the archive and the API belong to the daemon
//! (`localvox-light`), which lives in the tray and keeps running whether this window is open or
//! not. Closing the window must never stop a recording — that is the whole reason the shell is a
//! separate process rather than the app itself.
//!
//! The window loads `http://127.0.0.1:<port>` — the very same page the browser and the phone on
//! the LAN get. One frontend, one origin, no CORS, and no branch anywhere saying "but in the
//! desktop it works differently". Which is also why swapping this shell for another one costs
//! nothing: the seam is the HTTP API, not the window.

use std::net::TcpStream;
use std::time::Duration;

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// Where the daemon listens if nobody told us otherwise. The tray — which is what normally opens
/// this window — passes the address it actually bound to; this is the fallback for a double-click
/// on the exe.
const DEFAULT_ADDR: &str = "127.0.0.1:3017";

/// How long we wait for the daemon. It opens audio devices and takes the work-directory lock; a
/// couple of seconds is normal, fifteen means something is wrong — and then the window may as
/// well open and say so.
const STARTUP_WAIT: Duration = Duration::from_secs(15);

/// `--addr 127.0.0.1:3017`, the address the daemon reported. Nothing else is parsed: this process
/// has no other business.
fn addr_from_args() -> String {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--addr" {
            if let Some(v) = args.next() {
                return v;
            }
        }
    }
    std::env::var("LOCALVOX_API_BIND").unwrap_or_else(|_| DEFAULT_ADDR.into())
}

/// A listening socket is the honest question. "Is the daemon alive" asked of an HTTP endpoint
/// needs a client, a timeout and error handling; a TCP connect answers the same thing and cannot
/// be wrong about it.
fn listening(addr: &str) -> bool {
    let Ok(resolved) = std::net::ToSocketAddrs::to_socket_addrs(addr) else {
        return false;
    };
    resolved
        .into_iter()
        .any(|a| TcpStream::connect_timeout(&a, Duration::from_millis(300)).is_ok())
}

fn daemon_exe() -> &'static str {
    if cfg!(windows) {
        "localvox-light.exe"
    } else {
        "localvox-light"
    }
}

/// Opening the app must make it work.
///
/// If the daemon is not up (someone double-clicked the shell instead of using the tray), we start
/// it — it lives next to us in the same distribution. Two daemons cannot happen: it takes a lock
/// on the work directory, and the second one exits.
fn ensure_daemon(addr: &str) -> bool {
    if listening(addr) {
        return true;
    }
    if let Some(daemon) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join(daemon_exe())))
        .filter(|p| p.exists())
    {
        let _ = std::process::Command::new(daemon).arg("--tray").spawn();
    }
    let deadline = std::time::Instant::now() + STARTUP_WAIT;
    while std::time::Instant::now() < deadline {
        if listening(addr) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    false
}

/// Shown instead of the app when the daemon never answered. A browser's "connection refused" page
/// tells a person nothing they can act on.
fn no_daemon_page(addr: &str) -> String {
    format!(
        "data:text/html;charset=utf-8,\
<style>body{{font:15px/1.6 system-ui;margin:4rem auto;max-width:34rem;padding:0 1rem;\
color:%23e3e8ee;background:%2315181d}}code{{background:%2321262d;padding:.15rem .4rem;\
border-radius:4px}}</style>\
<h2>Движок не отвечает</h2>\
<p>Это окно — только вид на демон localvox, а он не слушает <code>{addr}</code>.</p>\
<p>Запустите его и откройте интерфейс из трея:</p>\
<p><code>localvox-light.exe --tray</code></p>"
    )
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let addr = addr_from_args();

    tauri::Builder::default()
        // The tray item can be clicked twice. A second window onto the same archive is not a
        // feature — it is two views of one truth, drifting apart. We focus the open one.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .setup(move |app| {
            let alive = ensure_daemon(&addr);
            let url = if alive {
                WebviewUrl::External(format!("http://{addr}/").parse()?)
            } else {
                WebviewUrl::External(no_daemon_page(&addr).parse()?)
            };
            WebviewWindowBuilder::new(app, "main", url)
                .title("localvox")
                .inner_size(1360.0, 860.0)
                // Below this the three panes stop being three panes.
                .min_inner_size(900.0, 560.0)
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("не удалось запустить окно localvox");
}
