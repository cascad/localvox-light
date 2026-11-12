//! Exercise the actual launcher script in an isolated PowerShell process.
//! Cargo and executable artifacts are fixtures; no capture engine is started.
#![cfg(windows)]
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn invoke(harness: &Path, script: &Path, directory: &Path, case: &str) -> Output {
    let shell = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    Command::new(shell)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(harness)
        .arg(script)
        .arg(directory)
        .arg(case)
        .output()
        .unwrap()
}

#[test]
fn run_script_obeys_build_only_flags_artifacts_and_failure() {
    let directory = tempfile::tempdir().unwrap();
    let harness = directory.path().join("harness.ps1");
    fs::write(&harness, include_str!("fixtures/run-script.ps1")).unwrap();
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/run.ps1");
    for case in [
        "build-only-tui",
        "run-tui",
        "run-daemon",
        "run-desktop",
        "conflict",
        "build-failure",
        "missing-artifact",
        "different-directories",
        "normalized-directory",
    ] {
        let caller = if case == "normalized-directory" {
            directory.path().join(".").join(case)
        } else {
            directory.path().join(case)
        };
        let output = invoke(&harness, &script, &caller, case);
        assert!(
            output.status.success(),
            "{case}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn launcher_fixture_rejects_a_script_that_leaves_the_callers_directory() {
    let directory = tempfile::tempdir().unwrap();
    let scripts = directory.path().join("repo/scripts");
    fs::create_dir_all(&scripts).unwrap();
    let script = scripts.join("bad-run.ps1");
    fs::write(&script, "param([switch]$Tui,[switch]$BuildOnly,[string]$Profile)\nPush-Location -LiteralPath $PSScriptRoot\n").unwrap();
    let harness = directory.path().join("harness.ps1");
    fs::write(&harness, include_str!("fixtures/run-script.ps1")).unwrap();
    let output = invoke(
        &harness,
        &script,
        &directory.path().join("caller"),
        "build-only-tui",
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("run.ps1 did not restore cwd"));
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
