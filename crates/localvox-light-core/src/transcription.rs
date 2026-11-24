//! Durable ASR choice. Global defaults apply to new sessions, never the old archive.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const BACKEND_KEY: &str = "LOCALVOX_ASR_BACKEND";
pub const PUNCTUATION_KEY: &str = "LOCALVOX_ASR_PUNCTUATION";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    #[default]
    Gigaam,
    Tone,
}
impl Backend {
    pub const ALL: [Self; 2] = [Self::Gigaam, Self::Tone];
    pub fn id(self) -> &'static str {
        match self {
            Self::Gigaam => "gigaam",
            Self::Tone => "tone",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Gigaam => "GigaAM",
            Self::Tone => "T-one · greedy",
        }
    }
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim() {
            "gigaam" => Ok(Self::Gigaam),
            "tone" => Ok(Self::Tone),
            _ => anyhow::bail!("неизвестный ASR backend «{value}»: gigaam или tone"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    #[serde(default)]
    pub backend: Backend,
    #[serde(default)]
    pub punctuation: bool,
}
impl Choice {
    pub fn validate_for(self, session: &Path, gigaam: &Path) -> Result<()> {
        let language = crate::lang::asr(session);
        match self.backend {
            Backend::Tone => {
                ensure!(
                    language == "ru",
                    "T-one поддерживает русский язык; язык записи «{language}»"
                );
                self.tone_directory()?;
            }
            Backend::Gigaam => {
                let model = crate::lang::asr_model(&language, gigaam)?;
                if model.engine == crate::lang::Engine::Onnx {
                    ensure!(
                        model.dir.join("v3_e2e_ctc_vocab.txt").is_file(),
                        "нет словаря ASR: {}",
                        model.dir.display()
                    );
                }
            }
        }
        if self.punctuation {
            let found = crate::model_paths::Context::capture()
                .resolve(crate::model_paths::component("sbert"), None);
            ensure!(
                found.ready,
                "SbertPuncCase не готов: {}; выполните vox setup",
                found.path.unwrap_or_default().display()
            );
        }
        Ok(())
    }
    /// Caller must own the session mutation lease and check readiness first.
    pub fn save(self, session: &Path) -> Result<()> {
        let path = session.join("meta.json");
        let mut meta: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        ensure!(meta.is_object(), "meta.json must be an object");
        meta["asr"] = serde_json::to_value(self)?;
        crate::artifacts::atomic_write(&path, &serde_json::to_vec_pretty(&meta)?)
    }
    pub fn configured() -> Result<Self> {
        let backend = std::env::var(BACKEND_KEY)
            .unwrap_or_else(|_| crate::presets::baked(BACKEND_KEY).into());
        let punctuation = std::env::var(PUNCTUATION_KEY)
            .unwrap_or_else(|_| crate::presets::baked(PUNCTUATION_KEY).into());
        Self::parse(&backend, &punctuation)
    }
    pub fn parse(backend: &str, punctuation: &str) -> Result<Self> {
        let punctuation = punctuation.trim().to_ascii_lowercase();
        ensure!(
            ["on", "off", "true", "false", "1", "0"].contains(&punctuation.as_str()),
            "{PUNCTUATION_KEY}: on или off"
        );
        Ok(Self {
            backend: Backend::parse(backend)?,
            punctuation: crate::presets::bool_value(&punctuation),
        })
    }
    pub fn from_meta(meta: &serde_json::Value) -> Result<Self> {
        match meta.get("asr") {
            None | Some(serde_json::Value::Null) => Ok(Self::default()),
            Some(value) => serde_json::from_value(value.clone()).context("выбор ASR в meta.json"),
        }
    }
    pub fn for_session(session: &Path) -> Result<Self> {
        match std::fs::read(session.join("meta.json")) {
            Ok(bytes) => Self::from_meta(&serde_json::from_slice(&bytes)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }
    pub fn label(self, language: &str) -> String {
        match self.backend {
            Backend::Gigaam => crate::lang::label(language),
            Backend::Tone => format!("tone-greedy-{language}"),
        }
    }
    /// Resolve with exactly the model placement rules used by setup/doctor.
    pub fn tone_directory(self) -> Result<PathBuf> {
        ensure!(self.backend == Backend::Tone, "T-one is not selected");
        let found = crate::model_paths::Context::capture()
            .resolve(crate::model_paths::component("tone"), None);
        let path = found
            .path
            .context("не определена папка T-one; выполните vox setup")?;
        ensure!(
            found.ready,
            "модель T-one не готова: {}; выполните vox setup",
            path.display()
        );
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_choice_does_not_follow_a_new_global_default() {
        assert_eq!(
            Choice::from_meta(&serde_json::json!({})).unwrap(),
            Choice::default()
        );
        let choice =
            Choice::from_meta(&serde_json::json!({"asr":{"backend":"tone","punctuation":true}}))
                .unwrap();
        assert_eq!(choice.backend, Backend::Tone);
        assert!(choice.punctuation);
        assert_ne!(choice.label("ru"), crate::lang::label("ru"));
        assert!(Choice::from_meta(&serde_json::json!({"asr":{"backend":"bad"}})).is_err());
        assert!(Backend::parse("bad").is_err());
    }
}
