//! Versioned child-to-renderer progress. No terminal operations or HTTP here.
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

pub const PREFIX: &str = "@localvox-download-v1 ";
pub const PROTOCOL: &str = "localvox-download-v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Connecting,
    Downloading,
    Verifying,
    Unpacking,
    Complete,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Progress {
    pub name: String,
    pub phase: Phase,
    #[serde(default)]
    pub bytes: u64,
    pub total: Option<u64>,
    #[serde(default)]
    pub elapsed_ms: u64,
}

pub fn label(text: &str) -> String {
    console::strip_ansi_codes(text)
        .chars()
        .filter(|c| !c.is_control())
        .take(120)
        .collect()
}

impl Progress {
    pub fn stage(name: &str, phase: Phase) -> Self {
        Self {
            name: label(name),
            phase,
            bytes: 0,
            total: None,
            elapsed_ms: 0,
        }
    }
    pub fn write(&self, output: &mut impl Write) -> io::Result<()> {
        let mut safe = self.clone();
        safe.name = label(&safe.name);
        write!(output, "{PREFIX}{}\n", serde_json::to_string(&safe)?)?;
        output.flush()
    }
    pub fn parse(line: &str) -> Option<Self> {
        let value: Self = serde_json::from_str(line.strip_prefix(PREFIX)?).ok()?;
        if value.name.len() > 1024 || value.total.is_some_and(|total| value.bytes > total) {
            return None;
        }
        Some(Self {
            name: label(&value.name),
            ..value
        })
    }
    pub fn message(&self) -> String {
        let phase = match self.phase {
            Phase::Connecting => "Соединение",
            Phase::Downloading => "Скачивание",
            Phase::Verifying => "Проверка",
            Phase::Unpacking => "Распаковка",
            Phase::Complete => "Скачивание завершено",
        };
        format!("{phase} · {}", self.name)
    }
    pub fn plain(&self) -> String {
        if self.phase != Phase::Downloading {
            return self.message();
        }
        let speed = self.bytes as f64 * 1000.0 / self.elapsed_ms.max(1) as f64;
        let bytes = |n: f64| {
            if n >= 1024.0_f64.powi(3) {
                format!("{:.2} GiB", n / 1024.0_f64.powi(3))
            } else if n >= 1024.0_f64.powi(2) {
                format!("{:.1} MiB", n / 1024.0_f64.powi(2))
            } else {
                format!("{:.1} KiB", n / 1024.0)
            }
        };
        let suffix = self
            .total
            .filter(|total| *total > 0)
            .map(|total| {
                let seconds = if speed > 0.0 {
                    (total.saturating_sub(self.bytes) as f64 / speed).ceil() as u64
                } else {
                    0
                };
                format!(
                    "/{} · {}% · осталось {:02}:{:02}",
                    bytes(total as f64),
                    self.bytes.saturating_mul(100) / total,
                    seconds / 60,
                    seconds % 60
                )
            })
            .unwrap_or_default();
        format!(
            "{} · {}{suffix} · {}/s",
            self.name,
            bytes(self.bytes as f64),
            bytes(speed)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protocol_is_bounded_and_terminal_text_is_inert() {
        let event = Progress {
            name: "\x1b[31mимя\n\x1b[0m".into(),
            phase: Phase::Downloading,
            bytes: 512,
            total: Some(1024),
            elapsed_ms: 1000,
        };
        let mut bytes = Vec::new();
        event.write(&mut bytes).unwrap();
        let wire = String::from_utf8(bytes).unwrap();
        let parsed = Progress::parse(wire.trim()).unwrap();
        assert_eq!(parsed.name, "имя");
        assert!(parsed.plain().contains("50%"));
        assert!(Progress::parse(&wire.replace("512", "2048")).is_none());
        let unknown = Progress {
            total: None,
            ..parsed
        };
        assert!(!unknown.plain().contains('%') && !unknown.plain().contains("осталось"));
    }
}
