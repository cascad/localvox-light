//! Text-only processors, independent of acoustic models and LLM providers.
#[cfg(feature = "onnx")]
pub mod pipeline;
#[cfg(feature = "onnx")]
pub mod sbert;

use crate::versions::TranscriptLine;
use anyhow::{ensure, Result};

/// A processor can change casing and boundary punctuation, never words or audio coordinates.
pub fn validate_text(raw: &str, rendered: &str) -> Result<()> {
    let raw: Vec<_> = raw.split_whitespace().collect();
    let rendered: Vec<_> = rendered.split_whitespace().collect();
    ensure!(
        raw.len() == rendered.len(),
        "пунктуация изменила число слов"
    );
    for (before, after) in raw.into_iter().zip(rendered) {
        let original = before.trim_end_matches(['.', ',', '?', '!', ';', ':']);
        let before = original.to_lowercase();
        let after = after.trim_end_matches(['.', ',', '?', '!', ';', ':']);
        let mut chars = before.chars();
        let title = chars
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default()
            + chars.as_str();
        ensure!(
            after == original
                || after == before
                || after == title
                || after == before.to_uppercase(),
            "пунктуация изменила слово или число"
        );
    }
    Ok(())
}
pub fn validate_lines(raw: &[TranscriptLine], rendered: &[TranscriptLine]) -> Result<()> {
    ensure!(
        raw.len() == rendered.len(),
        "пунктуация изменила число реплик"
    );
    for (a, b) in raw.iter().zip(rendered) {
        ensure!(
            a.source_id == b.source_id
                && a.start_sec == b.start_sec
                && a.end_sec == b.end_sec
                && a.speaker == b.speaker,
            "пунктуация изменила источник, участника или время реплики"
        );
        validate_text(&a.text, &b.text)?;
    }
    Ok(())
}

pub fn validate_partial(
    raw: &[crate::incremental::PartialLine],
    rendered: &[crate::incremental::PartialLine],
) -> Result<()> {
    ensure!(
        raw.len() == rendered.len(),
        "пунктуация изменила число готовых реплик"
    );
    for (a, b) in raw.iter().zip(rendered) {
        ensure!(
            a.id == b.id
                && a.source_id == b.source_id
                && a.start_sample == b.start_sample
                && a.end_sample == b.end_sample,
            "пунктуация изменила идентичность или границы готовой реплики"
        );
        validate_text(&a.text, &b.text)?;
    }
    Ok(())
}

pub trait TextProcessor: Send + Sync {
    fn name(&self) -> &str;
    fn process(&self, text: &str) -> anyhow::Result<String>;
}
