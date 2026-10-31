//! OS integration tests. Helpers are this same test executable, never an installed external tool.
use localvox_light_process::{run, Limits, Termination};
use std::{
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn helper(mode: &str) -> Command {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["--ignored", "--exact", "fixture", "--nocapture"])
        .env("LOCALVOX_TEST_MODE", mode);
    cmd
}

#[test]
#[ignore = "invoked as a subprocess fixture"]
fn fixture() {
    match std::env::var("LOCALVOX_TEST_MODE").unwrap().as_str() {
        "duplex" => {
            // Deliberately fill stdout BEFORE reading stdin: the old Claude runner deadlocked here.
            std::io::stdout()
                .write_all(&vec![b'x'; 2 * 1024 * 1024])
                .unwrap();
            let mut data = Vec::new();
            std::io::stdin().read_to_end(&mut data).unwrap();
            println!("received={}", data.len());
        }
        "noisy" => {
            std::io::stdout()
                .write_all(&vec![b'x'; 2 * 1024 * 1024])
                .unwrap();
        }
        "stream" => {
            print!("first-fragment");
            std::io::stdout().flush().unwrap();
            let release =
                std::path::PathBuf::from(std::env::var_os("LOCALVOX_TEST_RELEASE").unwrap());
            let until = Instant::now() + Duration::from_secs(5);
            while !release.exists() {
                assert!(Instant::now() < until, "observer did not release fixture");
                std::thread::sleep(Duration::from_millis(10));
            }
            println!("-last-fragment");
        }
        "leaf" => {
            let path = std::env::var_os("LOCALVOX_TEST_PULSE").unwrap();
            let mut file = std::fs::File::create(path).unwrap();
            for _ in 0..1000 {
                file.write_all(b".").unwrap();
                file.flush().unwrap();
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        "tree" | "orphan" => {
            let mut leaf = helper("leaf").spawn().unwrap();
            let pulse = std::env::var_os("LOCALVOX_TEST_PULSE").unwrap();
            await_pulse(Path::new(&pulse));
            if std::env::var("LOCALVOX_TEST_MODE").unwrap() == "tree" {
                leaf.wait().unwrap();
            }
            // Otherwise exit leaving a child with inherited stdout/stderr.
        }
        "owner" => {
            let mut limits = Limits::new(Duration::from_secs(25));
            if std::env::var(localvox_light_process::CONTAINED_ENV).as_deref() == Ok("1") {
                limits.scope = localvox_light_process::Scope::Inherit;
            }
            run(helper("tree"), None, limits, || false, |_, _| {}).unwrap();
        }
        _ => panic!("unknown fixture"),
    }
}

fn await_pulse(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while std::fs::metadata(path)
        .map(|m| m.len() == 0)
        .unwrap_or(true)
    {
        assert!(Instant::now() < deadline, "fixture failed to start");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn assert_stopped(path: &Path) {
    // Allow OS job termination to finish; then require the descendant's heartbeat to stay flat.
    std::thread::sleep(Duration::from_millis(200));
    let first = std::fs::metadata(path).unwrap().len();
    std::thread::sleep(Duration::from_millis(250));
    assert_eq!(
        first,
        std::fs::metadata(path).unwrap().len(),
        "descendant survived"
    );
}

#[test]
fn simultaneous_large_input_and_output_cannot_deadlock() {
    let out = run(
        helper("duplex"),
        Some(&vec![b'a'; 2 * 1024 * 1024]),
        Limits::new(Duration::from_secs(10)),
        || false,
        |_, _| {},
    )
    .unwrap();
    assert!(out.termination.success(), "{:?}", out.termination);
    assert!(String::from_utf8_lossy(&out.stdout).contains("received=2097152"));
}

#[test]
fn managed_tool_drop_stops_its_descendants() {
    let directory = tempfile::tempdir().unwrap();
    let pulse = directory.path().join("pulse");
    let mut command = helper("tree");
    command.env("LOCALVOX_TEST_PULSE", &pulse);
    let mut process = localvox_light_process::ManagedProcess::spawn(command).unwrap();
    await_pulse(&pulse);
    assert!(!process.has_exited().unwrap());
    drop(process);
    assert_stopped(&pulse);
}
#[test]
fn managed_tool_has_bounded_wait_and_reports_start_failure() {
    assert!(localvox_light_process::ManagedProcess::spawn(Command::new(
        "localvox-nonexistent-process-fixture"
    ))
    .is_err());
    let directory = tempfile::tempdir().unwrap();
    let mut command = helper("leaf");
    command.env("LOCALVOX_TEST_PULSE", directory.path().join("pulse"));
    let mut process = localvox_light_process::ManagedProcess::spawn(command).unwrap();
    assert!(process.wait_for_exit(Duration::from_millis(5)).is_err());
    process.terminate().unwrap();
    assert!(process.has_exited().unwrap());
}

#[test]
fn output_is_observed_before_exit_without_moving_the_writer_offset() {
    let temp = tempfile::tempdir().unwrap();
    let release = temp.path().join("release");
    let mut command = helper("stream");
    command.env("LOCALVOX_TEST_RELEASE", &release);
    let (observer, rx, _) = localvox_light_process::OutputObserver::channel(16);
    let child = std::thread::spawn(move || {
        localvox_light_process::run_observed(
            command,
            None,
            Limits::new(Duration::from_secs(8)),
            || false,
            |_, _| {},
            Some(observer),
        )
        .unwrap()
    });
    let mut seen = Vec::new();
    while !String::from_utf8_lossy(&seen).contains("first-fragment") {
        let chunk = rx.recv_timeout(Duration::from_secs(3)).unwrap();
        seen.extend(chunk.bytes);
    }
    assert!(
        !child.is_finished(),
        "output must arrive before process exit"
    );
    std::fs::write(&release, b"release").unwrap();
    let out = child.join().unwrap();
    assert!(out.termination.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("first-fragment-last-fragment"));
}

#[test]
fn a_full_observation_queue_never_blocks_supervision() {
    let (observer, _unread, dropped) = localvox_light_process::OutputObserver::channel(1);
    let started = Instant::now();
    let out = localvox_light_process::run_observed(
        helper("noisy"),
        None,
        Limits::new(Duration::from_secs(8)),
        || false,
        |_, _| {},
        Some(observer),
    )
    .unwrap();
    assert!(out.termination.success());
    assert!(dropped.load(std::sync::atomic::Ordering::Relaxed) > 0);
    assert!(started.elapsed() < Duration::from_secs(4));
    assert!(out.stdout.len() >= 2 * 1024 * 1024);
}

#[test]
fn cancellation_stops_the_entire_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let pulse = tmp.path().join("pulse");
    let mut cmd = helper("tree");
    cmd.env("LOCALVOX_TEST_PULSE", &pulse);
    let out = run(
        cmd,
        None,
        Limits::new(Duration::from_secs(10)),
        || pulse.exists(),
        |_, _| {},
    )
    .unwrap();
    assert!(matches!(out.termination, Termination::Cancelled));
    assert_stopped(&pulse);
}

#[test]
fn nested_supervisors_remain_owned_by_the_outer_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let pulse = tmp.path().join("pulse");
    let mut cmd = helper("owner");
    cmd.env("LOCALVOX_TEST_PULSE", &pulse);
    let out = run(
        cmd,
        None,
        Limits::new(Duration::from_secs(10)),
        || pulse.exists(),
        |_, _| {},
    )
    .unwrap();
    assert!(matches!(out.termination, Termination::Cancelled));
    assert_stopped(&pulse);
}

#[test]
fn timeout_stops_the_entire_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let pulse = tmp.path().join("pulse");
    let mut cmd = helper("tree");
    cmd.env("LOCALVOX_TEST_PULSE", &pulse);
    let out = run(
        cmd,
        None,
        Limits::new(Duration::from_secs(2)),
        || false,
        |_, _| {},
    )
    .unwrap();
    assert!(matches!(out.termination, Termination::TimedOut));
    assert_stopped(&pulse);
}

#[test]
fn normal_parent_exit_cleans_descendants_without_waiting_for_pipes() {
    let tmp = tempfile::tempdir().unwrap();
    let pulse = tmp.path().join("pulse");
    let mut cmd = helper("orphan");
    cmd.env("LOCALVOX_TEST_PULSE", &pulse);
    let out = run(
        cmd,
        None,
        Limits::new(Duration::from_secs(10)),
        || false,
        |_, _| {},
    )
    .unwrap();
    assert!(out.termination.success(), "{:?}", out.termination);
    assert_stopped(&pulse);
}

#[test]
fn output_is_bounded_even_if_child_already_exited() {
    let mut limits = Limits::new(Duration::from_secs(10));
    limits.output_bytes = 4096;
    let out = run(helper("noisy"), None, limits, || false, |_, _| {}).unwrap();
    assert!(matches!(out.termination, Termination::OutputLimit));
    assert!(out.stdout.len() <= 4096);
}

#[test]
fn spawn_failure_returns_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(run(
        Command::new(tmp.path().join("does-not-exist")),
        None,
        Limits::new(Duration::from_secs(1)),
        || false,
        |_, _| {}
    )
    .is_err());
}

#[cfg(windows)]
#[test]
fn abrupt_supervisor_death_closes_the_windows_job() {
    let tmp = tempfile::tempdir().unwrap();
    let pulse = tmp.path().join("pulse");
    let mut owner = helper("owner")
        .env("LOCALVOX_TEST_PULSE", &pulse)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    await_pulse(&pulse);
    owner.kill().unwrap();
    owner.wait().unwrap();
    assert_stopped(&pulse);
}
