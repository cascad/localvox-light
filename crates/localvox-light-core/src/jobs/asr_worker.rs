//! Internal single-request ASR protocol; no engine dependency in the daemon.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const PROTOCOL: &str = "localvox-asr/1";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub protocol: String,
    pub id: u64,
    pub session: PathBuf,
    pub model_dir: PathBuf,
    pub prefix: bool,
    pub input: Option<crate::incremental::FrozenInput>,
    pub force: bool,
}
impl Request {
    pub fn new(id: u64, session: PathBuf, model_dir: PathBuf) -> Self {
        Self {
            protocol: PROTOCOL.into(),
            id,
            session,
            model_dir,
            prefix: false,
            input: None,
            force: false,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub protocol: String,
    pub id: u64,
    pub result: Result<serde_json::Value, String>,
    pub model_loads: u64,
}
impl Reply {
    pub fn validate(&self, id: u64) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.protocol == PROTOCOL && self.id == id,
            "ASR reply protocol/id mismatch"
        );
        Ok(())
    }
}
