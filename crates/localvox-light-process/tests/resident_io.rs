//! A custom harness keeps the subprocess fixture's stdout strictly framed.
use localvox_light_process::{Limits, StdioSession};
use std::{
    io::{BufRead, Write},
    process::Command,
    time::{Duration, Instant},
};

fn helper(mode: &str) -> Command {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.env("LOCALVOX_STDIO_FIXTURE", mode);
    cmd
}
fn fixture(mode: &str) {
    if mode == "leaf" {
        let mut file =
            std::fs::File::create(std::env::var_os("LOCALVOX_STDIO_PULSE").unwrap()).unwrap();
        loop {
            file.write_all(b".").unwrap();
            file.flush().unwrap();
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap();
        match mode {
            "echo" => {
                eprintln!("diagnostic:{line}");
                println!("{line}");
                std::io::stdout().flush().unwrap();
            }
            "idle-diagnostic" => {
                println!("ok");
                std::io::stdout().flush().unwrap();
                std::thread::sleep(Duration::from_millis(100));
                std::io::stderr().write_all(&vec![b'x'; 16384]).unwrap();
                std::thread::sleep(Duration::from_secs(30));
            }
            "eof" => return,
            "oversize" => {
                print!("{}", "x".repeat(16384));
                std::io::stdout().flush().unwrap();
            }
            "diagnostic" => {
                std::io::stderr().write_all(&vec![b'x'; 16384]).unwrap();
                println!("ok");
                std::io::stdout().flush().unwrap();
            }
            "tree" => {
                let _child = helper("leaf").spawn().unwrap();
                std::thread::sleep(Duration::from_secs(30));
            }
            _ => std::thread::sleep(Duration::from_secs(30)),
        }
    }
}
fn main() {
    if let Ok(mode) = std::env::var("LOCALVOX_STDIO_FIXTURE") {
        fixture(&mode);
        return;
    }
    let mut session = StdioSession::spawn(helper("echo")).unwrap();
    let pid = session.pid();
    for input in [b"first".as_slice(), b"second".as_slice()] {
        let reply = session
            .request(
                input,
                Limits::new(Duration::from_secs(5)),
                || false,
                |_, _| {},
            )
            .unwrap();
        assert_eq!(reply.pid, pid);
        assert_eq!(reply.stdout, input);
        let diagnostic = String::from_utf8_lossy(&reply.stderr);
        assert!(diagnostic.contains(std::str::from_utf8(input).unwrap()));
        if input == b"second" {
            assert!(
                !diagnostic.contains("first"),
                "previous request diagnostics leaked"
            );
        }
    }
    drop(session);
    let mut session = StdioSession::spawn(helper("idle-diagnostic")).unwrap();
    let mut limits = Limits::new(Duration::from_secs(5));
    limits.output_bytes = 1024;
    session
        .request(b"request", limits, || false, |_, _| {})
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !session.has_exited().unwrap() {
        assert!(
            Instant::now() < deadline,
            "idle diagnostic flood must evict the worker"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    for mode in ["eof", "hang", "oversize", "diagnostic"] {
        let mut session = StdioSession::spawn(helper(mode)).unwrap();
        let mut limits = Limits::new(Duration::from_millis(400));
        limits.output_bytes = 1024;
        assert!(
            session
                .request(b"request", limits, || false, |_, _| {})
                .is_err(),
            "{mode}"
        );
        assert!(session.has_exited().unwrap());
    }
    let mut session = StdioSession::spawn(helper("hang")).unwrap();
    let started = Instant::now();
    assert!(session
        .request(
            b"cancel",
            Limits::new(Duration::from_secs(10)),
            || started.elapsed() > Duration::from_millis(100),
            |_, _| {}
        )
        .is_err());
    assert!(started.elapsed() < Duration::from_secs(3) && session.has_exited().unwrap());
    let temp = tempfile::tempdir().unwrap();
    let pulse = temp.path().join("pulse");
    let mut cmd = helper("tree");
    cmd.env("LOCALVOX_STDIO_PULSE", &pulse);
    let mut session = StdioSession::spawn(cmd).unwrap();
    assert!(session
        .request(
            b"tree",
            Limits::new(Duration::from_secs(1)),
            || false,
            |_, _| {}
        )
        .is_err());
    std::thread::sleep(Duration::from_millis(200));
    let size = std::fs::metadata(&pulse).unwrap().len();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        std::fs::metadata(&pulse).unwrap().len(),
        size,
        "descendant survived resident timeout"
    );
    let mut session = StdioSession::spawn(helper("echo")).unwrap();
    assert!(session
        .request(
            &vec![b'x'; 65537],
            Limits::new(Duration::from_secs(1)),
            || false,
            |_, _| {}
        )
        .is_err());
    assert!(session.has_exited().unwrap());
    println!("resident IO: two replies, EOF, timeout, cancel, stdout/stderr/input bounds and descendant cleanup passed");
}
