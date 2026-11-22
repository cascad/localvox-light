//! One terminal-owned indicator, with complete child lines printed above it.
use crate::progress::{Phase, Progress};
use indicatif::{ProgressBar, ProgressStyle};
use std::{
    io::{self, IsTerminal, Write},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub struct Activity {
    label: String,
    started: Instant,
    bar: Option<ProgressBar>,
    finished: bool,
    progress: Arc<Mutex<ProgressState>>,
}

#[derive(Default)]
struct ProgressState {
    name: String,
    phase: Option<Phase>,
    percent: Option<u64>,
    elapsed_ms: u64,
}

fn spinner_style() -> ProgressStyle {
    use cliclack::Theme;
    ProgressStyle::with_template("│  {spinner:.cyan} {msg} · {elapsed_precise}")
        .expect("constant spinner template")
        .tick_chars(&super::InstallerTheme { can_go_back: false }.spinner_chars())
}

impl Activity {
    pub fn start(label: impl Into<String>) -> Self {
        let label = label.into();
        let bar = if io::stderr().is_terminal() && std::env::var("TERM").as_deref() != Ok("dumb") {
            let bar = ProgressBar::new_spinner().with_style(spinner_style());
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
            progress: Arc::new(Mutex::new(ProgressState::default())),
        }
    }

    pub fn writer(&self) -> ActivityWriter {
        ActivityWriter {
            bar: self.bar.clone(),
            pending: Vec::new(),
            progress: self.progress.clone(),
        }
    }

    pub fn progress(&self, event: &Progress) {
        self.writer().draw_progress(event);
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
    progress: Arc<Mutex<ProgressState>>,
}

impl ActivityWriter {
    fn draw_progress(&self, event: &Progress) {
        let mut state = self.progress.lock().unwrap_or_else(|e| e.into_inner());
        let changed = state.name != event.name || state.phase != Some(event.phase);
        let percent = event
            .total
            .filter(|total| *total > 0)
            .map(|total| event.bytes.saturating_mul(100) / total);
        if let Some(bar) = &self.bar {
            if state.name != event.name || event.phase == Phase::Connecting {
                bar.reset();
            }
            if event.phase == Phase::Downloading {
                let style = if let Some(total) = event.total.filter(|total| *total > 0) {
                    bar.set_length(total);
                    ProgressStyle::with_template("│  {spinner:.cyan} {msg}\n│  {wide_bar:.cyan/dim} {percent:>3}% · {bytes}/{total_bytes} · {bytes_per_sec} · осталось {eta_precise}")
                } else {
                    ProgressStyle::with_template("│  {spinner:.cyan} {msg} · {bytes} · {bytes_per_sec} · {elapsed_precise}")
                }.expect("constant byte progress template");
                use cliclack::Theme;
                bar.set_style(
                    style.tick_chars(&super::InstallerTheme { can_go_back: false }.spinner_chars()),
                );
                bar.set_position(event.bytes);
            } else {
                bar.set_style(spinner_style());
            }
            bar.set_message(event.message());
        } else if changed
            || percent.is_some_and(|p| {
                (p == 100 && state.percent != Some(100))
                    || state.percent.is_none_or(|old| p / 10 != old / 10)
            })
            || (percent.is_none() && event.elapsed_ms.saturating_sub(state.elapsed_ms) >= 15_000)
        {
            eprintln!("│  {}", event.plain());
            state.percent = percent;
            state.elapsed_ms = event.elapsed_ms;
        }
        state.name = event.name.clone();
        state.phase = Some(event.phase);
    }

    fn emit(&mut self) {
        let decoded = String::from_utf8_lossy(&self.pending);
        if decoded.starts_with(crate::progress::PREFIX) {
            if let Some(event) = Progress::parse(decoded.trim_end_matches('\r')) {
                self.draw_progress(&event);
            } else {
                let message = "│  Не удалось прочитать ход загрузки";
                if let Some(bar) = &self.bar {
                    bar.suspend(|| eprintln!("{message}"));
                } else {
                    eprintln!("{message}");
                }
            }
            self.pending.clear();
            return;
        }
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
