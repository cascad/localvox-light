//! Fixture runner: capture a real ConPTY/PTY without adding a Python PTY dependency.
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::{
    io::{Read, Write},
    sync::mpsc,
    time::{Duration, Instant},
};

struct OwnedChild(Box<dyn portable_pty::Child + Send + Sync>);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn main() {
    let mut args = std::env::args_os().skip(1);
    let transcript = args.next().expect("transcript path");
    let mut command = CommandBuilder::new(args.next().expect("fixture program"));
    command.args(args);
    command.env("TERM", "xterm-256color");
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut child = OwnedChild(pair.slave.spawn_command(command).unwrap());
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let (sender, receiver) = mpsc::sync_channel(16);
    std::thread::spawn(move || {
        let mut bytes = [0; 8192];
        while let Ok(count) = reader.read(&mut bytes) {
            if count == 0 || sender.send(bytes[..count].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut output = Vec::new();
    let mut queries = 0;
    let mut cancelled = false;
    let cancel_on = std::env::var("LOCALVOX_TEST_CANCEL_ON").ok();
    let deadline = Instant::now() + Duration::from_secs(40);
    let status = loop {
        if let Ok(bytes) = receiver.recv_timeout(Duration::from_millis(50)) {
            output.extend(bytes);
            assert!(output.len() < 4 * 1024 * 1024, "bounded terminal output");
            std::fs::write(&transcript, &output).unwrap();
        }
        let text = String::from_utf8_lossy(&output);
        let count = text.matches("\x1b[6n").count();
        while queries < count {
            writer.write_all(b"\x1b[1;1R").unwrap();
            writer.flush().unwrap();
            queries += 1;
        }
        if !cancelled
            && cancel_on
                .as_ref()
                .is_some_and(|marker| text.contains(marker))
        {
            writer.write_all(b"\x03").unwrap();
            writer.flush().unwrap();
            cancelled = true;
        }
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "fixture terminal timed out");
    };
    // Closing the master flushes ConPTY's remaining output; consume it before
    // reporting the child status instead of losing its final progress/error.
    drop(writer);
    drop(pair.master);
    for bytes in receiver {
        output.extend(bytes);
        assert!(output.len() < 4 * 1024 * 1024, "bounded terminal output");
    }
    std::fs::write(&transcript, &output).unwrap();
    std::process::exit(status.exit_code() as i32);
}
