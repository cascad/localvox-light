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

pub fn read_env(path: &Path) -> Result<String> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e.into()),
    }
}

pub fn apply(plan: &Plan, layout: &Layout, install: &Path, env: &Path, before: &str) -> Result<()> {
    if plan.actions.is_empty() && plan.changes.is_empty() {
        return Ok(());
    }
    let _ownership = localvox_light_core::session::try_instance_lock(&layout.work_dir)?
        .context("Демон или другой мастер использует архив. Останови его и повтори настройку.")?;
    anyhow::ensure!(
        read_env(env)? == before,
        ".env изменился после показа плана. Повтори настройку."
    );
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
                    cmd.arg("-Root").arg(root).arg("-Only").arg(component.id());
                    if *repair_zip {
                        cmd.arg("-RepairZip");
                    }
                } else {
                    cmd.arg("--root")
                        .arg(root)
                        .arg("--only")
                        .arg(component.id());
                    if *repair_zip {
                        cmd.arg("--repair-zip");
                    }
                }
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
                println!("Заверши установку, запусти Ollama и нажми Enter здесь.");
                let mut line = String::new();
                anyhow::ensure!(
                    std::io::stdin().read_line(&mut line)? > 0,
                    "Установка не подтверждена; повтори --setup после запуска Ollama"
                );
                probe::tags("http://localhost:11434/v1")
                    .context("Ollama ещё не отвечает; запусти её и повтори --setup")?;
            }
            Action::EnsureTag { base, model, gguf } => {
                ensure_tag(base, model, gguf.as_deref(), install)?
            }
        }
    }
    // Recheck before committing settings as well; a UI may have saved them during a long pull.
    anyhow::ensure!(
        read_env(env)? == before,
        ".env изменился во время установки. Модели сохранены; повтори мастер для настройки."
    );
    if !plan.changes.is_empty() {
        let after = env_file::apply(before, &plan.changes);
        if after != before {
            std::fs::create_dir_all(env.parent().context("Каталог .env")?)?;
            localvox_light_core::artifacts::atomic_write(env, after.as_bytes())?;
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
