//! Typed, deterministic plan. Building one has no side effects.
use anyhow::{Context, Result};
use localvox_light_core::doctor::{self, Layout, State};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Component {
    Vosk,
    Gigaam,
    Diarize,
    Ner,
}
impl Component {
    pub const ALL: [Self; 4] = [Self::Vosk, Self::Gigaam, Self::Diarize, Self::Ner];
    pub fn id(self) -> &'static str {
        match self {
            Self::Vosk => "vosk",
            Self::Gigaam => "gigaam",
            Self::Diarize => "diarize",
            Self::Ner => "ner",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Vosk => "LOCALVOX_LIGHT_MODEL",
            Self::Gigaam => "LOCALVOX_ASR_MODEL_DIR",
            Self::Diarize => "LOCALVOX_DIARIZE_MODEL_DIR",
            Self::Ner => "LOCALVOX_NER_MODEL_DIR",
        }
    }
    pub fn path(self, l: &Layout) -> Option<&Path> {
        match self {
            Self::Vosk => Some(&l.vosk_model),
            Self::Gigaam => l.gigaam.as_deref(),
            Self::Diarize => l.diarize.as_deref(),
            Self::Ner => l.ner.as_deref(),
        }
    }
    pub fn set(self, l: &mut Layout, path: PathBuf) {
        match self {
            Self::Vosk => l.vosk_model = path,
            Self::Gigaam => l.gigaam = Some(path),
            Self::Diarize => l.diarize = Some(path),
            Self::Ner => l.ner = Some(path),
        }
    }
    pub fn valid(self, l: &Layout) -> bool {
        doctor::inspect(l)
            .iter()
            .any(|f| f.id == self.id() && f.state == State::Ok)
    }
    pub fn validate(self, l: &Layout, path: &Path) -> Result<()> {
        let mut candidate = l.clone();
        self.set(&mut candidate, path.into());
        let finding = doctor::inspect(&candidate)
            .into_iter()
            .find(|f| f.id == self.id())
            .unwrap();
        anyhow::ensure!(finding.state == State::Ok, "{}", finding.detail);
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub enum ModelChoice {
    Keep,
    Existing(PathBuf),
    Download { root: PathBuf, destination: PathBuf },
}
#[derive(Debug, Clone)]
pub enum LlmChoice {
    Keep,
    Disabled,
    Server {
        base: String,
        model: String,
    },
    Ollama {
        base: String,
        model: String,
        install: bool,
        gguf: Option<PathBuf>,
    },
}
#[derive(Debug, Clone)]
pub enum Action {
    Download {
        component: Component,
        root: PathBuf,
        destination: PathBuf,
        repair_zip: bool,
    },
    InstallOllama,
    EnsureTag {
        base: String,
        model: String,
        gguf: Option<PathBuf>,
    },
}
#[derive(Debug, Clone)]
pub struct Plan {
    pub actions: Vec<Action>,
    pub changes: Vec<(String, Option<String>)>,
}

pub fn absolute(path: &Path) -> Result<PathBuf> {
    anyhow::ensure!(!path.as_os_str().is_empty(), "Пустой путь");
    Ok(if path.is_absolute() {
        path.into()
    } else {
        std::env::current_dir()?.join(path)
    })
}
pub fn valid_tag(name: &str) -> Result<()> {
    anyhow::ensure!(
        !name.is_empty()
            && name.len() <= 256
            && !name.starts_with('-')
            && name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"/._:-".contains(&c)),
        "Неверное имя модели"
    );
    Ok(())
}
pub fn manifest_destination(install: &Path, root: &Path, c: Component) -> Result<PathBuf> {
    let data: serde_json::Value =
        serde_json::from_slice(&std::fs::read(install.join("scripts/models.json"))?)?;
    let item = data["components"]
        .as_array()
        .context("components в models.json")?
        .iter()
        .find(|v| v["id"] == c.id())
        .context("Компонент отсутствует в models.json")?["items"]
        .as_array()
        .context("items в models.json")?
        .first()
        .context("Пустой компонент")?;
    let relative = if item["kind"] == "zip" {
        Path::new(item["unpack_to"].as_str().context("unpack_to")?)
    } else {
        Path::new(item["dest"].as_str().context("dest")?)
            .parent()
            .context("dest parent")?
    };
    anyhow::ensure!(
        !relative.is_absolute()
            && relative
                .components()
                .all(|p| matches!(p, std::path::Component::Normal(_))),
        "Небезопасный путь в manifest"
    );
    Ok(absolute(root)?.join(relative))
}

pub fn build(
    layout: &Layout,
    models: &[(Component, ModelChoice)],
    llm: LlmChoice,
    tags: &[super::probe::Tag],
) -> Result<Plan> {
    let mut plan = Plan {
        actions: Vec::new(),
        changes: Vec::new(),
    };
    for (c, choice) in models {
        let path = match choice {
            ModelChoice::Keep => continue,
            ModelChoice::Existing(path) => {
                c.validate(layout, path)?;
                absolute(path)?
            }
            ModelChoice::Download { root, destination } => {
                let path = absolute(destination)?;
                if c.validate(layout, &path).is_err() {
                    plan.actions.push(Action::Download {
                        component: *c,
                        root: absolute(root)?,
                        destination: path.clone(),
                        repair_zip: *c == Component::Vosk && path.exists(),
                    });
                }
                path
            }
        };
        if c.path(layout) != Some(path.as_path()) {
            plan.changes
                .push((c.key().into(), Some(path.to_string_lossy().into_owned())));
        }
    }
    let mut change = |key: &str, value: &str, current: &str| {
        if value != current {
            plan.changes.push((key.into(), Some(value.into())));
        }
    };
    match llm {
        LlmChoice::Keep => (),
        LlmChoice::Disabled => change(
            "LOCALVOX_LIGHT_AUTOCOOK_SUMMARY",
            "off",
            if layout.summary_enabled { "on" } else { "off" },
        ),
        choice => {
            let (base, model, ollama) = match choice {
                LlmChoice::Server { base, model } => (base, model, None),
                LlmChoice::Ollama {
                    base,
                    model,
                    install,
                    gguf,
                } => (base, model, Some((install, gguf))),
                _ => unreachable!(),
            };
            let base = localvox_light_core::presets::normalize_server(&base)?;
            if ollama.is_some() {
                valid_tag(&model)?;
            } else {
                anyhow::ensure!(
                    !model.trim().is_empty()
                        && model.len() <= 1024
                        && !model.chars().any(char::is_control),
                    "Некорректное имя модели сервера"
                );
            }
            change("LOCALVOX_LLM_BASE_URL", &base, &layout.llm_base_url);
            change("LOCALVOX_LLM_MODEL", &model, &layout.llm_model);
            change(
                "LOCALVOX_SUMMARY_PROVIDER",
                "ollama",
                &layout.summary_provider,
            );
            change(
                "LOCALVOX_LIGHT_AUTOCOOK_SUMMARY",
                "on",
                if layout.summary_enabled { "on" } else { "off" },
            );
            if let Some((install, gguf)) = ollama {
                if let Some(file) = &gguf {
                    anyhow::ensure!(
                        super::probe::local(&base),
                        "GGUF импорт доступен только в локальную Ollama"
                    );
                    let mut input = std::fs::File::open(file).context("Открытие GGUF")?;
                    let mut magic = [0; 4];
                    std::io::Read::read_exact(&mut input, &mut magic)?;
                    anyhow::ensure!(&magic == b"GGUF", "Файл не имеет заголовка GGUF");
                    anyhow::ensure!(
                        !super::probe::has_tag(tags, &model),
                        "Имя уже существует; выбери новое имя для GGUF"
                    );
                }
                if install {
                    anyhow::ensure!(
                        super::probe::local(&base),
                        "Установка доступна только для локальной Ollama"
                    );
                    plan.actions.push(Action::InstallOllama);
                }
                if !super::probe::has_tag(tags, &model) {
                    plan.actions.push(Action::EnsureTag { base, model, gguf });
                }
            }
        }
    }
    Ok(plan)
}

impl Plan {
    pub fn describe(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for action in &self.actions {
            lines.push(match action {
                Action::Download { component, destination, repair_zip, .. } => format!("{} {} → {}", if *repair_zip { "Восстановить неполный ZIP-комплект" } else { "Скачать недостающее" }, component.id(), destination.display()),
                Action::InstallOllama => "Открыть официальный установщик Ollama; завершить установку в окне ОС".into(),
                Action::EnsureTag { model, gguf: Some(file), .. } => format!("Импортировать {} → {model}; Ollama сохранит дополнительную копию весов в своём хранилище", file.display()),
                Action::EnsureTag { model, .. } => format!("Скачать отсутствующую модель Ollama {model} в хранилище работающего сервера"),
            });
        }
        for (key, value) in &self.changes {
            lines.push(format!("{key} = {}", value.as_deref().unwrap_or("preset")));
        }
        if lines.is_empty() {
            lines.push("Всё выбранное уже готово. Файлы и настройки не изменятся.".into());
        }
        lines
    }
}
