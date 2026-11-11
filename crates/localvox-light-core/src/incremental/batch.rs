//! Bounded computed outputs. Accepted results are released only after journal sync
//! and fresh verification. Dropping a pending group cannot publish a checkpoint.
use super::cache::Prepared;
use super::*;

const MAX_RESULTS: usize = super::wal::MAX_GROUP_RECORDS;
const MAX_BYTES: usize = 1024 * 1024;

/// Preview for building an in-memory transcript; never a durable acceptance token.
pub(crate) struct PreparedOutput {
    pub text: Option<String>,
    pub reused: bool,
    pub durable: bool,
}
pub(crate) struct ResultBatch<'a> {
    cache: &'a Cache,
    pending: Vec<Prepared>,
    bytes: usize,
    accepted: Vec<Accepted>,
}
impl<'a> ResultBatch<'a> {
    pub fn new(cache: &'a Cache) -> Self {
        Self {
            cache,
            pending: Vec::new(),
            bytes: 0,
            accepted: Vec::new(),
        }
    }
    pub fn len(&self) -> usize {
        self.accepted.len() + self.pending.len()
    }
    pub fn resolve(
        &mut self,
        window: VerifiedWindow<'_>,
        recognize: impl FnMut(&[i16]) -> Result<String>,
    ) -> Result<PreparedOutput> {
        let key = self.cache.key(window.descriptor())?;
        if let Some(previous) = self.pending.iter().find(|p| p.key() == key) {
            return Ok(PreparedOutput {
                text: previous.text().map(str::to_owned),
                reused: true,
                durable: false,
            });
        }
        let prepared = self.cache.prepare(window, recognize)?;
        let bytes = prepared.bytes();
        if self.bytes + bytes > MAX_BYTES {
            self.flush()?;
        }
        let mut preview = PreparedOutput {
            text: prepared.text().map(str::to_owned),
            reused: prepared.reused(),
            durable: false,
        };
        self.bytes += bytes;
        self.pending.push(prepared);
        if self.pending.len() >= MAX_RESULTS || self.bytes >= MAX_BYTES {
            self.flush()?;
            preview.durable = true;
        }
        Ok(preview)
    }
    fn flush(&mut self) -> Result<()> {
        let accepted = self.cache.commit_prepared(&self.pending)?;
        self.accepted.extend(accepted);
        self.pending.clear();
        self.bytes = 0;
        Ok(())
    }
    pub fn finish(mut self) -> Result<Vec<Accepted>> {
        self.flush()?;
        Ok(self.accepted)
    }
}
