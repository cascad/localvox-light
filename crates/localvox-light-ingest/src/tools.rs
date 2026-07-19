//! Locating yt-dlp / ffmpeg / the JS runtime + an optional settings.json.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub yt_dlp_path: Option<String>,
    #[serde(default)]
    pub ffmpeg_path: Option<String>,
    #[serde(default)]
    pub js_runtime: Option<String>,
    #[serde(default)]
    pub js_runtime_path: Option<String>,
    /// The default directory for the results.
    #[serde(default)]
    pub output_dir: Option<String>,
}

/// Candidates for finding the settings file: every name is looked for both next to the
/// exe and in the cwd.
fn settings_candidates(filenames: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in filenames {
                out.push(dir.join(name));
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        for name in filenames {
            out.push(cwd.join(name));
        }
    }
    out
}

/// Load `Settings` from the first `settings.json` that is found (next to the exe or in
/// the cwd).
pub fn load_settings() -> Settings {
    load_settings_named(&["settings.json"])
}

/// The same logic, but with an explicit list of file names. Use it to give a binary a
/// name of its own (`localvox-onnx-settings.json`, …) ahead of the common
/// `settings.json`.
pub fn load_settings_named(filenames: &[&str]) -> Settings {
    for path in settings_candidates(filenames) {
        if path.is_file() {
            if let Ok(s) = std::fs::read_to_string(&path) {
                if let Ok(cfg) = serde_json::from_str::<Settings>(&s) {
                    return cfg;
                }
            }
        }
    }
    Settings::default()
}

pub fn resolve_yt_dlp(settings: &Settings, cli_override: Option<&PathBuf>) -> String {
    if let Some(p) = cli_override {
        return p.to_string_lossy().to_string();
    }
    // The operator's explicit choice via `.env`, ranked above settings.json to match the rest of
    // the system (clap `env=`, `resolve_ffmpeg_for_decode`).
    if let Some(p) = from_env_file("LOCALVOX_LIGHT_YT_DLP") {
        return p;
    }
    if let Some(ref s) = settings.yt_dlp_path {
        let p = resolve_maybe_relative(s);
        if p.is_file() {
            return p.to_string_lossy().to_string();
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        for name in ["yt-dlp.exe", "yt-dlp"] {
            let p = cwd.join(name);
            if p.is_file() {
                return p.to_string_lossy().to_string();
            }
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in ["yt-dlp.exe", "yt-dlp"] {
                let p = dir.join(name);
                if p.is_file() {
                    return p.to_string_lossy().to_string();
                }
            }
        }
    }
    "yt-dlp".to_string()
}

pub fn resolve_ffmpeg(settings: &Settings, cli_override: Option<&PathBuf>) -> String {
    if let Some(p) = cli_override {
        return p.to_string_lossy().to_string();
    }
    // `.env` first, same as yt-dlp above. A bare `ffmpeg` on PATH still works — this only wins
    // when the operator named an explicit build (`F:/ffmpeg7.1.1/bin/ffmpeg.exe`).
    if let Some(p) = from_env_file("LOCALVOX_LIGHT_YT_FFMPEG") {
        return p;
    }
    if let Some(ref s) = settings.ffmpeg_path {
        let p = resolve_maybe_relative(s);
        if p.is_file() {
            return p.to_string_lossy().to_string();
        }
        if p.is_dir() {
            for name in ["ffmpeg.exe", "ffmpeg"] {
                let exe = p.join(name);
                if exe.is_file() {
                    return exe.to_string_lossy().to_string();
                }
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        for name in ["ffmpeg.exe", "ffmpeg"] {
            let p = cwd.join(name);
            if p.is_file() {
                return p.to_string_lossy().to_string();
            }
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in ["ffmpeg.exe", "ffmpeg"] {
                let p = dir.join(name);
                if p.is_file() {
                    return p.to_string_lossy().to_string();
                }
            }
        }
    }
    "ffmpeg".to_string()
}

fn resolve_maybe_relative(s: &str) -> PathBuf {
    let p = PathBuf::from(s);
    if p.is_absolute() {
        p
    } else if let Ok(cwd) = std::env::current_dir() {
        cwd.join(&p)
    } else {
        p
    }
}

/// An executable path from an environment variable, resolved and confirmed to exist.
///
/// This is the SAME `LOCALVOX_LIGHT_YT_*` convention the rest of the system already runs on: the
/// youtube/asr CLIs read it through clap `env=`, and `resolve_ffmpeg_for_decode` reads it for the
/// FLAC pass. The ingest tool-locator used to be the one place that did NOT — it read only
/// `settings.json`, so a machine whose `.env` pointed at a bundled `bin/yt-dlp.exe` (and had no
/// `settings.json`) resolved to the bare name and failed with "yt-dlp is not installed" while the
/// binary sat right there. One `.env`, every door.
///
/// A relative value is resolved against the cwd, exactly like a `settings.json` path. The value is
/// returned ONLY if it names a real file — otherwise the caller falls through to the next source
/// rather than committing to a path that is not there.
fn from_env_file(var: &str) -> Option<String> {
    let raw = std::env::var(var).ok()?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let p = resolve_maybe_relative(raw);
    p.is_file().then(|| p.to_string_lossy().into_owned())
}

pub fn resolve_js_runtime(
    settings: &Settings,
    cli_runtime: Option<&str>,
    cli_path: Option<&str>,
) -> Option<String> {
    // `.env` between the CLI flag and settings.json, same rank as yt-dlp/ffmpeg above. YouTube's
    // nsig deciphering often needs a JS engine; the daemon ingest used to ignore this variable and
    // only some sites would then fail, intermittently — the hardest kind of gap to diagnose.
    let env_runtime = std::env::var("LOCALVOX_LIGHT_YT_JS_RUNTIME").ok();
    let env_path = std::env::var("LOCALVOX_LIGHT_YT_JS_RUNTIME_PATH").ok();
    let runtime = cli_runtime
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or(env_runtime.as_deref().map(str::trim).filter(|s| !s.is_empty()))
        .or_else(|| {
            settings
                .js_runtime
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .unwrap_or("node");
    let path = cli_path
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or(env_path.as_deref().map(str::trim).filter(|s| !s.is_empty()))
        .or(settings.js_runtime_path.as_deref().map(str::trim))
        .filter(|s| !s.is_empty());

    if let Some(p) = path {
        let p = resolve_maybe_relative(p);
        if p.is_file() {
            let name = runtime.split(':').next().unwrap_or("node").trim();
            return Some(format!(
                "{}:{}",
                if name.is_empty() { "node" } else { name },
                p.to_string_lossy()
            ));
        }
    }

    if runtime.contains(['\\', '/']) {
        let p = PathBuf::from(runtime);
        if p.is_file() {
            return Some(format!("node:{}", p.to_string_lossy()));
        }
    }

    if runtime.contains(':') {
        return Some(runtime.to_string());
    }

    if !runtime.is_empty() && runtime != "false" {
        Some(runtime.to_string())
    } else {
        None
    }
}

pub fn resolve_ffmpeg_location_for_ytdlp(ffmpeg_path: &str) -> Option<String> {
    let p = PathBuf::from(ffmpeg_path);
    if p == PathBuf::from("ffmpeg") {
        return None;
    }
    let dir = if p.is_file() {
        p.parent()?.to_path_buf()
    } else if p.is_dir() {
        p
    } else {
        return None;
    };
    Some(dir.to_string_lossy().to_string())
}

/// A fast check before the model is loaded: yt-dlp in PATH or at an explicit path.
pub fn verify_yt_dlp(executable: &str) -> Result<()> {
    let status = Command::new(executable)
        .args(["--version"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| {
            format!(
                "yt-dlp («{executable}»): failed to start — install https://github.com/yt-dlp/yt-dlp or set the path through the CLI/env"
            )
        })?;
    if !status.success() {
        anyhow::bail!("yt-dlp («{executable}»): the --version command exited with an error");
    }
    Ok(())
}

pub fn verify_ffmpeg(executable: &str) -> Result<()> {
    let status = Command::new(executable)
        .args(["-hide_banner", "-version"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| {
            format!(
                "ffmpeg («{executable}»): failed to start — add it to PATH or set the path through the CLI/env"
            )
        })?;
    if !status.success() {
        anyhow::bail!("ffmpeg («{executable}»): the -version command exited with an error");
    }
    Ok(())
}

/// Only an explicit path in the form `node:C:\\…\\node.exe` / `deno:…`; a bare `node` is
/// not checked.
pub fn verify_js_runtime_path_if_explicit(js_runtime: Option<&str>) -> Result<()> {
    let Some(rt) = js_runtime.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    let Some((engine_name, path_tail)) = rt.split_once(':') else {
        return Ok(());
    };
    if engine_name.contains(['/', '\\']) {
        return Ok(());
    }
    let path = path_tail.trim();
    if path.is_empty() {
        return Ok(());
    }
    let p = Path::new(path);
    if p.is_file() {
        return Ok(());
    }
    anyhow::bail!(
        "js-runtime: «{rt}» is expected to point at an existing file: {}",
        p.display()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Environment variables are process-global; these tests mutate them, so they must not run
    // concurrently with each other. They are the ONLY tests in this crate that touch
    // `LOCALVOX_LIGHT_YT_*`, so one lock held for the duration is enough to keep them deterministic.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// A real file the resolvers will accept — this test executable itself always exists.
    fn a_real_file() -> PathBuf {
        std::env::current_exe().unwrap()
    }

    /// THE BUG THIS FIXES (17.07.2026). The daemon ingest read only `settings.json`; a machine
    /// whose `.env` pointed `LOCALVOX_LIGHT_YT_DLP` at a bundled `bin/yt-dlp.exe`, with no
    /// `settings.json`, resolved to the bare name «yt-dlp» and failed as "not installed" while the
    /// binary sat right there. The `.env` fed every other door — the cook, the youtube CLI — but
    /// not this one.
    #[test]
    fn env_var_locates_yt_dlp_when_settings_is_silent() {
        let _g = ENV_LOCK.lock().unwrap();
        let exe = a_real_file();
        std::env::set_var("LOCALVOX_LIGHT_YT_DLP", &exe);
        let got = resolve_yt_dlp(&Settings::default(), None);
        std::env::remove_var("LOCALVOX_LIGHT_YT_DLP");
        assert_eq!(PathBuf::from(got), exe);
    }

    /// The CLI flag still outranks the env — an explicit argument is the operator's most immediate
    /// intent, above a file they edited once.
    #[test]
    fn a_cli_override_still_beats_the_env() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("LOCALVOX_LIGHT_YT_DLP", a_real_file());
        let flag = PathBuf::from("C:/explicit/yt-dlp.exe");
        let got = resolve_yt_dlp(&Settings::default(), Some(&flag));
        std::env::remove_var("LOCALVOX_LIGHT_YT_DLP");
        assert_eq!(PathBuf::from(got), flag);
    }

    /// An env var that points at nothing must NOT pin a dead path — the resolver falls through to
    /// the next source (here: the bare PATH name), so a stale `.env` line degrades to the default
    /// instead of breaking every ingest.
    #[test]
    fn a_missing_env_path_falls_through_instead_of_pinning_a_ghost() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("LOCALVOX_LIGHT_YT_DLP", "Z:/nope/yt-dlp.exe");
        let got = resolve_yt_dlp(&Settings::default(), None);
        std::env::remove_var("LOCALVOX_LIGHT_YT_DLP");
        assert_eq!(got, "yt-dlp");
    }

    /// ffmpeg goes through the same env door as yt-dlp.
    #[test]
    fn env_var_locates_ffmpeg_too() {
        let _g = ENV_LOCK.lock().unwrap();
        let exe = a_real_file();
        std::env::set_var("LOCALVOX_LIGHT_YT_FFMPEG", &exe);
        let got = resolve_ffmpeg(&Settings::default(), None);
        std::env::remove_var("LOCALVOX_LIGHT_YT_FFMPEG");
        assert_eq!(PathBuf::from(got), exe);
    }

    /// The JS runtime — the same gap, and the one that fails intermittently: YouTube's nsig
    /// deciphering needs it only for some videos. The env path becomes `node:<path>`.
    #[test]
    fn env_var_locates_the_js_runtime() {
        let _g = ENV_LOCK.lock().unwrap();
        let exe = a_real_file();
        std::env::set_var("LOCALVOX_LIGHT_YT_JS_RUNTIME", "node");
        std::env::set_var("LOCALVOX_LIGHT_YT_JS_RUNTIME_PATH", &exe);
        let got = resolve_js_runtime(&Settings::default(), None, None);
        std::env::remove_var("LOCALVOX_LIGHT_YT_JS_RUNTIME");
        std::env::remove_var("LOCALVOX_LIGHT_YT_JS_RUNTIME_PATH");
        assert_eq!(got, Some(format!("node:{}", exe.to_string_lossy())));
    }
}
