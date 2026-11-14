//! Composition for the approved installer UI and the shared setup planner/executor.
use super::{
    bundle::{self, Bundle},
    execute, pending,
    plan::{self, Component, LlmChoice, ModelChoice, Plan},
    probe,
    tools::{self, Tool},
};
use anyhow::{Context, Result};
use localvox_light_core::{
    doctor::{self, Layout, State},
    env_file, model_paths, presets,
};
use localvox_setup_ui::{Feature, ModelKind, Selection, SummarySource, ViewData};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub fn default_program_dir() -> PathBuf {
    if let Some(location) = pending::location() {
        return location;
    }
    let home = std::env::home_dir().unwrap_or_else(|| PathBuf::from("."));
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Local"))
            .join("Programs/LocalVox")
    } else if cfg!(target_os = "macos") {
        home.join("Applications/LocalVox")
    } else {
        home.join(".local/share/localvox")
    }
}

fn model_kind(component: Component) -> ModelKind {
    match component {
        Component::Vosk => ModelKind::Vosk,
        Component::Gigaam => ModelKind::Gigaam,
        Component::Diarize => ModelKind::Diarize,
        Component::Ner => ModelKind::Ner,
    }
}

pub fn configuration(
    install: &Path,
) -> Result<(BTreeMap<String, String>, env_file::Snapshot, bool)> {
    configuration_at(&install.join(".env"))
}

pub fn configuration_at(
    path: &Path,
) -> Result<(BTreeMap<String, String>, env_file::Snapshot, bool)> {
    let snapshot = env_file::Snapshot::read(&path)?;
    let mut config: BTreeMap<_, _> = std::env::vars()
        .filter(|(key, _)| key.starts_with("LOCALVOX_"))
        .collect();
    let existing = match std::fs::read_to_string(&path) {
        Ok(content) => {
            for item in dotenvy::from_read_iter(content.as_bytes()) {
                let (key, value) = item.map_err(|_| anyhow::anyhow!("Ошибка синтаксиса {}. Проверьте KEY=value и кавычки; содержимое настройки не выводится.",path.display()))?;
                config.insert(key, value);
            }
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error).with_context(|| format!("Чтение {}", path.display())),
    };
    if let Some(base) = config.get("LOCALVOX_LLM_BASE_URL") {
        presets::normalize_server(base)?;
    }
    Ok((config, snapshot, existing))
}

fn value(config: &BTreeMap<String, String>, key: &str) -> String {
    config
        .get(key)
        .cloned()
        .unwrap_or_else(|| presets::baked(key).into())
}
fn enabled(config: &BTreeMap<String, String>, key: &str) -> bool {
    !matches!(
        value(config, key).trim().to_ascii_lowercase().as_str(),
        "off" | "false" | "0"
    )
}

pub fn layout(config: &BTreeMap<String, String>, source: &Path, destination: &Path) -> Layout {
    let context = model_paths::Context {
        home: std::env::home_dir(),
        exe: Some(destination.into()),
        cwd: Some(destination.into()),
        overrides: config.clone(),
    };
    let dir = |id| context.resolve(model_paths::component(id), None).path;
    let audio = config
        .get("LOCALVOX_LIGHT_AUDIO_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| "archive".into());
    Layout {
        vosk_model: dir("vosk").unwrap_or_default(),
        gigaam: dir("gigaam"),
        diarize: dir("diarize"),
        ner: dir("ner"),
        cook: Some(source.join(doctor::cook_exe_name())),
        work_dir: if audio.is_absolute() {
            audio
        } else {
            destination.join(audio)
        },
        llm_base_url: value(config, "LOCALVOX_LLM_BASE_URL"),
        llm_model: value(config, "LOCALVOX_LLM_MODEL"),
        summary_enabled: enabled(config, "LOCALVOX_LIGHT_AUTOCOOK_SUMMARY"),
        summary_provider: value(config, "LOCALVOX_SUMMARY_PROVIDER"),
        api_bind: value(config, "LOCALVOX_API_BIND"),
    }
}

pub fn view(
    config: &BTreeMap<String, String>,
    layout: &Layout,
    destination: &Path,
    existing: bool,
) -> Result<ViewData> {
    let found = probe::tags(&layout.llm_base_url).ok();
    let mut installed = vec![];
    if Component::Diarize.valid(layout) {
        installed.push(Feature::Speakers);
    }
    if Component::Ner.valid(layout) {
        installed.push(Feature::Names);
    }
    if found
        .as_ref()
        .is_some_and(|tags| probe::has_tag(tags, &layout.llm_model))
    {
        installed.push(Feature::Summary);
    }
    if Tool::Ffmpeg.ready(config, destination) && Tool::YtDlp.ready(config, destination) {
        installed.push(Feature::Links);
    }
    if Tool::Mpv.ready(config, destination) {
        installed.push(Feature::Player);
    }
    let models = config
        .get("LOCALVOX_SETUP_MODELS_DIR")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            model_paths::Context::capture()
                .user_root()
                .unwrap_or_else(|| destination.into())
                .join("models")
        });
    let mut features = vec![Feature::Links, Feature::Player];
    if enabled(config, "LOCALVOX_DIARIZE") {
        features.push(Feature::Speakers);
    }
    if layout.summary_enabled {
        features.push(Feature::Summary);
    }
    if installed.contains(&Feature::Names) {
        features.push(Feature::Names);
    }
    if let Some(saved) = config.get("LOCALVOX_SETUP_FEATURES") {
        let ids: Vec<String> =
            serde_json::from_str(saved).context("Повреждён LOCALVOX_SETUP_FEATURES")?;
        features = ids
            .iter()
            .map(|id| {
                Feature::ALL
                    .into_iter()
                    .find(|feature| feature.id() == id)
                    .with_context(|| format!("Неизвестный компонент установщика: {id}"))
            })
            .collect::<Result<_>>()?;
    }
    let summary = if found.is_some()
        || layout.llm_base_url.trim_end_matches('/') == "http://localhost:11434/v1"
    {
        SummarySource::Ollama(layout.llm_model.clone())
    } else {
        SummarySource::Server {
            base: layout.llm_base_url.clone(),
            model: layout.llm_model.clone(),
        }
    };
    Ok(ViewData {
        program_dir: destination.display().to_string(),
        models_dir: models.display().to_string(),
        summary_model: layout.llm_model.clone(),
        core_ready: Component::Vosk.valid(layout) && Component::Gigaam.valid(layout),
        ollama_ready: probe::cli().is_some() || found.is_some(),
        installed,
        preview: false,
        initial: existing.then(|| Selection {
            custom: false,
            features,
            program_dir: destination.display().to_string(),
            models_dir: models.display().to_string(),
            summary,
            model_paths: Default::default(),
        }),
        available_model_paths: Component::ALL
            .into_iter()
            .filter(|component| component.valid(layout))
            .filter_map(|component| {
                component
                    .path(layout)
                    .map(|path| (model_kind(component), path.display().to_string()))
            })
            .collect(),
    })
}

pub struct Installation {
    pub plan: Plan,
    pub destination: PathBuf,
    pub layout: Layout,
    pub settings: env_file::Snapshot,
    pub tools: Vec<Tool>,
    pub register_path: bool,
    pub selected: Selection,
    bundled_tools: Vec<Tool>,
    catalog: serde_json::Value,
    config: BTreeMap<String, String>,
}

pub fn prepare(source: &Path, selected: &Selection, discovered: &Layout) -> Result<Installation> {
    prepare_at(source, selected, discovered, None)
}

/// Configure the existing program and the same .env used by its runtime.
pub fn prepare_configuration(
    source: &Path,
    selected: &Selection,
    discovered: &Layout,
    settings_path: &Path,
) -> Result<Installation> {
    anyhow::ensure!(bundle::canonical_destination(Path::new(&selected.program_dir))? == bundle::canonical_destination(source)?, "Для переноса программы запустите localvox-install. Этот мастер настраивает текущую установку.");
    prepare_at(source, selected, discovered, Some(settings_path))
}

fn prepare_at(
    source: &Path,
    selected: &Selection,
    discovered: &Layout,
    settings_path: Option<&Path>,
) -> Result<Installation> {
    let destination = plan::absolute(Path::new(&selected.program_dir))?;
    let models_dir = plan::absolute(Path::new(&selected.models_dir))?;
    for path in [&destination, &models_dir] {
        anyhow::ensure!(
            !path.exists() || path.is_dir(),
            "Нужна папка, а не файл: {}",
            path.display()
        );
    }
    let (config, settings, _) = match settings_path {
        Some(path) => configuration_at(path)?,
        None => configuration(&destination)?,
    };
    let mut target = layout(&config, source, &destination);
    let components = [Component::Vosk, Component::Gigaam].into_iter().chain(
        [
            (Feature::Speakers, Component::Diarize),
            (Feature::Names, Component::Ner),
        ]
        .into_iter()
        .filter_map(|(feature, component)| {
            selected.features.contains(&feature).then_some(component)
        }),
    );
    let mut choices = vec![];
    for component in components {
        if let Some(path) = selected
            .model_paths
            .get(&model_kind(component))
            .filter(|path| !path.trim().is_empty())
        {
            let path = plan::absolute(Path::new(path))?;
            component.validate(&target, &path)?;
            choices.push((component, ModelChoice::Existing(path)));
            continue;
        }
        // Reuse any valid configured model, including one outside the chosen download folder.
        let path = if component.valid(discovered) {
            component.path(discovered).unwrap().to_owned()
        } else if component.valid(&target) {
            component.path(&target).unwrap().to_owned()
        } else {
            models_dir.join(
                model_paths::component(component.id())
                    .directory
                    .file_name()
                    .context("Имя папки модели")?,
            )
        };
        let choice = if component.validate(&target, &path).is_ok() {
            ModelChoice::Existing(path)
        } else {
            ModelChoice::Download {
                root: models_dir.clone(),
                destination: path,
            }
        };
        choices.push((component, choice));
    }
    let mut tags = vec![];
    let mut ensure_base = None;
    let llm = if !selected.features.contains(&Feature::Summary) {
        LlmChoice::Disabled
    } else {
        match &selected.summary {
            SummarySource::Server { base, model } => LlmChoice::Server {
                base: base.clone(),
                model: model.clone(),
            },
            summary => {
                let base = if probe::local(&discovered.llm_base_url) {
                    discovered.llm_base_url.clone()
                } else {
                    "http://localhost:11434/v1".to_string()
                };
                let available = probe::tags(&base);
                let missing_api = available.is_err();
                tags = available.unwrap_or_default();
                if missing_api {
                    ensure_base = Some(base.clone());
                }
                let (model, gguf) = match summary {
                    SummarySource::Ollama(model) => (model.clone(), None),
                    SummarySource::Gguf { path, name } => {
                        (name.clone(), Some(plan::absolute(Path::new(path))?))
                    }
                    _ => unreachable!(),
                };
                LlmChoice::Ollama {
                    base,
                    model,
                    install: missing_api && probe::cli().is_none(),
                    gguf,
                }
            }
        }
    };
    let mut plan = plan::build(&target, &choices, llm, &tags)?;
    let ids: Vec<_> = Feature::ALL
        .into_iter()
        .filter(|feature| selected.features.contains(feature))
        .map(Feature::id)
        .collect();
    let ids = serde_json::to_string(&ids)?;
    if config.get("LOCALVOX_SETUP_FEATURES") != Some(&ids) {
        plan.changes
            .push(("LOCALVOX_SETUP_FEATURES".into(), Some(ids)));
    }
    let models_value = models_dir.display().to_string();
    if config.get("LOCALVOX_SETUP_MODELS_DIR") != Some(&models_value) {
        plan.changes
            .push(("LOCALVOX_SETUP_MODELS_DIR".into(), Some(models_value)));
    }
    for (key, value) in &plan.changes {
        if let Some(path) = value {
            for component in Component::ALL {
                if key == component.key() {
                    component.set(&mut target, path.into());
                }
            }
            if key == "LOCALVOX_LLM_BASE_URL" {
                target.llm_base_url = path.clone();
            }
            if key == "LOCALVOX_LLM_MODEL" {
                target.llm_model = path.clone();
            }
        }
    }
    target.summary_enabled = selected.features.contains(&Feature::Summary);
    if let Some(base) = ensure_base {
        let index = plan
            .actions
            .iter()
            .position(|action| matches!(action, plan::Action::EnsureTag { .. }))
            .unwrap_or(plan.actions.len());
        plan.actions
            .insert(index, plan::Action::EnsureOllama { base });
    }
    if !config.contains_key("LOCALVOX_LIGHT_AUDIO_DIR") {
        plan.changes.push((
            "LOCALVOX_LIGHT_AUDIO_DIR".into(),
            Some(target.work_dir.display().to_string()),
        ));
    }
    let diarize = if selected.features.contains(&Feature::Speakers) {
        "on"
    } else {
        "off"
    };
    if value(&config, "LOCALVOX_DIARIZE") != diarize {
        plan.changes
            .push(("LOCALVOX_DIARIZE".into(), Some(diarize.into())));
    }
    let mut tools = vec![];
    if !cfg!(windows)
        && plan
            .actions
            .iter()
            .any(|a| matches!(a, plan::Action::Download { .. }))
    {
        tools.extend([Tool::Jq, Tool::Unzip]);
    }
    if selected.features.contains(&Feature::Links) {
        tools.extend([Tool::Ffmpeg, Tool::YtDlp]);
    }
    if selected.features.contains(&Feature::Player) {
        tools.push(Tool::Mpv);
    }
    for tool in &tools {
        if let Some(path) = tool
            .key()
            .and_then(|key| config.get(key))
            .filter(|value| !value.is_empty())
        {
            anyhow::ensure!(tool.ready(&config,&destination), "Заданный путь {} не готов: {path}. Исправьте его в настройках или снимите соответствующую галочку.", tool.name());
        }
    }
    let mut selected = selected.clone();
    selected.program_dir = destination.display().to_string();
    selected.models_dir = models_dir.display().to_string();
    for path in selected.model_paths.values_mut() {
        *path = plan::absolute(Path::new(path))?.display().to_string();
    }
    if let SummarySource::Gguf { path, .. } = &mut selected.summary {
        *path = plan::absolute(Path::new(path))?.display().to_string();
    }
    let bundled_tools = tools
        .iter()
        .copied()
        .filter(|tool| {
            *tool == Tool::Mpv && source.join("tools/mpv").is_dir() && tool.ready(&config, source)
        })
        .collect();
    Ok(Installation {
        plan,
        destination,
        layout: target,
        settings,
        tools,
        register_path: true,
        selected,
        bundled_tools,
        catalog: if settings_path.is_some() {
            serde_json::from_str(include_str!("../../../../scripts/models.json"))?
        } else {
            serde_json::from_slice(&std::fs::read(source.join("scripts/models.json"))?)?
        },
        config,
    })
}

impl Installation {
    pub fn describe(&self) -> String {
        let mut lines = vec![
            format!("Программа: {}", self.destination.display()),
            format!("Архив: {}", self.layout.work_dir.display()),
        ];
        lines.push(if self.layout.summary_enabled {
            format!(
                "Сводки: {} · {}",
                self.layout.llm_base_url, self.layout.llm_model
            )
        } else {
            "Сводки: выключены".into()
        });
        for action in &self.plan.actions {
            lines.push(match action {
                plan::Action::Download {
                    component,
                    destination,
                    ..
                } => {
                    let item = self.catalog["components"]
                        .as_array()
                        .and_then(|items| items.iter().find(|item| item["id"] == component.id()));
                    let title = item
                        .and_then(|item| item["title"].as_str())
                        .unwrap_or(component.id());
                    let size = item
                        .and_then(|item| item["approx"].as_str())
                        .map(|size| format!(" (≈{size})"))
                        .unwrap_or_default();
                    format!("Скачать {title}{size} → {}", destination.display())
                }
                plan::Action::InstallOllama => "Установить Ollama официальным установщиком".into(),
                plan::Action::EnsureOllama { .. } => {
                    "Запустить локальную Ollama, если она ещё не отвечает".into()
                }
                plan::Action::EnsureTag {
                    model, gguf: None, ..
                } => format!("Скачать модель сводки: {model}"),
                plan::Action::EnsureTag {
                    model,
                    gguf: Some(path),
                    ..
                } => format!(
                    "Импортировать {} → {model} (дополнительная копия весов)",
                    path.display()
                ),
            });
        }
        for component in Component::ALL {
            if let Some(path) = component
                .path(&self.layout)
                .filter(|_| component.valid(&self.layout))
            {
                lines.push(format!("Готово · {} · {}", component.id(), path.display()));
            }
        }
        for tool in &self.tools {
            lines.push(format!(
                "{} · {}",
                if tool.ready(&self.config, &self.destination) {
                    "Готово"
                } else if self.bundled_tools.contains(tool) {
                    "В комплекте"
                } else {
                    "Доустановить"
                },
                tool.name()
            ));
        }
        lines.push("Готовые модели и существующие настройки будут сохранены.".into());
        lines.push(
            "Объём новых загрузок зависит от модели сводки и недостающих инструментов.".into(),
        );
        if !self.tools.is_empty() {
            lines.push(
                "Менеджер ОС может запросить системные права для недостающих программ.".into(),
            );
        }
        lines.push(if self.register_path {
            "После установки: vox".into()
        } else {
            format!("Запуск из папки программы: {}", bundle::executable("vox"))
        });
        lines.join("\n")
    }

    /// Returns whether the short `vox` command was registered without a conflict.
    pub fn apply(&self, bundle: &Bundle, source: &Path) -> Result<bool> {
        self.apply_inner(Some(bundle), source)
    }

    pub fn apply_configuration(&self, source: &Path) -> Result<bool> {
        anyhow::ensure!(
            bundle::canonical_destination(&self.destination)?
                == bundle::canonical_destination(source)?,
            "Настройка не переносит программу"
        );
        self.apply_inner(None, source)
    }

    fn apply_inner(&self, bundle: Option<&Bundle>, source: &Path) -> Result<bool> {
        let setup_dir = self.destination.join("cache/setup");
        let _installer = localvox_light_core::session::try_instance_lock(&setup_dir)?
            .context("Другой установщик уже работает с этой папкой")?;
        self.settings.verify()?;
        let noop = self.plan.actions.is_empty()
            && self.plan.changes.is_empty()
            && self
                .tools
                .iter()
                .all(|tool| tool.ready(&self.config, &self.destination))
            && bundle::canonical_destination(source)?
                == bundle::canonical_destination(&self.destination)?;
        let _ownership = if noop {
            None
        } else {
            Some(
                localvox_light_core::session::try_instance_lock(&self.layout.work_dir)?.context(
                    "LocalVox использует архив. Остановите демон и повторите установку.",
                )?,
            )
        };
        let checkpoint = pending::write(&self.destination, &self.selected, self.register_path)?;
        let _models = if self
            .plan
            .actions
            .iter()
            .any(|action| matches!(action, plan::Action::Download { .. }))
        {
            let root = Path::new(&self.selected.models_dir);
            let identity = bundle::canonical_destination(root)?;
            if identity == bundle::canonical_destination(&self.layout.work_dir)?
                || identity == bundle::canonical_destination(&setup_dir)?
            {
                None
            } else {
                Some(
                    localvox_light_core::session::try_instance_lock(root)?
                        .context("Другой установщик уже загружает модели в эту папку")?,
                )
            }
        } else {
            None
        };
        if let Some(bundle) = bundle {
            bundle.publish(source, &self.destination)?;
        }
        tools::install(
            &self.tools,
            &self.destination,
            &self.config,
            &self.destination,
        )?;
        execute::apply_actions(&self.plan, &self.layout, &self.destination)?;
        let mut changes = self.plan.changes.clone();
        for tool in &self.tools {
            if let (Some(key), Some(path)) =
                (tool.key(), tool.find(&self.config, &self.destination))
            {
                if self.config.get(key).is_none_or(|s| s.is_empty()) {
                    changes.push((key.to_string(), Some(path.display().to_string())));
                }
            }
        }
        self.settings.save(&changes).context(
            "Не удалось сохранить настройки; готовые компоненты сохранены, повторите мастер",
        )?;
        let registered = if self.register_path {
            let status =
                std::process::Command::new(self.destination.join(bundle::executable("vox")))
                    .arg("--install-command")
                    .status()?;
            anyhow::ensure!(
                status.success() || status.code() == Some(1),
                "Не удалось добавить vox в PATH"
            );
            status.success()
        } else {
            false
        };
        pending::complete(&self.destination, &checkpoint)?;
        Ok(registered)
    }
}

#[derive(Default)]
pub struct Options {
    pub check: bool,
    pub register_path: bool,
    pub staging_root: Option<PathBuf>,
    pub discovery: Option<Layout>,
    pub settings_path: Option<PathBuf>,
}

pub fn run(source: &Path, destination: &Path, options: Options) -> Result<i32> {
    let destination = plan::absolute(destination)?;
    let (config, _, existing) = match &options.settings_path {
        Some(path) => configuration_at(path)?,
        None => configuration(&destination)?,
    };
    let discovered = options
        .discovery
        .clone()
        .unwrap_or_else(|| layout(&config, source, &destination));
    if options.check {
        Bundle::read(&destination)?;
        let findings = super::report(&discovered);
        super::print_report(&findings);
        return Ok(match doctor::worst(&findings) {
            State::Ok => 0,
            State::Warn => 1,
            State::Fail => 2,
        });
    }
    // --setup configures an existing program; it never republishes binaries or
    // requires an installation archive. The standalone installer always verifies one.
    let bundle = if options.discovery.is_some() {
        None
    } else {
        Some(Bundle::read(source)?)
    };
    let mut data = view(&config, &discovered, &destination, existing)?;
    match pending::read(&destination) {
        Ok(Some(selected)) => {
            eprintln!("Есть незавершённая установка. Проверьте сохранённый выбор и подтвердите продолжение.");
            data.initial = Some(selected);
        }
        Ok(None) => (),
        Err(error) => eprintln!("Не удалось прочитать сохранённый план: {error:#}. Выберите компоненты заново; готовые файлы сохранятся."),
    }
    let chosen = localvox_setup_ui::choose_with(&data, |selected| {
        let mut installation = match &options.settings_path {
            Some(path) => prepare_configuration(source, selected, &discovered, path),
            None => prepare(source, selected, &discovered),
        }
        .map_err(|e| std::io::Error::other(format!("{e:#}")))?;
        if options.discovery.is_some() && installation.destination == source {
            installation.layout.work_dir =
                plan::absolute(&discovered.work_dir).map_err(std::io::Error::other)?;
            installation
                .plan
                .changes
                .retain(|(key, _)| key != "LOCALVOX_LIGHT_AUDIO_DIR");
            if config.get("LOCALVOX_LIGHT_AUDIO_DIR")
                != Some(&installation.layout.work_dir.display().to_string())
            {
                installation.plan.changes.push((
                    "LOCALVOX_LIGHT_AUDIO_DIR".into(),
                    Some(installation.layout.work_dir.display().to_string()),
                ));
            }
        }
        if let Some(staging) = &options.staging_root {
            let staging = staging.canonicalize().map_err(std::io::Error::other)?;
            let mut paths = vec![
                installation.destination.as_path(),
                Path::new(&selected.models_dir),
                installation.layout.work_dir.as_path(),
            ];
            paths.extend(
                Component::ALL
                    .into_iter()
                    .filter_map(|component| component.path(&installation.layout)),
            );
            if let SummarySource::Gguf { path, .. } = &selected.summary {
                paths.push(Path::new(path));
            }
            for path in paths {
                let path = bundle::canonical_destination(path)
                    .map_err(|e| std::io::Error::other(format!("{e:#}")))?;
                if path.starts_with(&staging) {
                    return Err(std::io::Error::other(
                        "Выберите папку вне временного комплекта установки",
                    ));
                }
            }
        }
        installation.register_path = options.register_path;
        Ok((installation.describe(), installation))
    })?;
    if let Some(installation) = chosen {
        let registered = match &bundle {
            Some(bundle) => installation.apply(bundle, source)?,
            None => installation.apply_configuration(source)?,
        };
        println!(
            "\n└ LocalVox готов. Запуск: {}",
            if registered {
                "vox".into()
            } else {
                installation
                    .destination
                    .join(bundle::executable("vox"))
                    .display()
                    .to_string()
            }
        );
    }
    Ok(0)
}
