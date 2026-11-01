use super::*;

fn command(binary: &Path, root: &Path, worker: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(binary);
    command.env_clear();
    for key in [
        "PATH",
        "SystemRoot",
        "WINDIR",
        "COMSPEC",
        "PATHEXT",
        "LD_LIBRARY_PATH",
        "DYLD_LIBRARY_PATH",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command
        .current_dir(root)
        .env("TEMP", root)
        .env("TMP", root)
        .env("LOCALVOX_LIGHT_AUTOCOOK", "1")
        .env("LOCALVOX_LIGHT_AUTOCOOK_SUMMARY", "0")
        .env("LOCALVOX_LIGHT_AUTOCOOK_INTERVAL_SEC", "1")
        .env("LOCALVOX_LIGHT_AUTOCOOK_QUIESCENT_SEC", "3600")
        .env("LOCALVOX_ASR_MODEL_DIR", root.join("missing-model"))
        .arg("--work-dir")
        .arg(root)
        .arg("--worker")
        .arg(worker);
    command
}

#[test]
#[ignore = "requires sibling localvox-queue-profile and localvox-process builds; no models/devices"]
fn real_coordinator_empty_timeout_and_preflight_failure_are_bounded() {
    let directory = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let runner = directory.join(if cfg!(windows) {
        "localvox-queue-profile.exe"
    } else {
        "localvox-queue-profile"
    });
    let worker = directory.join(worker_name());
    assert!(
        runner.is_file() && worker.is_file(),
        "build queue runner and worker first"
    );
    for mode in ["empty", "timeout", "failed", "cancelled", "incompatible"] {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join(MARKER), "localvox-profile/v1\n").unwrap();
        if matches!(mode, "timeout" | "failed" | "cancelled") {
            fs::create_dir_all(root.path().join("sessions/fixture")).unwrap();
            JobQueue::mutate(root.path(), |q| {
                q.enqueue_cook("fixture", false, false, false)
            });
            if mode == "cancelled" {
                JobQueue::mutate(root.path(), |q| q.try_complete_cancellation("fixture")).unwrap();
            }
        }
        let (binary, chosen_worker) = if mode == "incompatible" {
            let bin = root.path().join("bin");
            fs::create_dir(&bin).unwrap();
            let copied = bin.join(runner.file_name().unwrap());
            fs::copy(&runner, &copied).unwrap();
            // The Rust test harness rejects --worker-protocol; it never executes a phase.
            let bad_worker = bin.join(worker_name());
            fs::copy(std::env::current_exe().unwrap(), &bad_worker).unwrap();
            (copied, bad_worker)
        } else {
            (runner.clone(), worker.clone())
        };
        let mut command = command(&binary, root.path(), &chosen_worker);
        command
            .arg("--timeout-sec")
            .arg(if mode == "timeout" { "1" } else { "8" });
        let output = localvox_light_process::run(
            command,
            None,
            localvox_light_process::Limits::new(Duration::from_secs(15)),
            || false,
            |_, _| {},
        )
        .unwrap();
        let localvox_light_process::Termination::Exited(status) = output.termination else {
            panic!("{mode}: coordinator did not stop cooperatively")
        };
        assert_eq!(status.success(), mode == "empty", "{mode}");
        let report: Value =
            serde_json::from_slice(&fs::read(root.path().join(REPORT)).unwrap()).unwrap();
        assert_eq!(report["coordinator_joined"], true, "{mode}");
        assert_eq!(
            report["complete"],
            matches!(mode, "empty" | "failed" | "cancelled"),
            "{mode}"
        );
        assert_eq!(report["all_success"], mode == "empty", "{mode}");
        assert_eq!(
            report["reason"],
            match mode {
                "empty" | "failed" | "cancelled" => "terminal",
                "timeout" => "timeout",
                _ => "coordinator_stopped",
            }
        );
        assert_eq!(report["events_lost"], 0);
        assert!(!root.path().join(LOCK).exists());
        if mode == "timeout" {
            assert!(!report["events"].as_array().unwrap().is_empty());
            assert_eq!(report["jobs"].as_array().unwrap().len(), 1);
            assert_ne!(report["jobs"][0]["state"], "Done");
        }
        if mode == "cancelled" {
            assert_eq!(report["jobs"][0]["state"], "Cancelled");
            assert_eq!(report["jobs"][0]["attempt"], 0);
            assert!(report["events"].as_array().unwrap().is_empty());
            assert!(root.path().join("sessions/fixture").is_dir());
        }
        if mode == "failed" {
            assert_eq!(report["jobs"][0]["state"], "Failed");
            assert_eq!(report["jobs"][0]["attempt"], 3);
            assert_eq!(report["jobs"][0]["has_error"], true);
            assert!(report["jobs"][0].get("last_error").is_none());
        }
    }
}

fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(MARKER), "localvox-profile/v1\n").unwrap();
    let bin = tempfile::tempdir_in(root.path()).unwrap().keep();
    let worker = bin.join(worker_name());
    fs::write(&worker, b"not executed by validation").unwrap();
    let runner = bin.join("runner");
    (root, worker, runner)
}

#[test]
fn guards_reject_unmarked_recording_existing_report_and_unreadable_queue() {
    let (root, worker, runner) = fixture();
    assert!(validate(root.path(), &worker, &runner).is_ok());
    for (name, value) in [
        (MARKER, "wrong"),
        (jobs::RECORDING_MARKER, ""),
        (REPORT, "{}"),
        ("jobs.json", "broken"),
    ] {
        fs::write(root.path().join(name), value).unwrap();
        assert!(validate(root.path(), &worker, &runner).is_err(), "{name}");
        if name == MARKER {
            fs::write(root.path().join(name), "localvox-profile/v1\n").unwrap();
        } else {
            fs::remove_file(root.path().join(name)).unwrap();
        }
    }
    fs::remove_file(root.path().join(MARKER)).unwrap();
    assert!(validate(root.path(), &worker, &runner).is_err());
}

#[test]
fn worker_identity_and_exclusive_claim_are_checked_before_execution() {
    let (root, worker, runner) = fixture();
    let other = root.path().join("other-worker");
    fs::write(&other, b"other").unwrap();
    assert!(validate(root.path(), &other, &runner).is_err());
    let claim = WorkspaceLock::acquire(root.path()).unwrap();
    assert!(WorkspaceLock::acquire(root.path()).is_err());
    drop(claim);
    assert!(WorkspaceLock::acquire(root.path()).is_ok());
    assert_eq!(fs::read(worker).unwrap(), b"not executed by validation");
}

#[test]
fn clocks_remain_unknown_if_missing_reversed_or_malformed() {
    assert_eq!(elapsed(None, Some("2026-11-01T10:00:00Z")), None);
    assert_eq!(elapsed(Some("bad"), Some("bad")), None);
    assert_eq!(
        elapsed(Some("2026-11-01T10:00:01Z"), Some("2026-11-01T10:00:00Z")),
        None
    );
    assert_eq!(
        elapsed(
            Some("2026-11-01T10:00:00Z"),
            Some("2026-11-01T10:00:01.250Z")
        ),
        Some(1.25)
    );
    assert_eq!(
        elapsed(Some("2026-11-01T10:00:00Z"), Some("2026-11-01T10:00:00Z")),
        Some(0.0)
    );
}

#[test]
fn unsafe_or_running_session_is_rejected_and_error_contents_are_not_reported() {
    let (root, worker, runner) = fixture();
    fs::create_dir_all(root.path().join("sessions/fixture")).unwrap();
    JobQueue::mutate(root.path(), |q| {
        q.enqueue_cook("fixture", false, false, false)
    });
    let queue = JobQueue::load(root.path());
    let mut job = queue.jobs()[0].clone();
    assert!(validate(root.path(), &worker, &runner).is_ok());
    job.last_error = Some("PRIVATE ERROR BODY".into());
    assert_eq!(metadata(&job)["has_error"], true);
    assert!(!metadata(&job).to_string().contains("PRIVATE"));
    for name in ["../outside", "C:/archive", "fixture/nested"] {
        job.session = name.into();
        fs::write(
            root.path().join("jobs.json"),
            serde_json::to_vec(&json!({"next_id":2,"jobs":[job.clone()]})).unwrap(),
        )
        .unwrap();
        assert!(validate(root.path(), &worker, &runner).is_err());
    }
    job.session = "fixture".into();
    for state in [JobState::Running, JobState::Cancelling] {
        job.state = state;
        fs::write(
            root.path().join("jobs.json"),
            serde_json::to_vec(&json!({"next_id":2,"jobs":[job.clone()]})).unwrap(),
        )
        .unwrap();
        assert!(validate(root.path(), &worker, &runner).is_err());
    }
}
