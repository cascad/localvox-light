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

use anyhow::{bail, Context, Result};
use webrtc_vad::{SampleRate, Vad, VadMode};

use crate::asr::onnx::adapters::GigaamV3E2eCtc;
use crate::asr::onnx::OnnxEngine;
use crate::asr::AsrEngine;
use crate::lang;
use crate::versions::{now_rfc3339, TranscriptLine, VersionEntry, VersionStore};

const SAMPLE_RATE: u32 = 16_000;
const FRAME_SAMPLES: usize = 320; // 20 ms at 16 kHz

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
        if let Some(v) = manifest
            .versions
            .iter()
            // The NEWEST version of this recipe, not the first one found. Measured 19.07.2026:
            // after a `--force` re-cook the session held v001 and v003 of one recipe, and the
            // next cook run picked v001 — then decided `best` (v003) pointed at «someone else's»
            // version and moved it back onto v001. A deliberate re-cook was silently undone, and
            // the readable text was rebuilt from the older transcript.
            .filter(|v| v.params.get("recipe").and_then(|r| r.as_str()) == Some(recipe.as_str()))
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

    // The model follows the session language. No model for the language — fail loudly:
    // silently cooking English speech with a Russian model means emitting plausible
    // mush, and afterwards nobody will tell it apart from a bad recording.
    let model = lang::asr_model(&lang, &p.model_dir)
        .with_context(|| format!("session language «{lang}»"))?;
    let (engine, model_name): (Box<dyn AsrEngine>, String) = match model.engine {
        lang::Engine::Onnx => {
            let file = find_model_file(&model.dir)?;
            let adapter = GigaamV3E2eCtc::from_model_dir(&model.dir)
                .context("building the GigaAM v3 E2E CTC adapter")?;
            let name = file
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into();
            let engine = OnnxEngine::new(&file, adapter).context("loading the ONNX model")?;
            (Box::new(engine), name)
        }
        lang::Engine::Vosk => {
            let engine =
                crate::asr::vosk::VoskEngine::new(&model.dir).context("loading the Vosk model")?;
            let name = model
                .dir
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into();
            (Box::new(engine), name)
        }
    };

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
    crate::progress::progress(session_dir, crate::progress::Stage::Transcribe, 0, total_sec);

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
        let mut emit = |start_samples: u64, samples: &[i16]| -> Result<()> {
            let pcm: Vec<f32> = samples.iter().map(|&s| f32::from(s) / 32768.0).collect();
            let text = engine.transcribe(&pcm).context("recognizing the window")?;
            let text = text.trim().to_string();
            // An utterance without a single real word does not get into the transcript.
            // On near-silence GigaAM emits «.», «Т.», «а-а» — that is not speech but
            // recognition noise, and it has to be fixed here, at the source: once in
            // the transcript it travels into search, into the index and into the LLM
            // prompt, where the model is obliged to «write a summary» out of the line
            // «.».
            if has_real_word(&text) {
                let start = start_samples as f64 / f64::from(SAMPLE_RATE);
                let end = start + samples.len() as f64 / f64::from(SAMPLE_RATE);
                lines.push((source_id, start, end, text));
            }
            Ok(())
        };

        // Diarization runs in the SAME pass: the audio is being read anyway — there is
        // no point in reading it a second time for the speakers. ASR cuts the stream
        // into windows by silence, while diarization slides its own window; all they
        // have in common is the source of samples.
        let mut diar = speakers
            .as_ref()
            .map(|m| crate::diarize::Runner::new(&m.seg, &m.emb, crate::diarize::Options::default()));

        for path in &chunks {
            let samples = read_chunk_samples(path)?;
            audio_sec += samples.len() as f64 / f64::from(SAMPLE_RATE);
            if let Some(d) = &mut diar {
                let pcm: Vec<f32> = samples.iter().map(|&s| f32::from(s) / 32768.0).collect();
                d.push(&pcm)?;
            }
            // Feed the recognizer in slices so ONE long file still moves the bar. Each slice is a
            // heartbeat: the percentage a person watches AND the liveness a stall is read from —
            // while these keep coming (every ~15 s of audio) the transcribe is alive, not hung.
            for slice in samples.chunks(slice_samples) {
                windower.feed(slice, &mut classify, &mut emit)?;
                processed_sec += slice.len() as f64 / f64::from(SAMPLE_RATE);
                crate::progress::progress(
                    session_dir,
                    crate::progress::Stage::Transcribe,
                    processed_sec.round() as u32,
                    total_sec,
                );
            }
        }
        windower.finish(&mut emit)?;

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
        }),
        created_at: now_rfc3339(),
        parents: vec![],
    })?;

    // A re-cook MUST move `best` onto itself. Otherwise the pointer would stay on a
    // derivative of the OLD version (usually `refined` from that same version), and
    // search, export and summary would keep serving that very unreadable transcript —
    // the re-cook would have been pointless. If refine comes after us in the job, it
    // will move best onto itself, having been built from the fresh cook already.
    store.set_best(id)?;

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
    );

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
    use crate::diarize::{merge, names, profiles, roster, timeline, Options};

    if tracks.is_empty() {
        return vec![None; lines.len()];
    }
    let merged = merge(tracks, Options::default());
    if merged.participants.is_empty() {
        return vec![None; lines.len()];
    }

    // The voices a human once named himself are shared across the whole archive. This
    // works in one direction only: recognizing an acquaintance is allowed, inventing a
    // name for a stranger is not.
    let work_dir = session_dir
        .parent()
        .and_then(Path::parent)
        .unwrap_or(session_dir);
    let known = profiles::load(work_dir);
    let labels = names(&merged.participants, &known, lang);

    // The roster of the recording — so that «"Участник 2" is Иван» can be said LATER,
    // without running both models over the audio again.
    let members = merged
        .participants
        .iter()
        .zip(&labels)
        .map(|(p, label)| roster::Member {
            id: p.id,
            label: label.clone(),
            embedding: p.embedding.clone(),
            speech_sec: p.speech_sec,
            owner: p.owner,
        })
        .collect();
    if let Err(e) = roster::save(session_dir, &roster::Roster { members }) {
        // The roster was not saved — the labels in the transcript stay, but afterwards
        // there will be nothing left to name a person with. We must not stay silent
        // about that.
        tracing::error!("the participant roster was not saved ({e:#}) — renaming a voice will not work");
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

/// Whether the text contains at least one real word.
///
/// Real is not the same as «non-empty»: «.», «Т.», «э-э», «а-а» consist of punctuation,
/// of one letter, or of a repetition of one letter. That is recognition noise, not
/// speech.
///
/// DIGITS are always real. GigaAM v3 writes numbers as digits and separates the groups
/// with a space («1 000 000», «9 472 824»), so the rule «≥2 different characters in a
/// token» killed them: «1» is a single digit, «000» is a repetition. The note
/// «Миллион.» disappeared entirely and silently (caught by review). The model's
/// hallucinations on silence are punctuation and vowel fillers, but not numbers: it
/// does not pull a sum or a deadline out of thin air — and losing those is
/// categorically unacceptable for a secretary.
pub fn has_real_word(text: &str) -> bool {
    if text.chars().any(|c| c.is_ascii_digit()) {
        return true;
    }
    text.split_whitespace().any(|w| {
        let letters: Vec<char> = w
            .chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect();
        letters.len() >= 2 && letters.iter().any(|c| *c != letters[0])
    })
}

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
    if path.extension().and_then(|x| x.to_str()) == Some("flac") {
        return Ok(decode_flac_16k_mono(path)?.len() as f64 / f64::from(SAMPLE_RATE));
    }
    let reader =
        hound::WavReader::open(path).with_context(|| format!("opening {}", path.display()))?;
    Ok(reader.duration() as f64 / f64::from(SAMPLE_RATE))
}

fn read_chunk_samples(path: &Path) -> Result<Vec<i16>> {
    if path.extension().and_then(|x| x.to_str()) == Some("flac") {
        return decode_flac_16k_mono(path);
    }
    let mut reader =
        hound::WavReader::open(path).with_context(|| format!("opening {}", path.display()))?;
    let spec = reader.spec();
    if spec.sample_rate != SAMPLE_RATE || spec.channels != 1 || spec.bits_per_sample != 16 {
        bail!(
            "unexpected format of {} (16 kHz mono s16 expected): {:?}",
            path.display(),
            spec
        );
    }
    reader
        .samples::<i16>()
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("reading samples of {}", path.display()))
}

fn decode_flac_16k_mono(path: &Path) -> Result<Vec<i16>> {
    let ffmpeg = crate::chunks::resolve_ffmpeg_for_decode();
    // -xerror: mid-stream decode errors are FATAL. Without it a truncated/corrupted
    // FLAC decodes partially with exit 0 — a silent loss of audio would shift the
    // timecodes of every following chunk (verified empirically on ffmpeg 8).
    let out = std::process::Command::new(&ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-xerror", "-i"])
        .arg(path)
        .args(["-f", "s16le", "-ac", "1", "-ar", "16000", "-"])
        .output()
        .with_context(|| format!("running {} for {}", ffmpeg.display(), path.display()))?;
    if !out.status.success() {
        bail!(
            "ffmpeg did not decode {} (exit {}): {}",
            path.display(),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(out
        .stdout
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect())
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

// ─────────────────────── windowing by silence ───────────────────────

/// Cuts a continuous stream of samples into windows for ASR: after `min_cut_sec` the
/// window closes at the first sufficient silence, at `max_window_sec` — hard.
/// The frame classifier (20 ms) is passed in from outside — in tests it is substituted.
///
/// A HARD CUT NEVER LANDS MID-WORD IF THERE IS ANYWHERE ELSE TO PUT IT.
///
/// It used to. `min_cut_sec`/`silence_ms` demand half a second of silence, and a monologue does
/// not offer one: measured on the owner's downloaded video, 72 of its 76 windows were EXACTLY
/// 15.0 seconds long — the ceiling did all the cutting, straight through the middle of
/// «Топ-три | ри качества». The recognizer then wrote «...» at both broken ends, the readable
/// text carried a word split in half, and no later stage could repair it: the halves live in
/// different windows and were recognized independently.
///
/// So when the ceiling is reached we RETREAT to the quietest place inside the window and cut
/// there, carrying the rest over into the next one. The alternative in the literature is
/// overlapping windows stitched by matching the repeated text (whisper.cpp, faster-whisper,
/// NeMo's buffered inference); it needs fuzzy alignment of two recognitions and can drop or
/// double a word at every seam. Retreating touches no text at all and is exactly as
/// deterministic as the cut it replaces: same audio in, same windows out.
pub struct Windower {
    max_samples: usize,
    min_cut_samples: usize,
    silence_frames_needed: u32,
    /// The tail that is not a multiple of a VAD frame (carried across feed calls).
    pending: Vec<i16>,
    /// The accumulated window.
    buf: Vec<i16>,
    /// Absolute position of the first sample of `buf` in the source's timeline.
    buf_start: u64,
    silence_run: u32,
    /// Whether the accumulated window had at least one speech frame. A window of pure
    /// silence is not handed to the model: GigaAM hallucinates on it («.», fragments),
    /// and inference costs money anyway.
    had_speech: bool,
    /// Where the current run of silence began, in samples from the start of `buf`.
    silence_from: Option<usize>,
    /// The widest gap between words seen in this window: `(start, end)` in samples from the
    /// start of `buf`. This is where a hard cut retreats to — the point furthest from the
    /// speech on either side of it.
    best_gap: Option<(usize, usize)>,
}

impl Windower {
    pub fn new(max_window_sec: f64, min_cut_sec: f64, silence_ms: u32) -> Self {
        let frame_ms = 1000 * FRAME_SAMPLES as u32 / SAMPLE_RATE;
        Self {
            max_samples: (max_window_sec * f64::from(SAMPLE_RATE)) as usize,
            min_cut_samples: (min_cut_sec * f64::from(SAMPLE_RATE)) as usize,
            silence_frames_needed: silence_ms.div_ceil(frame_ms).max(1),
            pending: Vec::new(),
            buf: Vec::new(),
            buf_start: 0,
            silence_run: 0,
            had_speech: false,
            silence_from: None,
            best_gap: None,
        }
    }

    pub fn feed<C, E>(&mut self, samples: &[i16], classify: &mut C, emit: &mut E) -> Result<()>
    where
        C: FnMut(&[i16]) -> bool,
        E: FnMut(u64, &[i16]) -> Result<()>,
    {
        self.pending.extend_from_slice(samples);
        while self.pending.len() >= FRAME_SAMPLES {
            let frame: Vec<i16> = self.pending.drain(..FRAME_SAMPLES).collect();
            let is_speech = classify(&frame);
            let frame_start = self.buf.len();
            self.buf.extend_from_slice(&frame);
            if is_speech {
                // A gap has just closed. It counts only if speech came BEFORE it as well:
                // silence at the head of a window is not a gap between two words.
                if let Some(from) = self.silence_from.take() {
                    let wider = match self.best_gap {
                        Some((a, b)) => frame_start - from >= b - a,
                        None => true,
                    };
                    if self.had_speech && wider {
                        self.best_gap = Some((from, frame_start));
                    }
                }
                self.silence_run = 0;
                self.had_speech = true;
            } else {
                if self.silence_from.is_none() {
                    self.silence_from = Some(frame_start);
                }
                self.silence_run += 1;
            }

            let cut_hard = self.buf.len() >= self.max_samples;
            let cut_soft = self.buf.len() >= self.min_cut_samples
                && self.silence_run >= self.silence_frames_needed;
            if cut_soft {
                self.flush(emit)?;
            } else if cut_hard {
                self.cut_at_quietest(emit)?;
            }
        }
        Ok(())
    }

    /// End of the stream: flush the remainder (including an incomplete frame).
    pub fn finish<E>(&mut self, emit: &mut E) -> Result<()>
    where
        E: FnMut(u64, &[i16]) -> Result<()>,
    {
        let tail: Vec<i16> = std::mem::take(&mut self.pending);
        self.buf.extend_from_slice(&tail);
        self.flush(emit)
    }

    /// The ceiling is reached: cut in the middle of the widest gap between words instead of
    /// wherever the sample counter happens to stand.
    ///
    /// The middle of the gap, not an edge — that is the point furthest from the speech on both
    /// sides, so neither window steals the start of the other's word. A gap found before
    /// `min_cut_samples` is used anyway if it is all there is: a short window is a smaller harm
    /// than a split word. With no gap at all — unbroken speech for the whole window — there is
    /// nothing to be done, and we SAY so instead of pretending the cut was clean.
    fn cut_at_quietest<E>(&mut self, emit: &mut E) -> Result<()>
    where
        E: FnMut(u64, &[i16]) -> Result<()>,
    {
        let Some((from, to)) = self.best_gap else {
            tracing::debug!(
                "window at {:.1} s: not one gap between words in {} samples — cutting on the ceiling, a word may be split",
                self.buf_start as f64 / f64::from(SAMPLE_RATE),
                self.buf.len()
            );
            return self.flush(emit);
        };
        let cut = (from + to) / 2;
        if cut == 0 || cut >= self.buf.len() {
            return self.flush(emit);
        }
        if self.had_speech {
            emit(self.buf_start, &self.buf[..cut])?;
        }
        // The rest is neither thrown away nor read twice: it becomes the head of the next
        // window, so the stream stays continuous and every sample is recognized exactly once.
        self.buf.drain(..cut);
        self.buf_start += cut as u64;
        self.silence_run = 0;
        self.had_speech = true; // the tail was carved out of the middle of speech
        self.silence_from = None;
        self.best_gap = None;
        Ok(())
    }

    fn flush<E>(&mut self, emit: &mut E) -> Result<()>
    where
        E: FnMut(u64, &[i16]) -> Result<()>,
    {
        if !self.buf.is_empty() {
            // A window without a single speech frame is skipped — but we still move the
            // timeline (otherwise the following windows would drift in time).
            if self.had_speech {
                emit(self.buf_start, &self.buf)?;
            }
            self.buf_start += self.buf.len() as u64;
            self.buf.clear();
        }
        self.silence_run = 0;
        self.had_speech = false;
        self.silence_from = None;
        self.best_gap = None;
        Ok(())
    }
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
