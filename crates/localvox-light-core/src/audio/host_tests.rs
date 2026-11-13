//! Fresh processes matter: another test must not pre-initialize CPAL's static enumerator.
use super::*;
use localvox_light_process::{run, Limits};
use std::process::Command;

#[test]
fn device_discovery_survives_short_lived_threads() {
    for mode in ["host-only", "sequential", "concurrent", "sta-first"] {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args([
            "--ignored",
            "--exact",
            "audio::host_tests::discovery_fixture",
            "--nocapture",
        ]);
        command.env("LOCALVOX_AUDIO_HOST_FIXTURE", mode);
        let mut limits = Limits::new(Duration::from_secs(25));
        limits.background = false;
        let result = run(command, None, limits, || false, |_, _| {}).unwrap();
        assert!(
            result.termination.success(),
            "audio discovery {mode}: {:?}\n{}\n{}",
            result.termination,
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

fn discover() {
    let inputs = input_device_choices();
    assert!(inputs.iter().any(|choice| choice.is_default));
    let _ = default_capture_id();
    assert!(!output_device_choices().is_empty());
}

#[test]
#[ignore = "owned subprocess only; no capture, devices may be absent"]
fn discovery_fixture() {
    let Ok(mode) = std::env::var("LOCALVOX_AUDIO_HOST_FIXTURE") else {
        return;
    };
    if mode == "sta-first" {
        thread::spawn(|| {
            wasapi::initialize_sta().ok().unwrap();
            let choices = input_device_choices();
            let default_id = default_capture_id();
            wasapi::deinitialize();
            assert!(choices.iter().all(|choice| choice.is_default));
            assert!(default_id.is_none());
        })
        .join()
        .unwrap();
    }
    for _ in 0..4 {
        let workers: Vec<_> = (0..if mode == "concurrent" { 4 } else { 1 })
            .map(|_| {
                let host_only = mode == "host-only";
                thread::spawn(move || {
                    if host_only {
                        let _ = default_host().unwrap().input_devices().unwrap().count();
                    } else {
                        discover();
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
    }
}
