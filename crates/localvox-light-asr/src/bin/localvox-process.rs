//! `localvox-process` — the slow-lane "cook" (F8, WP-A3): chunk sessions → transcript
//! versions. With no arguments it processes every not-yet-cooked session in the working
//! directory; a repeated run is idempotent (a version with the same label is skipped).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use clap::Parser;
use localvox_light_core::chunks::{ChunkParams, ChunkRecorder, SessionMeta};
use localvox_light_core::cook::{cook_session_asr, CookOutcome, CookParams};
use localvox_light_core::export::{export_session, ExportFormat};
use localvox_light_ingest::{
    load_settings_named,
    progress::{convert_to_pcm_with_progress, download_audio_with_progress},
    resolve_ffmpeg, resolve_ffmpeg_location_for_ytdlp, resolve_js_runtime, resolve_yt_dlp,
};

#[derive(Parser)]
#[command(
    name = "localvox-process",
    about = "Cooking sessions: chunks → a transcript version (GigaAM, windows cut on VAD silence)"
)]
struct Cli {
    /// Directory of one specific session (…/sessions/<date>); without it — every session in work-dir
    session: Option<PathBuf>,

    /// The localvox working directory (the same one the recording uses)
    #[arg(
        long,
        default_value = "localvox-audio",
        env = "LOCALVOX_LIGHT_AUDIO_DIR"
    )]
    work_dir: PathBuf,

    /// ONNX model directory (model + vocabulary)
    #[arg(
        long,
        default_value = "models/gigaam-v3-e2e-ctc",
        env = "LOCALVOX_ASR_MODEL_DIR"
    )]
    model_dir: PathBuf,

    /// Version label in the manifest (by default — derived from the recording's language)
    #[arg(long)]
    label: Option<String>,

    /// Cook again even if the session has already been cooked with this very recipe
    #[arg(long)]
    force: bool,

    /// Hard ceiling of the inference window, sec (GigaAM's limit is ~200; windows longer than
    /// 120 s mash conversational speech into porridge — WER 93 %, see docs/asr-bench.md)
    #[arg(long, default_value_t = localvox_light_core::versions::DEFAULT_MAX_WINDOW_SEC)]
    max_window_sec: f64,

    /// After this duration the window is closed at the first silence, sec
    #[arg(long, default_value_t = localvox_light_core::versions::DEFAULT_MIN_CUT_SEC)]
    min_cut_sec: f64,

    /// After the cook, generate a summary (summary.md) via the LLM.
    ///
    /// Precisely a SUMMARY, not "minutes": a meeting has minutes, but a recording can be a
    /// lecture, a video or thinking out loud — and the file is one and the same.
    #[arg(long)]
    summary: bool,

    /// After the cook, clean up the transcript (processed.md) via the LLM
    #[arg(long)]
    cleanup: bool,

    /// After the cook, have the LLM clean up the transcript as a NEW version (timecodes intact)
    /// and make it best — search/export/player then take the cleaned-up variant
    #[arg(long)]
    refine: bool,

    /// Show the session's transcript versions and exit (needs a session path)
    #[arg(long)]
    list_versions: bool,

    /// Make the version with this id the working one (best) and exit (needs a session path)
    #[arg(long, value_name = "ID")]
    set_best: Option<u32>,

    /// OpenAI-compatible endpoint: Ollama / OpenAI / OpenRouter
    #[arg(
        long,
        default_value = "http://localhost:11434/v1",
        env = "LOCALVOX_LLM_BASE_URL"
    )]
    llm_base_url: String,

    /// LLM model
    #[arg(long, default_value = "qwen3.5:9b", env = "LOCALVOX_LLM_MODEL")]
    llm_model: String,

    /// Name of the env variable holding the API key (not needed for Ollama)
    #[arg(long, env = "LOCALVOX_LLM_API_KEY_ENV")]
    llm_api_key_env: Option<String>,

    /// Directory of term glossaries (*.toml)
    #[arg(
        long,
        default_value = "assets/glossary",
        env = "LOCALVOX_LLM_GLOSSARY_DIR"
    )]
    glossary_dir: PathBuf,

    /// Directory of user prompt templates (*.md)
    #[arg(long, env = "LOCALVOX_LLM_TEMPLATES_DIR")]
    templates_dir: Option<PathBuf>,

    /// Template for --summary: video-notes-ru (digest), or your own. By default — chosen by the
    /// recording's language and its length: a summary, or a note for a short recording.
    ///
    /// Binding it to env is MANDATORY, not a convenience: the daemon computes the artifact's
    /// recipe from this very variable, and it launches us as a child process WITHOUT this flag.
    /// Were we to read it differently, the recipes would diverge, and the daemon would forever
    /// demand a summary that we honestly produce every time.
    #[arg(long, env = "LOCALVOX_LLM_SUMMARY_TEMPLATE")]
    summary_template: Option<String>,

    /// Language of the recording (ISO-639: ru, en…). By default — as recorded in the session,
    /// otherwise LOCALVOX_LANG, otherwise auto-detection from the text.
    ///
    /// Specifying the language is WRITTEN INTO the session and invalidates everything derived:
    /// the language is part of the recipes, so the transcript and the summary will be made anew.
    #[arg(long, value_name = "CODE")]
    lang: Option<String>,

    /// Pull an external source (a YouTube URL or a media file) into a new session and process it
    /// through the common pipeline; may be given several times
    #[arg(long, value_name = "URL_OR_FILE")]
    ingest: Vec<String>,

    /// Export the best version: txt, md, srt; may be given several times
    #[arg(long, value_name = "FORMAT")]
    export: Vec<String>,
}

/// `--list-versions` / `--set-best`: managing the versions of one session without cooking.
fn manage_versions(session: &Path, list: bool, set_best: Option<u32>) -> Result<()> {
    use localvox_light_core::versions::VersionStore;
    let store = VersionStore::open(session).context("opening the version store")?;
    if let Some(id) = set_best {
        // the same lock as the cook/refine — versions.json is written serialized
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(session.join(".cook.lock"))
            .context("session version lock")?;
        lock.lock().context("acquiring the version lock")?;
        store
            .set_best(id)
            .with_context(|| format!("set-best v{id:03}"))?;
        eprintln!("best → v{id:03}");
    }
    if list {
        let m = store.load();
        let best = m.best;
        if m.versions.is_empty() {
            eprintln!("no versions (cook first)");
        }
        for v in &m.versions {
            let mark = if Some(v.id) == best { " *best" } else { "" };
            let parents = if v.parents.is_empty() {
                String::new()
            } else {
                format!(", from v{:03?}", v.parents)
            };
            eprintln!(
                "  v{:03} {:<14} {} ({}){parents}{mark}",
                v.id, v.label, v.model, v.created_at
            );
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    // A broken line in .env makes it abandon reading the file — everything below is silently lost.
    if let Err(e) = dotenvy::dotenv() {
        if !matches!(e, dotenvy::Error::Io(_)) {
            eprintln!("ERROR in .env: {e}");
            eprintln!("  → variables AFTER the broken line were NOT read (format: KEY=value or # comment)");
        }
    }
    let cli = Cli::parse();

    // SEPARATION OF STREAMS (the daemon expects it, and the cleanliness of its log rests on it):
    //   stdout — the RESULT: what was cooked, where it went, how many lines. Business events.
    //   stderr — DIAGNOSTICS: our warnings and errors, plus everything foreign libraries inside
    //            us write there.
    // The daemon relays stdout into its log at INFO, and stderr into debug and into the "tail"
    // to explain a failure. That is why foreign chatter physically cannot clutter its log: it
    // goes down the other stream. This is not a blacklist of substrings, which would lag behind
    // the next library, but a separation of channels.
    //
    // Logs. Without this EVERY `tracing::warn!`/`error!` of this utility went nowhere: "name
    // checking is off", "diarization does not work", "the model is quantized and lies" — all of
    // it was written and none of it was seen by anyone. A silent warning channel is worse than
    // a missing one: it creates the illusion that we will be warned.
    //
    // The filter and the colour come from a SHARED place: our output is captured by the daemon,
    // and deciding this separately in each binary means one day flooding its log with someone
    // else's debug output.
    localvox_light_core::cli::init_tracing_tool("info");

    // Version management — fast operations on one session, without cooking.
    if cli.list_versions || cli.set_best.is_some() {
        let session = cli
            .session
            .clone()
            .context("a session path is required (…/sessions/<date>)")?;
        return manage_versions(&session, cli.list_versions, cli.set_best);
    }

    // Export formats are validated before the long work.
    let export_formats: Vec<ExportFormat> = cli
        .export
        .iter()
        .map(|s| s.parse())
        .collect::<Result<_>>()?;

    // Half-cooked `.part` files left after an engine crash: recover them before the scan.
    // Only if the engine is not writing right now (otherwise we would "recover" live chunks —
    // std opens files with FILE_SHARE_DELETE, so the rename would succeed).
    match localvox_light_core::session::try_instance_lock(&cli.work_dir) {
        Ok(Some(_lock)) => {
            let recovered = localvox_light_core::chunks::recover_orphan_chunks(&cli.work_dir);
            if recovered > 0 {
                eprintln!("recovery: recovered {recovered} chunks (.part → .wav)");
            }
        }
        Ok(None) => eprintln!("recovery: skipped — the recording engine is running right now"),
        Err(e) => eprintln!("recovery: skipped — the lock is unavailable ({e})"),
    }

    // Ingest of external sources → new sessions in the common format.
    let mut ingested: Vec<PathBuf> = Vec::new();
    for input in &cli.ingest {
        eprintln!("⇣ ingest: {input}");
        let session =
            ingest_to_session(input, &cli.work_dir).with_context(|| format!("ingest {input}"))?;
        eprintln!("  session: {}", session.display());
        ingested.push(session);
    }

    // `--lang` changes ONE recording. Without a session path it would stamp the language onto the
    // whole archive at once: every session's cook recipe and summary recipe would change, the
    // entire archive would become "cooked the wrong way" and would go for a re-cook — with a
    // model that most likely is not there. There is no undo button for that.
    anyhow::ensure!(
        cli.lang.is_none() || cli.session.is_some() || !cli.ingest.is_empty(),
        "--lang changes the language of a SPECIFIC recording — pass the session directory. \
         The language for new recordings is set by the LOCALVOX_LANG variable (which rewrites nothing)"
    );

    let sessions: Vec<PathBuf> = match &cli.session {
        Some(s) => vec![s.clone()],
        None if !ingested.is_empty() => ingested,
        None => list_sessions(&cli.work_dir)?,
    };
    if sessions.is_empty() {
        eprintln!(
            "no sessions found in {}",
            cli.work_dir.join("sessions").display()
        );
        return Ok(());
    }

    let params = CookParams {
        model_dir: cli.model_dir.clone(),
        label: cli.label.clone(),
        max_window_sec: cli.max_window_sec,
        min_cut_sec: cli.min_cut_sec,
        force: cli.force,
        ..CookParams::default()
    };

    let llm = if cli.summary || cli.cleanup || cli.refine {
        let api_key = cli
            .llm_api_key_env
            .as_deref()
            .and_then(|name| std::env::var(name).ok());
        Some((
            localvox_light_llm::LlmClient::new(localvox_light_llm::LlmProfile {
                base_url: cli.llm_base_url.clone(),
                model: cli.llm_model.clone(),
                api_key,
                ..localvox_light_llm::LlmProfile::default()
            }),
            localvox_light_llm::pipeline::ProcessParams {
                glossary_dir: cli.glossary_dir.clone(),
                templates_dir: cli.templates_dir.clone(),
                summary_template: cli.summary_template.clone(),
                // The entity tagger: if the model is there, names and dates are checked BY
                // CONTEXT and without a single list; if it is not, they are not checked — and
                // that is said out loud.
                entities: build_entities(&cli.model_dir),
                ..localvox_light_llm::pipeline::ProcessParams::default()
            },
        ))
    } else {
        None
    };

    // Two different counters: a failure of the COOK (there is no data) and a failure of
    // POST-processing (LLM/export with the version already committed) — the autocook daemon must
    // tell "the session is not cooked" from "it is cooked, but Ollama was down".
    let mut failed_cook = 0usize;
    let mut failed_post = 0usize;
    for session in &sessions {
        println!("→ {}", session.display());

        // We wrote the language into the session — and that is all. There is no need to reset the
        // derived data by hand: the language is part of the recipes, so the transcript and the
        // summary have already become "made the wrong way" and will be made anew
        // (see core/src/lang.rs).
        //
        // We MUST NOT write into a LIVE session: the engine keeps meta in memory and on the next
        // chunk rotation will rewrite the file — the language would vanish silently, after we
        // have already promised "we will redo it".
        if let Some(code) = &cli.lang {
            let name = session
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if localvox_light_core::jobs::recording_session(&cli.work_dir).as_deref()
                == Some(name.as_str())
            {
                failed_cook += 1;
                eprintln!("  error: the session is being recorded right now — stop the recording");
                continue;
            }
            match localvox_light_core::lang::set(session, Some(code)) {
                Ok(true) => println!("  language: {code} (everything derived will be redone)"),
                Ok(false) => {}
                Err(e) => {
                    failed_cook += 1;
                    eprintln!("  error: the language was not written: {e:#}");
                    continue;
                }
            }
        }

        let cooked = match cook_session_asr(session, &params) {
            Ok(CookOutcome::Done {
                version_id,
                file,
                lines,
                audio_sec,
                wall_sec,
            }) => {
                let rtf = if audio_sec > 0.0 {
                    wall_sec / audio_sec
                } else {
                    0.0
                };
                println!(
                    "  cook: v{version_id:03} ({lines} lines, {audio_sec:.0} s audio in {wall_sec:.1} s, RTF {rtf:.3})"
                );
                println!("  {}", file.display());
                lines > 0
            }
            Ok(CookOutcome::Skipped { existing_version }) => {
                println!("  cook: v{existing_version:03} already exists (--force to re-cook)");
                true
            }
            Ok(CookOutcome::Empty) => {
                println!("  cook: no audio");
                false
            }
            Err(e) => {
                failed_cook += 1;
                eprintln!("  cook error: {e:#}");
                false
            }
        };

        // Nothing to COOK does not mean nothing to PROCESS. The LLM needs the transcript, not the
        // audio: retention cleans the audio out after 14 days, but the transcript stays.
        //
        // There used to be a bare `continue` here, and such a session fell into an eternal loop:
        // discovery sees speech in the transcript and demands a summary → the daemon launches us
        // → the cook returns Empty → we silently walk away, writing nothing into the journal →
        // 5 seconds later it all starts over. Discovery decided by the transcript, and we by the
        // audio; that divergence IS the loop.
        let proceed = cooked || has_transcript_lines(session);
        if !proceed {
            continue;
        }

        for fmt in &export_formats {
            match export_session(session, *fmt) {
                Ok(p) => println!("  export: {}", p.display()),
                Err(e) => {
                    failed_post += 1;
                    eprintln!("  export error: {e:#}");
                }
            }
        }

        let Some((client, pp)) = &llm else { continue };

        // --refine: clean up the transcript as a new version (before summary/cleanup, so that
        // those are built from the already cleaned-up best).
        if cli.refine {
            match localvox_light_llm::pipeline::refine_session(session, client, pp, cli.force) {
                Ok(o) if o.skipped => {
                    println!("  refine: best is already cleaned up v{:03} (--force to repeat)", o.version_id);
                }
                Ok(o) => println!(
                    "  refine: v{:03} best ({}/{} lines fixed, {} with no LLM answer, {} calls, {:.1} s)",
                    o.version_id, o.changed, o.lines, o.omitted, o.llm_calls, o.wall_sec
                ),
                Err(e) => {
                    failed_post += 1;
                    eprintln!("  refine error: {e:#}");
                }
            }
        }

        let mut tasks: Vec<localvox_light_llm::pipeline::Task> = Vec::new();
        if cli.cleanup {
            tasks.push(localvox_light_llm::pipeline::Task::Cleanup);
        }
        if cli.summary {
            tasks.push(localvox_light_llm::pipeline::Task::Summary);
        }
        // The recording's language — the same one the processor itself will see (it picks the
        // template by it). The recipe MUST be computed IDENTICALLY here and in the daemon's
        // discovery, otherwise the artifact will be made forever.
        let lang = localvox_light_core::lang::text(session);
        for task in &tasks {
            use localvox_light_core::processing::{self, Outcome};
            let (artifact, style) = match task {
                localvox_light_llm::pipeline::Task::Cleanup => {
                    (processing::PROCESSED, lang.clone())
                }
                localvox_light_llm::pipeline::Task::Summary => (
                    processing::SUMMARY,
                    processing::llm_style(cli.summary_template.as_deref(), &lang),
                ),
            };
            // The artifact's recipe: kind of work + style (template or language) + model +
            // prompt revision. If anything changed — we redo it; if nothing did — we do not
            // touch it.
            let recipe = processing::llm_recipe(artifact, &style, &cli.llm_model);

            match localvox_light_llm::pipeline::process_session(session, task, client, pp) {
                // A silent session is not an error: silence from an always-on recorder is
                // normal, whereas a summary invented out of silence is a catastrophe.
                // And this IS work COMPLETED: there is nothing to repeat (otherwise — a loop).
                Ok(o) if o.skipped.is_some() => {
                    let why = o.skipped.unwrap_or_default();
                    println!("  llm: skipped — {why}");
                    processing::record(
                        session,
                        artifact,
                        &recipe,
                        Outcome::Nothing,
                        Some(why),
                        o.source,
                    );
                }
                Ok(o) => {
                    println!(
                        "  llm: {} ({} calls, {:.1} s, glossary: {} rules fired)",
                        o.out_path.display(),
                        o.llm_calls,
                        o.wall_sec,
                        o.replacements
                    );
                    // Not an error and not a loss: the document IS written. This is a MARK —
                    // «worth double-checking, may be a lie or an imprecision» — not a verdict.
                    // The check compares literally and will always have false positives
                    // (measured: «Сергей» and «муж» flagged although both were said out loud;
                    // «Санкт-Петербург» was the model's synonym for the «Питер» that was said).
                    // A human reads it and decides — one button in the archive removes the mark.
                    if let Some(what) = &o.unverified {
                        eprintln!("  ⚠ worth double-checking: {what}");
                    }
                    processing::record(
                        session,
                        artifact,
                        &recipe,
                        if o.unverified.is_some() {
                            Outcome::Unverified
                        } else {
                            Outcome::Ok
                        },
                        o.unverified,
                        o.source,
                    );
                }
                Err(e) => {
                    failed_post += 1;
                    eprintln!("  llm error: {e:#}");
                    processing::record(
                        session,
                        artifact,
                        &recipe,
                        Outcome::Failed,
                        Some(format!("{e:#}")),
                        None,
                    );
                }
            }
        }
    }
    // Exit codes: 1 — the cook failed (there is no data, the whole thing must be repeated);
    // 2 — the cook succeeded, only post-processing failed (LLM/export): the version is
    // committed, and it is enough for the daemon to repeat the post-processing later.
    if failed_cook > 0 {
        eprintln!("{failed_cook} sessions not cooked, {failed_post} post-processing errors");
        std::process::exit(1);
    }
    if failed_post > 0 {
        eprintln!("the cook succeeded; post-processing (LLM/export) errors: {failed_post}");
        std::process::exit(2);
    }
    Ok(())
}

/// The entity tagger for the grounding check.
///
/// A bridge between GLiNER (core, ONNX) and the check (the LLM crate). The check does not know,
/// and must not know, WHAT exactly tags the entities — it only knows that it is not a generative
/// model: spans of the input, not generated text.
struct GlinerEntities {
    ner: localvox_light_core::ner::Ner,
    /// The NAME label — what we accuse over.
    name: String,
    /// The ROLE label — not accusatory. It exists so that the name has something to be compared
    /// against: the name must outweigh the role on the same span (see `Ner::names`).
    role: String,
}

/// The labels are STRINGS. Not a single name and not a single language in the code: this is what
/// we ARE LOOKING FOR, not what we know in advance.
///
/// Two labels, and the second is no less important than the first. Recordings without names
/// exist, and the summary has to call the speaker something — "спикер", "говорящий",
/// "рассказчик". That is a ROLE. Without the second label every honest summary of a nameless
/// recording went into quarantine (acceptance on 2026-07-13, on the live archive).
fn ner_labels() -> (String, String) {
    const NAME: &str = "имя человека";
    const ROLE: &str = "говорящий или роль";
    let name = std::env::var("LOCALVOX_NER_LABEL_NAME").unwrap_or_else(|_| NAME.into());
    let role = std::env::var("LOCALVOX_NER_LABEL_ROLE").unwrap_or_else(|_| ROLE.into());
    (name, role)
}

/// The threshold for the SOURCE is lower than for the answer. A superfluous entity in the source
/// is harmless — it can only ground something in the answer; a superfluous one in the answer is
/// a false accusation.
const SOURCE_THRESHOLD: f32 = 0.3;

impl localvox_light_llm::grounding::Entities for GlinerEntities {
    fn people(&self, text: &str) -> Vec<String> {
        self.names_at(text, self.ner.threshold(), "answer")
    }

    fn people_in_source(&self, text: &str) -> Vec<String> {
        // We read the source GENEROUSLY: an extra person there is harmless — they can only ground
        // the answer. An extra one in the answer is a false accusation.
        self.names_at(text, SOURCE_THRESHOLD, "source")
    }
}

impl GlinerEntities {
    fn names_at(&self, text: &str, threshold: f32, where_: &str) -> Vec<String> {
        match self.ner.names(text, &self.name, &self.role, threshold) {
            Ok(e) => e.into_iter().map(|e| e.text).collect(),
            Err(err) => {
                // We must not silently return empty: an empty list means "there are no people",
                // and name checking would switch itself off unnoticed.
                eprintln!("  NER ERROR on the {where_}: {err:#}");
                Vec::new()
            }
        }
    }
}

/// Build the tagger if the model is there. If it is not — we return `None` and SAY SO: silently
/// pretending that the names were checked is not allowed.
fn build_entities(
    asr_model_dir: &Path,
) -> Option<std::sync::Arc<dyn localvox_light_llm::grounding::Entities>> {
    // We look for the NER directory NEXT TO THE ASR MODEL, whose path was passed to us in a flag.
    // The daemon launches us from its own working directory (which under autostart is system32),
    // and a cwd-based search silently failed to find the model — name checking switched itself
    // off, and there was nothing to notice it by.
    let Some(dir) = localvox_light_core::ner::model_dir_near(Some(asr_model_dir)) else {
        // We must not keep quiet: "names are not being checked" must be audible.
        eprintln!("  name checking is OFF: no NER model (models/ner-gliner)");
        eprintln!("    numbers are always checked; names are not. To install: scripts/setup-ner.ps1");
        return None;
    };
    match localvox_light_core::ner::Ner::open(&dir) {
        Ok(ner) => {
            let (name, role) = ner_labels();
            eprintln!(
                "  name checking: {} («{name}» against «{role}»)",
                dir.display()
            );
            Some(std::sync::Arc::new(GlinerEntities { ner, name, role }))
        }
        Err(e) => {
            eprintln!("  WARNING: the NER model failed to load ({e:#})");
            eprintln!("    names, dates and amounts will NOT be checked — only numbers");
            None
        }
    }
}

/// Is there at least one line of speech in the working version of the transcript? That is exactly
/// the input for the LLM — it does not need the audio.
fn has_transcript_lines(session: &Path) -> bool {
    use localvox_light_core::versions::{read_transcript_lines, VersionStore};
    VersionStore::open(session)
        .ok()
        .and_then(|s| s.best().map(|v| (s, v)))
        .and_then(|(s, v)| s.resolve(v.id))
        .and_then(|p| read_transcript_lines(&p).ok())
        .is_some_and(|l| !l.is_empty())
}

/// An external source (URL/file) → PCM 16k mono → a new session with chunks.
/// From then on the session is indistinguishable from one recorded live — a common pipeline.
fn ingest_to_session(input: &str, work_dir: &Path) -> Result<PathBuf> {
    let settings = load_settings_named(&["localvox-asr-settings.json", "settings.json"]);
    let ffmpeg = resolve_ffmpeg(&settings, None);

    let is_url = input.starts_with("http://") || input.starts_with("https://");
    let (pcm_bytes, label) = if is_url {
        let yt_dlp = resolve_yt_dlp(&settings, None);
        let ffmpeg_location = resolve_ffmpeg_location_for_ytdlp(&ffmpeg);
        let js_runtime = resolve_js_runtime(&settings, None, None);
        let temp = download_audio_with_progress(
            false,
            &yt_dlp,
            input,
            ffmpeg_location.as_deref(),
            js_runtime.as_deref(),
            false,
        )
        .with_context(|| format!("downloading {input}"))?;
        let pcm =
            convert_to_pcm_with_progress(false, &ffmpeg, &temp, false).context("ffmpeg → pcm")?;
        let _ = std::fs::remove_file(&temp);
        let label = temp
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "youtube".into());
        (pcm, label)
    } else {
        let path = PathBuf::from(input);
        anyhow::ensure!(path.is_file(), "neither a file nor a URL: {input}");
        let pcm =
            convert_to_pcm_with_progress(false, &ffmpeg, &path, false).context("ffmpeg → pcm")?;
        let label = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".into());
        (pcm, label)
    };

    let samples: Vec<i16> = pcm_bytes
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();
    anyhow::ensure!(!samples.is_empty(), "empty audio after conversion");

    let (audio_dir, meta_path) =
        localvox_light_core::chunks::create_session_dir(work_dir, Some(&label))?;
    let session_dir = meta_path.parent().unwrap_or(&audio_dir).to_path_buf();
    let params = Arc::new(ChunkParams {
        audio_dir,
        meta_path: meta_path.clone(),
        chunk_sec: 300.0,
        flac: false,
        ffmpeg: PathBuf::from(&ffmpeg),
    });
    let meta = Arc::new(Mutex::new(SessionMeta {
        started_at: localvox_light_core::versions::now_rfc3339(),
        sample_rate: 16_000,
        chunks: Vec::new(),
        ..Default::default()
    }));
    let mut rec = ChunkRecorder::new(0, params, meta.clone());
    for block in samples.chunks(64 * 1024) {
        rec.feed(block);
    }
    rec.finalize_current();
    // The directory could have been swept away (a neighbouring daemon's sweep of empty sessions
    // in the moment between creation and the first chunk) — the loss must be loud.
    anyhow::ensure!(
        !meta.lock().unwrap().chunks.is_empty(),
        "not a single audio chunk was written (did the session directory vanish?) — repeat the ingest"
    );
    Ok(session_dir)
}

fn list_sessions(work_dir: &std::path::Path) -> Result<Vec<PathBuf>> {
    let root = work_dir.join("sessions");
    let mut out: Vec<PathBuf> = std::fs::read_dir(&root)
        .with_context(|| format!("reading {}", root.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join("meta.json").exists())
        .collect();
    out.sort();
    Ok(out)
}
