use super::{plan::*, probe};
use anyhow::{Context, Result};
use localvox_light_core::doctor::Layout;
use std::{
    io::{BufRead, Write},
    path::Path,
};

struct Cancel;
impl std::fmt::Display for Cancel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Настройка отменена")
    }
}
impl std::fmt::Debug for Cancel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}
impl std::error::Error for Cancel {}

pub fn ask(
    input: &mut impl BufRead,
    out: &mut impl Write,
    label: &str,
    default: &str,
) -> Result<String> {
    write!(out, "{label} [{default}]: ")?;
    out.flush()?;
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 || line.trim().eq_ignore_ascii_case("q") {
        return Err(Cancel.into());
    }
    let line = line.trim();
    Ok(if line.is_empty() {
        default.into()
    } else {
        line.into()
    })
}

pub fn choose(
    layout: &Layout,
    install: &Path,
    input: &mut impl BufRead,
    out: &mut impl Write,
) -> Result<Option<Plan>> {
    let result = choices(layout, install, input, out);
    match result {
        Err(e) if e.is::<Cancel>() => {
            writeln!(out, "Отменено. Ничего не изменено.")?;
            Ok(None)
        }
        other => other,
    }
}
fn choices(
    layout: &Layout,
    install: &Path,
    input: &mut impl BufRead,
    out: &mut impl Write,
) -> Result<Option<Plan>> {
    writeln!(out, "\nНастройка LocalVox · Enter — сохранить выбор, q — отмена.\nПроверяется наличие файлов и API, без запуска моделей.")?;
    let mut models = Vec::new();
    for c in Component::ALL {
        writeln!(
            out,
            "\n{} · {}",
            c.id(),
            c.path(layout)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "не найдено".into())
        )?;
        let ready = c.valid(layout);
        writeln!(
            out,
            "  0 оставить {} · 1 готовая папка · 2 скачать недостающее",
            if ready {
                "готовую модель"
            } else {
                "как есть (модель не готова)"
            }
        )?;
        let choice = loop {
            match ask(
                input,
                out,
                "Выбор",
                if !ready && matches!(c, Component::Vosk | Component::Gigaam) {
                    "2"
                } else {
                    "0"
                },
            )?
            .as_str()
            {
                "0" => break ModelChoice::Keep,
                "1" => {
                    let path = absolute(Path::new(&ask(input, out, "Папка модели", "")?))?;
                    match c.validate(layout, &path) {
                        Ok(()) => break ModelChoice::Existing(path),
                        Err(e) => writeln!(out, "  Не подходит: {e}")?,
                    }
                }
                "2" => {
                    let chosen = ask(
                        input,
                        out,
                        "Корень загрузки (модели попадут в models/)",
                        &localvox_light_core::model_paths::Context::capture()
                            .user_root()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default(),
                    )?;
                    anyhow::ensure!(
                        !chosen.trim().is_empty(),
                        "Не удалось определить home; укажите корень загрузки явно"
                    );
                    let root = absolute(Path::new(&chosen))?;
                    let destination = manifest_destination(install, &root, c)?;
                    break ModelChoice::Download { root, destination };
                }
                _ => writeln!(out, "Выбери 0, 1 или 2.")?,
            }
        };
        models.push((c, choice));
    }
    writeln!(out, "\nСводка · {} · {}\n  0 оставить · 1 Ollama · 2 свой OpenAI-совместимый сервер · 3 без автоматической сводки", layout.llm_model, safe_base(&layout.llm_base_url))?;
    let mut found = Vec::new();
    let llm = loop {
        match ask(input, out, "Выбор", "0")?.as_str() {
            "0" => break LlmChoice::Keep,
            "3" => break LlmChoice::Disabled,
            "2" => {
                let base = ask(
                    input,
                    out,
                    "Адрес сервера (включая /v1)",
                    "http://localhost:11434/v1",
                )?;
                let base = localvox_light_core::presets::normalize_server(&base)?;
                let model = ask(input, out, "Имя модели сервера", &layout.llm_model)?;
                break LlmChoice::Server { base, model };
            }
            "1" => {
                let base = ask(
                    input,
                    out,
                    "Адрес Ollama",
                    if probe::local(&layout.llm_base_url) {
                        &layout.llm_base_url
                    } else {
                        "http://localhost:11434/v1"
                    },
                )?;
                let base = localvox_light_core::presets::normalize_server(&base)?;
                let available = probe::tags(&base);
                let install = if available.is_err() && probe::cli().is_none() && probe::local(&base)
                {
                    writeln!(out, "Ollama не найдена. Официальный установщик откроется после подтверждения плана.")?;
                    ask(input, out, "Установить Ollama? yes/no", "no")? == "yes"
                } else {
                    false
                };
                if available.is_err() && !install {
                    anyhow::bail!("Ollama не отвечает. Запусти её приложение и повтори --setup; готовые модели сохранятся.");
                }
                found = available.unwrap_or_default();
                for (i, tag) in found.iter().enumerate() {
                    writeln!(out, "  {} {}", i + 1, tag.name)?;
                }
                writeln!(out, "  n скачать по имени · g импортировать GGUF (дополнительная копия в хранилище Ollama)")?;
                let (model, gguf) = loop {
                    let selection = ask(input, out, "Модель: номер / n / g", "n")?;
                    if let Ok(n) = selection.parse::<usize>() {
                        if let Some(t) = n.checked_sub(1).and_then(|i| found.get(i)) {
                            break (t.name.clone(), None);
                        }
                    }
                    match selection.as_str() {
                        "n" => {
                            break (
                                ask(input, out, "Имя модели для скачивания", &layout.llm_model)?,
                                None,
                            )
                        }
                        "g" => {
                            anyhow::ensure!(
                                install || probe::cli().is_some(),
                                "Для импорта GGUF нужна установленная CLI Ollama"
                            );
                            let path = absolute(Path::new(&ask(input, out, "Путь к GGUF", "")?))?;
                            let name = ask(
                                input,
                                out,
                                "Новое имя модели (не заменяет существующее)",
                                "",
                            )?;
                            break (name, Some(path));
                        }
                        _ => writeln!(out, "Выбери номер модели, n или g.")?,
                    }
                };
                break LlmChoice::Ollama {
                    base,
                    model,
                    install,
                    gguf,
                };
            }
            _ => writeln!(out, "Выбери 0, 1, 2 или 3.")?,
        }
    };
    let plan = build(layout, &models, llm, &found)?;
    writeln!(out, "\nПлан изменений:")?;
    for line in plan.describe() {
        writeln!(out, "  └ {line}")?;
    }
    writeln!(
        out,
        "  Настройки вступят в силу при следующем запуске демона."
    )?;
    if plan.actions.is_empty() && plan.changes.is_empty() {
        return Ok(Some(plan));
    }
    if ask(input, out, "Выполнить? yes/no", "no").context("Подтверждение")? != "yes"
    {
        writeln!(out, "Отменено. Ничего не изменено.")?;
        return Ok(None);
    }
    Ok(Some(plan))
}
pub fn safe_base(base: &str) -> String {
    localvox_light_core::presets::normalize_server(base)
        .unwrap_or_else(|_| "некорректный адрес (скрыт)".into())
}
