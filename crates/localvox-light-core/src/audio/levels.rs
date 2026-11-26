//! Ephemeral per-input meters shared by embedded and standalone archive servers.
//! The callback accumulates in ordinary RAM. The stream owner copies each word
//! to a shared page; clients can read independently without consuming a peak.
use anyhow::Result;
use memmap2::{MmapOptions, MmapRaw};
use std::{
    fs::OpenOptions,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const FILE: &str = ".capture-levels-v1";
const BYTES: usize = 2 * size_of::<AtomicU64>();

pub(super) fn bucket() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
        / 250
}

// High bits: quarter-second timestamp. Low bytes: greatest segment count in
// the current and preceding window, plus one to distinguish silence from no PCM.
pub(super) fn accumulate(old: u64, now: u64, rms: f32) -> u64 {
    // RMS thresholds in dBFS: -72, -48, -36, -24, -12, -6.
    let level = 1 + [0.00025, 0.00398, 0.01585, 0.0631, 0.2512, 0.5012]
        .partition_point(|threshold| rms >= *threshold) as u64;
    let previous = match now.checked_sub(old >> 16) {
        Some(0) => return (old & !255) | level.max(old & 255),
        Some(1) => old & 255,
        _ => 0,
    };
    (now << 16) | (previous << 8) | level
}

fn recent(word: u64, now: u64) -> Option<u8> {
    let current = word as u8;
    match now.checked_sub(word >> 16) {
        Some(0) => current.max((word >> 8) as u8).checked_sub(1),
        Some(1) => current.checked_sub(1),
        _ => None,
    }
}

pub(super) struct Levels(MmapRaw);

impl Levels {
    pub(super) fn create(workspace: &Path) -> Result<Self> {
        // Each engine replaces the file under the workspace lease. An existing
        // reader keeps its old mapping; no mapped file is truncated in place.
        crate::artifacts::atomic_write(&workspace.join(FILE), &[0; BYTES])?;
        Self::open(workspace)
    }

    fn open(workspace: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(workspace.join(FILE))?;
        anyhow::ensure!(
            file.metadata()?.len() == BYTES as u64,
            "invalid capture meter page"
        );
        Ok(Self(MmapOptions::new().len(BYTES).map_raw(&file)?))
    }

    fn slot(&self, index: usize) -> &AtomicU64 {
        assert!(index < 2);
        // SAFETY: page-aligned read/write mapping, exactly two u64s, accessed
        // exclusively through atomics. The reference cannot outlive the map.
        unsafe { AtomicU64::from_ptr(self.0.as_mut_ptr().cast::<u64>().add(index)) }
    }

    pub(super) fn set(&self, source: u8, word: u64) {
        self.slot(source as usize).store(word, Ordering::Relaxed);
    }
}

pub fn read(workspace: &Path) -> [Option<u8>; 2] {
    let Ok(page) = Levels::open(workspace) else {
        return [None; 2];
    };
    let now = bucket();
    std::array::from_fn(|i| recent(page.slot(i).load(Ordering::Relaxed), now))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peak_survives_a_short_pause_then_expires_and_a_clock_jump_is_not_sound() {
        let peak = accumulate(0, 100, 0.6);
        let quiet = accumulate(peak, 100, 0.0);
        assert_eq!(recent(quiet, 100), Some(6));
        let silent = accumulate(quiet, 101, 0.0);
        assert_eq!(recent(silent, 101), Some(6));
        assert_eq!(recent(silent, 102), Some(0));
        assert_eq!(recent(silent, 103), None);
        assert_eq!(recent(silent, 99), None);
    }
}
