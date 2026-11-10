//! Built only by explicit `asr-fixtures`, selected only by its hidden CLI argument.
use super::*;
use serde::Deserialize;
use std::sync::Mutex;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    answer: String,
    include_pcm_sha: bool,
    calls_file: PathBuf,
}
pub(super) struct FixtureEngine {
    spec: Spec,
    calls: Mutex<fs::File>,
}
impl FixtureEngine {
    pub fn load(path: &Path) -> Result<Self> {
        let spec: Spec = serde_json::from_slice(&fs::read(path)?)?;
        let calls = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&spec.calls_file)?;
        Ok(Self {
            spec,
            calls: Mutex::new(calls),
        })
    }
}
impl AsrEngine for FixtureEngine {
    fn name(&self) -> &str {
        "synthetic-worker-fixture"
    }
    fn transcribe(&self, samples: &[f32]) -> Result<String> {
        let pcm: Vec<_> = samples
            .iter()
            .map(|s| (s * 32768.0).clamp(-32768.0, 32767.0) as i16)
            .collect();
        let sha = crate::windowing::pcm_sha256(&pcm);
        let mut calls = self.calls.lock().unwrap();
        writeln!(calls, "{sha}")?;
        calls.sync_all()?;
        Ok(if self.spec.include_pcm_sha {
            format!("{} {sha}", self.spec.answer)
        } else {
            self.spec.answer.clone()
        })
    }
}
