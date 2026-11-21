//! One terminal-owned indicator, with complete child lines printed above it.
use indicatif::{ProgressBar, ProgressStyle};
use std::{
    io::{self, IsTerminal, Write},
    time::{Duration, Instant},
};

pub struct Activity {
    label: String,
    started: Instant,
    bar: Option<ProgressBar>,
    finished: bool,
}

impl Activity {
    pub fn start(label: impl Into<String>) -> Self {
        let label = label.into();
        let bar = if io::stderr().is_terminal() && std::env::var("TERM").as_deref() != Ok("dumb") {
            use cliclack::Theme;
            let theme = super::InstallerTheme { can_go_back: false };
            let style =
                ProgressStyle::with_template("│  {spinner:.cyan} {msg} · {elapsed_precise}")
                    .expect("constant activity template")
                    .tick_chars(&theme.spinner_chars());
            let bar = ProgressBar::new_spinner().with_style(style);
            bar.set_message(label.clone());
            bar.enable_steady_tick(Duration::from_millis(100));
            Some(bar)
        } else {
            eprintln!("┌ {label}");
            None
        };
        Self {
            label,
            started: Instant::now(),
            bar,
            finished: false,
        }
    }

    pub fn writer(&self) -> ActivityWriter {
        ActivityWriter {
            bar: self.bar.clone(),
            pending: Vec::new(),
        }
    }

    pub fn finish(&mut self, success: bool) {
        if self.finished {
            return;
        }
        self.finished = true;
        let elapsed = self.started.elapsed().as_secs();
        let status = if success {
            "✓ Готово"
        } else {
            "× Ошибка"
        };
        let message = format!(
            "{status} · {} · {:02}:{:02}",
            self.label,
            elapsed / 60,
            elapsed % 60
        );
        if let Some(bar) = &self.bar {
            bar.finish_and_clear();
            eprintln!("│  {message}");
        } else {
            eprintln!("└ {message}");
        }
    }
}

impl Drop for Activity {
    fn drop(&mut self) {
        self.finish(false);
    }
}

/// stdout and stderr each keep their own incomplete line; UTF-8 may cross chunks.
pub struct ActivityWriter {
    bar: Option<ProgressBar>,
    pending: Vec<u8>,
}

impl ActivityWriter {
    fn emit(&mut self) {
        let decoded = String::from_utf8_lossy(&self.pending);
        let clean = console::strip_ansi_codes(decoded.trim_end_matches('\r'));
        let line: String = clean
            .chars()
            .filter(|c| !c.is_control() || *c == '\t')
            .collect();
        if !line.is_empty() {
            if let Some(bar) = &self.bar {
                bar.suspend(|| eprintln!("│  {line}"));
            } else {
                eprintln!("│  {line}");
            }
        }
        self.pending.clear();
    }
}

impl Write for ActivityWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        for &byte in bytes {
            if byte == b'\n' {
                self.emit();
            } else {
                self.pending.push(byte);
                if self.pending.len() >= 64 * 1024 {
                    self.emit();
                }
            }
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        if !self.pending.is_empty() {
            self.emit();
        }
        Ok(())
    }
}

impl Drop for ActivityWriter {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}
