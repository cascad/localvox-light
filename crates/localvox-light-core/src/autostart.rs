//! Start with the system (Windows, WP-C5): a value under
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` — no admin rights,
//! no Task Scheduler. We launch the current exe with `--tray`.

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
    format!("\"{exe}\" --tray --cwd \"{cwd}\"")
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
        Ok(exe) => stored.eq_ignore_ascii_case(&run_command(&exe)),
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
            cmd.starts_with(r#""C:\Program Files\lv\localvox-light.exe" --tray --cwd ""#),
            "{cmd}"
        );
        // the working directory is pinned (for autostart cwd = system32)
        let cwd = std::env::current_dir().unwrap().display().to_string();
        assert!(cmd.contains(&cwd), "{cmd}");
    }
}
