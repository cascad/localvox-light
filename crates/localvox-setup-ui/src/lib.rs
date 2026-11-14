//! Presentation only. Receives discovery data; never downloads, probes or writes configuration.
use std::io;

struct InstallerTheme;
impl cliclack::Theme for InstallerTheme {
    fn format_confirm(&self, state: &cliclack::ThemeState, confirm: bool) -> String {
        let yes = self.radio_item(state, confirm, "Продолжить", "");
        let no = self.radio_item(state, !confirm, "Отмена", "");
        let divider = if matches!(state, cliclack::ThemeState::Active) {
            " / "
        } else {
            ""
        };
        format!(
            "{}  {yes}{divider}{no}\n",
            self.bar_color(state).apply_to("│")
        )
    }
    fn format_footer_with_message(&self, state: &cliclack::ThemeState, message: &str) -> String {
        let line = match state {
            cliclack::ThemeState::Active => format!("└  {message}"),
            cliclack::ThemeState::Cancel => "└  Отменено".into(),
            cliclack::ThemeState::Submit => "│".into(),
            cliclack::ThemeState::Error(error) => format!("└  {error}"),
        };
        format!("{}\n", self.bar_color(state).apply_to(line))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    Speakers,
    Summary,
    Links,
    Player,
    Names,
}
impl Feature {
    pub const ALL: [Self; 5] = [
        Self::Speakers,
        Self::Summary,
        Self::Links,
        Self::Player,
        Self::Names,
    ];
    pub fn title(self) -> &'static str {
        match self {
            Self::Speakers => "Различать голоса участников",
            Self::Summary => "Готовить сводки встреч",
            Self::Links => "Обрабатывать ссылки YouTube",
            Self::Player => "Слушать записи в терминале",
            Self::Names => "Дополнительно проверять имена",
        }
    }
    fn hint(self) -> &'static str {
        match self {
            Self::Speakers => "Подписи «Говорящий 1, 2…» · модель ≈46 МБ",
            Self::Summary => "Ollama + модель · установим недостающее",
            Self::Links => "yt-dlp + ffmpeg · загрузка и подготовка аудио",
            Self::Player => "mpv · прослушивание и перемотка в TUI",
            Self::Names => "GLiNER ≈580 МБ · необязательно",
        }
    }
}

pub struct ViewData {
    pub program_dir: String,
    pub models_dir: String,
    pub summary_model: String,
    pub core_ready: bool,
    pub installed: Vec<Feature>,
    pub ollama_ready: bool,
    pub preview: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SummarySource {
    Ollama(String),
    Server { base: String, model: String },
    Gguf { path: String, name: String },
}

#[derive(Debug, Clone)]
pub struct Selection {
    pub custom: bool,
    pub features: Vec<Feature>,
    pub program_dir: String,
    pub models_dir: String,
    pub summary: SummarySource,
}

pub fn recommended(data: &ViewData) -> Selection {
    Selection {
        custom: false,
        features: vec![
            Feature::Speakers,
            Feature::Summary,
            Feature::Links,
            Feature::Player,
        ],
        program_dir: data.program_dir.clone(),
        models_dir: data.models_dir.clone(),
        summary: SummarySource::Ollama(data.summary_model.clone()),
    }
}

fn text(prompt: &str, value: &str) -> io::Result<String> {
    cliclack::input(prompt)
        .default_input(value)
        .validate(|input: &String| {
            if input.trim().is_empty() || input.chars().any(char::is_control) {
                Err("Введите непустое значение без управляющих символов")
            } else {
                Ok(())
            }
        })
        .interact()
}

/// Collect a reviewable selection. The caller owns all real installation actions.
pub fn choose(data: &ViewData) -> io::Result<Option<Selection>> {
    cliclack::set_theme(InstallerTheme);
    match choose_inner(data) {
        Err(error) if error.kind() == io::ErrorKind::Interrupted => {
            cliclack::outro_cancel("Отменено. Ничего не изменено.")?;
            Ok(None)
        }
        result => result,
    }
}

fn choose_inner(data: &ViewData) -> io::Result<Option<Selection>> {
    cliclack::intro(
        console::style(" LocalVox · установка ")
            .on_cyan()
            .black()
            .bold(),
    )?;
    if data.preview {
        cliclack::log::info("ДЕМО · Ничего не скачивается и не устанавливается.")?;
    }
    cliclack::log::info(if data.core_ready {
        "Запись и распознавание уже готовы. Используем установленные компоненты."
    } else {
        "Подготовим запись, расшифровку и интерфейсы. Готовое используем повторно."
    })?;
    let custom = cliclack::select("Как установить LocalVox?")
        .item(
            false,
            "Установить всё необходимое",
            "Рекомендуется · распознавание, голоса, сводки, ссылки и плеер",
        )
        .item(
            true,
            "Выбрать компоненты",
            "Галочки, модель для сводок и папки",
        )
        .initial_value(false)
        .interact()?;
    let mut result = recommended(data);
    result.custom = custom;
    if custom {
        cliclack::note(
            "Всегда включено",
            "✓ Запись и расшифровка · Vosk + GigaAM\n  Входит в любую установку.",
        )?;
        let mut checklist =
            cliclack::multiselect("Компоненты · ↑/↓ · Пробел — галочка · Enter — дальше")
                .required(false)
                .initial_values(result.features.clone());
        for feature in Feature::ALL {
            let hint = if data.installed.contains(&feature) {
                "Уже установлено · повторное скачивание не нужно"
            } else {
                feature.hint()
            };
            checklist = checklist.item(feature, feature.title(), hint);
        }
        result.features = checklist.interact()?;
        cliclack::log::info("Существующие программы, модели и записи не удаляются.")?;
        if result.features.contains(&Feature::Summary) {
            let source = cliclack::select("Откуда брать модель для сводок?")
                .item(
                    0,
                    "Ollama",
                    if data.ollama_ready {
                        "Программа и модель уже есть"
                    } else {
                        "Установим Ollama и скачаем модель"
                    },
                )
                .item(1, "Свой сервер", "OpenAI-совместимый API")
                .item(
                    2,
                    "Файл GGUF",
                    "Импорт в Ollama · займёт дополнительное место",
                )
                .initial_value(0)
                .interact()?;
            result.summary = match source {
                0 => SummarySource::Ollama(text("Модель Ollama", &data.summary_model)?),
                1 => SummarySource::Server {
                    base: text(
                        "Адрес сервера без пароля и токенов",
                        "http://localhost:11434/v1",
                    )?,
                    model: text("Имя модели на сервере", &data.summary_model)?,
                },
                _ => SummarySource::Gguf {
                    path: text("Путь к файлу GGUF", "")?,
                    name: text("Новое имя модели в Ollama", "localvox-summary")?,
                },
            };
        }
        cliclack::note(
            "Рекомендуемые папки",
            format!(
                "Программа: {}\nМодели: {}",
                data.program_dir, data.models_dir
            ),
        )?;
        let custom_dirs = cliclack::select("Куда установить?")
            .item(false, "Рекомендуемые папки", "Использовать папки выше")
            .item(
                true,
                "Выбрать другие папки",
                "Например, перенести большие модели на другой диск",
            )
            .initial_value(false)
            .interact()?;
        if custom_dirs {
            result.program_dir = text("Папка программы", &data.program_dir)?;
            result.models_dir = text("Папка моделей", &data.models_dir)?;
        }
    }
    cliclack::note("Перед установкой", review(data, &result))?;
    let accepted = cliclack::confirm(if data.preview {
        "Показать демоустановку?"
    } else {
        "Начать установку?"
    })
    .initial_value(true)
    .interact()?;
    if !accepted {
        cliclack::outro_cancel("Отменено. Ничего не изменено.")?;
        return Ok(None);
    }
    Ok(Some(result))
}

pub fn review(data: &ViewData, selected: &Selection) -> String {
    let mut lines = vec![format!(
        "✓ Запись и расшифровка — {}",
        if data.core_ready {
            "уже готовы"
        } else {
            "Vosk + GigaAM, ≈2 ГБ"
        }
    )];
    for feature in Feature::ALL {
        if selected.features.contains(&feature) {
            lines.push(format!(
                "✓ {}{}",
                feature.title(),
                if data.installed.contains(&feature) {
                    " — уже готово"
                } else {
                    ""
                }
            ));
        }
    }
    if selected.features.contains(&Feature::Summary) {
        lines.push(match &selected.summary {
            SummarySource::Ollama(model) => format!(
                "  Ollama · {model} · {}",
                if data.ollama_ready && model == &data.summary_model {
                    "уже готово"
                } else {
                    "будут установлены недостающие компоненты"
                }
            ),
            SummarySource::Server { base, model } => format!("  Сервер · {base} · {model}"),
            SummarySource::Gguf { path, name } => {
                format!("  GGUF · {path} → {name} · дополнительная копия весов")
            }
        });
    }
    lines.extend([
        String::new(),
        format!("Программа: {}", selected.program_dir),
        format!("Модели:    {}", selected.models_dir),
    ]);
    if selected.features.contains(&Feature::Summary)
        && !matches!(selected.summary, SummarySource::Server { .. })
    {
        lines.push("Модель сводки: стандартное хранилище Ollama".into());
    }
    lines.extend([
        String::new(),
        "Точный объём загрузок зависит от модели сводки и уже готовых файлов.".into(),
        "После установки запуск одной командой: vox".into(),
    ]);
    lines.join("\n")
}

/// Deliberately no backend callbacks: it is impossible for this preview to install anything.
pub fn demonstrate(selected: &Selection) -> io::Result<()> {
    let spinner = cliclack::spinner();
    spinner.start("Демо · подготовка записи и расшифровки");
    std::thread::sleep(std::time::Duration::from_millis(300));
    spinner.stop("Демо · запись и расшифровка готовы");
    for feature in &selected.features {
        cliclack::log::info(format!("Демо · {}", feature.title()))?;
    }
    cliclack::outro("Демо завершено. Ничего не скачано и не изменено.")
}
