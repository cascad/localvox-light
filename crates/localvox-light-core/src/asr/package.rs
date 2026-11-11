//! Native model inspection/construction. Callers supply policy (language/recipe)
//! and verify content identity; adapters expose the files that affect inference.
use super::{
    onnx::{adapters::GigaamV3E2eCtc, OnnxEngine},
    AsrEngine,
};
use crate::lang;
use anyhow::{bail, ensure, Context, Result};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub struct ModelPackage {
    model: lang::AsrModel,
    onnx_file: Option<PathBuf>,
    pub engine: &'static str,
    pub name: String,
}
impl ModelPackage {
    pub fn resolve(model: lang::AsrModel) -> Result<Self> {
        let onnx_file = (model.engine == lang::Engine::Onnx)
            .then(|| find_model_file(&model.dir))
            .transpose()?;
        let name = onnx_file
            .as_deref()
            .unwrap_or(&model.dir)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let engine = if onnx_file.is_some() {
            "gigaam-v3-e2e-ctc"
        } else {
            "vosk"
        };
        Ok(Self {
            model,
            onnx_file,
            engine,
            name,
        })
    }

    pub fn dependencies(&self, cancelled: impl Fn() -> bool) -> Result<BTreeMap<String, PathBuf>> {
        let mut files = BTreeMap::new();
        if let Some(file) = &self.onnx_file {
            files.insert("model".into(), file.clone());
            files.insert(
                "vocabulary".into(),
                self.model
                    .dir
                    .join(super::onnx::adapters::gigaam_v3_e2e_ctc::VOCAB_FILENAME),
            );
        } else {
            let mut pending = vec![self.model.dir.clone()];
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
                            path.strip_prefix(&self.model.dir)?
                                .to_string_lossy()
                                .replace('\\', "/"),
                            path,
                        );
                    }
                }
            }
        }
        Ok(files)
    }

    pub fn load(&self) -> Result<Box<dyn AsrEngine>> {
        match self.model.engine {
            lang::Engine::Onnx => {
                let adapter = GigaamV3E2eCtc::from_model_dir(&self.model.dir)
                    .context("building the GigaAM v3 E2E CTC adapter")?;
                Ok(Box::new(
                    OnnxEngine::new(
                        self.onnx_file.as_deref().context("ONNX model selection")?,
                        adapter,
                    )
                    .context("loading the ONNX model")?,
                ))
            }
            lang::Engine::Vosk => Ok(Box::new(
                super::vosk::VoskEngine::new(&self.model.dir).context("loading the Vosk model")?,
            )),
        }
    }
}

fn find_model_file(dir: &Path) -> Result<PathBuf> {
    let mut candidates: Vec<PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("model directory: {}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("onnx"))
        .collect();
    if candidates.is_empty() {
        bail!("no *.onnx in {}", dir.display());
    }
    candidates.sort_by_key(|p| !p.to_string_lossy().contains("int8"));
    Ok(candidates.remove(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn onnx_package_prefers_int8_and_exposes_the_same_weight_and_vocabulary_dependencies() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("model.onnx"), "fp32").unwrap();
        let int8 = root.path().join("model.int8.onnx");
        fs::write(&int8, "int8").unwrap();
        let package = ModelPackage::resolve(lang::AsrModel {
            engine: lang::Engine::Onnx,
            dir: root.path().into(),
        })
        .unwrap();
        assert_eq!(package.name, "model.int8.onnx");
        assert_eq!(package.engine, "gigaam-v3-e2e-ctc");
        let dependencies = package.dependencies(|| false).unwrap();
        assert_eq!(dependencies.len(), 2);
        assert_eq!(dependencies["model"], int8);
        assert_eq!(
            dependencies["vocabulary"],
            root.path()
                .join(super::super::onnx::adapters::gigaam_v3_e2e_ctc::VOCAB_FILENAME)
        );
    }
    #[test]
    fn vosk_package_fingerprints_nested_files_and_honours_cancellation() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("am")).unwrap();
        fs::write(root.path().join("am/final.mdl"), "weights").unwrap();
        fs::write(root.path().join("config"), "configuration").unwrap();
        let package = ModelPackage::resolve(lang::AsrModel {
            engine: lang::Engine::Vosk,
            dir: root.path().into(),
        })
        .unwrap();
        assert_eq!(package.engine, "vosk");
        assert_eq!(
            package
                .dependencies(|| false)
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["am/final.mdl", "config"]
        );
        assert!(package.dependencies(|| true).is_err());
    }
}
