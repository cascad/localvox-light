//! The only mutation boundary. Called after the user has reviewed and confirmed a plan.
use super::{
    plan::{Action, Plan},
    probe,
};
use anyhow::{Context, Result};
use localvox_light_core::{doctor::Layout, env_file};
use std::{
    io::{BufRead, Read, Write},
    path::Path,
    process::Command,
    time::Duration,
};

pub fn apply(
    plan: &Plan,
    layout: &Layout,
    install: &Path,
    settings: &env_file::Snapshot,
) -> Result<()> {
    if plan.actions.is_empty() && plan.changes.is_empty() {
        return Ok(());
    }
    let _ownership = localvox_light_core::session::try_instance_lock(&layout.work_dir)?
        .context("Демон или другой мастер использует архив. Останови его и повтори настройку.")?;
    apply_owned(plan, layout, install, settings)
}

/// The installer holds the same archive ownership while publishing its bundle and tools.
pub(super) fn apply_owned(
    plan: &Plan,
    layout: &Layout,
    install: &Path,
    settings: &env_file::Snapshot,
) -> Result<()> {
    settings
        .verify()
        .context("Настройки изменились после показа плана")?;
    apply_actions(plan, layout, install)?;
    // The shared writer rechecks the snapshot after a long download before publishing.
    settings
        .save(&plan.changes)
        .context("Не удалось сохранить настройки. Готовые модели сохранены; повтори мастер.")?;
    Ok(())
}

pub(super) fn apply_actions(plan: &Plan, layout: &Layout, install: &Path) -> Result<()> {
    for action in &plan.actions {
        let label = Plan {
            actions: vec![action.clone()],
            changes: vec![],
        }
        .describe()[0]
            .clone();
        match action {
            Action::Download {
                component,
                root,
                destination,
                repair_zip,
            } => {
                // Another successful attempt may already have completed the work.
                if component.validate(layout, destination).is_ok() {
                    println!("│ ✓ {label} · Уже готово");
                    continue;
                }
                let mut cmd = script_command(install, "fetch-models");
                if cfg!(windows) {
                    cmd.arg("-Root")
                        .arg(root)
                        .arg("-Only")
                        .arg(component.id())
                        .arg("-ComponentDir")
                        .arg(destination);
                    if *repair_zip {
                        cmd.arg("-RepairZip");
                    }
                } else {
                    cmd.arg("--root")
                        .arg(root)
                        .arg("--only")
                        .arg(component.id())
                        .arg("--component-dir")
                        .arg(destination);
                    if *repair_zip {
                        cmd.arg("--repair-zip");
                    }
                }
                cmd.env(
                    "PATH",
                    super::tools::command_path(&Default::default(), install)?,
                );
                let downloader = install.join(super::bundle::executable("localvox-install"));
                if downloader.is_file()
                    && Command::new(&downloader)
                        .arg("--download-protocol")
                        .output()
                        .is_ok_and(|result| {
                            result.status.success()
                                && String::from_utf8_lossy(&result.stdout).trim()
                                    == localvox_setup_ui::progress::PROTOCOL
                        })
                {
                    cmd.env("LOCALVOX_MODEL_DOWNLOADER", downloader)
                        .env("LOCALVOX_DOWNLOAD_PROTOCOL", "1");
                }
                run_command_named(cmd, &label)?;
                component
                    .validate(layout, destination)
                    .context("Загрузчик завершился, но комплект модели не готов")?;
            }
            Action::InstallOllama => {
                if probe::cli().is_some() || probe::tags("http://localhost:11434/v1").is_ok() {
                    println!("│ ✓ Ollama уже установлена");
                    continue;
                }
                // Deliberate OS-owned handoff, outside worker kill-on-close jobs: the official
                // installer can launch the persistent Ollama app. Never terminate that app.
                println!("└ {label}");
                let status = script_command(install, "setup-ollama")
                    .arg(if cfg!(windows) {
                        "-CacheDir"
                    } else {
                        "--cache"
                    })
                    .arg(install.join("cache/ollama-installer"))
                    .status()?;
                anyhow::ensure!(
                    status.success(),
                    "Официальный установщик Ollama завершился с ошибкой"
                );
                ensure_ollama("http://localhost:11434/v1", install)?;
            }
            Action::EnsureTag { base, model, gguf } => {
                ensure_tag(base, model, gguf.as_deref(), install)?
            }
            Action::EnsureOllama { base } => {
                let mut activity = localvox_setup_ui::activity::Activity::start(label);
                let result = ensure_ollama(base, install);
                activity.finish(result.is_ok());
                result?;
            }
        }
    }
    Ok(())
}

pub fn script_command(install: &Path, name: &str) -> Command {
    if cfg!(windows) {
        let mut cmd = Command::new("powershell.exe");
        // pwsh's modules can target .NET APIs absent in Windows PowerShell 5.
        // Let this host construct its own default module paths, even when vox
        // or the bootstrap was launched from PowerShell 7.
        cmd.env_remove("PSModulePath");
        cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(install.join(format!("scripts/{name}.ps1")));
        cmd
    } else {
        let mut cmd = Command::new("bash");
        cmd.arg(install.join(format!("scripts/{name}.sh")));
        cmd
    }
}

pub fn ensure_ollama(base: &str, install: &Path) -> Result<()> {
    if probe::tags(base).is_ok() {
        return Ok(());
    }
    anyhow::ensure!(
        probe::local(base),
        "Удалённый сервер Ollama недоступен; запуск возможен только локально"
    );
    let cli = probe::cli().context("Ollama не найдена после установки")?;
    std::fs::create_dir_all(install.join("cache"))?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(install.join("cache/ollama-start.log"))?;
    let mut command = Command::new(&cli);
    command
        .arg("serve")
        .env(
            "OLLAMA_HOST",
            probe::endpoint(base, "")?.trim_end_matches('/'),
        )
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000 | 0x00000200);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    struct OwnedServer(Option<std::process::Child>);
    impl Drop for OwnedServer {
        fn drop(&mut self) {
            if let Some(child) = self.0.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    let mut server = OwnedServer(Some(command.spawn().context("Запуск локальной Ollama")?));
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    let mut exited = None;
    loop {
        if probe::tags(base).is_ok() {
            if let Some(mut child) = server.0.take() {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            return Ok(());
        }
        if exited.is_none() {
            exited = server.0.as_mut().unwrap().try_wait()?;
        }
        // The official desktop installer may start its own server concurrently.
        // Wait for the API even if our child lost that race; never stop an existing server.
        anyhow::ensure!(
            std::time::Instant::now() < deadline,
            "Ollama не ответила за минуту (дочерний процесс: {exited:?}); подробности в cache/ollama-start.log"
        );
        std::thread::sleep(Duration::from_millis(250));
    }
}

pub fn run_command(cmd: Command) -> Result<()> {
    run_command_named(cmd, "Выполнение действия")
}

pub fn run_command_named(cmd: Command, label: &str) -> Result<()> {
    let mut activity = localvox_setup_ui::activity::Activity::start(label);
    let mut stdout = activity.writer();
    let mut stderr = activity.writer();
    let (observer, receiver, dropped) = localvox_light_process::OutputObserver::channel(64);
    let display = std::thread::spawn(move || {
        for chunk in receiver {
            let writer = match chunk.stream {
                localvox_light_process::Stream::Stdout => &mut stdout,
                localvox_light_process::Stream::Stderr => &mut stderr,
            };
            let _ = writer.write_all(&chunk.bytes);
        }
    });
    let output = localvox_light_process::run_observed(
        cmd,
        None,
        localvox_light_process::Limits::new(Duration::from_secs(4 * 3600)),
        || false,
        |_, _| {},
        Some(observer),
    );
    let _ = display.join();
    let output = output?;
    let success = output.termination.success();
    if !success && dropped.load(std::sync::atomic::Ordering::Relaxed) != 0 {
        let mut writer = activity.writer();
        let tail = &output.stderr[output.stderr.len().saturating_sub(16 * 1024)..];
        let _ = writer.write_all(tail);
        let _ = writer.flush();
    }
    activity.finish(success);
    if !success {
        use localvox_light_process::Termination;
        let reason = match output.termination {
            Termination::Exited(status) => format!("процесс завершился: {status}"),
            Termination::Cancelled => "действие отменено".into(),
            Termination::TimedOut => "истекло время ожидания".into(),
            Termination::OutputLimit => "превышен предел размера журнала".into(),
        };
        anyhow::bail!("{label}: {reason}");
    }
    Ok(())
}

pub fn ensure_tag(base: &str, model: &str, gguf: Option<&Path>, install: &Path) -> Result<()> {
    super::plan::valid_tag(model)?;
    if probe::has_tag(&probe::tags(base)?, model) {
        println!("│ ✓ Модель сводки уже готова: {model}");
        return Ok(());
    }
    if let Some(file) = gguf {
        anyhow::ensure!(probe::local(base), "Импорт GGUF требует локальную Ollama");
        let cli = probe::cli()
            .context("CLI Ollama не найдена; запусти мастер повторно после установки")?;
        let file = super::plan::absolute(file)?
            .to_string_lossy()
            .replace('\\', "/");
        // Ollama Modelfile quotes do not support escaped quotes inside a quoted path.
        anyhow::ensure!(
            !file.contains(['"', '\n', '\r']),
            "Путь GGUF содержит кавычки или перевод строки; выбери обычное имя файла"
        );
        let cache = install.join("cache");
        std::fs::create_dir_all(&cache)?;
        let mut model_file = tempfile::NamedTempFile::new_in(&cache)?;
        writeln!(model_file, "FROM \"{file}\"")?;
        model_file.flush()?;
        let mut command = Command::new(cli);
        command
            .args(["create", model, "-f"])
            .arg(model_file.path())
            .env(
                "OLLAMA_HOST",
                probe::endpoint(base, "")?.trim_end_matches('/'),
            );
        run_command_named(command, &format!("Импортировать модель сводки: {model}"))?;
    } else {
        let mut activity =
            localvox_setup_ui::activity::Activity::start(format!("Скачать модель сводки: {model}"));
        let url = probe::endpoint(base, "api/pull")?;
        let response = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(4 * 3600))
            .redirects(0)
            .build()
            .post(&url)
            .send_json(serde_json::json!({"model": model, "stream": true}))
            .map_err(|_| anyhow::anyhow!("Не удалось начать скачивание модели Ollama"))?;
        let result = pull_progress(
            std::io::BufReader::new(response.into_reader()),
            &mut activity.writer(),
        );
        activity.finish(result.is_ok());
        result?;
    }
    anyhow::ensure!(
        probe::has_tag(&probe::tags(base)?, model),
        "Ollama не подтвердила наличие модели после операции"
    );
    Ok(())
}

pub fn pull_progress(mut input: impl BufRead, out: &mut impl Write) -> Result<()> {
    use localvox_setup_ui::progress::{Phase, Progress};
    let mut previous = String::new();
    let mut success = false;
    let mut started = std::time::Instant::now();
    let mut last = std::time::Instant::now();
    loop {
        let mut line = Vec::new();
        let len = input
            .by_ref()
            .take(1024 * 1024 + 1)
            .read_until(b'\n', &mut line)?;
        if len == 0 {
            break;
        }
        anyhow::ensure!(len <= 1024 * 1024, "Слишком длинная строка ответа Ollama");
        let event: serde_json::Value =
            serde_json::from_slice(&line).context("Повреждённый поток pull Ollama")?;
        if let Some(error) = event["error"].as_str() {
            anyhow::bail!("Ollama: {error}");
        }
        if let Some(status) = event["status"].as_str() {
            let digest = event["digest"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(12)
                .collect::<String>();
            let name = if digest.is_empty() {
                status.to_string()
            } else {
                format!("{status} · {digest}")
            };
            let bytes = event["completed"].as_u64().unwrap_or(0);
            let total = event["total"].as_u64();
            anyhow::ensure!(
                total.is_none_or(|total| bytes <= total),
                "Ollama прислала неверный размер загрузки"
            );
            let changed = name != previous;
            if changed {
                started = std::time::Instant::now();
            }
            if changed
                || last.elapsed() >= Duration::from_millis(250)
                || total == Some(bytes)
                || status == "success"
            {
                let phase = if status == "success" {
                    Phase::Complete
                } else if total.is_some() || bytes > 0 {
                    Phase::Downloading
                } else if status.contains("verif")
                    || status.contains("manifest") && !status.starts_with("pulling")
                {
                    Phase::Verifying
                } else {
                    Phase::Connecting
                };
                Progress {
                    name: name.clone(),
                    phase,
                    bytes,
                    total,
                    elapsed_ms: started.elapsed().as_millis() as u64,
                }
                .write(out)?;
                last = std::time::Instant::now();
            }
            previous = name;
            success |= status == "success";
        }
    }
    anyhow::ensure!(
        success,
        "Поток скачивания оборвался до подтверждения success"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ollama_uses_the_same_byte_protocol_and_rejects_impossible_counters() {
        use localvox_setup_ui::progress::{Phase, Progress};
        let input = b"{\"status\":\"pulling layer\",\"digest\":\"abc\",\"total\":100,\"completed\":20}\n{\"status\":\"pulling layer\",\"digest\":\"abc\",\"total\":100,\"completed\":100}\n{\"status\":\"verifying sha256 digest\"}\n{\"status\":\"success\"}\n";
        let mut output = Vec::new();
        pull_progress(std::io::Cursor::new(input), &mut output).unwrap();
        let events: Vec<_> = std::str::from_utf8(&output)
            .unwrap()
            .lines()
            .map(|line| Progress::parse(line).unwrap())
            .collect();
        assert_eq!((events[0].bytes, events[0].total), (20, Some(100)));
        assert!(events
            .iter()
            .any(|e| e.phase == Phase::Downloading && e.bytes == 100));
        assert!(events.iter().any(|e| e.phase == Phase::Verifying));
        assert_eq!(events.last().unwrap().phase, Phase::Complete);
        assert!(pull_progress(
            std::io::Cursor::new(b"{\"status\":\"pulling\",\"total\":1,\"completed\":2}\n"),
            &mut Vec::new()
        )
        .is_err());
    }

    #[test]
    fn command_failure_keeps_its_stage_and_child_error_in_plain_output() {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "setup::execute::tests::command_output_fixture",
                "--ignored",
                "--nocapture",
            ])
            .env("LOCALVOX_SETUP_OUTPUT_FIXTURE", "1")
            .env("FORCE_COLOR", "1")
            .output()
            .unwrap();
        assert!(output.status.success());
        let text = String::from_utf8_lossy(&output.stderr);
        assert!(text.contains("Проверка модели"), "{text}");
        assert!(text.contains("checksum-test-failure"), "{text}");
        assert!(text.contains("× Ошибка"), "{text}");
        assert!(
            !text.contains('\u{1b}') && !text.contains("Выполняется ·"),
            "{text}"
        );
    }

    #[test]
    #[ignore = "subprocess fixture; invoked by command_failure test"]
    fn command_output_fixture() {
        assert!(std::env::var_os("LOCALVOX_SETUP_OUTPUT_FIXTURE").is_some());
        let mut command = if cfg!(windows) {
            let mut command = Command::new("cmd.exe");
            command.args(["/D", "/C", "echo checksum-test-failure 1>&2 & exit /b 7"]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args(["-c", "printf 'checksum-test-failure\\n' >&2; exit 7"]);
            command
        };
        command.env_remove("FORCE_COLOR");
        assert!(run_command_named(command, "Проверка модели").is_err());
    }
}
