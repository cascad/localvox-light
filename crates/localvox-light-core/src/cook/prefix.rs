use super::*;
use crate::incremental::{consume_windows, AudioStats, Cache, Checkpoint, Horizon};
use std::collections::BTreeMap;

#[derive(Default, serde::Serialize)]
pub struct PrefixTimings {
    pub setup_sec: f64,
    pub model_sec: f64,
    pub recognize_sec: f64,
    pub cache_sec: f64,
    pub audio_journal_sec: f64,
    pub commit_sec: f64,
}
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
    pub timings: PrefixTimings,
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
    prefix_impl(session, params, None, None, &mut AsrRuntime::default())
}
pub fn transcribe_input(
    session: &Path,
    params: &CookParams,
    input: &crate::incremental::FrozenInput,
) -> Result<PrefixReport> {
    prefix_impl(
        session,
        params,
        None,
        Some(input),
        &mut AsrRuntime::default(),
    )
}
pub fn transcribe_warm(
    session: &Path,
    params: &CookParams,
    input: Option<&crate::incremental::FrozenInput>,
    runtime: &mut AsrRuntime,
) -> Result<PrefixReport> {
    prefix_impl(session, params, None, input, runtime)
}
#[cfg(feature = "asr-fixtures")]
pub fn transcribe_warm_fixture(
    session: &Path,
    params: &CookParams,
    spec: &Path,
    input: Option<&crate::incremental::FrozenInput>,
    runtime: &mut AsrRuntime,
) -> Result<PrefixReport> {
    prefix_impl(
        session,
        params,
        Some(model::Selection::fixture(session, params, spec)?),
        input,
        runtime,
    )
}
#[cfg(feature = "asr-fixtures")]
pub fn transcribe_prefix_fixture(
    session: &Path,
    params: &CookParams,
    spec: &Path,
    input: Option<&crate::incremental::FrozenInput>,
) -> Result<PrefixReport> {
    prefix_impl(
        session,
        params,
        Some(model::Selection::fixture(session, params, spec)?),
        input,
        &mut AsrRuntime::default(),
    )
}
fn prefix_impl(
    session: &Path,
    params: &CookParams,
    override_model: Option<model::Selection>,
    input: Option<&crate::incremental::FrozenInput>,
    runtime: &mut AsrRuntime,
) -> Result<PrefixReport> {
    let started = Instant::now();
    let horizon = match input {
        Some(input) => input.horizon(session)?,
        None => Horizon::read(session)?,
    };
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
            timings: PrefixTimings {
                setup_sec: started.elapsed().as_secs_f64(),
                ..Default::default()
            },
        });
    }
    let selection = match override_model {
        Some(model) => model,
        None => model::Selection::resolve(session, params, || false)?,
    };
    let cache = Cache::open(session, &selection.identity)?;
    let transaction = cache.begin(&horizon)?;
    let previous = transaction.state();
    let after: BTreeMap<_, _> = previous
        .sources
        .iter()
        .map(|(id, s)| (*id, s.end_sample))
        .collect();
    let loads_before = runtime.loads();
    let mut accepted = Vec::new();
    let mut hits = 0;
    let mut asr_calls = 0;
    let total = horizon.windows().values().map(|w| w.len()).sum::<usize>();
    let done_before = horizon
        .windows()
        .iter()
        .map(|(id, windows)| windows.iter().filter(|w| w.end_sample <= after[id]).count())
        .sum::<usize>();
    let mut timings = PrefixTimings {
        setup_sec: started.elapsed().as_secs_f64(),
        ..Default::default()
    };
    let mut operation = crate::journal::Operation::start(
        session,
        "transcribe",
        "Распознавание готовой части записи",
        None,
    );
    let audio_started = Instant::now();
    let mut cache_total = 0.0;
    let mut progress_at = Instant::now();
    crate::progress::progress(
        session,
        crate::progress::Stage::Transcribe,
        done_before as u32,
        total.max(1) as u32,
    );
    let audio = consume_windows(
        session,
        &horizon,
        &after,
        || false,
        |window| {
            let range = format!(
                "ASR source {} samples {}..{}",
                window.descriptor().source,
                window.descriptor().start_sample,
                window.descriptor().end_sample
            );
            // Stage counters report completed windows, coalesced to one second.
            // Avoid rewriting journal metadata for every technical PCM range;
            // retain that detail in debug diagnostics and in any failure context.
            tracing::debug!(
                source = window.descriptor().source,
                start_sample = window.descriptor().start_sample,
                end_sample = window.descriptor().end_sample,
                "ASR ready window"
            );
            let cache_started = Instant::now();
            let recognize_before = timings.recognize_sec + timings.model_sec;
            let (result, reused) = cache
                .resolve(window, |pcm| {
                    let model_started = Instant::now();
                    let engine = runtime.engine_for(&selection, session)?;
                    timings.model_sec += model_started.elapsed().as_secs_f64();
                    asr_calls += 1;
                    let recognize_started = Instant::now();
                    let input: Vec<_> = pcm.iter().map(|s| f32::from(*s) / 32768.0).collect();
                    let text = engine
                        .transcribe(&input)
                        .context("recognizing a ready window");
                    timings.recognize_sec += recognize_started.elapsed().as_secs_f64();
                    text
                })
                .with_context(|| range)?;
            let resolve_sec = cache_started.elapsed().as_secs_f64();
            cache_total += resolve_sec;
            timings.cache_sec += (resolve_sec
                - (timings.recognize_sec + timings.model_sec - recognize_before))
                .max(0.0);
            hits += usize::from(reused);
            accepted.push(result);
            selection.crash_if_fixture("after_result");
            if progress_at.elapsed() >= std::time::Duration::from_secs(1) {
                crate::progress::progress(
                    session,
                    crate::progress::Stage::Transcribe,
                    (done_before + accepted.len()) as u32,
                    total.max(1) as u32,
                );
                progress_at = Instant::now();
            }
            Ok(())
        },
    )
    .inspect_err(|_| operation.failed())?;
    crate::progress::progress(
        session,
        crate::progress::Stage::Transcribe,
        (done_before + accepted.len()) as u32,
        total.max(1) as u32,
    );
    timings.audio_journal_sec = (audio_started.elapsed().as_secs_f64() - cache_total).max(0.0);
    let commit_started = Instant::now();
    let checkpoint = transaction
        .finish(session, &accepted)
        .inspect_err(|_| operation.failed())?;
    timings.commit_sec = commit_started.elapsed().as_secs_f64();
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
        model_loaded: runtime.loads() > loads_before,
        checkpoint: Some(checkpoint),
        wall_sec: started.elapsed().as_secs_f64(),
        timings,
    })
}
