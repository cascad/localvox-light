//! Exercise the installed entry point without loading models or starting devices/services.
use serde_json::Value;
use std::{path::Path, process::Command};

fn paths(exe: &Path, cwd: &Path, home: &Path, args: &[&str]) -> Value {
    let mut command = Command::new(exe);
    command
        .current_dir(cwd)
        .arg("--model-paths-json")
        .args(args);
    for (key, _) in std::env::vars().filter(|(key, _)| key.starts_with("LOCALVOX_")) {
        command.env_remove(key);
    }
    command.env("HOME", home).env("USERPROFILE", home);
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn entry<'a>(report: &'a Value, id: &str) -> &'a Value {
    report["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == id)
        .unwrap()
}

#[test]
fn copied_binary_resolves_fresh_portable_dotenv_and_cli_without_writes() {
    let temp = tempfile::tempdir().unwrap();
    let install = temp.path().join("Установка с пробелами");
    let cwd = temp.path().join("workspace");
    let home = temp.path().join("Профиль с пробелами");
    for directory in [&install, &cwd, &home] {
        std::fs::create_dir(directory).unwrap();
    }
    let original = Path::new(env!("CARGO_BIN_EXE_localvox-light"));
    let exe = install.join(original.file_name().unwrap());
    std::fs::copy(original, &exe).unwrap();
    // dotenv searches ancestors of cwd; fence off the developer's repository .env.
    std::fs::write(install.join(".env"), "# isolated fresh configuration\n").unwrap();
    // Native loaders may resolve relative to the copied executable (macOS/Windows).
    for file in std::fs::read_dir(original.parent().unwrap())
        .unwrap()
        .flatten()
    {
        let path = file.path();
        if path.extension().is_some_and(|extension| {
            extension == "dll" || extension == "dylib" || extension == "so"
        }) {
            std::fs::copy(&path, install.join(path.file_name().unwrap())).unwrap();
        }
    }
    let report = paths(&exe, &cwd, &home, &[]);
    assert_eq!(report["schema"], 1);
    assert_eq!(
        report["user_root"],
        home.join(".localvox").to_string_lossy().as_ref()
    );
    for c in localvox_light_core::model_paths::components() {
        assert_eq!(
            entry(&report, &c.id)["path"],
            home.join(".localvox")
                .join(&c.directory)
                .to_string_lossy()
                .as_ref()
        );
        assert_eq!(entry(&report, &c.id)["source"], "user");
    }
    let portable = install.join("models/gigaam-v3-e2e-ctc");
    std::fs::create_dir_all(&portable).unwrap();
    let report = paths(&exe, &cwd, &home, &[]);
    assert_eq!(
        entry(&report, "gigaam")["path"],
        portable.to_string_lossy().as_ref()
    );
    assert_eq!(entry(&report, "gigaam")["source"], "portable");
    let external = temp.path().join("Веса на другом диске");
    let content = format!(
        "LOCALVOX_ASR_MODEL_DIR={}\nLOCALVOX_LIGHT_MODEL={}\n",
        serde_json::to_string(&external).unwrap(),
        serde_json::to_string(&external.join("vosk")).unwrap()
    );
    let env = install.join(".env");
    std::fs::write(&env, &content).unwrap();
    let mtime = env.metadata().unwrap().modified().unwrap();
    let report = paths(&exe, &cwd, &home, &["--model", "cli/vosk"]);
    assert_eq!(
        entry(&report, "gigaam")["path"],
        external.to_string_lossy().as_ref()
    );
    assert_eq!(entry(&report, "gigaam")["source"], "explicit");
    assert_eq!(
        entry(&report, "vosk")["path"],
        cwd.join("cli/vosk").to_string_lossy().as_ref()
    );
    assert_eq!(entry(&report, "vosk")["source"], "explicit");
    assert!(!entry(&report, "gigaam")["ready"].as_bool().unwrap());
    assert_eq!(std::fs::read_to_string(&env).unwrap(), content);
    assert_eq!(env.metadata().unwrap().modified().unwrap(), mtime);
    assert_eq!(std::fs::read_dir(&home).unwrap().count(), 0);
    assert_eq!(std::fs::read_dir(&cwd).unwrap().count(), 0);
    assert!(!external.exists());
}
