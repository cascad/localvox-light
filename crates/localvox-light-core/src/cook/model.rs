//! The same concrete engine and weights are used by early and final transcription.
use super::*;
use crate::incremental::Identity;
use std::collections::BTreeMap;

pub struct Selection {
    backend: Backend,
    pub name: String,
    pub identity: Identity,
    files: BTreeMap<String, PathBuf>,
}
enum Backend {
    Native {
        model: lang::AsrModel,
        onnx_file: Option<PathBuf>,
    },
    #[cfg(feature = "asr-fixtures")]
    Fixture(PathBuf),
}
impl Selection {
    pub fn resolve(
        session: &Path,
        params: &CookParams,
        cancelled: impl Fn() -> bool,
    ) -> Result<Self> {
        let language = lang::asr(session);
        let model = lang::asr_model(&language, &params.model_dir)
            .with_context(|| format!("session language «{language}»"))?;
        let onnx_file = (model.engine == lang::Engine::Onnx)
            .then(|| find_model_file(&model.dir))
            .transpose()?;
        let mut files = BTreeMap::new();
        let (engine, name) = if let Some(file) = &onnx_file {
            files.insert("model".into(), file.clone());
            files.insert(
                "vocabulary".into(),
                model
                    .dir
                    .join(crate::asr::onnx::adapters::gigaam_v3_e2e_ctc::VOCAB_FILENAME),
            );
            (
                "gigaam-v3-e2e-ctc",
                file.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            )
        } else {
            // Vosk loads several files; fingerprint every regular model dependency.
            let mut pending = vec![model.dir.clone()];
            let mut visited = std::collections::BTreeSet::new();
            while let Some(directory) = pending.pop() {
                ensure!(!cancelled(), "model fingerprint cancelled");
                if !visited.insert(directory.canonicalize()?) {
                    continue;
                }
                let mut entries = fs::read_dir(directory)?
                    .map(|e| e.map(|e| e.path()))
                    .collect::<std::io::Result<Vec<_>>>()?;
                entries.sort();
                for path in entries {
                    let kind = fs::metadata(&path)?.file_type();
                    if kind.is_dir() {
                        pending.push(path);
                    } else if kind.is_file() {
                        files.insert(
                            path.strip_prefix(&model.dir)?
                                .to_string_lossy()
                                .replace('\\', "/"),
                            path,
                        );
                    }
                }
            }
            (
                "vosk",
                model
                    .dir
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            )
        };
        let identity = Identity::from_files(
            engine.into(),
            language.clone(),
            params.recipe(&language),
            files.clone(),
            cancelled,
        )?;
        Ok(Self {
            backend: Backend::Native { model, onnx_file },
            name,
            identity,
            files,
        })
    }
    pub fn load(&self) -> Result<Box<dyn AsrEngine>> {
        let engine = match &self.backend {
            Backend::Native { model, onnx_file } => load_native(model, onnx_file.as_deref())?,
            #[cfg(feature = "asr-fixtures")]
            Backend::Fixture(path) => {
                Box::new(super::fixture::FixtureEngine::load(path)?) as Box<dyn AsrEngine>
            }
        };
        let current = Identity::from_files(
            self.identity.engine.clone(),
            self.identity.language.clone(),
            self.identity.recipe.clone(),
            self.files.clone(),
            || false,
        )?;
        ensure!(
            current == self.identity,
            "model dependencies changed while loading ASR"
        );
        Ok(engine)
    }
    #[cfg(feature = "asr-fixtures")]
    pub fn fixture(session: &Path, params: &CookParams, path: &Path) -> Result<Self> {
        let path = path.canonicalize()?;
        let files: BTreeMap<_, _> = [("fixture-spec".into(), path.clone())].into();
        let language = lang::asr(session);
        let identity = Identity::from_files(
            "synthetic-worker-fixture".into(),
            language.clone(),
            params.recipe(&language),
            files.clone(),
            || false,
        )?;
        Ok(Self {
            backend: Backend::Fixture(path),
            name: "synthetic-worker-fixture".into(),
            identity,
            files,
        })
    }
    pub(super) fn crash_if_fixture(&self, point: &str) {
        #[cfg(feature = "asr-fixtures")]
        if matches!(&self.backend, Backend::Fixture(_))
            && std::env::var("LOCALVOX_ASR_FIXTURE_CRASH_POINT").as_deref() == Ok(point)
        {
            std::process::exit(73);
        }
        let _ = point;
    }
}

pub(super) fn legacy(session: &Path, params: &CookParams) -> Result<(Box<dyn AsrEngine>, String)> {
    let language = lang::asr(session);
    let model = lang::asr_model(&language, &params.model_dir)?;
    let file = (model.engine == lang::Engine::Onnx)
        .then(|| find_model_file(&model.dir))
        .transpose()?;
    let name = file
        .as_deref()
        .unwrap_or(&model.dir)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    Ok((load_native(&model, file.as_deref())?, name))
}

fn load_native(model: &lang::AsrModel, file: Option<&Path>) -> Result<Box<dyn AsrEngine>> {
    match model.engine {
        lang::Engine::Onnx => {
            let adapter = GigaamV3E2eCtc::from_model_dir(&model.dir)
                .context("building the GigaAM v3 E2E CTC adapter")?;
            Ok(Box::new(
                OnnxEngine::new(file.context("ONNX model selection")?, adapter)
                    .context("loading the ONNX model")?,
            ))
        }
        lang::Engine::Vosk => Ok(Box::new(
            crate::asr::vosk::VoskEngine::new(&model.dir).context("loading the Vosk model")?,
        )),
    }
}
