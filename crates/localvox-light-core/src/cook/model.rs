//! The same concrete engine and weights are used by early and final transcription.
use super::CookParams;
use crate::incremental::Identity;
use crate::{
    asr::{package::ModelPackage, AsrEngine},
    lang,
};
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct Selection {
    backend: Backend,
    pub name: String,
    pub identity: Identity,
    files: BTreeMap<String, PathBuf>,
}
enum Backend {
    Native(ModelPackage),
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
        let package = ModelPackage::resolve(model)?;
        let files = package.dependencies(&cancelled)?;
        let engine = package.engine;
        let name = package.name.clone();
        let identity = Identity::from_files(
            engine.into(),
            language.clone(),
            params.recipe(&language),
            files.clone(),
            cancelled,
        )?;
        Ok(Self {
            backend: Backend::Native(package),
            name,
            identity,
            files,
        })
    }
    pub fn load(&self) -> Result<Box<dyn AsrEngine>> {
        let engine = match &self.backend {
            Backend::Native(package) => package.load()?,
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
    let package = ModelPackage::resolve(model)?;
    let name = package.name.clone();
    Ok((package.load()?, name))
}
