//! Drive the shipped preview with real terminal keys, not piped answers.
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

const DOWN: &[u8] = b"\x1b[B";
const ENTER: &[u8] = b"\r";

struct Child(Box<dyn portable_pty::Child + Send + Sync>);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Preview {
    child: Child,
    writer: Box<dyn Write + Send>,
    rx: mpsc::Receiver<Vec<u8>>,
    parser: vt100::Parser,
    output: Vec<u8>,
    queries: usize,
    _master: Box<dyn portable_pty::MasterPty + Send>,
    sandbox: tempfile::TempDir,
    before: BTreeMap<PathBuf, Vec<u8>>,
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, dir: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for item in std::fs::read_dir(dir).unwrap() {
            let item = item.unwrap();
            let path = item.path();
            if path.is_dir() {
                files.insert(path.strip_prefix(root).unwrap().to_owned(), vec![]);
                visit(root, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    std::fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

impl Preview {
    fn new(scenario: &str, cols: u16) -> Self {
        Self::terminal(scenario, cols, true)
    }

    fn terminal(scenario: &str, cols: u16, unicode: bool) -> Self {
        let sandbox = tempfile::tempdir().unwrap();
        // A broken private config must not be loaded by the visual simulator.
        std::fs::write(sandbox.path().join(".env"), b"PRIVATE_TOKEN='broken\n").unwrap();
        std::fs::create_dir(sandbox.path().join("keep-models")).unwrap();
        std::fs::write(sandbox.path().join("keep-models/weights"), b"do not touch").unwrap();
        let before = snapshot(sandbox.path());
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 36,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_localvox-setup-preview"));
        command.args(["--scenario", scenario]);
        command.cwd(sandbox.path());
        for key in ["HOME", "USERPROFILE", "LOCALAPPDATA", "APPDATA"] {
            command.env(key, sandbox.path());
        }
        command.env("TERM", "xterm-256color");
        command.env("CLICOLOR_FORCE", "1");
        // The prompt library uses these advertised capabilities to select its glyphs.
        // Exercise both the modern UTF-8 presentation and the ASCII fallback.
        if unicode {
            command.env("WT_SESSION", "installer-preview-fixture");
            command.env("LANG", "C.UTF-8");
        } else {
            command.env_remove("WT_SESSION");
            command.env("LANG", "C");
        }
        command.env("OLLAMA_HOST", "http://127.0.0.1:1");
        let child = Child(pair.slave.spawn_command(command).unwrap());
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer = pair.master.take_writer().unwrap();
        let (tx, rx) = mpsc::channel();
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
            rx,
            parser: vt100::Parser::new(36, cols, 0),
            output: vec![],
            queries: 0,
            _master: pair.master,
            sandbox,
            before,
        }
    }

    fn send(&mut self, keys: &[u8]) {
        self.writer.write_all(keys).unwrap();
        self.writer.flush().unwrap();
    }

    fn receive(&mut self, timeout: Duration) -> bool {
        let Ok(bytes) = self.rx.recv_timeout(timeout) else {
            return false;
        };
        self.parser.process(&bytes);
        self.output.extend(bytes);
        assert!(self.output.len() < 1024 * 1024, "bounded terminal output");
        // ConPTY asks the emulator for its inherited cursor position.
        let queries = String::from_utf8_lossy(&self.output)
            .matches("\x1b[6n")
            .count();
        while self.queries < queries {
            self.send(b"\x1b[1;1R");
            self.queries += 1;
        }
        true
    }

    fn wait(&mut self, text: &str) {
        self.wait_for(text, |screen| screen.contains(text));
    }

    fn active(&mut self, label: &str) {
        self.wait_for(label, |screen| {
            screen.lines().any(|line| {
                (line.starts_with("◆ ") || line.starts_with("* ")) && line.contains(label)
            })
        });
    }

    fn back(&mut self) {
        self.send(b"\x1b");
        self.wait("Продолжить текущий шаг");
        self.send(ENTER);
    }

    fn wait_for(&mut self, text: &str, condition: impl Fn(&str) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(12);
        while !condition(&self.parser.screen().contents()) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero() && self.receive(remaining),
                "waiting for {text:?}; screen:\n{}",
                self.parser.screen().contents()
            );
        }
    }

    fn capture(&mut self, name: &str) {
        if let Some(dir) = std::env::var_os("LOCALVOX_INSTALLER_CAPTURE_DIR") {
            // Only capture mode waits for the final paint; assertions use screen state.
            while self.receive(Duration::from_millis(60)) {}
            let dir = PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("{name}.ansi")), &self.output).unwrap();
            std::fs::write(
                dir.join(format!("{name}.txt")),
                self.parser.screen().contents(),
            )
            .unwrap();
        }
    }

    fn finish(mut self, message: &str) {
        self.wait(message);
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = self.child.0.try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline, "preview must exit");
            self.receive(Duration::from_millis(20));
        };
        while self.receive(Duration::from_millis(20)) {}
        assert!(status.success(), "{status:?}");
        assert!(!self.parser.screen().hide_cursor(), "cursor restored");
        assert_eq!(
            snapshot(self.sandbox.path()),
            self.before,
            "no changes to home/config/models"
        );
        let output = String::from_utf8_lossy(&self.output);
        assert!(!output.contains("PRIVATE_TOKEN") && !output.contains("Operation cancelled"));
    }
}

#[test]
fn recommended_fresh_and_existing_reuse_are_safe() {
    for scenario in ["fresh", "existing"] {
        let mut demo = Preview::new(scenario, 110);
        demo.wait("Выбрать компоненты");
        demo.capture(&format!("{scenario}-welcome"));
        demo.send(ENTER);
        demo.wait("Показать демоустановку?");
        let screen = demo.parser.screen().contents();
        assert!(screen.contains("Ollama") && screen.contains("vox"));
        if scenario == "existing" {
            assert!(screen.contains("уже готово"));
        } else {
            assert!(screen.contains("≈2 ГБ"));
        }
        demo.capture(&format!("{scenario}-review"));
        demo.send(ENTER);
        demo.finish("Демо завершено");
    }
}

#[test]
fn space_toggles_components_and_skips_disabled_summary_at_80_columns() {
    let mut demo = Preview::terminal("fresh", 80, false);
    demo.wait("Выбрать компоненты");
    demo.send(DOWN);
    demo.send(ENTER);
    demo.wait("Дополнительно проверять имена");
    demo.capture("custom-components");
    // Leave only the optional name checker; recognition cannot be unchecked.
    for _ in 0..4 {
        demo.send(b" ");
        demo.send(DOWN);
    }
    demo.send(b" ");
    demo.send(ENTER);
    demo.wait("Выбрать другие папки");
    demo.send(ENTER);
    demo.wait("Показать демоустановку?");
    let screen = demo.parser.screen().contents();
    assert!(screen.contains("✓ Дополнительно проверять имена"));
    assert!(
        !screen.contains("✓ Готовить сводки") && !screen.contains("стандартное хранилище Ollama")
    );
    demo.capture("custom-review");
    demo.send(ENTER);
    demo.finish("Демо завершено");
}

#[test]
fn custom_model_and_paths_reach_the_review_without_writing_them() {
    let mut demo = Preview::new("fresh", 110);
    demo.wait("Выбрать компоненты");
    demo.send(DOWN);
    demo.send(ENTER);
    demo.wait("Дополнительно проверять имена");
    demo.capture("components-wide");
    demo.send(ENTER);
    demo.wait("Файл GGUF");
    demo.capture("summary-source");
    demo.send(ENTER);
    demo.wait("Модель Ollama");
    // The suggested default is a placeholder; typing replaces it.
    demo.send(b"fixture-model:small");
    demo.send(ENTER);
    demo.wait("Выбрать другие папки");
    demo.capture("folders");
    demo.send(DOWN);
    demo.send(ENTER);
    demo.wait("Папка программы");
    let program = demo
        .sandbox
        .path()
        .join("chosen program")
        .display()
        .to_string();
    demo.send(program.as_bytes());
    demo.send(ENTER);
    demo.wait("Папка моделей");
    let models = demo
        .sandbox
        .path()
        .join("chosen models")
        .display()
        .to_string();
    demo.send(models.as_bytes());
    demo.send(ENTER);
    demo.wait("Показать демоустановку?");
    let screen = demo.parser.screen().contents();
    assert!(
        screen.contains("Ollama · fixture-model:small")
            && screen.contains("chosen program")
            && screen.contains("chosen models")
    );
    demo.capture("custom-paths-review");
    demo.send(ENTER);
    demo.finish("Демо завершено");
}

#[test]
fn esc_ctrl_c_and_decline_restore_cursor_and_leave_no_changes() {
    for (keys, stage) in [(b"\x1b".as_slice(), 0), (b"\x03".as_slice(), 1), (DOWN, 2)] {
        let mut demo = Preview::new("fresh", 100);
        demo.wait("Выбрать компоненты");
        if stage == 1 {
            demo.send(DOWN);
            demo.send(ENTER);
            demo.wait("Дополнительно проверять имена");
        } else if stage == 2 {
            demo.send(ENTER);
            demo.wait("Показать демоустановку?");
        }
        demo.send(keys);
        if stage == 1 {
            demo.wait("Продолжить текущий шаг");
            demo.send(b"\x03"); // Ctrl+C again leaves the navigation menu.
        } else if stage == 2 {
            demo.send(DOWN); // Continue / Back / Cancel.
            demo.send(ENTER);
        }
        demo.finish("Отменено. Ничего не изменено.");
    }
}

#[test]
fn review_back_preserves_custom_choices_across_profile_changes() {
    let mut demo = Preview::new("fresh", 110);
    demo.wait("Выбрать компоненты");
    demo.send(DOWN);
    demo.send(ENTER);
    demo.wait("Дополнительно проверять имена");
    // Remove Summary, then return all the way to Profile.
    demo.send(DOWN);
    demo.send(b" ");
    demo.send(ENTER);
    demo.active("Куда установить?");
    demo.back();
    demo.active("Компоненты");
    demo.send(ENTER); // Keep the previously confirmed checkboxes.
    demo.active("Куда установить?");
    demo.send(ENTER);
    demo.active("Показать демоустановку?");
    assert!(!demo
        .parser
        .screen()
        .contents()
        .contains("✓ Готовить сводки"));
    demo.send(DOWN);
    demo.send(ENTER); // Explicit Back in Review.
    demo.active("Куда установить?");
    demo.back();
    demo.active("Компоненты");
    demo.back();
    demo.active("Как установить LocalVox?");
    // Switch to recommended, verify its full set, then return to custom.
    demo.send(b"\x1b[A");
    demo.send(ENTER);
    demo.active("Показать демоустановку?");
    assert!(demo
        .parser
        .screen()
        .contents()
        .contains("✓ Готовить сводки"));
    demo.send(DOWN);
    demo.send(ENTER);
    demo.active("Как установить LocalVox?");
    demo.send(DOWN);
    demo.send(ENTER);
    demo.active("Компоненты");
    demo.send(ENTER);
    demo.active("Куда установить?");
    demo.send(ENTER);
    demo.active("Показать демоустановку?");
    assert!(!demo
        .parser
        .screen()
        .contents()
        .contains("✓ Готовить сводки"));
    demo.capture("back-review");
    demo.send(ENTER);
    demo.finish("Демо завершено");
}

#[test]
fn back_from_source_can_disable_summary_without_visiting_stale_model_questions() {
    let mut demo = Preview::new("fresh", 110);
    demo.wait("Выбрать компоненты");
    demo.send(DOWN);
    demo.send(ENTER);
    demo.wait("Дополнительно проверять имена");
    demo.send(ENTER);
    demo.active("Откуда брать модель");
    demo.send(b"\x1b");
    demo.wait("Продолжить текущий шаг");
    demo.capture("back-menu-wide");
    demo.send(ENTER);
    demo.active("Компоненты");
    demo.send(DOWN);
    demo.send(b" ");
    demo.send(ENTER);
    demo.active("Куда установить?");
    demo.send(ENTER);
    demo.active("Показать демоустановку?");
    assert!(!demo
        .parser
        .screen()
        .contents()
        .contains("✓ Готовить сводки"));
    demo.send(ENTER);
    demo.finish("Демо завершено");
}

#[test]
fn back_from_review_keeps_typed_paths_and_allows_correction() {
    let mut demo = Preview::new("fresh", 110);
    demo.wait("Выбрать компоненты");
    demo.send(DOWN);
    demo.send(ENTER);
    demo.wait("Дополнительно проверять имена");
    demo.send(ENTER);
    demo.active("Откуда брать модель");
    demo.send(ENTER);
    demo.active("Модель Ollama");
    demo.send(b"retained-model");
    demo.send(ENTER);
    demo.active("Куда установить?");
    demo.send(DOWN);
    demo.send(ENTER);
    demo.active("Папка программы");
    demo.send(b"first program");
    demo.send(ENTER);
    demo.active("Папка моделей");
    demo.send(b"retained models");
    demo.send(ENTER);
    demo.active("Показать демоустановку?");
    demo.send(DOWN);
    demo.send(ENTER);
    demo.active("Папка моделей");
    demo.wait("retained models");
    demo.back();
    demo.active("Папка программы");
    demo.wait("first program");
    demo.send(b"corrected program");
    demo.send(ENTER);
    demo.active("Папка моделей");
    demo.send(ENTER); // Accept retained path.
    demo.active("Показать демоустановку?");
    let screen = demo.parser.screen().contents();
    assert!(screen.contains("Программа: corrected program"));
    assert!(screen.contains("Модели:    retained models"));
    assert!(screen.contains("Ollama · retained-model"));
    demo.send(ENTER);
    demo.finish("Демо завершено");
}

#[test]
fn navigation_can_resume_the_current_step_or_cancel_explicitly() {
    let mut demo = Preview::terminal("fresh", 80, false);
    demo.wait("Выбрать компоненты");
    demo.send(DOWN);
    demo.send(ENTER);
    demo.wait("Дополнительно проверять имена");
    demo.send(b"\x1b");
    demo.wait("Продолжить текущий шаг");
    demo.capture("back-menu");
    demo.send(DOWN);
    demo.send(ENTER); // Stay.
    demo.active("Компоненты");
    demo.send(b"\x1b");
    demo.wait("Продолжить текущий шаг");
    demo.send(DOWN);
    demo.send(DOWN);
    demo.send(ENTER);
    demo.finish("Отменено. Ничего не изменено.");
}

#[test]
fn custom_server_and_gguf_are_presented_without_probing_or_importing() {
    for source in [1, 2] {
        let mut demo = Preview::new("fresh", 110);
        demo.wait("Выбрать компоненты");
        demo.send(DOWN);
        demo.send(ENTER);
        demo.wait("Дополнительно проверять имена");
        demo.send(ENTER);
        demo.wait("Файл GGUF");
        for _ in 0..source {
            demo.send(DOWN);
        }
        demo.send(ENTER);
        if source == 1 {
            demo.wait("Адрес сервера без пароля");
            demo.send(b"http://127.0.0.1:1/v1");
            demo.send(ENTER);
            demo.wait("Имя модели на сервере");
            demo.send(b"private-model");
            demo.send(ENTER);
        } else {
            demo.wait("Путь к файлу GGUF");
            // Even a deliberately nonexistent file is only a display value in this demo.
            demo.send(
                demo.sandbox
                    .path()
                    .join("missing.gguf")
                    .display()
                    .to_string()
                    .as_bytes(),
            );
            demo.send(ENTER);
            demo.wait("Новое имя модели в Ollama");
            demo.send(ENTER);
        }
        demo.wait("Выбрать другие папки");
        demo.send(ENTER);
        demo.wait("Показать демоустановку?");
        let screen = demo.parser.screen().contents();
        assert!(screen.contains(if source == 1 {
            "private-model"
        } else {
            "missing.gguf"
        }));
        if source == 1 {
            assert!(!screen.contains("стандартное хранилище Ollama"));
        }
        demo.send(ENTER);
        demo.finish("Демо завершено");
    }
}

#[test]
fn separate_ready_model_directories_keep_program_and_download_folder_choices() {
    let mut demo = Preview::new("fresh", 110);
    demo.wait("Выбрать компоненты");
    demo.send(DOWN);
    demo.send(ENTER);
    demo.wait("Дополнительно проверять имена");
    demo.send(b" ");
    demo.send(DOWN);
    demo.send(b" ");
    demo.send(ENTER);
    demo.wait("Указать готовые папки моделей");
    demo.send(DOWN);
    demo.send(DOWN);
    demo.send(ENTER);
    demo.wait("Папка программы");
    demo.send(b"F:/Chosen LocalVox");
    demo.send(ENTER);
    demo.wait("Папка моделей");
    demo.send(b"F:/Future models");
    demo.send(ENTER);
    demo.wait("Vosk · готовая папка");
    demo.send(b"F:/ready vosk");
    demo.send(ENTER);
    demo.wait("GigaAM · готовая папка");
    demo.send(b"F:/ready gigaam");
    demo.send(ENTER);
    demo.wait("Показать демоустановку?");
    let screen = demo.parser.screen().contents();
    for path in [
        "F:/Chosen LocalVox",
        "F:/Future models",
        "F:/ready vosk",
        "F:/ready gigaam",
    ] {
        assert!(screen.contains(path), "{screen}");
    }
    demo.send(ENTER);
    demo.finish("Демо завершено");
}

#[test]
fn pipe_is_rejected_and_help_needs_no_terminal() {
    let sandbox = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_localvox-setup-preview"))
            .args(args)
            .current_dir(sandbox.path())
            .output()
            .unwrap()
    };
    let piped = run(&[]);
    assert!(!piped.status.success());
    assert!(String::from_utf8_lossy(&piped.stderr).contains("интерактивном терминале"));
    let help = run(&["--help"]);
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("fresh|existing"));
    assert!(snapshot(sandbox.path()).is_empty());
}
