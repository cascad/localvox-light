//! Installation plans and package publication use only private filesystem fixtures.
use localvox_light::setup::{
    bundle::{self, Bundle},
    install,
    plan::{Action, Component},
};
use localvox_setup_ui::{Selection, SummarySource};
use std::path::Path;

fn put(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn fixture_bundle(root: &Path) {
    for app in [
        "localvox-light",
        "localvox-process",
        "localvox-tui",
        "vox",
        "localvox-install",
    ] {
        put(&root.join(bundle::executable(app)), b"binary fixture");
    }
    for name in [".env.defaults", "scripts/models.json"] {
        put(&root.join(name), b"{}");
    }
    for name in ["fetch-models", "model-paths", "setup-tools", "setup-ollama"] {
        put(
            &root.join(format!(
                "scripts/{name}.{}",
                if cfg!(windows) { "ps1" } else { "sh" }
            )),
            b"# fixture",
        );
    }
    for name in if cfg!(windows) {
        vec![
            "libvosk.dll",
            "libgcc_s_seh-1.dll",
            "libstdc++-6.dll",
            "libwinpthread-1.dll",
        ]
    } else if cfg!(target_os = "macos") {
        vec!["libvosk.dylib"]
    } else {
        vec!["libvosk.so"]
    } {
        put(&root.join(name), b"library fixture");
    }
    manifest(root);
}

fn manifest(root: &Path) {
    let mut files = vec![];
    fn scan(root: &Path, dir: &Path, files: &mut Vec<serde_json::Value>) {
        for item in std::fs::read_dir(dir).unwrap() {
            let path = item.unwrap().path();
            if path.is_dir() {
                scan(root, &path, files)
            } else if path != root.join("bundle.json") {
                files.push(serde_json::json!({"name":path.strip_prefix(root).unwrap().to_string_lossy().replace('\\',"/"),"sha256":bundle::digest(&path).unwrap(),"bytes":path.metadata().unwrap().len()}));
            }
        }
    }
    scan(root, root, &mut files);
    put(
        &root.join("bundle.json"),
        serde_json::to_string(
            &serde_json::json!({"schema":1,"target":bundle::target(),"files":files}),
        )
        .unwrap()
        .as_bytes(),
    );
}

fn ready_models(root: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let vosk = root.join("Vosk готовая модель");
    let gigaam = root.join("GigaAM other drive");
    put(&vosk.join("am/final.mdl"), b"fixture");
    for name in [
        "v3_e2e_ctc.int8.onnx",
        "v3_e2e_ctc.yaml",
        "v3_e2e_ctc_vocab.txt",
    ] {
        put(&gigaam.join(name), b"fixture");
    }
    (vosk, gigaam)
}

fn ready_config(destination: &Path, vosk: &Path, gigaam: &Path) {
    let values = [
        ("LOCALVOX_LIGHT_MODEL", vosk.display().to_string()),
        ("LOCALVOX_ASR_MODEL_DIR", gigaam.display().to_string()),
        ("LOCALVOX_LLM_BASE_URL", "http://127.0.0.1:1/v1".into()),
        ("LOCALVOX_LIGHT_AUTOCOOK_SUMMARY", "off".into()),
        ("LOCALVOX_DIARIZE", "off".into()),
        ("LOCALVOX_SETUP_FEATURES", "[]".into()),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_string(), Some(value)))
    .collect::<Vec<_>>();
    put(
        &destination.join(".env"),
        localvox_light_core::env_file::apply("# private\n", &values).as_bytes(),
    );
}

#[test]
fn runtime_startup_accepts_missing_env_and_reports_parse_errors_without_secret_values() {
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let binary = bin.join(bundle::executable("localvox-light"));
    std::fs::copy(env!("CARGO_BIN_EXE_localvox-light"), &binary).unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let libraries = if cfg!(windows) {
        vec![
            "libvosk.dll",
            "libgcc_s_seh-1.dll",
            "libstdc++-6.dll",
            "libwinpthread-1.dll",
        ]
    } else if cfg!(target_os = "macos") {
        vec!["libvosk.dylib"]
    } else {
        vec!["libvosk.so"]
    };
    for name in libraries {
        std::fs::copy(repo.join("vosk-lib").join(name), bin.join(name)).unwrap();
    }
    std::fs::copy(repo.join(".env.defaults"), bin.join(".env.defaults")).unwrap();
    let cwd = temp.path().join("cwd");
    std::fs::create_dir(&cwd).unwrap();
    // Parent .env must not accidentally be loaded when the cwd has none.
    put(
        &temp.path().join(".env"),
        b"PRIVATE_TOKEN='never-print-this\n",
    );
    put(&bin.join(".env"), b"PRIVATE_TOKEN='never-print-this\n");
    let mut command = std::process::Command::new(&binary);
    command
        .arg("--model-paths-json")
        .current_dir(&cwd)
        .env("HOME", temp.path())
        .env("USERPROFILE", temp.path());
    let broken = command.output().unwrap();
    assert!(String::from_utf8_lossy(&broken.stderr).contains("ERROR"));
    assert!(!String::from_utf8_lossy(&broken.stderr).contains("never-print-this"));
    std::fs::remove_file(bin.join(".env")).unwrap();
    let fresh = command.output().unwrap();
    assert!(
        fresh.status.success(),
        "{}",
        String::from_utf8_lossy(&fresh.stderr)
    );
    assert!(!String::from_utf8_lossy(&fresh.stderr).contains("ERROR"));
    serde_json::from_slice::<serde_json::Value>(&fresh.stdout).unwrap();
}

#[test]
fn separate_ready_model_paths_are_validated_and_noop_reconciliation_preserves_files() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let destination = temp.path().join("installed");
    fixture_bundle(&source);
    let (vosk, gigaam) = ready_models(temp.path());
    ready_config(&destination, &vosk, &gigaam);
    let (config, _, _) = install::configuration(&destination).unwrap();
    let discovered = install::layout(&config, &source, &destination);
    let selected = Selection {
        custom: true,
        features: vec![],
        program_dir: destination.display().to_string(),
        models_dir: temp.path().join("future downloads").display().to_string(),
        model_paths: [
            (
                localvox_setup_ui::ModelKind::Vosk,
                vosk.display().to_string(),
            ),
            (
                localvox_setup_ui::ModelKind::Gigaam,
                gigaam.display().to_string(),
            ),
        ]
        .into(),
        summary: SummarySource::Ollama("not-used".into()),
    };
    let mut first = install::prepare(&source, &selected, &discovered).unwrap();
    assert!(first.plan.actions.is_empty());
    first.register_path = false;
    first
        .apply(&Bundle::read(&source).unwrap(), &source)
        .unwrap();
    assert!(!destination.join("cache/installation.pending.json").exists());
    let env = destination.join(".env");
    let before = (
        std::fs::read(&env).unwrap(),
        env.metadata().unwrap().modified().unwrap(),
    );
    let (config, _, existing) = install::configuration(&destination).unwrap();
    let layout = install::layout(&config, &destination, &destination);
    let view = install::view(&config, &layout, &destination, existing).unwrap();
    let selected = view.initial.unwrap();
    assert!(selected.features.is_empty());
    assert_eq!(selected.models_dir, first.selected.models_dir);
    let mut repeat = install::prepare(&destination, &selected, &layout).unwrap();
    repeat.register_path = false;
    assert!(repeat.plan.actions.is_empty());
    assert!(repeat.plan.changes.is_empty(), "{:?}", repeat.plan.changes);
    // A true no-op must also work while the daemon owns its archive.
    let _daemon = localvox_light_core::session::try_instance_lock(&layout.work_dir)
        .unwrap()
        .unwrap();
    repeat
        .apply(&Bundle::read(&destination).unwrap(), &destination)
        .unwrap();
    assert_eq!(
        (
            std::fs::read(&env).unwrap(),
            env.metadata().unwrap().modified().unwrap()
        ),
        before
    );
    assert_eq!(
        std::fs::read(vosk.join("am/final.mdl")).unwrap(),
        b"fixture"
    );
    let mut bad = selected;
    bad.model_paths.insert(
        localvox_setup_ui::ModelKind::Vosk,
        temp.path().join("missing").display().to_string(),
    );
    assert!(install::prepare(&destination, &bad, &layout).is_err());
}

#[test]
fn interrupted_intent_is_recoverable_and_never_becomes_active_settings() {
    use localvox_light::setup::pending;
    let temp = tempfile::tempdir().unwrap();
    let install = temp.path().join("installed");
    let selected = Selection {
        custom: true,
        features: vec![],
        program_dir: install.display().to_string(),
        models_dir: temp.path().join("weights").display().to_string(),
        summary: SummarySource::Ollama("tiny:test".into()),
        model_paths: Default::default(),
    };
    assert!(pending::read(&install).unwrap().is_none());
    let first = pending::write(&install, &selected, false).unwrap();
    assert!(!install.join(".env").exists());
    assert_eq!(
        pending::read(&install).unwrap().unwrap().models_dir,
        selected.models_dir
    );
    let mut next = selected;
    next.models_dir = temp.path().join("other weights").display().to_string();
    let second = pending::write(&install, &next, false).unwrap();
    pending::complete(&install, &first).unwrap();
    assert!(pending::read(&install).unwrap().is_some());
    pending::complete(&install, &second).unwrap();
    assert!(pending::read(&install).unwrap().is_none());
    put(&install.join("cache/installation.pending.json"), b"broken");
    assert!(pending::read(&install).is_err());
}

#[test]
fn configuration_mode_needs_no_bundle_and_preserves_the_active_env_location() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("development binaries");
    std::fs::create_dir(&source).unwrap();
    let (vosk, gigaam) = ready_models(temp.path());
    ready_config(&source, &vosk, &gigaam);
    let active = temp.path().join("owner.env");
    std::fs::rename(source.join(".env"), &active).unwrap();
    let (config, _, _) = install::configuration_at(&active).unwrap();
    let layout = install::layout(&config, &source, &source);
    let selected = Selection {
        custom: true,
        features: vec![],
        program_dir: source.display().to_string(),
        models_dir: temp.path().join("models").display().to_string(),
        summary: SummarySource::Ollama("unused".into()),
        model_paths: Default::default(),
    };
    let mut configuration =
        install::prepare_configuration(&source, &selected, &layout, &active).unwrap();
    configuration.register_path = false;
    assert!(configuration.plan.actions.is_empty());
    assert!(!configuration.apply_configuration(&source).unwrap());
    assert!(std::fs::read_to_string(&active)
        .unwrap()
        .contains("LOCALVOX_SETUP_FEATURES=[]"));
    assert!(!source.join(".env").exists());
    assert!(!source.join("bundle.json").exists());
    let mut moved = selected;
    moved.program_dir = temp.path().join("other program").display().to_string();
    assert!(install::prepare_configuration(&source, &moved, &layout, &active).is_err());
}

mod terminal {
    use super::*;
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    use std::{
        io::{Read, Write},
        sync::mpsc,
        time::{Duration, Instant},
    };

    struct Console {
        child: Box<dyn portable_pty::Child + Send + Sync>,
        writer: Box<dyn Write + Send>,
        receiver: mpsc::Receiver<Vec<u8>>,
        output: Vec<u8>,
        queries: usize,
        _master: Box<dyn portable_pty::MasterPty + Send>,
    }
    impl Drop for Console {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
    impl Console {
        fn start(source: &Path, destination: &Path, home: &Path) -> Self {
            let pair = native_pty_system()
                .openpty(PtySize {
                    rows: 36,
                    cols: 110,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .unwrap();
            let mut command =
                CommandBuilder::new(source.join(bundle::executable("localvox-install")));
            command.arg("--install-dir");
            command.arg(destination);
            command.arg("--no-path-update");
            command.cwd(home);
            for key in ["HOME", "USERPROFILE", "LOCALAPPDATA", "APPDATA"] {
                command.env(key, home);
            }
            command.env("TERM", "xterm-256color");
            command.env("WT_SESSION", "localvox-install-fixture");
            command.env("LANG", "C.UTF-8");
            let child = pair.slave.spawn_command(command).unwrap();
            drop(pair.slave);
            let mut reader = pair.master.try_clone_reader().unwrap();
            let writer = pair.master.take_writer().unwrap();
            let (tx, receiver) = mpsc::channel();
            std::thread::spawn(move || {
                let mut buffer = [0; 8192];
                while let Ok(n) = reader.read(&mut buffer) {
                    if n == 0 || tx.send(buffer[..n].to_vec()).is_err() {
                        break;
                    }
                }
            });
            Self {
                child,
                writer,
                receiver,
                output: vec![],
                queries: 0,
                _master: pair.master,
            }
        }
        fn send(&mut self, keys: &[u8]) {
            self.writer.write_all(keys).unwrap();
            self.writer.flush().unwrap();
        }
        fn receive(&mut self) {
            if let Ok(bytes) = self.receiver.recv_timeout(Duration::from_millis(50)) {
                self.output.extend(bytes);
                assert!(self.output.len() < 1024 * 1024, "bounded fixture output");
                let queries = String::from_utf8_lossy(&self.output)
                    .matches("\x1b[6n")
                    .count();
                while self.queries < queries {
                    self.send(b"\x1b[1;1R");
                    self.queries += 1;
                }
            }
        }
        fn wait(&mut self, text: &str) {
            let deadline = Instant::now() + Duration::from_secs(25);
            while !String::from_utf8_lossy(&self.output).contains(text) {
                assert!(
                    Instant::now() < deadline,
                    "waiting for {text}: {}",
                    String::from_utf8_lossy(&self.output)
                );
                self.receive();
            }
        }
        fn finish(&mut self) -> String {
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                self.receive();
                if let Some(status) = self.child.try_wait().unwrap() {
                    assert!(
                        status.success(),
                        "{}",
                        String::from_utf8_lossy(&self.output)
                    );
                    return String::from_utf8_lossy(&self.output).into_owned();
                }
                assert!(
                    Instant::now() < deadline,
                    "installer did not exit: {}",
                    String::from_utf8_lossy(&self.output)
                );
            }
        }
    }

    #[test]
    fn native_wizard_cancel_apply_and_repeat_use_a_real_terminal() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("staging");
        let destination = temp.path().join("installed");
        let home = temp.path().join("isolated home");
        std::fs::create_dir(&home).unwrap();
        fixture_bundle(&source);
        std::fs::copy(
            env!("CARGO_BIN_EXE_localvox-install"),
            source.join(bundle::executable("localvox-install")),
        )
        .unwrap();
        #[cfg(windows)]
        for name in [
            "libvosk.dll",
            "libgcc_s_seh-1.dll",
            "libstdc++-6.dll",
            "libwinpthread-1.dll",
        ] {
            std::fs::copy(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../vosk-lib")
                    .join(name),
                source.join(name),
            )
            .unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                source.join("localvox-install"),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
            let name = if cfg!(target_os = "macos") {
                "libvosk.dylib"
            } else {
                "libvosk.so"
            };
            std::fs::copy(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../vosk-lib")
                    .join(name),
                source.join(name),
            )
            .unwrap();
        }
        manifest(&source);
        let (vosk, gigaam) = ready_models(temp.path());
        ready_config(&destination, &vosk, &gigaam);
        let before = std::fs::read(destination.join(".env")).unwrap();
        let mut cancel = Console::start(&source, &destination, &home);
        cancel.wait("Сохранить текущий набор");
        cancel.send(b"\r");
        cancel.wait("Начать установку?");
        cancel.send(b"\x1b[B\x1b[B\r");
        cancel.finish();
        drop(cancel);
        assert_eq!(std::fs::read(destination.join(".env")).unwrap(), before);
        assert!(!destination.join("cache").exists());
        assert!(!destination.join("vox.exe").exists());
        let invalid = localvox_light_core::env_file::apply(
            std::str::from_utf8(&before).unwrap(),
            &[
                (
                    "LOCALVOX_SETUP_FEATURES".into(),
                    Some("[\"player\"]".into()),
                ),
                (
                    "LOCALVOX_MPV".into(),
                    Some(temp.path().join("missing-player").display().to_string()),
                ),
            ],
        );
        put(&destination.join(".env"), invalid.as_bytes());
        let mut rejected = Console::start(&source, &destination, &home);
        rejected.wait("Сохранить текущий набор");
        rejected.send(b"\r");
        rejected.wait("Заданный путь mpv не готов");
        rejected.wait("Продолжить текущий шаг");
        rejected.send(b"\x1b[B\x1b[B\r");
        rejected.finish();
        drop(rejected);
        assert_eq!(
            std::fs::read(destination.join(".env")).unwrap(),
            invalid.as_bytes()
        );
        assert!(!destination.join("cache").exists());
        put(&destination.join(".env"), &before);
        let mut apply = Console::start(&source, &destination, &home);
        apply.wait("Сохранить текущий набор");
        apply.send(b"\r");
        apply.wait("Начать установку?");
        apply.send(b"\r");
        assert!(apply.finish().contains("LocalVox готов"));
        drop(apply);
        Bundle::read(&destination).unwrap();
        assert!(!destination.join("cache/installation.pending.json").exists());
        let env = destination.join(".env");
        let before = (
            std::fs::read(&env).unwrap(),
            env.metadata().unwrap().modified().unwrap(),
        );
        let mut repeat = Console::start(&destination, &destination, &home);
        repeat.wait("Сохранить текущий набор");
        repeat.send(b"\r");
        repeat.wait("Начать установку?");
        repeat.send(b"\r");
        repeat.finish();
        drop(repeat);
        assert_eq!(
            (
                std::fs::read(&env).unwrap(),
                env.metadata().unwrap().modified().unwrap()
            ),
            before
        );
        assert!(
            !home.join(".localvox").exists(),
            "portable fixture never registers a real command/location"
        );
    }
}

#[test]
fn corrupt_source_and_unsafe_manifest_fail_before_destination_changes() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("staging");
    let destination = temp.path().join("must not exist");
    fixture_bundle(&source);
    let bundle = Bundle::read(&source).unwrap();
    put(&source.join(bundle::executable("vox")), b"corrupt");
    assert!(bundle.publish(&source, &destination).is_err());
    assert!(!destination.exists());
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(source.join("bundle.json")).unwrap()).unwrap();
    manifest["files"][0]["name"] = "../outside".into();
    put(
        &source.join("bundle.json"),
        serde_json::to_string(&manifest).unwrap().as_bytes(),
    );
    assert!(Bundle::read(&source).is_err());
    assert!(!temp.path().join("outside").exists());
}

#[test]
fn publishing_reuses_unchanged_files_and_preserves_private_configuration() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("staging");
    let destination = temp.path().join("LocalVox with spaces");
    fixture_bundle(&source);
    put(&destination.join(".env"), b"# private\nOTHER=keep\n");
    let bundle = Bundle::read(&source).unwrap();
    bundle.publish(&source, &destination).unwrap();
    Bundle::read(&destination).unwrap();
    let binary = destination.join(bundle::executable("localvox-install"));
    let before = binary.metadata().unwrap().modified().unwrap();
    bundle.publish(&source, &destination).unwrap();
    assert_eq!(binary.metadata().unwrap().modified().unwrap(), before);
    assert_eq!(
        std::fs::read(destination.join(".env")).unwrap(),
        b"# private\nOTHER=keep\n"
    );
}

#[test]
fn missing_config_is_normal_but_bad_syntax_is_rejected_without_exposing_values() {
    let temp = tempfile::tempdir().unwrap();
    assert!(!install::configuration(temp.path()).unwrap().2);
    put(
        &temp.path().join(".env"),
        b"PRIVATE_TOKEN='do-not-log-this\n",
    );
    let error = match install::configuration(temp.path()) {
        Ok(_) => panic!("invalid config accepted"),
        Err(error) => format!("{error:#}"),
    };
    assert!(error.contains("KEY=value"));
    assert!(!error.contains("do-not-log-this"));
}

#[test]
fn selected_download_folder_is_exact_and_disabled_summary_never_schedules_llm() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("staging");
    let destination = temp.path().join("install");
    fixture_bundle(&source);
    let config=format!("LOCALVOX_LIGHT_MODEL='{}'\nLOCALVOX_ASR_MODEL_DIR='{}'\nLOCALVOX_LLM_BASE_URL=http://127.0.0.1:1/v1\nLOCALVOX_LIGHT_AUTOCOOK_SUMMARY=off\n",temp.path().join("absent-vosk").display(),temp.path().join("absent-gigaam").display());
    put(&destination.join(".env"), config.as_bytes());
    let (config, _, _) = install::configuration(&destination).unwrap();
    let layout = install::layout(&config, &source, &destination);
    let model_dir = temp.path().join("Custom weights");
    let selected = Selection {
        custom: true,
        features: vec![],
        program_dir: destination.display().to_string(),
        models_dir: model_dir.display().to_string(),
        summary: SummarySource::Ollama("must-not-run".into()),
        model_paths: Default::default(),
    };
    let installation = install::prepare(&source, &selected, &layout).unwrap();
    assert_eq!(installation.plan.actions.len(), 2);
    for action in &installation.plan.actions {
        let Action::Download {
            component,
            destination,
            ..
        } = action
        else {
            panic!("unexpected LLM/system action")
        };
        assert_eq!(destination.parent(), Some(model_dir.as_path()));
        assert!([Component::Vosk, Component::Gigaam].contains(component));
    }
    assert!(installation.describe().contains("Сводки: выключены"));
    assert!(!model_dir.exists());
}

#[test]
fn future_destination_identity_detects_staging_descendants_without_creating_them() {
    let temp = tempfile::tempdir().unwrap();
    let nested = temp.path().join("new/path");
    assert!(bundle::canonical_destination(&nested)
        .unwrap()
        .starts_with(temp.path().canonicalize().unwrap()));
    assert!(!nested.exists());
}

#[test]
fn bundled_player_has_priority_but_never_overrides_an_explicit_user_path() {
    use localvox_light::setup::tools::Tool;
    let temp = tempfile::tempdir().unwrap();
    let player = temp
        .path()
        .join("tools/mpv")
        .join(bundle::executable("mpv"));
    put(&player, b"player fixture");
    assert_eq!(
        Tool::Mpv.find(&Default::default(), temp.path()),
        Some(player)
    );
    let explicit = temp.path().join(bundle::executable("chosen-player"));
    let config = [("LOCALVOX_MPV".into(), explicit.display().to_string())].into();
    assert!(Tool::Mpv.find(&config, temp.path()).is_none());
    put(&explicit, b"chosen fixture");
    assert_eq!(Tool::Mpv.find(&config, temp.path()), Some(explicit));
}
