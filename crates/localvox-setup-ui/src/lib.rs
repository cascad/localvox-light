//! Presentation only. Receives discovery data; never downloads, probes or writes configuration.
use std::collections::BTreeMap;
use std::io;
mod wizard;

struct InstallerTheme {
    can_go_back: bool,
}
impl cliclack::Theme for InstallerTheme {
    fn format_footer_with_message(&self, state: &cliclack::ThemeState, message: &str) -> String {
        let help = if self.can_go_back {
            "Esc — назад / отмена"
        } else {
            "Esc — отмена"
        };
        let line = match state {
            cliclack::ThemeState::Active => format!(
                "└  {}{help}",
                if message.is_empty() {
                    String::new()
                } else {
                    format!("{message} · ")
                }
            ),
            cliclack::ThemeState::Cancel => "└  Ввод прерван".into(),
            cliclack::ThemeState::Submit => "│".into(),
            cliclack::ThemeState::Error(error) => format!("└  {error} · {help}"),
        };
        format!("{}\n", self.bar_color(state).apply_to(line))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    Speakers,
    Summary,
    Links,
    Player,
    Names,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ModelKind {
    Vosk,
    Gigaam,
    Diarize,
    Ner,
}
impl ModelKind {
    pub fn title(self) -> &'static str {
        match self {
            Self::Vosk => "Vosk",
            Self::Gigaam => "GigaAM",
            Self::Diarize => "Диаризация",
            Self::Ner => "GLiNER",
        }
    }
}
impl Feature {
    pub fn id(self) -> &'static str {
        match self {
            Self::Speakers => "speakers",
            Self::Summary => "summary",
            Self::Links => "links",
            Self::Player => "player",
            Self::Names => "names",
        }
    }
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
    /// Existing user choices; absent on a fresh installation or in the fresh demo.
    pub initial: Option<Selection>,
    pub available_model_paths: BTreeMap<ModelKind, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum SummarySource {
    Ollama(String),
    Server { base: String, model: String },
    Gguf { path: String, name: String },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Selection {
    pub custom: bool,
    pub features: Vec<Feature>,
    pub program_dir: String,
    pub models_dir: String,
    pub summary: SummarySource,
    #[serde(default)]
    pub model_paths: BTreeMap<ModelKind, String>,
}

pub fn recommended(data: &ViewData) -> Selection {
    if let Some(initial) = &data.initial {
        let mut selected = initial.clone();
        selected.custom = false;
        return selected;
    }
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
        model_paths: BTreeMap::new(),
    }
}

/// Collect a reviewable selection. The caller owns all real installation actions.
pub fn choose(data: &ViewData) -> io::Result<Option<Selection>> {
    choose_with(data, |selected| {
        Ok((review(data, selected), selected.clone()))
    })
}

/// Prepare and validate the exact backend plan before it is shown and confirmed.
pub fn choose_with<P>(
    data: &ViewData,
    mut prepare: impl FnMut(&Selection) -> io::Result<(String, P)>,
) -> io::Result<Option<P>> {
    cliclack::set_theme(InstallerTheme { can_go_back: false });
    match wizard::choose(data, &mut prepare) {
        Err(error) if error.kind() == io::ErrorKind::NotConnected => Err(io::Error::other("Откройте мастер в интерактивном терминале. Для проверки без вопросов: localvox-install --check")),
        Err(error) if error.kind() == io::ErrorKind::Interrupted => {
            cliclack::outro_cancel("Отменено. Ничего не изменено.")?;
            Ok(None)
        }
        result => result,
    }
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
    for (model, path) in &selected.model_paths {
        lines.push(format!("{}: {path}", model.title()));
    }
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
