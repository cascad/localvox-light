//! Slow-lane cook (F8, WP-A3): session chunks → a transcript version.
//!
//! A detachable module (P5): works only with session files — `meta.json`,
//! `audio/*.wav`, `transcripts/` + `versions.json`. Chunks of one source are glued
//! sample-by-sample (file boundaries mean nothing), and the ASR windows are cut ON
//! TOP of the glued stream: the cut happens at VAD silence after `min_cut_sec`,
//! otherwise hard at `max_window_sec`. Windows of pure silence are not fed to the
//! model.
//!
//! **A transcript line = a WINDOW.** Not a phrase, not a word: the timecode of a
//! line is the boundary of the audio piece that was actually fed to the model,
//! counted in samples. That is why «▶» in the player plays exactly what is written
//! — by construction, not by luck. The attempt to cut a window into phrases by CTC
//! frames (13.07.2026) failed: CTC places tokens imprecisely, and a single phrase
//! drifted along the timeline by 13 seconds («Что здесь» / «есть?»). The model's
//! guess about time is a poor foundation; the number of samples is a good one.
//!
//! The output is jsonl lines `{source_id, start_sec, end_sec, text}` with timecodes
//! in the source's audio timeline; the version is registered in the manifest
//! (`versions.rs`).

use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, ensure, Context, Result};
use webrtc_vad::{SampleRate, Vad, VadMode};

use crate::asr::onnx::adapters::GigaamV3E2eCtc;
use crate::asr::onnx::OnnxEngine;
use crate::asr::AsrEngine;
use crate::lang;
use crate::versions::{now_rfc3339, TranscriptLine, VersionEntry, VersionStore};

const SAMPLE_RATE: u32 = 16_000;
#[cfg(test)]
use crate::windowing::FRAME_SAMPLES;
// Keep the public benchmark API while both capture and cook share the same planner.
pub use crate::windowing::Windower;

#[cfg(feature = "asr-fixtures")]
mod fixture;
pub mod model;
mod prefix;
#[cfg(feature = "asr-fixtures")]
pub use prefix::transcribe_prefix_fixture;
pub use prefix::{transcribe_prefix, PrefixReport};

pub struct CookParams {
    /// Model directory of the DEFAULT LANGUAGE (the caller knows its path: next to
    /// the exe in a built application). A model of another language is looked up by
    /// the language itself — see `lang::asr_model`.
    pub model_dir: PathBuf,
    /// Version label. `None` — derive it from the session language (the usual path:
    /// `ru` → `gigaam-int8`, `en` → `asr-en`). An explicit one is for experiments, so
    /// that two cooks of the same session by different engines do not overwrite each
    /// other.
    pub label: Option<String>,
    /// Hard ceiling of the inference window (GigaAM's input limit is ~200 s).
    pub max_window_sec: f64,
    /// After this duration the window closes at the first VAD silence.
    pub min_cut_sec: f64,
    /// How much continuous silence counts as a window boundary, ms.
    pub silence_ms: u32,
    pub force: bool,
}

impl Default for CookParams {
    fn default() -> Self {
        // Acceptance 2026-07-12: 150-second windows were choked by conversational
        // speech (speech is rare, thinned out by pauses) — GigaAM produced mush. The
        // benchmark confirmed it with a number: WER 93 % against 7.3 % on 30/15 s
        // windows (docs/asr-bench.md). The values themselves live in versions.rs: the
        // «recipe» depends on them, and by that recipe the auto-cook decides whether
        // it is time to re-cook the archive.
        Self {
            model_dir: PathBuf::from("models/gigaam-v3-e2e-ctc"),
            label: None,
            max_window_sec: crate::versions::DEFAULT_MAX_WINDOW_SEC,
            min_cut_sec: crate::versions::DEFAULT_MIN_CUT_SEC,
            silence_ms: crate::versions::DEFAULT_SILENCE_MS,
            force: false,
        }
    }
}

impl CookParams {
    /// Version label for the session language.
    pub fn label_for(&self, lang: &str) -> String {
        self.label.clone().unwrap_or_else(|| lang::label(lang))
    }

    /// The fingerprint of this cook's recipe — it goes into the version manifest. The
    /// LANGUAGE is part of the recipe (through the label): change the language and the
    /// session is «cooked by a different recipe», so it gets re-cooked by the usual
    /// archive migration mechanism. Speaker labelling is part of the recipe too, and we
    /// ask about it in exactly the same place as the daemon does (`diarize::enabled`),
    /// otherwise the recipes diverge and the session loops forever.
    pub fn recipe(&self, lang: &str) -> String {
        crate::versions::cook_recipe(
            &self.label_for(lang),
            self.max_window_sec,
            self.min_cut_sec,
            self.silence_ms,
            crate::diarize::enabled(),
        )
    }
}

pub enum CookOutcome {
    /// A version with this label already exists (restart/repeat) — we did nothing.
    Skipped { existing_version: u32 },
    /// The session has no audio chunks (e.g. an empty ambient one before the first
    /// call under WP-C6 sessionization) — not an error, there is simply nothing to
    /// cook.
    Empty,
    Done {
        version_id: u32,
        file: PathBuf,
        lines: usize,
        audio_sec: f64,
        wall_sec: f64,
    },
}

/// A full ASR cook of a session: all sources, all chunks → one transcript version.
///
/// The list of chunks is taken by **scanning `audio/`**, not from meta.json: chunks of
/// one source are glued sample-by-sample in the order of the numbers in their file
/// names, so the order is all the cook needs. That makes the cook resilient to
/// post-crash sessions, where the recovered chunks are missing from meta.
pub fn cook_session_asr(session_dir: &Path, p: &CookParams) -> Result<CookOutcome> {
    cook_session_impl(session_dir, p, None)
}
#[cfg(feature = "asr-fixtures")]
pub fn cook_session_asr_fixture(
    session_dir: &Path,
    p: &CookParams,
    spec: &Path,
) -> Result<CookOutcome> {
    cook_session_impl(
        session_dir,
        p,
        Some(model::Selection::fixture(session_dir, p, spec)?),
    )
}
fn cook_session_impl(
    session_dir: &Path,
    p: &CookParams,
    override_model: Option<model::Selection>,
) -> Result<CookOutcome> {
    ensure!(
        crate::chunks::capture::is_finalized(session_dir)?,
        "запись или подтверждение сохранённого аудио ещё не завершены"
    );
    let store = VersionStore::open(session_dir)?;

    // A cross-process lock on cooking this session: the auto-cook daemon and a manual
    // `localvox-process` must not cook the same session in parallel (a race on the
    // two-phase commit of versions.json). The loser blocks here, then sees the already
    // committed version below and leaves as Skipped.
    let cook_lock = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(session_dir.join(".cook.lock"))
        .context("session cook lock")?;
    cook_lock.lock().context("acquiring the cook lock")?;

    // We skip only if the session was cooked by the SAME recipe. A matching label is
    // not enough: the window parameters changed (180/150 → 30/15), and a version with
    // the old recipe is that very unreadable transcript. Checking by recipe IS the
    // archive migration mechanism: a session is re-cooked exactly once.
    // The language is part of the recipe: it picks both the model and the version label.
    let lang = lang::asr(session_dir);
    let label = p.label_for(&lang);
    let recipe = p.recipe(&lang);
    if !p.force {
        let manifest = store.load();
        let ledger = crate::processing::load(session_dir);
        if let Some(v) = manifest
            .versions
            .iter()
            // The NEWEST version of this recipe, not the first one found. Measured 19.07.2026:
            // after a `--force` re-cook the session held v001 and v003 of one recipe, and the
            // next cook run picked v001 — then decided `best` (v003) pointed at «someone else's»
            // version and moved it back onto v001. A deliberate re-cook was silently undone, and
            // the readable text was rebuilt from the older transcript.
            .filter(|v| v.params.get("recipe").and_then(|r| r.as_str()) == Some(recipe.as_str()))
            .filter(|v| {
                store
                    .resolve(v.id)
                    .and_then(|path| std::fs::read(path).ok())
                    .is_some_and(|bytes| crate::artifacts::transcript(&bytes).is_ok())
            })
            .filter(|v| {
                ledger
                    .artifacts
                    .get(crate::processing::TRANSCRIPT)
                    .is_none_or(|r| {
                        r.source != Some(v.id)
                            || r.receipt.is_none()
                            || crate::artifacts::verify(
                                session_dir,
                                crate::processing::TRANSCRIPT,
                                r,
                            )
                            .is_ok()
                    })
            })
            .max_by_key(|v| v.id)
        {
            // Cooked by this recipe — but does `best` POINT at it?
            //
            // The session could have been cooked in a different language (best moved
            // onto that version) and then the language switched back. A version with
            // the right recipe already exists → the cook is skipped → and `best` stays
            // FOREVER on someone else's mush: search, export and the summary are all
            // built from it. Skipping the work does not mean agreeing with a foreign
            // result.
            //
            // A derivative of the right version (`refined` from that same version) we
            // leave alone — it IS «this version, only cleaned up».
            let best_ok = manifest.best.is_some_and(|b| {
                b == v.id
                    || manifest
                        .versions
                        .iter()
                        .find(|x| x.id == b)
                        .is_some_and(|x| x.parents.contains(&v.id))
            });
            if !best_ok {
                tracing::info!(
                    "{}: best pointed at a version of another recipe — moving it back to v{:03}",
                    session_dir.display(),
                    v.id
                );
                store.set_best(v.id)?;
            }
            let path = store.resolve(v.id).context("committed transcript path")?;
            let lines = crate::artifacts::transcript(&std::fs::read(path)?)?;
            crate::processing::record(
                session_dir,
                crate::processing::TRANSCRIPT,
                &recipe,
                if lines.is_empty() {
                    crate::processing::Outcome::Nothing
                } else {
                    crate::processing::Outcome::Ok
                },
                Some(format!("{} lines", lines.len())),
                Some(v.id),
            )?;
            return Ok(CookOutcome::Skipped {
                existing_version: v.id,
            });
        }
        if let Some(old) = manifest.versions.iter().find(|v| v.label == label) {
            tracing::info!(
                "re-cooking {}: version v{:03} was cooked by another recipe ({} instead of {recipe})",
                session_dir.display(),
                old.id,
                old.params
                    .get("recipe")
                    .and_then(|r| r.as_str())
                    .unwrap_or("no recipe, pre-WP-C14")
            );
        }
    }

    crate::chunks::capture::validate_final_input(session_dir)?;
    // An empty session is cut off BEFORE the model is loaded: not an error, nothing to
    // cook. An error (e.g. a FLAC chunk) is propagated — that is not «empty».
    let mut has_chunks = false;
    for sid in [0u8, 1u8] {
        if !chunk_files_for_source(session_dir, sid)?.is_empty() {
            has_chunks = true;
        }
    }
    if !has_chunks {
        return Ok(CookOutcome::Empty);
    }

    let capture = session_dir
        .join("capture")
        .exists()
        .then(|| crate::chunks::capture::read(session_dir))
        .transpose()?;
    let window_recipe = crate::chunks::planning::Recipe {
        revision: 1,
        max_samples: (p.max_window_sec * SAMPLE_RATE as f64) as u64,
        min_cut_samples: (p.min_cut_sec * SAMPLE_RATE as f64) as u64,
        silence_ms: p.silence_ms,
    };
    // Explicit force retains its fresh-recognition meaning; legacy archives retain their route.
    let use_cache = capture.is_some() && !p.force && window_recipe.validate().is_ok();
    let selection = match override_model {
        Some(model) => Some(model),
        None => use_cache
            .then(|| model::Selection::resolve(session_dir, p, || false))
            .transpose()?,
    };
    let cache = if use_cache {
        Some(crate::incremental::Cache::open(
            session_dir,
            &selection.as_ref().unwrap().identity,
        )?)
    } else {
        None
    };
    let window_fingerprint = use_cache.then(|| window_recipe.fingerprint()).transpose()?;
    let (mut engine, model_name) = if let Some(selection) = &selection {
        (None, selection.name.clone())
    } else {
        let _load = (std::env::var("LOCALVOX_PROFILE_TIMINGS").as_deref() == Ok("1")).then(|| {
            crate::journal::Operation::start(
                session_dir,
                "transcribe",
                "Загрузка ASR-модели и адаптера",
                None,
            )
        });
        let (engine, name) = model::legacy(session_dir, p)?;
        (Some(engine), name)
    };
    let mut cache_hits = 0usize;
    let mut asr_calls = 0usize;

    // The diarization models — if they exist at all. If they do not (or it is switched
    // off), we work as before: we do not know the speakers, and we say so with
    // emptiness rather than with a guess.
    let speakers = crate::diarize::enabled()
        .then(crate::diarize::Models::open)
        .transpose()
        .unwrap_or_else(|e| {
            // The model is there, but it did not open. That is NOT a reason to lose the
            // transcript — but we must not stay silent either: the recipe promises
            // labelling that will not happen.
            tracing::error!("diarization DOES NOT WORK ({e:#}) — speakers are not labelled");
            None
        });

    let t0 = Instant::now();
    let mut lines: Vec<(u8, f64, f64, String)> = Vec::new();
    let mut audio_sec = 0.0f64;
    let mut tracks: Vec<crate::diarize::Track> = Vec::new();

    // The denominator of the transcribe bar is AUDIO SECONDS, not chunk FILES. An ingested video is
    // written as ONE chunk file (`ChunkRecorder::feed` lands a one-shot buffer whole, then rotates —
    // it is built for the live recorder that feeds small frames over time), so a file-count bar sat
    // at «1/1» for ten minutes while GigaAM ground through dozens of VAD windows INSIDE that file.
    // Counted up front from the WAV headers so the first heartbeat says «0 of N» seconds.
    let total_sec: u32 = [0u8, 1u8]
        .iter()
        .filter_map(|&s| chunk_files_for_source(session_dir, s).ok())
        .flatten()
        .filter_map(|p| chunk_seconds(&p).ok())
        .sum::<f64>()
        .round()
        .max(1.0) as u32;
    let mut processed_sec: f64 = 0.0;
    // A slice of audio between heartbeats: fed to the recognizer, then the bar moves. Small enough
    // that even one long file advances every few seconds, large enough not to spam the journal.
    let slice_samples: usize = 15 * SAMPLE_RATE as usize;
    crate::progress::progress(
        session_dir,
        crate::progress::Stage::Transcribe,
        0,
        total_sec,
    );

    let mut any_chunks = false;
    for source_id in [0u8, 1u8] {
        let chunks = chunk_files_for_source(session_dir, source_id)?;
        if chunks.is_empty() {
            continue;
        }
        any_chunks = true;

        let mut vad = Vad::new_with_rate_and_mode(SampleRate::Rate16kHz, VadMode::LowBitrate);
        vad.set_sample_rate(SampleRate::Rate16kHz);
        let mut classify = |frame: &[i16]| vad.is_voice_segment(frame).unwrap_or(true);

        let mut windower = Windower::new(p.max_window_sec, p.min_cut_sec, p.silence_ms);
        let mut emit = |window: crate::windowing::Window<'_>| -> Result<()> {
            let mut recognize = |samples: &[i16]| -> Result<String> {
                if engine.is_none() {
                    let _load = crate::journal::Operation::start(
                        session_dir,
                        "transcribe",
                        "Загрузка ASR-модели и адаптера",
                        None,
                    );
                    engine = Some(selection.as_ref().context("ASR selection")?.load()?);
                }
                asr_calls += 1;
                let pcm: Vec<f32> = samples.iter().map(|&s| f32::from(s) / 32768.0).collect();
                engine
                    .as_ref()
                    .unwrap()
                    .transcribe(&pcm)
                    .context("recognizing the window")
            };
            let text = if let Some(cache) = &cache {
                let descriptor = crate::chunks::planning::Descriptor::from_pcm(
                    &capture.as_ref().unwrap().generation,
                    window_fingerprint.as_deref().unwrap(),
                    source_id,
                    window.start_sample,
                    window.pcm,
                    window.kind,
                )?;
                let (result, reused) = cache.resolve(
                    crate::incremental::VerifiedWindow::new(&descriptor, window.pcm)?,
                    &mut recognize,
                )?;
                if let Some(selection) = &selection {
                    selection.crash_if_fixture("after_result");
                }
                cache_hits += usize::from(reused);
                result.text().map(str::to_owned)
            } else if window.kind == crate::windowing::WindowKind::Speech {
                let text = recognize(window.pcm)?.trim().to_string();
                has_real_word(&text).then_some(text)
            } else {
                None
            };
            if let Some(text) = text {
                let start = window.start_sample as f64 / SAMPLE_RATE as f64;
                let end = start + window.pcm.len() as f64 / SAMPLE_RATE as f64;
                lines.push((source_id, start, end, text));
            }
            Ok(())
        };

        // Diarization runs in the SAME pass: the audio is being read anyway — there is
        // no point in reading it a second time for the speakers. ASR cuts the stream
        // into windows by silence, while diarization slides its own window; all they
        // have in common is the source of samples.
        let mut diar = speakers.as_ref().map(|m| {
            crate::diarize::Runner::new(&m.seg, &m.emb, crate::diarize::Options::default())
        });

        let mut last_report = processed_sec;
        for path in &chunks {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .context("chunk stem")?;
            crate::chunks::codec::with_read(
                path.parent().context("audio directory")?,
                stem,
                |selected| {
                    crate::chunks::codec::stream(
                        selected,
                        Path::new(&crate::chunks::resolve_ffmpeg_for_decode()),
                        || false,
                        |bytes| {
                            let samples: Vec<i16> = bytes
                                .chunks_exact(2)
                                .map(|b| i16::from_le_bytes([b[0], b[1]]))
                                .collect();
                            audio_sec += samples.len() as f64 / SAMPLE_RATE as f64;
                            if let Some(d) = &mut diar {
                                let pcm: Vec<f32> =
                                    samples.iter().map(|s| f32::from(*s) / 32768.0).collect();
                                d.push(&pcm)?;
                            }
                            windower.feed_all(&samples, &mut classify, &mut emit)?;
                            processed_sec += samples.len() as f64 / SAMPLE_RATE as f64;
                            if processed_sec - last_report
                                >= slice_samples as f64 / SAMPLE_RATE as f64
                            {
                                crate::progress::progress(
                                    session_dir,
                                    crate::progress::Stage::Transcribe,
                                    processed_sec.round() as u32,
                                    total_sec,
                                );
                                last_report = processed_sec;
                            }
                            Ok(())
                        },
                    )
                },
            )?;
            crate::progress::progress(
                session_dir,
                crate::progress::Stage::Transcribe,
                processed_sec.round() as u32,
                total_sec,
            );
        }
        windower.finish_all(&mut emit)?;
        if let Some(capture) = &capture {
            ensure!(
                windower.input_end() == capture.sources[&source_id].end()
                    && windower.closed_end() == capture.sources[&source_id].end(),
                "final ASR did not cover the sealed source EOF"
            );
        }

        if let Some(d) = diar {
            match d.finish() {
                Ok(diarization) => tracks.push(crate::diarize::Track {
                    source_id,
                    diarization,
                }),
                // The labelling failed — the transcript does not disappear because of it.
                Err(e) => tracing::error!("diarization of track {source_id} failed: {e:#}"),
            }
        }
    }
    if !any_chunks {
        bail!("no audio chunks in {}", session_dir.join("audio").display());
    }

    lines.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));

    // Who is who: merge the tracks into people, recognize familiar voices, label the
    // lines. All of that — WITHOUT a single generated word: a name comes either from a
    // profile that a human created, or it does not come at all.
    let who = speakers_for_lines(session_dir, tracks, &lines, &lang);

    let (id, path) = store.next_version(&label)?;
    {
        let mut w = BufWriter::new(fs::File::create(&path)?);
        for (i, (source_id, start, end, text)) in lines.iter().enumerate() {
            serde_json::to_writer(
                &mut w,
                &TranscriptLine {
                    source_id: *source_id,
                    start_sec: *start,
                    end_sec: *end,
                    text: text.clone(),
                    // Empty is an HONEST answer, not a forgotten field: either there was
                    // no diarization, or two people split the line evenly and attributing
                    // it to one of them would put someone else's words in his mouth.
                    speaker: who.get(i).cloned().flatten(),
                },
            )?;
            w.write_all(b"\n")?;
        }
        w.flush()?;
        w.get_ref().sync_all()?;
    }
    if let Some(selection) = &selection {
        selection.crash_if_fixture("after_transcript_file");
    }
    store.commit(VersionEntry {
        id,
        label: label.clone(),
        file: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        model: model_name,
        params: serde_json::json!({
            "recipe": recipe,
            "lang": lang,
            "max_window_sec": p.max_window_sec,
            "min_cut_sec": p.min_cut_sec,
            "silence_ms": p.silence_ms,
            "asr_cache": {"hits":cache_hits,"calls":asr_calls,"recognizer":selection.as_ref().map(|s|s.identity.fingerprint()).transpose()?},
        }),
        created_at: now_rfc3339(),
        parents: vec![],
    })?;
    if let Some(selection) = &selection {
        selection.crash_if_fixture("after_manifest");
    }

    // A re-cook MUST move `best` onto itself. Otherwise the pointer would stay on a
    // derivative of the OLD version (usually `refined` from that same version), and
    // search, export and summary would keep serving that very unreadable transcript —
    // the re-cook would have been pointless. If refine comes after us in the job, it
    // will move best onto itself, having been built from the fresh cook already.
    store.set_best(id)?;
    if let Some(selection) = &selection {
        selection.crash_if_fixture("after_best");
    }

    // The language of the text — now, when the text exists. A human's explicit choice we
    // do not touch: a guess does not argue with a decision. What was detected goes into
    // the choice of templates and the language of the LLM's answer (but not into the
    // choice of the ASR model — that one has already done its work, see lang.rs).
    if lang::explicit(session_dir).is_none() && !lines.is_empty() {
        let text: String = lines
            .iter()
            .map(|(_, _, _, t)| t.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        if let Some(detected) = lang::detect(&text) {
            if detected != lang {
                tracing::warn!(
                    "{}: recognized with the «{lang}» model, but the text looks like «{detected}» — \
                     if the recording really is in «{detected}», set the language explicitly and re-cook",
                    session_dir.display()
                );
            }
            lang::remember_detected(session_dir, &detected);
        }
    }

    // The fact of processing — EXPLICITLY, not by the presence of a file: the daemon
    // must know that a cook with this recipe already happened, even if it produced zero
    // lines.
    crate::processing::record(
        session_dir,
        crate::processing::TRANSCRIPT,
        &recipe,
        if lines.is_empty() {
            crate::processing::Outcome::Nothing
        } else {
            crate::processing::Outcome::Ok
        },
        Some(format!("{} lines", lines.len())),
        Some(id),
    )?;
    if let Some(selection) = &selection {
        selection.crash_if_fixture("after_receipt");
    }

    Ok(CookOutcome::Done {
        version_id: id,
        file: path,
        lines: lines.len(),
        audio_sec,
        wall_sec: t0.elapsed().as_secs_f64(),
    })
}

/// Who uttered each line: merge the tracks into people, recognize familiar voices,
/// save the roster of the recording and return a label for every line.
///
/// `None` in the answer is an honest «we do not know»: either there was no diarization
/// at all, or two people split the line evenly (an utterance on the seam between
/// speakers), and attributing it to one of them would put someone else's words in his
/// mouth.
fn speakers_for_lines(
    session_dir: &Path,
    tracks: Vec<crate::diarize::Track>,
    lines: &[(u8, f64, f64, String)],
    lang: &str,
) -> Vec<Option<String>> {
    use crate::diarize::{merge, roster, timeline, Options};

    if tracks.is_empty() {
        return vec![None; lines.len()];
    }
    let merged = merge(tracks, Options::default());
    if merged.participants.is_empty() {
        return vec![None; lines.len()];
    }

    let imported = std::fs::read(session_dir.join("meta.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<crate::chunks::SessionMeta>(&b).ok())
        .is_some_and(|m| m.source.is_some());
    let current = roster::for_participants(
        &merged.participants,
        &roster::load(session_dir),
        lang,
        imported,
    );
    let labels: Vec<_> = current.members.iter().map(|m| m.label.clone()).collect();
    if let Err(e) = roster::save(session_dir, &current) {
        // The roster was not saved — the labels in the transcript stay, but afterwards
        // there will be nothing left to name a person with. We must not stay silent
        // about that.
        tracing::error!(
            "the participant roster was not saved ({e:#}) — renaming a voice will not work"
        );
    }
    tracing::info!(
        "diarization: {} participants ({})",
        labels.len(),
        labels.join(", ")
    );

    lines
        .iter()
        .map(|(source_id, start, end, _)| {
            // We look for the utterance on the timeline of ITS OWN track: in the very
            // same second, different people speak into the microphone and out of the
            // speakers.
            let turns: Vec<_> = merged
                .turns
                .iter()
                .filter(|(src, _)| src == source_id)
                .map(|(_, t)| t.clone())
                .collect();
            timeline::who_said(*start, *end, &turns).and_then(|id| labels.get(id).cloned())
        })
        .collect()
}

pub use crate::asr::has_real_word;

/// The source's chunks in gluing order: `src{N}_chunk{SEQ}.{wav|flac}`, sorted by SEQ.
/// If both extensions of the same chunk are present (the conversion did not manage to
/// delete the WAV), we prefer `.wav` — it is read without ffmpeg.
fn chunk_files_for_source(session_dir: &Path, source_id: u8) -> Result<Vec<PathBuf>> {
    let audio = session_dir.join("audio");
    let prefix = format!("src{source_id}_chunk");
    let mut found: std::collections::BTreeMap<u32, PathBuf> = std::collections::BTreeMap::new();
    let entries = fs::read_dir(&audio).with_context(|| format!("reading {}", audio.display()))?;
    for e in entries.flatten() {
        let path = e.path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(seq) = stem
            .strip_prefix(&prefix)
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        match path.extension().and_then(|x| x.to_str()) {
            Some("wav") => {
                found.insert(seq, path);
            }
            Some("flac") => {
                found.entry(seq).or_insert(path); // wav takes priority
            }
            _ => {}
        }
    }
    Ok(found.into_values().collect())
}

/// Samples of a chunk (16 kHz mono s16): `.wav` — hound, `.flac` — decoded through
/// ffmpeg (the same one that compressed the chunk; the path comes from the env var
/// `LOCALVOX_LIGHT_YT_FFMPEG`).
/// Duration of a chunk in seconds — for the transcribe progress denominator. WAV: from the header
/// (`duration()` reads no samples), so summing across chunks is cheap even for a long recording.
/// FLAC: decoded length (the mic path; rarer, and a correct denominator beats a saved decode here).
fn chunk_seconds(path: &Path) -> Result<f64> {
    let audio = path.parent().context("audio directory")?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .context("chunk stem")?;
    crate::chunks::codec::with_read(audio, stem, |path| {
        Ok(crate::chunks::codec::samples(path)? as f64 / f64::from(SAMPLE_RATE))
    })
}

fn find_model_file(dir: &Path) -> Result<PathBuf> {
    let mut candidates: Vec<PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("model directory: {}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("onnx"))
        .collect();
    if candidates.is_empty() {
        bail!("no *.onnx in {}", dir.display());
    }
    // with several, take int8 (our speed default)
    candidates.sort_by_key(|p| !p.to_string_lossy().contains("int8"));
    Ok(candidates.remove(0))
}

#[cfg(test)]
mod tests {
    /// THE SPLIT WORD, measured on the owner's downloaded video: 72 of its 76 windows were
    /// exactly 15.0 seconds because a monologue never offers the half-second of silence the soft
    /// cut wants, so the ceiling cut straight through «Топ-три | ри качества».
    ///
    /// Here the speech has one short gap — far too short for a soft cut — and the ceiling is
    /// reached. The window must end IN THAT GAP, not on the ceiling.
    #[test]
    fn a_hard_cut_retreats_to_the_gap_between_words() {
        // 1 s windows, soft cut never fires (it wants 5 s of run-up and 500 ms of silence).
        let mut w = Windower::new(1.0, 5.0, 500);
        let frames = 50; // 50 × 20 ms = 1 s
        let gap_at = 30; // a single silent frame, 600 ms in
        let mut cuts: Vec<(u64, usize)> = Vec::new();
        let mut emit = |start: u64, s: &[i16]| {
            cuts.push((start, s.len()));
            Ok(())
        };
        let mut n = 0usize;
        let mut classify = |_: &[i16]| {
            let speech = n != gap_at;
            n += 1;
            speech
        };
        let audio = vec![1i16; FRAME_SAMPLES * frames];
        w.feed(&audio, &mut classify, &mut emit).unwrap();

        assert_eq!(cuts.len(), 1, "the ceiling did not close a window");
        let (start, len) = cuts[0];
        assert_eq!(start, 0);
        // The gap is frame 30: samples [30 × 320, 31 × 320). The cut is its middle.
        let expected = (30 * FRAME_SAMPLES + 31 * FRAME_SAMPLES) / 2;
        assert_eq!(len, expected, "the cut did not land in the gap");
        assert!(
            len < FRAME_SAMPLES * frames,
            "the window still ran to the ceiling — a word gets split here"
        );
    }

    /// Unbroken speech with no gap anywhere. There is nothing better to do than cut on the
    /// ceiling — but the stream must stay continuous and lose nothing.
    #[test]
    fn without_any_gap_the_ceiling_still_cuts_and_loses_nothing() {
        let mut w = Windower::new(1.0, 5.0, 500);
        let mut cuts: Vec<(u64, usize)> = Vec::new();
        let mut emit = |start: u64, s: &[i16]| {
            cuts.push((start, s.len()));
            Ok(())
        };
        let mut classify = |_: &[i16]| true;
        let audio = vec![1i16; FRAME_SAMPLES * 150]; // 3 s
        w.feed(&audio, &mut classify, &mut emit).unwrap();
        w.finish(&mut emit).unwrap();

        let total: usize = cuts.iter().map(|(_, l)| l).sum();
        assert_eq!(total, audio.len(), "samples were lost or duplicated");
        // Windows follow one another without a hole and without an overlap.
        let mut at = 0u64;
        for (start, len) in &cuts {
            assert_eq!(*start, at, "a hole or an overlap in the timeline");
            at += *len as u64;
        }
    }

    /// The tail carried over is not recognized twice, and the timeline stays exact — this is what
    /// makes retreating safe where overlapping windows would need the text to be de-duplicated.
    #[test]
    fn the_carried_tail_keeps_the_timeline_exact() {
        let mut w = Windower::new(1.0, 5.0, 500);
        let mut cuts: Vec<(u64, usize)> = Vec::new();
        let mut emit = |start: u64, s: &[i16]| {
            cuts.push((start, s.len()));
            Ok(())
        };
        let mut n = 0usize;
        // A gap every 25 frames — several ceilings in a row, each retreating.
        let mut classify = |_: &[i16]| {
            n += 1;
            n % 25 != 0
        };
        let audio = vec![1i16; FRAME_SAMPLES * 250]; // 5 s
        w.feed(&audio, &mut classify, &mut emit).unwrap();
        w.finish(&mut emit).unwrap();

        assert!(cuts.len() >= 4, "expected several windows: {cuts:?}");
        let total: usize = cuts.iter().map(|(_, l)| l).sum();
        assert_eq!(total, audio.len(), "samples were lost or duplicated");
        let mut at = 0u64;
        for (start, len) in &cuts {
            assert_eq!(*start, at);
            at += *len as u64;
        }
    }

    use super::*;

    fn collect_windows(
        w: &mut Windower,
        total_frames: usize,
        mut classify: impl FnMut(&[i16]) -> bool,
    ) -> Vec<(u64, usize)> {
        let mut out: Vec<(u64, usize)> = Vec::new();
        let frame = vec![0i16; FRAME_SAMPLES];
        for _ in 0..total_frames {
            w.feed(&frame, &mut classify, &mut |start, s: &[i16]| {
                out.push((start, s.len()));
                Ok(())
            })
            .unwrap();
        }
        w.finish(&mut |start, s: &[i16]| {
            out.push((start, s.len()));
            Ok(())
        })
        .unwrap();
        out
    }

    #[test]
    fn cuts_at_silence_after_min_and_keeps_continuity() {
        // min_cut 1 s, max 10 s, silence 100 ms (5 frames).
        // Speech with a pause at the end of every second: 45 speech frames + 5 of silence.
        let mut w = Windower::new(10.0, 1.0, 100);
        let mut i = 0usize;
        let windows = collect_windows(&mut w, 300, |_| {
            let speech = i % 50 < 45;
            i += 1;
            speech
        }); // 6 s
        assert!(windows.len() >= 5, "windows ~1 s: {windows:?}");
        // continuity and exact coverage
        let mut pos = 0u64;
        for (start, len) in &windows {
            assert_eq!(*start, pos);
            pos += *len as u64;
        }
        assert_eq!(pos, 300 * FRAME_SAMPLES as u64);
        // soft cut: windows ≈ min_cut, not max
        assert!(windows[0].1 <= 2 * 16_000);
    }

    #[test]
    fn silent_windows_are_dropped_but_timeline_keeps_moving() {
        // 2 s of silence, then 2 s of speech. The silence does not go to the model
        // (GigaAM hallucinates on it), but its duration MUST shift the speech timecode.
        let mut w = Windower::new(1.0, 0.5, 100);
        let mut i = 0usize;
        let windows = collect_windows(&mut w, 200, |_| {
            let speech = i >= 100;
            i += 1;
            speech
        });
        assert!(!windows.is_empty(), "speech windows lost");
        let silent_samples = 100 * FRAME_SAMPLES as u64;
        assert!(
            windows[0].0 >= silent_samples,
            "the first window {:?} must start after 2 s of silence ({silent_samples} samples)",
            windows[0]
        );
        let voiced: usize = windows.iter().map(|(_, len)| *len).sum();
        assert!(
            voiced <= 100 * FRAME_SAMPLES + FRAME_SAMPLES,
            "more went to the model than there was speech: {voiced}"
        );
    }

    /// An utterance without a single real word is not speech but recognition noise.
    /// Acceptance 12.07.2026: the archive was buried under lines «.» and «Т.» — GigaAM
    /// emits them on near-silence. They did not merely litter the web UI: getting into
    /// the transcript, they travelled into search, into the index and into the LLM
    /// prompt, where the model was obliged to «write a summary» out of the line «.».
    #[test]
    fn recognition_noise_is_not_a_line() {
        assert!(!has_real_word("."));
        assert!(!has_real_word("Т."));
        assert!(!has_real_word("а-а"));
        assert!(!has_real_word("э-э-э..."));
        assert!(!has_real_word("  ,  "));

        assert!(has_real_word("Ва!"));
        assert!(has_real_word("Говорю несколько пауз."));
        assert!(has_real_word("Разбиваю шесть яиц."));

        // NUMBERS are always speech. GigaAM writes them as digits and separates the
        // groups with a space, so the rule «≥2 different characters in a token» threw
        // «1 000 000» out entirely: «1» is a single digit, «000» is a repetition. The
        // note «Миллион.» disappeared silently (caught by review).
        assert!(has_real_word("1 000 000."));
        assert!(has_real_word("5 000"));
        assert!(has_real_word("33 000"));
        assert!(has_real_word("11."));
        assert!(has_real_word("5"));
    }

    #[test]
    fn hard_cut_at_max_when_no_silence() {
        // solid «speech»: we cut by the maximum only (2 s)
        let mut w = Windower::new(2.0, 1.0, 100);
        let windows = collect_windows(&mut w, 500, |_| true); // 10 s
        assert_eq!(windows.len(), 5);
        for (_, len) in &windows[..4] {
            assert_eq!(*len, 2 * 16_000);
        }
    }

    #[test]
    fn finish_flushes_partial_frame_tail() {
        let mut w = Windower::new(10.0, 5.0, 100);
        let mut out: Vec<(u64, usize)> = Vec::new();
        // a speech frame + 100 samples of «tail» (less than a VAD frame) — the tail is
        // not lost
        let mut push = |s, x: &[i16]| {
            out.push((s, x.len()));
            Ok(())
        };
        w.feed(&vec![1i16; FRAME_SAMPLES], &mut |_| true, &mut push)
            .unwrap();
        w.feed(&vec![1i16; 100], &mut |_| true, &mut push).unwrap();
        w.finish(&mut push).unwrap();
        assert_eq!(out, vec![(0, FRAME_SAMPLES + 100)]);
    }
}
