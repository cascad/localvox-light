//! Explicit navigation over completed answers; terminal input stays with cliclack.
use crate::{recommended, Feature, InstallerTheme, ModelKind, Selection, SummarySource, ViewData};
use std::collections::BTreeMap;
use std::io;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Profile,
    Components,
    Source,
    OllamaModel,
    ServerAddress,
    ServerModel,
    GgufPath,
    GgufName,
    Directories,
    ProgramDir,
    ModelsDir,
    Review,
    ModelPath(ModelKind),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Move {
    Next(Step),
    Back,
    Stay,
    Finish,
    Cancel,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Provider {
    Ollama,
    Server,
    Gguf,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Directories {
    Recommended,
    Custom,
    ExistingModels,
}

struct Draft {
    custom: bool,
    features: Vec<Feature>,
    source: Provider,
    ollama_model: String,
    server_base: String,
    server_model: String,
    gguf_path: String,
    gguf_name: String,
    directories: Directories,
    program_dir: String,
    models_dir: String,
    model_paths: BTreeMap<ModelKind, String>,
}

impl Draft {
    fn new(data: &ViewData) -> Self {
        let initial = recommended(data);
        let mut draft = Self {
            custom: false,
            features: initial.features,
            source: Provider::Ollama,
            ollama_model: data.summary_model.clone(),
            server_base: "http://localhost:11434/v1".into(),
            server_model: data.summary_model.clone(),
            gguf_path: String::new(),
            gguf_name: "localvox-summary".into(),
            directories: if !initial.model_paths.is_empty() {
                Directories::ExistingModels
            } else if initial.program_dir != data.program_dir
                || initial.models_dir != data.models_dir
            {
                Directories::Custom
            } else {
                Directories::Recommended
            },
            program_dir: initial.program_dir,
            models_dir: initial.models_dir,
            model_paths: if initial.model_paths.is_empty() {
                data.available_model_paths.clone()
            } else {
                initial.model_paths
            },
        };
        match initial.summary {
            SummarySource::Ollama(model) => draft.ollama_model = model,
            SummarySource::Server { base, model } => {
                draft.source = Provider::Server;
                draft.server_base = base;
                draft.server_model = model;
            }
            SummarySource::Gguf { path, name } => {
                draft.source = Provider::Gguf;
                draft.gguf_path = path;
                draft.gguf_name = name;
            }
        }
        draft
    }

    fn selected(&self, data: &ViewData) -> Selection {
        if !self.custom {
            return recommended(data);
        }
        Selection {
            custom: true,
            features: self.features.clone(),
            program_dir: if self.directories != Directories::Recommended {
                &self.program_dir
            } else {
                &data.program_dir
            }
            .clone(),
            models_dir: if self.directories != Directories::Recommended {
                &self.models_dir
            } else {
                &data.models_dir
            }
            .clone(),
            summary: match self.source {
                Provider::Ollama => SummarySource::Ollama(self.ollama_model.clone()),
                Provider::Server => SummarySource::Server {
                    base: self.server_base.clone(),
                    model: self.server_model.clone(),
                },
                Provider::Gguf => SummarySource::Gguf {
                    path: self.gguf_path.clone(),
                    name: self.gguf_name.clone(),
                },
            },
            model_paths: if self.directories == Directories::ExistingModels {
                self.model_paths
                    .iter()
                    .filter(|(_, path)| !path.trim().is_empty())
                    .map(|(kind, path)| (*kind, path.clone()))
                    .collect()
            } else {
                BTreeMap::new()
            },
        }
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

fn navigation() -> io::Result<Move> {
    // The library intentionally reports both Esc and Ctrl+C as Interrupted.
    // Offer explicit navigation instead of guessing which key was pressed.
    cliclack::set_theme(InstallerTheme { can_go_back: false });
    cliclack::select("Вернуться или отменить?")
        .item(Move::Back, "← Назад", "Подтверждённые ответы сохранятся")
        .item(Move::Stay, "Продолжить текущий шаг", "Вернуться к вводу")
        .item(Move::Cancel, "Отменить установку", "Выйти без изменений")
        .initial_value(Move::Back)
        .interact()
}

pub(super) fn choose<P>(
    data: &ViewData,
    prepare: &mut impl FnMut(&Selection) -> io::Result<(String, P)>,
) -> io::Result<Option<P>> {
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
    let mut draft = Draft::new(data);
    let mut history = Vec::new();
    let mut step = Step::Profile;
    loop {
        cliclack::set_theme(InstallerTheme {
            can_go_back: !history.is_empty(),
        });
        let mut prepared = None;
        let question = if step == Step::Review {
            match prepare(&draft.selected(data)) {
                Ok((details, plan)) => {
                    prepared = Some(plan);
                    ask(step, &mut draft, data, Some(&details))
                }
                Err(error) => {
                    cliclack::log::error(format!("План пока не готов: {error}"))?;
                    Ok(navigation()?)
                }
            }
        } else {
            ask(step, &mut draft, data, None)
        };
        let action = match question {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                if history.is_empty() {
                    Move::Cancel
                } else {
                    navigation()?
                }
            }
            answer => answer?,
        };
        match action {
            Move::Next(next) => {
                history.push(step);
                step = next;
            }
            Move::Back => {
                if let Some(previous) = history.pop() {
                    step = previous;
                }
            }
            Move::Stay => {}
            Move::Finish => {
                return prepared
                    .map(Some)
                    .ok_or_else(|| io::Error::other("Подтверждённый план отсутствует"))
            }
            Move::Cancel => {
                cliclack::outro_cancel("Отменено. Ничего не изменено.")?;
                return Ok(None);
            }
        }
    }
}

fn ask(step: Step, draft: &mut Draft, data: &ViewData, details: Option<&str>) -> io::Result<Move> {
    use Step::*;
    let next = match step {
        Profile => {
            draft.custom = cliclack::select("Как установить LocalVox?")
                .item(
                    false,
                    if data.initial.is_some() {
                        "Сохранить текущий набор"
                    } else {
                        "Установить всё необходимое"
                    },
                    if data.initial.is_some() {
                        "Проверить и доустановить только недостающее"
                    } else {
                        "Рекомендуется · распознавание, голоса, сводки, ссылки и плеер"
                    },
                )
                .item(
                    true,
                    "Выбрать компоненты",
                    "Галочки, модель для сводок и папки",
                )
                .initial_value(draft.custom)
                .interact()?;
            if draft.custom {
                Components
            } else {
                Review
            }
        }
        Components => {
            cliclack::note(
                "Всегда включено",
                "✓ Запись и расшифровка · Vosk + GigaAM\n  Входит в любую установку.",
            )?;
            let mut checklist =
                cliclack::multiselect("Компоненты · ↑/↓ · Пробел — галочка · Enter — дальше")
                    .required(false)
                    .initial_values(draft.features.clone());
            for feature in Feature::ALL {
                checklist = checklist.item(
                    feature,
                    feature.title(),
                    if data.installed.contains(&feature) {
                        "Уже установлено · повторное скачивание не нужно"
                    } else {
                        feature.hint()
                    },
                );
            }
            draft.features = checklist.interact()?;
            cliclack::log::info("Существующие программы, модели и записи не удаляются.")?;
            if draft.features.contains(&Feature::Summary) {
                Source
            } else {
                Directories
            }
        }
        Source => {
            draft.source = cliclack::select("Откуда брать модель для сводок?")
                .item(
                    Provider::Ollama,
                    "Ollama",
                    if data.installed.contains(&Feature::Summary) {
                        "Программа и модель уже есть"
                    } else if data.ollama_ready {
                        "Ollama уже есть · скачаем недостающую модель"
                    } else {
                        "Установим Ollama и скачаем модель"
                    },
                )
                .item(Provider::Server, "Свой сервер", "OpenAI-совместимый API")
                .item(
                    Provider::Gguf,
                    "Файл GGUF",
                    "Импорт в Ollama · займёт дополнительное место",
                )
                .initial_value(draft.source)
                .interact()?;
            match draft.source {
                Provider::Ollama => OllamaModel,
                Provider::Server => ServerAddress,
                Provider::Gguf => GgufPath,
            }
        }
        OllamaModel => {
            draft.ollama_model = text("Модель Ollama", &draft.ollama_model)?;
            Directories
        }
        ServerAddress => {
            draft.server_base = text("Адрес сервера без пароля и токенов", &draft.server_base)?;
            ServerModel
        }
        ServerModel => {
            draft.server_model = text("Имя модели на сервере", &draft.server_model)?;
            Directories
        }
        GgufPath => {
            draft.gguf_path = text("Путь к файлу GGUF", &draft.gguf_path)?;
            GgufName
        }
        GgufName => {
            draft.gguf_name = text("Новое имя модели в Ollama", &draft.gguf_name)?;
            Directories
        }
        Directories => {
            cliclack::note(
                "Рекомендуемые папки",
                format!(
                    "Программа: {}\nМодели: {}",
                    data.program_dir, data.models_dir
                ),
            )?;
            draft.directories = cliclack::select("Куда установить?")
                .item(
                    self::Directories::Recommended,
                    "Рекомендуемые папки",
                    "Использовать папки выше",
                )
                .item(
                    self::Directories::Custom,
                    "Выбрать другие папки",
                    "Например, перенести большие модели на другой диск",
                )
                .item(
                    self::Directories::ExistingModels,
                    "Указать готовые папки моделей",
                    "Vosk и GigaAM · можно из разных мест",
                )
                .initial_value(draft.directories)
                .interact()?;
            match draft.directories {
                self::Directories::Custom => ProgramDir,
                self::Directories::ExistingModels => ProgramDir,
                self::Directories::Recommended => Review,
            }
        }
        ProgramDir => {
            draft.program_dir = text("Папка программы", &draft.program_dir)?;
            ModelsDir
        }
        ModelsDir => {
            draft.models_dir = text("Папка моделей", &draft.models_dir)?;
            if draft.directories == self::Directories::ExistingModels {
                ModelPath(ModelKind::Vosk)
            } else {
                Review
            }
        }
        ModelPath(kind) => {
            let value: String = cliclack::input(format!(
                "{} · готовая папка (пусто — автоматически)",
                kind.title()
            ))
            .required(false)
            .default_input(
                draft
                    .model_paths
                    .get(&kind)
                    .map(String::as_str)
                    .unwrap_or(""),
            )
            .validate(|value: &String| {
                if value.chars().any(char::is_control) {
                    Err("Управляющие символы в пути недопустимы")
                } else {
                    Ok(())
                }
            })
            .interact()?;
            draft.model_paths.insert(kind, value);
            match kind {
                ModelKind::Vosk => ModelPath(ModelKind::Gigaam),
                ModelKind::Gigaam if draft.features.contains(&Feature::Speakers) => {
                    ModelPath(ModelKind::Diarize)
                }
                ModelKind::Gigaam | ModelKind::Diarize
                    if draft.features.contains(&Feature::Names) =>
                {
                    ModelPath(ModelKind::Ner)
                }
                _ => Review,
            }
        }
        Review => {
            cliclack::note(
                "Перед установкой",
                details.ok_or_else(|| io::Error::other("План не подготовлен"))?,
            )?;
            return cliclack::select(if data.preview {
                "Показать демоустановку?"
            } else {
                "Начать установку?"
            })
            .item(
                Move::Finish,
                "Продолжить",
                if data.preview {
                    "Только демонстрация, без установки"
                } else {
                    "Применить показанный план"
                },
            )
            .item(Move::Back, "← Назад", "Исправить предыдущий шаг")
            .item(Move::Cancel, "Отменить установку", "Выйти без изменений")
            .initial_value(Move::Finish)
            .interact();
        }
    };
    Ok(Move::Next(next))
}
