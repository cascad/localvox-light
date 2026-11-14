//! Explicit navigation over completed answers; terminal input stays with cliclack.
use crate::{recommended, review, Feature, InstallerTheme, Selection, SummarySource, ViewData};
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

struct Draft {
    custom: bool,
    features: Vec<Feature>,
    source: Provider,
    ollama_model: String,
    server_base: String,
    server_model: String,
    gguf_path: String,
    gguf_name: String,
    custom_dirs: bool,
    program_dir: String,
    models_dir: String,
}

impl Draft {
    fn new(data: &ViewData) -> Self {
        Self {
            custom: false,
            features: recommended(data).features,
            source: Provider::Ollama,
            ollama_model: data.summary_model.clone(),
            server_base: "http://localhost:11434/v1".into(),
            server_model: data.summary_model.clone(),
            gguf_path: String::new(),
            gguf_name: "localvox-summary".into(),
            custom_dirs: false,
            program_dir: data.program_dir.clone(),
            models_dir: data.models_dir.clone(),
        }
    }

    fn selected(&self, data: &ViewData) -> Selection {
        if !self.custom {
            return recommended(data);
        }
        Selection {
            custom: true,
            features: self.features.clone(),
            program_dir: if self.custom_dirs {
                &self.program_dir
            } else {
                &data.program_dir
            }
            .clone(),
            models_dir: if self.custom_dirs {
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

pub(super) fn choose(data: &ViewData) -> io::Result<Option<Selection>> {
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
        let action = match ask(step, &mut draft, data) {
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
            Move::Finish => return Ok(Some(draft.selected(data))),
            Move::Cancel => {
                cliclack::outro_cancel("Отменено. Ничего не изменено.")?;
                return Ok(None);
            }
        }
    }
}

fn ask(step: Step, draft: &mut Draft, data: &ViewData) -> io::Result<Move> {
    use Step::*;
    let next = match step {
        Profile => {
            draft.custom = cliclack::select("Как установить LocalVox?")
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
                    if data.ollama_ready {
                        "Программа и модель уже есть"
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
            draft.custom_dirs = cliclack::select("Куда установить?")
                .item(false, "Рекомендуемые папки", "Использовать папки выше")
                .item(
                    true,
                    "Выбрать другие папки",
                    "Например, перенести большие модели на другой диск",
                )
                .initial_value(draft.custom_dirs)
                .interact()?;
            if draft.custom_dirs {
                ProgramDir
            } else {
                Review
            }
        }
        ProgramDir => {
            draft.program_dir = text("Папка программы", &draft.program_dir)?;
            ModelsDir
        }
        ModelsDir => {
            draft.models_dir = text("Папка моделей", &draft.models_dir)?;
            Review
        }
        Review => {
            cliclack::note("Перед установкой", review(data, &draft.selected(data)))?;
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
