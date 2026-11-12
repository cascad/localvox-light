//! Record the real TUI through a platform PTY against a private demo API.
//! Usage: promo-capture <tui> <address> <scenario.json> <output.jsonl>
use anyhow::{Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Deserialize;
use std::{
    fs::File,
    io::{Read, Write},
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

#[derive(Deserialize)]
struct Step {
    at_ms: u64,
    keys: String,
}
#[derive(Deserialize)]
struct Scenario {
    cols: u16,
    rows: u16,
    duration_ms: u64,
    steps: Vec<Step>,
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    anyhow::ensure!(args.len() == 4, "expected tui, address, scenario, output");
    let scenario: Scenario = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    anyhow::ensure!(
        scenario.duration_ms <= 60000,
        "capture is bounded to one minute"
    );
    anyhow::ensure!(
        scenario.steps.windows(2).all(|w| w[0].at_ms <= w[1].at_ms),
        "steps must be sorted"
    );
    let pty = native_pty_system().openpty(PtySize {
        cols: scenario.cols,
        rows: scenario.rows,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    let working = tempfile::tempdir()?;
    let mut command = CommandBuilder::new(PathBuf::from(&args[0]).canonicalize()?);
    command.args(["--address", &args[1].to_string_lossy()]);
    command.cwd(working.path());
    command.env("COLORTERM", "truecolor");
    command.env("TERM", "xterm-256color");
    command.env_remove("NO_COLOR");
    command.env_remove("LOCALVOX_TUI_KEYMAP");
    command.env("LOCALVOX_API_TOKEN", "");
    let child = pty.slave.spawn_command(command)?;
    struct OwnedChild(Box<dyn portable_pty::Child + Send + Sync>);
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = OwnedChild(child);
    drop(pty.slave);
    let mut reader = pty.master.try_clone_reader()?;
    let mut writer = pty.master.take_writer()?;
    let (tx, rx) = mpsc::sync_channel(128);
    std::thread::spawn(move || {
        let mut buf = [0; 32768];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let mut output = File::create(&args[3])?;
    writeln!(
        output,
        "{}",
        serde_json::json!({"cols":scenario.cols,"rows":scenario.rows,"duration_ms":scenario.duration_ms,"source":"real localvox-tui / portable-pty; synthetic API"})
    )?;
    let start = Instant::now();
    let mut step = 0;
    let mut query_tail = Vec::new();
    while start.elapsed().as_millis() < scenario.duration_ms as u128 {
        while step < scenario.steps.len()
            && start.elapsed().as_millis() >= scenario.steps[step].at_ms as u128
        {
            writer.write_all(scenario.steps[step].keys.as_bytes())?;
            writer.flush()?;
            step += 1;
        }
        if let Ok(bytes) = rx.recv_timeout(Duration::from_millis(10)) {
            // ConPTY asks the host terminal for its cursor before starting the child.
            // This is the same DSR handshake used by the project's PTY tests.
            query_tail.extend_from_slice(&bytes);
            for _ in 0..query_tail.windows(4).filter(|w| *w == b"\x1b[6n").count() {
                writer.write_all(b"\x1b[1;1R")?;
                writer.flush()?;
            }
            if query_tail.len() > 3 {
                query_tail.drain(..query_tail.len() - 3);
            }
            writeln!(
                output,
                "{}",
                serde_json::json!({"at_ms":start.elapsed().as_millis(),"bytes":bytes})
            )?;
        }
        anyhow::ensure!(child.0.try_wait()?.is_none(), "TUI exited during capture");
    }
    writer.write_all(b"qq").context("closing owned TUI")?;
    output.sync_all()?;
    Ok(())
}
