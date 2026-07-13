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

pub fn resolve_js_runtime(
    settings: &Settings,
    cli_runtime: Option<&str>,
    cli_path: Option<&str>,
) -> Option<String> {
    let runtime = cli_runtime
        .map(str::trim)
        .filter(|s| !s.is_empty())
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
