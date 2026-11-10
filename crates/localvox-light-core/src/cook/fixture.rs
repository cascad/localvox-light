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
    #[serde(default)]
    delay_ms: u64,
    #[serde(default)]
    fault_file: Option<PathBuf>,
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
        if let Some(path) = &self.spec.fault_file {
            match fs::read_to_string(path) {
                Ok(mode) if mode.trim() == "fail" => anyhow::bail!("synthetic ASR failure"),
                Ok(mode) if mode.trim() == "wait" => {
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
                    while path.exists() && std::time::Instant::now() < deadline {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                    anyhow::bail!("synthetic ASR stall ended without a result");
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
                Ok(_) => {}
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(self.spec.delay_ms));
        Ok(if self.spec.include_pcm_sha {
            format!("{} {sha}", self.spec.answer)
        } else {
            self.spec.answer.clone()
        })
    }
}
