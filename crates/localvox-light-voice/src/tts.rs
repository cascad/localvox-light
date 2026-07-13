//! TTS confirmations (F2). V1 — Windows: SAPI (zero dependencies) or an external
//! Piper (`piper.exe` + a Russian model; sounds better). Speech is best effort:
//! errors are logged, the pipeline does not fall over. Text is handed over through
//! a UTF-8 file, not through arguments/pipes — that sidesteps Windows system
//! codepages (see the worklog on cp1251 in Python's stdin: PowerShell steps on the
//! same rake).

use std::path::PathBuf;
use std::process::Command;

#[derive(Default)]
pub enum Tts {
    Off,
    #[default]
    Sapi,
    Piper {
        exe: PathBuf,
        model: PathBuf,
    },
}

impl Tts {
    pub fn from_env() -> Self {
        match std::env::var("LOCALVOX_LIGHT_TTS")
            .unwrap_or_else(|_| "sapi".into())
            .to_lowercase()
            .as_str()
        {
            "off" | "0" | "false" | "no" => Tts::Off,
            "piper" => {
                let exe = std::env::var("LOCALVOX_LIGHT_PIPER").unwrap_or_else(|_| "piper".into());
                let Ok(model) = std::env::var("LOCALVOX_LIGHT_PIPER_MODEL") else {
                    tracing::warn!(
                        "LOCALVOX_LIGHT_TTS=piper without LOCALVOX_LIGHT_PIPER_MODEL — falling back to SAPI"
                    );
                    return Tts::Sapi;
                };
                Tts::Piper {
                    exe: PathBuf::from(exe),
                    model: PathBuf::from(model),
                }
            }
            _ => Tts::Sapi,
        }
    }

    pub fn describe(&self) -> &'static str {
        match self {
            Tts::Off => "off",
            Tts::Sapi => "sapi",
            Tts::Piper { .. } => "piper",
        }
    }

    /// Blocking speech (called from the voice module thread).
    pub fn speak(&self, text: &str) {
        let res = match self {
            Tts::Off => Ok(()),
            Tts::Sapi => speak_sapi(text),
            Tts::Piper { exe, model } => speak_piper(exe, model, text),
        };
        if let Err(e) = res {
            tracing::warn!("TTS failed: {e:#}");
        }
    }
}

fn unique_temp(ext: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "localvox-tts-{}-{}.{ext}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ))
}

fn write_utf8_temp(text: &str) -> std::io::Result<PathBuf> {
    let path = unique_temp("txt");
    std::fs::write(&path, text)?;
    Ok(path)
}

/// PowerShell single quotes: escaping means doubling them.
fn ps_quote(p: &std::path::Path) -> String {
    format!("'{}'", p.display().to_string().replace('\'', "''"))
}

#[cfg(windows)]
fn speak_sapi(text: &str) -> anyhow::Result<()> {
    let tmp = write_utf8_temp(text)?;
    let script = format!(
        "Add-Type -AssemblyName System.Speech; \
         $t = Get-Content -LiteralPath {} -Raw -Encoding UTF8; \
         $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
         $s.Speak($t)",
        ps_quote(&tmp)
    );
    let status = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .status()?;
    let _ = std::fs::remove_file(&tmp);
    anyhow::ensure!(status.success(), "powershell SAPI: {status}");
    Ok(())
}

#[cfg(not(windows))]
fn speak_sapi(_text: &str) -> anyhow::Result<()> {
    anyhow::bail!("SAPI is available on Windows only; set LOCALVOX_LIGHT_TTS=piper")
}

fn speak_piper(exe: &std::path::Path, model: &std::path::Path, text: &str) -> anyhow::Result<()> {
    use std::io::Write;
    let wav = unique_temp("wav");
    let mut child = Command::new(exe)
        .arg("--model")
        .arg(model)
        .arg("--output_file")
        .arg(&wav)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("spawning piper ({}): {e}", exe.display()))?;
    child
        .stdin
        .as_mut()
        .expect("piped stdin")
        .write_all(text.as_bytes())?;
    let status = child.wait()?;
    anyhow::ensure!(status.success(), "piper: {status}");
    play_wav(&wav)?;
    let _ = std::fs::remove_file(&wav);
    Ok(())
}

#[cfg(windows)]
fn play_wav(wav: &std::path::Path) -> anyhow::Result<()> {
    let script = format!(
        "(New-Object Media.SoundPlayer {}).PlaySync()",
        ps_quote(wav)
    );
    let status = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .status()?;
    anyhow::ensure!(status.success(), "SoundPlayer: {status}");
    Ok(())
}

#[cfg(not(windows))]
fn play_wav(wav: &std::path::Path) -> anyhow::Result<()> {
    // aplay (Linux) / afplay (macOS) — whichever is present
    for player in ["afplay", "aplay"] {
        if Command::new(player).arg(wav).status().map(|s| s.success()) == Ok(true) {
            return Ok(());
        }
    }
    anyhow::bail!("no wav player found (afplay/aplay)")
}
