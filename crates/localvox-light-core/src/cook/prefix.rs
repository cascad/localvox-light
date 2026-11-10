use super::*;
use crate::incremental::{consume_windows, AudioStats, Cache, Checkpoint, Horizon};
use std::collections::BTreeMap;

#[derive(serde::Serialize)]
pub struct PrefixReport {
    pub generation: String,
    pub horizon_revision: u64,
    pub audio: AudioStats,
    pub cache_hits: usize,
    pub asr_calls: usize,
    pub model_loaded: bool,
    #[serde(serialize_with = "checkpoint_receipt")]
    pub checkpoint: Option<Checkpoint>,
    pub wall_sec: f64,
}

fn checkpoint_receipt<S: serde::Serializer>(
    value: &Option<Checkpoint>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    let summary=value.as_ref().map(|state| {
        let sources:BTreeMap<_,_>=state.sources.iter().map(|(id,s)|(*id,serde_json::json!({"end_sample":s.end_sample,"eof":s.eof}))).collect();
        Ok::<_,anyhow::Error>(serde_json::json!({"version":state.version,"generation":state.generation,"recognizer":state.recognizer,
            "capture_revision":state.capture_revision,"common_end":state.common_end,"publication_end":state.publication_end,
            "complete":state.complete,"sources":sources,"sha256":state.fingerprint()?}))
    }).transpose().map_err(<S::Error as serde::ser::Error>::custom)?;
    serde::Serialize::serialize(&summary, serializer)
}

/// One finite ready-input horizon. Caller holds processing execution ownership;
/// this never writes versions, best, processed, summary or primary capture state.
pub fn transcribe_prefix(session: &Path, params: &CookParams) -> Result<PrefixReport> {
    prefix_impl(session, params, None)
}
#[cfg(feature = "asr-fixtures")]
pub fn transcribe_prefix_fixture(
    session: &Path,
    params: &CookParams,
    spec: &Path,
) -> Result<PrefixReport> {
    prefix_impl(
        session,
        params,
        Some(model::Selection::fixture(session, params, spec)?),
    )
}
fn prefix_impl(
    session: &Path,
    params: &CookParams,
    override_model: Option<model::Selection>,
) -> Result<PrefixReport> {
    let started = Instant::now();
    let horizon = Horizon::read(session)?;
    ensure!(
        horizon.recipe().max_samples == (params.max_window_sec * 16000.0) as u64
            && horizon.recipe().min_cut_samples == (params.min_cut_sec * 16000.0) as u64
            && horizon.recipe().silence_ms == params.silence_ms,
        "capture planner and ASR window recipes differ"
    );
    if horizon.windows().values().all(|windows| windows.is_empty()) {
        return Ok(PrefixReport {
            generation: horizon.generation().into(),
            horizon_revision: horizon.revision(),
            audio: AudioStats::default(),
            cache_hits: 0,
            asr_calls: 0,
            model_loaded: false,
            checkpoint: None,
            wall_sec: started.elapsed().as_secs_f64(),
        });
    }
    let selection = match override_model {
        Some(model) => model,
        None => model::Selection::resolve(session, params, || false)?,
    };
    let cache = Cache::open(session, &selection.identity)?;
    let (previous, _) = cache.read_checkpoint(&horizon)?;
    let after: BTreeMap<_, _> = previous
        .sources
        .iter()
        .map(|(id, s)| (*id, s.end_sample))
        .collect();
    let mut engine: Option<Box<dyn AsrEngine>> = None;
    let mut accepted = Vec::new();
    let mut hits = 0;
    let mut asr_calls = 0;
    let total = horizon.windows().values().map(|w| w.len()).sum::<usize>();
    let done_before = horizon
        .windows()
        .iter()
        .map(|(id, windows)| windows.iter().filter(|w| w.end_sample <= after[id]).count())
        .sum::<usize>();
    let operation = crate::journal::Operation::start(
        session,
        "transcribe",
        "Распознавание готовой части записи",
        None,
    );
    let audio = consume_windows(
        session,
        &horizon,
        &after,
        || false,
        |window| {
            let (result, reused) = cache.resolve(window, |pcm| {
                if engine.is_none() {
                    let _load = crate::journal::Operation::start(
                        session,
                        "transcribe",
                        "Загрузка ASR-модели и адаптера",
                        None,
                    );
                    engine = Some(selection.load()?);
                }
                asr_calls += 1;
                let input: Vec<_> = pcm.iter().map(|s| f32::from(*s) / 32768.0).collect();
                engine
                    .as_ref()
                    .unwrap()
                    .transcribe(&input)
                    .context("recognizing a ready window")
            })?;
            hits += usize::from(reused);
            accepted.push(result);
            selection.crash_if_fixture("after_result");
            crate::progress::progress(
                session,
                crate::progress::Stage::Transcribe,
                (done_before + accepted.len()) as u32,
                total.max(1) as u32,
            );
            Ok(())
        },
    )?;
    let checkpoint = cache.checkpoint(&horizon, &accepted)?;
    selection.crash_if_fixture("after_checkpoint");
    operation.log(&format!(
        "Готово до {} сэмплов; новых ASR окон: {asr_calls}, cache hits: {hits}",
        checkpoint.common_end
    ));
    Ok(PrefixReport {
        generation: horizon.generation().into(),
        horizon_revision: horizon.revision(),
        audio,
        cache_hits: hits,
        asr_calls,
        model_loaded: engine.is_some(),
        checkpoint: Some(checkpoint),
        wall_sec: started.elapsed().as_secs_f64(),
    })
}
