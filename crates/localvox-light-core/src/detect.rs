//! Auto-detection of calls/meetings (F3, WP-C4; Windows).
//!
//! The signal is the `CapabilityAccessManager\ConsentStore\microphone` registry key:
//! Windows keeps `LastUsedTimeStart/Stop` per application; `Stop == 0` means
//! "holding the microphone right now". Zoom/Teams/Discord/the browser during a call
//! are guaranteed to show up here — more reliable and cheaper than enumerating audio
//! sessions.
//!
//! On transitions we push a status to the UI and mark the meeting in the session's
//! `meta.json` (`meetings`: the application, the foreground window title at the start —
//! the future source of an auto-title, start/end times). We record continuously anyway —
//! detection gives metadata and visibility; autostart arrives with the daemon.

#![cfg(windows)]

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;

use crate::events::UiMsg;
use crate::pipeline::SessionSignal;

const POLL: Duration = Duration::from_secs(3);

/// Applications currently holding the microphone (names, lowercase), except our own
/// and the ignore list. Classic exes live under `NonPackaged\<path with #>`; Store apps
/// (Teams, Skype, …) sit directly under `microphone\<PackageFamilyName>`.
fn mic_users(ignore: &BTreeSet<String>) -> BTreeSet<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let mut out = BTreeSet::new();
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(store) = hkcu.open_subkey(
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone",
    ) else {
        return out;
    };
    let mut push = |name: String, app: &RegKey| {
        let stop: u64 = app.get_value("LastUsedTimeStop").unwrap_or(1);
        if stop == 0 && !ignore.contains(&name) {
            out.insert(name);
        }
    };
    for key_name in store.enum_keys().flatten() {
        let Ok(app) = store.open_subkey(&key_name) else {
            continue;
        };
        if key_name == "NonPackaged" {
            for exe_key in app.enum_keys().flatten() {
                let Ok(exe_app) = app.open_subkey(&exe_key) else {
                    continue;
                };
                push(exe_from_consent_key(&exe_key), &exe_app);
            }
        } else {
            // packaged: "MSTeams_8wekyb3d8bbwe" → "msteams"
            push(
                key_name
                    .split('_')
                    .next()
                    .unwrap_or(&key_name)
                    .to_lowercase(),
                &app,
            );
        }
    }
    out
}

/// The exe name out of a ConsentStore key: a path with `#` instead of `\`.
pub fn exe_from_consent_key(key: &str) -> String {
    key.rsplit('#').next().unwrap_or(key).to_lowercase()
}

/// Title and exe of the foreground window — raw material for the meeting auto-title.
pub fn foreground_window_info() -> Option<(String, String)> {
    #[link(name = "user32")]
    extern "system" {
        fn GetForegroundWindow() -> isize;
        fn GetWindowTextW(hwnd: isize, buf: *mut u16, n: i32) -> i32;
        fn GetWindowThreadProcessId(hwnd: isize, pid: *mut u32) -> u32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        fn QueryFullProcessImageNameW(h: isize, flags: u32, buf: *mut u16, size: *mut u32) -> i32;
        fn CloseHandle(h: isize) -> i32;
    }
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    // SAFETY: plain user32/kernel32 calls with local fixed-length buffers;
    // the process handle is closed on every path.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd == 0 {
            return None;
        }
        let mut tbuf = [0u16; 256];
        let n = GetWindowTextW(hwnd, tbuf.as_mut_ptr(), tbuf.len() as i32);
        if n <= 0 {
            return None;
        }
        let title = String::from_utf16_lossy(&tbuf[..n as usize]);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let mut exe = String::new();
        if pid != 0 {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h != 0 {
                let mut buf = [0u16; 512];
                let mut len = buf.len() as u32;
                if QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len) != 0 {
                    let full = String::from_utf16_lossy(&buf[..len as usize]);
                    exe = full
                        .rsplit(['\\', '/'])
                        .next()
                        .unwrap_or_default()
                        .to_lowercase();
                }
                CloseHandle(h);
            }
        }
        Some((title, exe))
    }
}

/// Does the foreground window belong to one of the calling applications? Otherwise the
/// title is about anything at all (the call may have started while the user was in
/// another window). For packaged names ("msteams") we compare by a normalized prefix
/// ("ms-teams.exe" ~ "msteams").
pub fn title_belongs_to(apps: &[String], exe: &str) -> bool {
    if exe.is_empty() {
        return false;
    }
    let norm = |s: &str| -> String {
        s.trim_end_matches(".exe")
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
            .to_lowercase()
    };
    let exe_n = norm(exe);
    if exe_n.is_empty() {
        return false;
    }
    apps.iter().any(|a| {
        a == exe || {
            let a_n = norm(a);
            !a_n.is_empty() && (a_n.starts_with(&exe_n) || exe_n.starts_with(&a_n))
        }
    })
}

/// A call state transition event.
#[derive(Debug, PartialEq)]
pub enum CallEvent {
    Started { apps: Vec<String> },
    Ended,
}

/// The detector's pure state machine (testable without the registry).
#[derive(Default)]
pub struct CallState {
    active: BTreeSet<String>,
}

impl CallState {
    pub fn update(&mut self, now_active: BTreeSet<String>) -> Vec<CallEvent> {
        let was = !self.active.is_empty();
        let is = !now_active.is_empty();
        // a partial change of the line-up inside a call makes no noise, but a full
        // swap (zoom hung up, discord picked up within one poll) means two calls
        let swapped = was && is && self.active.is_disjoint(&now_active);
        self.active = now_active;
        let started = || CallEvent::Started {
            apps: self.active.iter().cloned().collect(),
        };
        match (was, is) {
            (false, true) => vec![started()],
            (true, false) => vec![CallEvent::Ended],
            (true, true) if swapped => vec![CallEvent::Ended, started()],
            _ => vec![],
        }
    }
}

/// The detector's background thread: registry → events → UI status + sessionization
/// signals to the pipeline (it owns meta and cuts sessions). The handle must be joined
/// on exit — the final pass closes an open meeting.
pub fn spawn_call_detect(
    running: Arc<AtomicBool>,
    ui_tx: Option<Sender<UiMsg>>,
    session_tx: Sender<SessionSignal>,
    ignore_extra: Vec<String>,
) -> Option<std::thread::JoinHandle<()>> {
    let mut ignore: BTreeSet<String> = ignore_extra
        .into_iter()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    // our own processes hold the microphone all the time — that is not a call
    if let Some(me) = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()))
    {
        ignore.insert(me);
    }
    ignore.insert("localvox-light.exe".into());

    std::thread::Builder::new()
        .name("call-detect".into())
        .spawn(move || {
            let mut state = CallState::default();
            let mut started_at: Option<chrono::DateTime<chrono::Local>> = None;
            let mut in_call = false;
            while running.load(Ordering::Relaxed) {
                let users = mic_users(&ignore);
                for event in state.update(users) {
                    match event {
                        CallEvent::Started { apps } => {
                            let (title, exe) = foreground_window_info().unwrap_or_default();
                            // the window may belong to anyone — we take the title
                            // only if it comes from the calling application
                            let title = if title_belongs_to(&apps, &exe) {
                                title
                            } else {
                                String::new()
                            };
                            started_at = Some(chrono::Local::now());
                            in_call = true;
                            let label = apps.join(", ");
                            tracing::info!("detect: call started ({label}; window: {title})");
                            if let Some(ref t) = ui_tx {
                                let _ = t.send(UiMsg::Status(format!(
                                    "☎ звонок: {label}{}",
                                    if title.is_empty() {
                                        String::new()
                                    } else {
                                        format!(" — {title}")
                                    }
                                )));
                            }
                            let _ = session_tx.send(SessionSignal::CallStarted { apps, title });
                        }
                        CallEvent::Ended => {
                            let mins = started_at
                                .take()
                                .map(|t| (chrono::Local::now() - t).num_minutes())
                                .unwrap_or(0);
                            in_call = false;
                            tracing::info!("detect: call ended (~{mins} min)");
                            if let Some(ref t) = ui_tx {
                                let _ = t.send(UiMsg::Status(format!(
                                    "☎ звонок завершён (~{mins} мин)"
                                )));
                            }
                            let _ = session_tx.send(SessionSignal::CallEnded);
                        }
                    }
                }
                // in small steps: exit on `running` without a 3-second delay
                let woke = Instant::now();
                while running.load(Ordering::Relaxed) && woke.elapsed() < POLL {
                    std::thread::sleep(Duration::from_millis(200));
                }
            }
            // we are leaving mid-call — tell the pipeline to close the meeting
            if in_call {
                let _ = session_tx.send(SessionSignal::CallEnded);
            }
        })
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn exe_name_extracted_from_consent_key() {
        assert_eq!(
            exe_from_consent_key("C:#Program Files#Zoom#bin#Zoom.exe"),
            "zoom.exe"
        );
        assert_eq!(exe_from_consent_key("plain.exe"), "plain.exe");
    }

    #[test]
    fn call_state_emits_only_on_edges() {
        let mut st = CallState::default();
        assert_eq!(st.update(set(&[])), vec![]);
        assert_eq!(
            st.update(set(&["zoom.exe"])),
            vec![CallEvent::Started {
                apps: vec!["zoom.exe".into()]
            }]
        );
        // the line-up changes during the call — no event
        assert_eq!(st.update(set(&["zoom.exe", "chrome.exe"])), vec![]);
        assert_eq!(st.update(set(&["chrome.exe"])), vec![]);
        assert_eq!(st.update(set(&[])), vec![CallEvent::Ended]);
        assert_eq!(st.update(set(&[])), vec![]);
    }

    #[test]
    fn back_to_back_calls_in_one_poll_are_two_calls() {
        let mut st = CallState::default();
        st.update(set(&["zoom.exe"]));
        // zoom hung up and discord picked up within one poll — end + start
        assert_eq!(
            st.update(set(&["discord.exe"])),
            vec![
                CallEvent::Ended,
                CallEvent::Started {
                    apps: vec!["discord.exe".into()]
                }
            ]
        );
    }

    #[test]
    fn title_taken_only_from_calling_app_window() {
        let apps = vec!["zoom.exe".to_string(), "msteams".to_string()];
        assert!(title_belongs_to(&apps, "zoom.exe"));
        // a packaged name is matched by its normalized prefix
        assert!(title_belongs_to(&apps, "ms-teams.exe"));
        // the call is in the background, the user is in the browser — do not take the title
        assert!(!title_belongs_to(&apps, "firefox.exe"));
        assert!(!title_belongs_to(&apps, ""));
    }
}
