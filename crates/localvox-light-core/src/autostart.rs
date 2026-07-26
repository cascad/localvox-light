//! Start with the system (Windows, WP-C5): a value under
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` — no admin rights,
//! no Task Scheduler. We launch the current exe with `--daemon`.
//!
//! The macOS counterpart is a LaunchAgent plist under `~/Library/LaunchAgents`; it does not exist
//! yet, which is why this module is Windows-only rather than a trait with one implementation.

#![cfg(windows)]

use std::io;

use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "localvox-light";

/// The autostart command line. Besides the exe we remember the WORKING DIRECTORY:
/// for autostart cwd = system32, and everything depends on cwd — `.env` (vosk model,
/// slots, LLM), `models/`, the relative `LOCALVOX_LIGHT_AUDIO_DIR`. The daemon
/// will chdir into this directory before reading any config and will behave exactly
/// as if launched by hand from there.
fn run_command(exe: &str) -> String {
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    format!("\"{exe}\" --daemon --cwd \"{cwd}\"")
}

/// The executable a stored `Run` value launches — the first quoted token, or the whole line
/// if someone wrote it by hand without quotes.
///
/// We compare PATHS, not command lines. The flags in that value are ours and may be spelled
/// differently by an older build (`--tray`); the working directory is pinned at the moment the
/// switch is flipped and legitimately differs. Byte-comparing the whole line made the switch read
/// «disabled» while the system dutifully launched us at every login — a checkbox lying in the
/// direction that hides a running process is the worst of the two lies.
fn exe_in(command: &str) -> &str {
    let c = command.trim();
    match c.strip_prefix('"').and_then(|r| r.split_once('"')) {
        Some((exe, _rest)) => exe,
        None => c.split_whitespace().next().unwrap_or(c),
    }
}

fn current_exe() -> io::Result<String> {
    Ok(std::env::current_exe()?.to_string_lossy().into_owned())
}

/// Autostart counts as enabled only if the registry value points at the CURRENT
/// exe. After the binary is moved/updated, the stale path reads as
/// "disabled" — the next toggle rewrites the value with the fresh path (otherwise
/// the checkbox lies "enabled" while the system launches a nonexistent file at boot).
pub fn is_enabled() -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(key) = hkcu.open_subkey(RUN_KEY) else {
        return false;
    };
    let Ok(stored) = key.get_value::<String, _>(VALUE_NAME) else {
        return false;
    };
    match current_exe() {
        // Windows paths are case-insensitive
        Ok(exe) => exe_in(&stored).eq_ignore_ascii_case(&exe),
        Err(_) => true, // the exe cannot be determined — no worse than the previous behaviour
    }
}

pub fn enable() -> io::Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(RUN_KEY)?;
    key.set_value(VALUE_NAME, &run_command(&current_exe()?))
}

pub fn disable() -> io::Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu.open_subkey_with_flags(RUN_KEY, winreg::enums::KEY_SET_VALUE)?;
    match key.delete_value(VALUE_NAME) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()), // already disabled
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_command_quotes_exe_and_pins_workdir() {
        let cmd = run_command(r"C:\Program Files\lv\localvox-light.exe");
        assert!(
            cmd.starts_with(r#""C:\Program Files\lv\localvox-light.exe" --daemon --cwd ""#),
            "{cmd}"
        );
        // the working directory is pinned (for autostart cwd = system32)
        let cwd = std::env::current_dir().unwrap().display().to_string();
        assert!(cmd.contains(&cwd), "{cmd}");
    }

    /// A path with spaces is the normal case (`C:\Program Files\…`), so the quotes are what
    /// carries the answer — not whitespace.
    #[test]
    fn the_switch_reads_the_exe_out_of_a_stored_command() {
        assert_eq!(
            exe_in(r#""C:\Program Files\lv\localvox-light.exe" --daemon --cwd "D:\work""#),
            r"C:\Program Files\lv\localvox-light.exe"
        );
        // Written by hand, without quotes — still an answer, not a panic.
        assert_eq!(exe_in(r"C:\lv\localvox-light.exe --daemon"), r"C:\lv\localvox-light.exe");
    }

    /// The switch answers «does this launch THIS exe», not «is this byte-identical to what I
    /// would write today». An entry left by an older build says `--tray` and carries the working
    /// directory of the day it was flipped; both are ours, and neither means autostart is off.
    /// Reading it as off hid a process that kept starting at every login.
    #[test]
    fn an_older_spelling_of_our_own_command_still_counts_as_enabled() {
        let exe = r"C:\lv\localvox-light.exe";
        let legacy = format!(r#""{exe}" --tray --cwd "D:\somewhere-else""#);
        assert_eq!(exe_in(&legacy), exe);
        assert_ne!(legacy, run_command(exe), "otherwise the test proves nothing");
    }
}
