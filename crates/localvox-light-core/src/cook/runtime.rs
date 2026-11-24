//! One model, owned by one sequential ASR process. No session locks are retained.
use super::*;

#[derive(Default)]
pub struct AsrRuntime {
    identity: Option<crate::incremental::Identity>,
    engine: Option<Box<dyn AsrEngine>>,
    loads: u64,
    pub(super) punctuation: crate::text_processing::pipeline::Runtime,
}
impl AsrRuntime {
    pub fn loads(&self) -> u64 {
        self.loads
    }
    pub fn clear(&mut self) {
        self.punctuation.clear();
        self.engine = None;
        self.identity = None;
    }
    pub fn release_text_processor(&mut self) {
        self.punctuation.clear();
    }
    pub(super) fn engine_for(
        &mut self,
        selection: &model::Selection,
        session: &Path,
    ) -> Result<&dyn AsrEngine> {
        // Selection is freshly fingerprinted for each request; paths/mtimes are
        // never sufficient evidence for reusing model weights.
        if self.identity.as_ref() != Some(&selection.identity) || self.engine.is_none() {
            self.clear();
            let mut load = crate::journal::Operation::start(
                session,
                "transcribe",
                &format!("Загрузка ASR · {}", selection.name),
                None,
            );
            let engine = selection.load().inspect_err(|_| load.failed())?;
            self.identity = Some(selection.identity.clone());
            self.engine = Some(engine);
            self.loads = self.loads.saturating_add(1);
        }
        Ok(self.engine.as_deref().expect("verified ASR runtime"))
    }
}
