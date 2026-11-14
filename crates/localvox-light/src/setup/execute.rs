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
        println!(
            "\n└ {}",
            Plan {
                actions: vec![action.clone()],
                changes: vec![]
            }
            .describe()[0]
        );
        match action {
            Action::Download {
                component,
                root,
                destination,
                repair_zip,
            } => {
                // Another successful attempt may already have completed the work.
                if component.validate(layout, destination).is_ok() {
                    println!("  Уже готово; загрузка пропущена.");
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
                run_command(cmd)?;
                component
                    .validate(layout, destination)
                    .context("Загрузчик завершился, но комплект модели не готов")?;
            }
            Action::InstallOllama => {
                if probe::cli().is_some() || probe::tags("http://localhost:11434/v1").is_ok() {
                    println!("  Ollama уже есть; установка пропущена.");
                    continue;
                }
                // Deliberate OS-owned handoff, outside worker kill-on-close jobs: the official
                // installer can launch the persistent Ollama app. Never terminate that app.
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
            Action::EnsureOllama { base } => ensure_ollama(base, install)?,
        }
    }
    Ok(())
}

pub fn script_command(install: &Path, name: &str) -> Command {
    if cfg!(windows) {
        let mut cmd = Command::new("powershell.exe");
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
    let (observer, receiver, _) = localvox_light_process::OutputObserver::channel(16);
    let display = std::thread::spawn(move || {
        for chunk in receiver {
            eprint!("{}", String::from_utf8_lossy(&chunk.bytes));
        }
    });
    let output = localvox_light_process::run_observed(
        cmd,
        None,
        localvox_light_process::Limits::new(Duration::from_secs(4 * 3600)),
        || false,
        |_, elapsed| eprintln!("  Выполняется · {} мин", elapsed.as_secs() / 60),
        Some(observer),
    );
    let _ = display.join();
    let output = output?;
    anyhow::ensure!(
        output.termination.success(),
        "Действие не завершено: {:?}",
        output.termination
    );
    Ok(())
}

pub fn ensure_tag(base: &str, model: &str, gguf: Option<&Path>, install: &Path) -> Result<()> {
    super::plan::valid_tag(model)?;
    if probe::has_tag(&probe::tags(base)?, model) {
        println!("  Модель уже есть; pull/import пропущен.");
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
        run_command(command)?;
    } else {
        let url = probe::endpoint(base, "api/pull")?;
        let response = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(4 * 3600))
            .redirects(0)
            .build()
            .post(&url)
            .send_json(serde_json::json!({"model": model, "stream": true}))
            .map_err(|_| anyhow::anyhow!("Не удалось начать скачивание модели Ollama"))?;
        pull_progress(
            std::io::BufReader::new(response.into_reader()),
            &mut std::io::stderr(),
        )?;
    }
    anyhow::ensure!(
        probe::has_tag(&probe::tags(base)?, model),
        "Ollama не подтвердила наличие модели после операции"
    );
    Ok(())
}

pub fn pull_progress(mut input: impl BufRead, out: &mut impl Write) -> Result<()> {
    let mut previous = String::new();
    let mut success = false;
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
            if status != previous {
                writeln!(out, "  {status}")?;
                previous = status.into();
            }
            success |= status == "success";
        }
    }
    anyhow::ensure!(
        success,
        "Поток скачивания оборвался до подтверждения success"
    );
    Ok(())
}
