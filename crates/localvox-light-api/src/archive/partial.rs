use super::*;
use localvox_light_core::{chunks::capture, incremental::Published, processing};

#[derive(Clone, Debug, Serialize)]
pub struct PartialInfo {
    pub generation: String,
    pub revision: u64,
    pub snapshot: Option<String>,
    pub sealed_end_sec: f64,
    pub stt_end_sec: f64,
    pub text_end_sec: f64,
    pub audio_end_sec: f64,
}
impl Archive {
    pub(super) fn generation(dir: &Path) -> Option<String> {
        let meta: localvox_light_core::chunks::SessionMeta =
            serde_json::from_slice(&fs::read(dir.join("meta.json")).ok()?).ok()?;
        meta.capture_generation
    }
    pub(super) fn line_id(
        generation: Option<&str>,
        source: u8,
        start: f64,
        end: f64,
    ) -> Option<String> {
        generation.map(|generation| {
            format!(
                "{generation}/{source}/{}-{}",
                (start * SR).round() as u64,
                (end * SR).round() as u64
            )
        })
    }
    pub(super) fn partial_document(
        &self,
        dir: &Path,
    ) -> Result<Option<(PartialInfo, Vec<TranscriptLineOut>)>> {
        if !dir.join("capture").exists()
            || processing::inspect(dir, processing::TRANSCRIPT).complete()
        {
            return Ok(None);
        }
        let captured = capture::read(dir)?;
        let sealed_end_sec = captured
            .sources
            .values()
            .map(|s| s.confirmed_end())
            .min()
            .unwrap_or(0) as f64
            / SR;
        let mut info = PartialInfo {
            generation: captured.generation.clone(),
            revision: 0,
            snapshot: None,
            sealed_end_sec,
            stt_end_sec: 0.0,
            text_end_sec: 0.0,
            audio_end_sec: 0.0,
        };
        let Some(published) = Published::current(dir)? else {
            return Ok(Some((info, vec![])));
        };
        let snapshot = published.read(dir)?;
        info.revision = snapshot.input.revision;
        info.snapshot = Some(published.snapshot);
        info.stt_end_sec = snapshot.checkpoint.common_end as f64 / SR;
        info.text_end_sec = snapshot.publication_end as f64 / SR;
        info.audio_end_sec = snapshot.audio_end as f64 / SR;
        let meta: localvox_light_core::chunks::SessionMeta =
            serde_json::from_slice(&fs::read(dir.join("meta.json"))?)?;
        let lines = snapshot
            .lines
            .into_iter()
            .map(|l| TranscriptLineOut {
                id: Some(l.id),
                who: localvox_light_core::chunks::source_label(&meta, l.source_id),
                source_id: l.source_id,
                start_sec: l.start_sample as f64 / SR,
                end_sec: l.end_sample as f64 / SR,
                text: l.text,
            })
            .collect();
        Ok(Some((info, lines)))
    }
    pub fn readable_document(&self, name: &str) -> Result<Value> {
        let dir = self.session_dir(name)?;
        if let Some((partial, lines)) = self.partial_document(&dir)? {
            return Ok(
                json!({"version_id":0,"lines":lines,"provenance":null,"omitted":0,"rejected":0,"from_transcript":true,"partial":partial}),
            );
        }
        let mut value = serde_json::to_value(localvox_light_core::readable::document(&dir)?)?;
        let generation = Self::generation(&dir);
        if let Some(lines) = value["lines"].as_array_mut() {
            for line in lines {
                line["id"] = json!(Self::line_id(
                    generation.as_deref(),
                    line["source_id"].as_u64().unwrap_or(0) as u8,
                    line["start_sec"].as_f64().unwrap_or(0.0),
                    line["end_sec"].as_f64().unwrap_or(0.0)
                ));
            }
        }
        Ok(value)
    }
    pub fn playback(&self, name: &str) -> Result<Value> {
        let dir = self.session_dir(name)?;
        if let Some((partial, _)) = self.partial_document(&dir)? {
            return Ok(
                json!({"duration_sec":partial.audio_end_sec,"snapshot":partial.snapshot,"partial":partial}),
            );
        }
        let meta: localvox_light_core::chunks::SessionMeta =
            serde_json::from_slice(&fs::read(dir.join("meta.json"))?)?;
        let duration = meta
            .chunks
            .iter()
            .map(|c| c.start_offset_sec + c.duration_sec)
            .fold(0.0_f64, f64::max);
        Ok(json!({"duration_sec":duration,"snapshot":null,"partial":null}))
    }
}
