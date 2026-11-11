//! One model, owned by one sequential ASR process. No session locks are retained.
use super::*;

#[derive(Default)]
pub struct AsrRuntime {
    identity: Option<crate::incremental::Identity>,
    engine: Option<Box<dyn AsrEngine>>,
    loads: u64,
}
impl AsrRuntime {
    pub fn loads(&self) -> u64 {
        self.loads
    }
    pub fn clear(&mut self) {
        self.engine = None;
        self.identity = None;
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
                "Загрузка ASR-модели и адаптера",
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
