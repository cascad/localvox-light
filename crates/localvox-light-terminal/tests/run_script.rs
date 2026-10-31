//! Exercise the actual launcher script in an isolated PowerShell process.
//! Cargo and executable artifacts are fixtures; no capture engine is started.
#![cfg(windows)]
use std::{fs, path::PathBuf, process::Command};

#[test]
fn run_script_obeys_build_only_flags_artifacts_and_failure() {
    let directory = tempfile::tempdir().unwrap();
    let harness = directory.path().join("harness.ps1");
    fs::write(&harness, include_str!("fixtures/run-script.ps1")).unwrap();
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/run.ps1");
    let shell = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    for case in [
        "build-only-tui",
        "run-tui",
        "run-daemon",
        "run-desktop",
        "conflict",
        "build-failure",
        "missing-artifact",
        "different-directories",
    ] {
        let output = Command::new(&shell)
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&harness)
            .arg(&script)
            .arg(directory.path().join(case))
            .arg(case)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{case}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn terminal_binary_runs_from_a_clean_bundle_without_server_or_native_models() {
    let directory = tempfile::tempdir().unwrap();
    let exe = directory.path().join("localvox-tui.exe");
    fs::copy(env!("CARGO_BIN_EXE_localvox-tui"), &exe).unwrap();
    let output = Command::new(exe)
        .arg("--version")
        .current_dir(directory.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("localvox-tui "));
}
