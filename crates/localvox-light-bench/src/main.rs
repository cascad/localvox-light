//! `localvox-bench` — measuring recognition quality on reference datasets.
//!
//! Why: before this, quality was judged by ear ("porridge" / "intelligible"), and we caught the
//! window regression (150 s) by accident, at a live acceptance. The benchmark gives a number:
//! WER/CER on a set with a known reference — both comparable with the world (Golos, Common
//! Voice, Russian LibriSpeech: GigaAM, Vosk and Whisper measure themselves on these), and our
//! own (long-form: sparse conversational speech — the mode we got burned in; short reference
//! recordings do NOT cover that mode).
//!
//! Subcommands:
//!   `wer`      — run the model over a manifest, get WER/CER/RTF;
//!   `sweep`    — the same for a set of window parameters: a "window → WER" table;
//!   `longform` — build long audio out of short reference clips with pauses (the reference is
//!                known exactly) — a reproducible model of our own mode.
//!
//! The datasets and how to get them — `docs/asr-bench.md`.

mod dataset;
mod faithfulness;
mod fetch;
mod metrics;

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use localvox_light_core::asr::AsrEngine;
use localvox_light_core::cook::Windower;

use metrics::Errors;

const SAMPLE_RATE: usize = 16_000;

#[derive(Parser)]
#[command(
    name = "localvox-bench",
    about = "ASR quality in numbers: WER/CER on reference sets, a sweep over windows, long-form assembly"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Download a reference test set (Golos, Russian LibriSpeech)
    Fetch(FetchArgs),
    /// Run the model over a manifest and compute WER/CER
    Wer(WerArgs),
    /// A "window parameters → WER" table on one and the same set
    Sweep(SweepArgs),
    /// Build a long-form set out of short clips (the reference is known exactly)
    Longform(LongformArgs),
    /// Compare LLMs on "invention": refusal on an empty recording + the share of invented
    /// names/numbers in the summary. The prompt is exactly the production one (template + glossary).
    Llm(LlmArgs),
}

#[derive(clap::Args)]
struct LlmArgs {
    /// Models, comma-separated (Ollama): qwen3.5:4b,granite4.1:8b,…
    #[arg(long, default_value = "qwen3.5:4b")]
    models: String,

    /// Archive sessions to check against a real transcript (comma-separated).
    /// Without them we only run the trap — a recording with no meaningful speech.
    #[arg(long, default_value = "")]
    sessions: String,

    #[arg(
        long,
        default_value = "localvox-audio",
        env = "LOCALVOX_LIGHT_AUDIO_DIR"
    )]
    work_dir: PathBuf,

    #[arg(
        long,
        default_value = "http://localhost:11434/v1",
        env = "LOCALVOX_LLM_BASE_URL"
    )]
    llm_base_url: String,

    #[arg(
        long,
        default_value = "assets/glossary",
        env = "LOCALVOX_LLM_GLOSSARY_DIR"
    )]
    glossary_dir: PathBuf,

    #[arg(long, env = "LOCALVOX_LLM_TEMPLATES_DIR")]
    templates_dir: Option<PathBuf>,

    #[arg(long, default_value = "summary-ru")]
    template: String,

    #[arg(long, default_value = "600")]
    timeout_sec: u64,

    /// Where to dump the raw model answers (going through them by eye is mandatory —
    /// numbers without the text are deceptive)
    #[arg(long)]
    dump_dir: Option<PathBuf>,
}

#[derive(clap::Args, Clone)]
struct EngineArgs {
    /// Engine: gigaam (ONNX) or vosk
    #[arg(long, default_value = "gigaam")]
    engine: String,

    /// Model directory: ONNX model + vocabulary for gigaam, the model directory for vosk
    #[arg(
        long,
        env = "LOCALVOX_ASR_MODEL_DIR",
        default_value = "models/gigaam-v3-e2e-ctc"
    )]
    model_dir: PathBuf,
}

#[derive(clap::Args, Clone)]
struct WindowArgs {
    /// Cut the audio into windows on VAD silence — the way the cook does it.
    /// Without the flag the clip goes into the model whole (the short reference clip mode).
    #[arg(long)]
    windowed: bool,

    /// Hard ceiling of the window, sec
    #[arg(long, default_value = "30")]
    max_window_sec: f64,

    /// After this duration the window is closed at the first silence, sec
    #[arg(long, default_value = "15")]
    min_cut_sec: f64,

    /// How much continuous silence counts as a window boundary, ms
    #[arg(long, default_value = "500")]
    silence_ms: u32,
}

#[derive(clap::Args)]
struct FetchArgs {
    /// Name of a set from the registry (see --list)
    #[arg(long)]
    dataset: Option<String>,

    /// Show the available sets
    #[arg(long)]
    list: bool,

    /// Directory for the audio and the manifest
    #[arg(long)]
    out: Option<PathBuf>,

    /// How many records to take (test splits hold thousands; 200 is enough for a run)
    #[arg(long, default_value = "200")]
    limit: usize,

    /// Which line of the split to start from (for a second, independent sample)
    #[arg(long, default_value = "0")]
    offset: usize,
}

#[derive(clap::Args)]
struct WerArgs {
    /// NeMo manifest: {"audio_filepath": "...", "text": "reference"}
    manifest: PathBuf,

    #[command(flatten)]
    engine: EngineArgs,

    #[command(flatten)]
    window: WindowArgs,

    /// Take only the first N records (a quick check)
    #[arg(long)]
    limit: Option<usize>,

    /// Exclude records where the alphabets of the reference and the hypothesis are bound to
    /// diverge (Latin "YouTube" ↔ "ютьюб", ordinals "15-й" ↔ "пятнадцатый").
    /// By default they are counted — but shown on a separate line.
    #[arg(long)]
    skip_alphabet_mismatch: bool,

    /// Where to dump the per-file breakdown (jsonl: reference, hypothesis, WER)
    #[arg(long)]
    out: Option<PathBuf>,
}

#[derive(clap::Args)]
struct SweepArgs {
    manifest: PathBuf,

    #[command(flatten)]
    engine: EngineArgs,

    /// Window configurations, comma-separated: "max:min", plus `whole` — the clip as a whole.
    /// Example: whole,15:8,30:15,60:30,180:150
    #[arg(long, default_value = "whole,15:8,30:15,60:30,180:150")]
    windows: String,

    #[arg(long, default_value = "500")]
    silence_ms: u32,

    #[arg(long)]
    limit: Option<usize>,

    /// Exclude records with an alphabet/ordinal mismatch (see `wer`)
    #[arg(long)]
    skip_alphabet_mismatch: bool,
}

#[derive(clap::Args)]
struct LongformArgs {
    /// Manifest of short clips with references (Golos test, Common Voice, FLEURS…)
    manifest: PathBuf,

    /// Result directory (wav + longform.jsonl)
    #[arg(long, default_value = "bench/longform")]
    out: PathBuf,

    /// Duration of one long recording, sec
    #[arg(long, default_value = "600")]
    file_sec: f64,

    /// How many long recordings to build
    #[arg(long, default_value = "3")]
    files: usize,

    /// Share of the time with speech. 0.2 — a conversation with pauses (our real mode),
    /// 0.9 — a solid lecture. It was exactly the sparseness that killed long windows.
    #[arg(long, default_value = "0.2")]
    speech_ratio: f64,

    /// Seed of the pause generator — the set is reproducible byte for byte
    #[arg(long, default_value = "42")]
    seed: u64,
}

fn main() -> Result<()> {
    if let Err(e) = dotenvy::dotenv() {
        if !matches!(e, dotenvy::Error::Io(_)) {
            eprintln!("ERROR in .env: {e}");
        }
    }
    match Cli::parse().cmd {
        Cmd::Fetch(a) => cmd_fetch(a),
        Cmd::Wer(a) => cmd_wer(a),
        Cmd::Sweep(a) => cmd_sweep(a),
        Cmd::Longform(a) => cmd_longform(a),
        Cmd::Llm(a) => cmd_llm(a),
    }
}

fn cmd_llm(a: LlmArgs) -> Result<()> {
    let sessions: Vec<String> = a
        .sessions
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    let cases = faithfulness::build_cases(&a.work_dir, &sessions)?;
    let models: Vec<&str> = a
        .models
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let p = faithfulness::BenchParams {
        base_url: &a.llm_base_url,
        glossary_dir: &a.glossary_dir,
        templates_dir: a.templates_dir.as_deref(),
        template: &a.template,
        timeout_sec: a.timeout_sec,
        dump_dir: a.dump_dir.as_deref(),
    };

    println!("cases: {}", cases.len());
    for c in &cases {
        println!(
            "  {}{}",
            c.name,
            if c.is_trap {
                "  ← trap"
            } else {
                ""
            }
        );
    }
    println!();
    println!(
        "{:<18} {:<22} {:>12} {:>9} {:>8} {:>6}",
        "model", "case", "verdict", "invented", "of them n", "sec"
    );
    println!("{}", "─".repeat(82));

    for model in models {
        let verdicts = match faithfulness::run_model(model, &cases, &p) {
            Ok(v) => v,
            Err(e) => {
                println!("{model:<18} ERROR: {e:#}");
                continue;
            }
        };
        for v in &verdicts {
            let verdict = if v.fabricated() {
                "INVENTED"
            } else if v.over_refused() {
                "VAIN REFUSAL"
            } else if v.is_trap {
                "no invention"
            } else {
                "processed"
            };
            println!(
                "{model:<18} {:<22} {verdict:>12} {:>8.0}% {:>8} {:>6.1}",
                v.case.chars().take(22).collect::<String>(),
                v.invented_pct(),
                v.invented_numbers().len(),
                v.wall_sec
            );
            if v.fabricated() {
                let sample: Vec<&str> = v.invented.iter().take(8).map(String::as_str).collect();
                println!(
                    "{:<18}   {} chars out of nothing: {}",
                    "",
                    v.chars,
                    sample.join(", ")
                );
            }
        }
    }
    println!(
        "\nINVENTED — on a recording with no speech it brought facts out of nowhere (a catastrophe).\n\
         VAIN REFUSAL — it would not process a recording that DOES contain speech (also a defect).\n\
         «invented» — the share of names/numbers in the summary that are absent from the input; some of\n\
         them are REPAIRED names («XI от И» → «XAI»), so look at the neighbouring column:\n\
         «of them n» — invented NUMBERS (deadlines, quantities). Those cannot be repaired by anything."
    );
    Ok(())
}

fn cmd_fetch(a: FetchArgs) -> Result<()> {
    if a.list || a.dataset.is_none() {
        fetch::list();
        return Ok(());
    }
    let name = a.dataset.unwrap();
    let out = a.out.unwrap_or_else(|| PathBuf::from("bench").join(&name));
    fetch::fetch(&name, &out, a.limit, a.offset)
}

// ─────────────────────────── engine ───────────────────────────

fn load_engine(a: &EngineArgs) -> Result<Box<dyn AsrEngine>> {
    match a.engine.as_str() {
        "gigaam" => {
            let model_file = find_onnx(&a.model_dir)?;
            let adapter = localvox_light_core::asr::onnx::adapters::GigaamV3E2eCtc::from_model_dir(
                &a.model_dir,
            )
            .context("building the GigaAM v3 E2E CTC adapter")?;
            let engine = localvox_light_core::asr::onnx::OnnxEngine::new(&model_file, adapter)
                .context("loading the ONNX model")?;
            eprintln!("engine: gigaam ({})", model_file.display());
            Ok(Box::new(engine))
        }
        "vosk" => {
            let engine = localvox_light_core::asr::vosk::VoskEngine::new(&a.model_dir)
                .context("loading the Vosk model")?;
            eprintln!("engine: vosk ({})", a.model_dir.display());
            Ok(Box::new(engine))
        }
        other => bail!("unknown engine «{other}» (gigaam | vosk)"),
    }
}

fn find_onnx(dir: &Path) -> Result<PathBuf> {
    let mut c: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("model directory: {}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("onnx"))
        .collect();
    if c.is_empty() {
        bail!("no *.onnx in {}", dir.display());
    }
    c.sort_by_key(|p| !p.to_string_lossy().contains("int8"));
    Ok(c.remove(0))
}

/// Transcribe a clip: whole, or through the very same Windower the cook uses.
/// Returns the hypothesis — the windows glued together (for WER the text matters, not timecodes).
fn transcribe(engine: &dyn AsrEngine, samples: &[i16], w: &WindowArgs) -> Result<String> {
    let to_f32 = |s: &[i16]| -> Vec<f32> { s.iter().map(|&x| f32::from(x) / 32768.0).collect() };
    if !w.windowed {
        return engine.transcribe(&to_f32(samples));
    }
    let mut vad = webrtc_vad::Vad::new_with_rate_and_mode(
        webrtc_vad::SampleRate::Rate16kHz,
        webrtc_vad::VadMode::LowBitrate,
    );
    let mut classify = |frame: &[i16]| vad.is_voice_segment(frame).unwrap_or(true);

    let mut parts: Vec<String> = Vec::new();
    let mut err: Option<anyhow::Error> = None;
    let mut windower = Windower::new(w.max_window_sec, w.min_cut_sec, w.silence_ms);
    {
        let mut emit = |_start: u64, s: &[i16]| -> Result<()> {
            match engine.transcribe(&to_f32(s)) {
                Ok(t) if !t.trim().is_empty() => parts.push(t.trim().to_string()),
                Ok(_) => {}
                Err(e) => err = Some(e),
            }
            Ok(())
        };
        windower.feed(samples, &mut classify, &mut emit)?;
        windower.finish(&mut emit)?;
    }
    if let Some(e) = err {
        return Err(e);
    }
    Ok(parts.join(" "))
}

// ─────────────────────────── wer ───────────────────────────

#[derive(serde::Serialize)]
struct PerFile {
    audio: String,
    reference: String,
    hypothesis: String,
    wer: f64,
    cer: f64,
    ref_words: usize,
}

struct RunStats {
    total: Errors,
    audio_sec: f64,
    wall_sec: f64,
    files: usize,
    /// Records where an alphabet/ordinal mismatch inevitably spoils the WER — we count them
    /// separately, so that it is visible how much of the "errors" is really formatting.
    alphabet_mismatch: Errors,
    alphabet_files: usize,
    skipped: usize,
    per_file: Vec<PerFile>,
}

fn run(engine: &dyn AsrEngine, args: &WerArgs) -> Result<RunStats> {
    let utts = dataset::load_manifest(&args.manifest)?;
    let n = args.limit.unwrap_or(utts.len()).min(utts.len());
    let mut st = RunStats {
        total: Errors::default(),
        audio_sec: 0.0,
        wall_sec: 0.0,
        files: 0,
        alphabet_mismatch: Errors::default(),
        alphabet_files: 0,
        skipped: 0,
        per_file: Vec::new(),
    };
    for (i, u) in utts.iter().take(n).enumerate() {
        let samples = dataset::load_audio_16k_mono(&u.audio)?;
        // A divergence from the `duration` in the manifest is a sign of a corrupted/substituted
        // dataset distribution; measuring WER on it is pointless, better to say so out loud.
        if let Some(d) = u.duration_sec {
            let actual = samples.len() as f64 / SAMPLE_RATE as f64;
            if d > 0.0 && (actual - d).abs() > 0.5 + 0.1 * d {
                eprintln!(
                    "warning: {} — {d:.1} s in the manifest, {actual:.1} s in the file",
                    u.audio.display()
                );
            }
        }
        let t0 = Instant::now();
        let hyp = transcribe(engine, &samples, &args.window)
            .with_context(|| format!("recognizing {}", u.audio.display()))?;
        st.wall_sec += t0.elapsed().as_secs_f64();
        st.audio_sec += samples.len() as f64 / SAMPLE_RATE as f64;

        // Latin letters / ordinals: the reference and the hypothesis diverge in format, not in
        // what was heard.
        let mismatch = metrics::has_latin(&u.reference) != metrics::has_latin(&hyp)
            || metrics::has_ordinal_digits(&hyp);
        let e = Errors::compare(&u.reference, &hyp);
        if mismatch {
            st.alphabet_mismatch.add(e);
            st.alphabet_files += 1;
            if args.skip_alphabet_mismatch {
                st.skipped += 1;
                continue;
            }
        }
        st.total.add(e);
        st.files += 1;
        st.per_file.push(PerFile {
            audio: u.audio.display().to_string(),
            reference: u.reference.clone(),
            hypothesis: hyp,
            wer: e.wer(),
            cer: e.cer(),
            ref_words: e.ref_words,
        });
        if (i + 1) % 25 == 0 {
            eprint!("\r  {}/{n}…", i + 1);
        }
    }
    eprintln!("\r{:30}\r", "");
    if st.files == 0 {
        bail!("nothing to measure: all records were filtered out");
    }
    Ok(st)
}

fn cmd_wer(args: WerArgs) -> Result<()> {
    let engine = load_engine(&args.engine)?;
    let st = run(engine.as_ref(), &args)?;

    let mode = if args.window.windowed {
        format!(
            "windows {:.0}/{:.0} s",
            args.window.max_window_sec, args.window.min_cut_sec
        )
    } else {
        "whole clip".to_string()
    };
    println!(
        "set: {} ({} records, {})",
        args.manifest.display(),
        st.files,
        mode
    );
    println!("  WER: {:.2} %", st.total.wer());
    println!("  CER: {:.2} %", st.total.cer());
    if st.alphabet_files > 0 {
        let verb = if args.skip_alphabet_mismatch {
            "excluded"
        } else {
            "included"
        };
        println!(
            "  of them with an alphabet/ordinal mismatch: {} records ({verb}), their WER {:.1} %",
            st.alphabet_files,
            st.alphabet_mismatch.wer()
        );
    }
    println!(
        "  audio {:.1} min, inference {:.1} s, RTF {:.3}",
        st.audio_sec / 60.0,
        st.wall_sec,
        if st.audio_sec > 0.0 {
            st.wall_sec / st.audio_sec
        } else {
            0.0
        }
    );

    let mut worst: Vec<&PerFile> = st.per_file.iter().filter(|f| f.ref_words >= 3).collect();
    worst.sort_by(|a, b| b.wer.total_cmp(&a.wer));
    if !worst.is_empty() {
        println!("\nworst records:");
        for f in worst.iter().take(5) {
            println!("  WER {:6.1} %  {}", f.wer, f.audio);
            println!(
                "    reference:  {}",
                f.reference.chars().take(90).collect::<String>()
            );
            println!(
                "    hypothesis: {}",
                f.hypothesis.chars().take(90).collect::<String>()
            );
        }
    }

    if let Some(out) = &args.out {
        let mut s = String::new();
        for f in &st.per_file {
            s.push_str(&serde_json::to_string(f)?);
            s.push('\n');
        }
        std::fs::write(out, s).with_context(|| format!("writing {}", out.display()))?;
        println!("\nper-file breakdown: {}", out.display());
    }
    Ok(())
}

// ─────────────────────────── sweep ───────────────────────────

fn cmd_sweep(args: SweepArgs) -> Result<()> {
    let mut configs: Vec<(String, WindowArgs)> = Vec::new();
    for spec in args
        .windows
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if spec == "whole" {
            configs.push((
                "whole clip".into(),
                WindowArgs {
                    windowed: false,
                    max_window_sec: 0.0,
                    min_cut_sec: 0.0,
                    silence_ms: args.silence_ms,
                },
            ));
            continue;
        }
        let (max, min) = spec
            .split_once(':')
            .with_context(|| format!("window «{spec}»: expected max:min or whole"))?;
        let (max, min): (f64, f64) = (max.trim().parse()?, min.trim().parse()?);
        if min > max {
            bail!("window «{spec}»: min_cut ({min}) is greater than max_window ({max})");
        }
        configs.push((
            format!("{max:.0}/{min:.0} s"),
            WindowArgs {
                windowed: true,
                max_window_sec: max,
                min_cut_sec: min,
                silence_ms: args.silence_ms,
            },
        ));
    }

    // The model is loaded ONCE for all configurations — otherwise the difference in time between
    // the table's rows would be about warm-up, not about the windows.
    let engine = load_engine(&args.engine)?;
    println!("set: {}\n", args.manifest.display());
    println!("{:<16} {:>8} {:>8} {:>8}", "window", "WER %", "CER %", "RTF");
    println!("{}", "─".repeat(44));
    for (name, window) in configs {
        let a = WerArgs {
            manifest: args.manifest.clone(),
            engine: args.engine.clone(),
            window,
            limit: args.limit,
            skip_alphabet_mismatch: args.skip_alphabet_mismatch,
            out: None,
        };
        let st = run(engine.as_ref(), &a)?;
        println!(
            "{:<16} {:>8.2} {:>8.2} {:>8.3}",
            name,
            st.total.wer(),
            st.total.cer(),
            if st.audio_sec > 0.0 {
                st.wall_sec / st.audio_sec
            } else {
                0.0
            }
        );
    }
    Ok(())
}

// ─────────────────────────── longform ───────────────────────────

/// xorshift64* — deterministic pauses with no external dependency: a set assembled with the same
/// seed is the same byte for byte (otherwise runs cannot be compared).
struct Rng(u64);
impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// Uniformly in [lo, hi)
    fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
        let u = (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
        lo + u * (hi - lo)
    }
}

#[derive(serde::Serialize)]
struct LongSegment {
    start_sec: f64,
    end_sec: f64,
    text: String,
    source: String,
}

#[derive(serde::Serialize)]
struct LongEntry {
    audio_filepath: String,
    text: String,
    duration: f64,
    speech_ratio: f64,
    segments: Vec<LongSegment>,
}

fn cmd_longform(args: LongformArgs) -> Result<()> {
    if !(0.01..=1.0).contains(&args.speech_ratio) {
        bail!("--speech-ratio must be in (0.01, 1.0]");
    }
    let utts = dataset::load_manifest(&args.manifest)?;
    let usable: Vec<_> = utts
        .iter()
        .filter(|u| !u.reference.trim().is_empty())
        .collect();
    if usable.is_empty() {
        bail!("the manifest has no records with reference text");
    }

    std::fs::create_dir_all(&args.out)
        .with_context(|| format!("creating {}", args.out.display()))?;
    let mut rng = Rng(args.seed.max(1));
    let mut manifest = String::new();
    let mut cursor = 0usize; // clips are taken in order, with no repeats between files

    for file_idx in 0..args.files {
        let target = (args.file_sec * SAMPLE_RATE as f64) as usize;
        let mut audio: Vec<i16> = Vec::with_capacity(target);
        let mut segments: Vec<LongSegment> = Vec::new();
        let mut speech_samples = 0usize;

        while audio.len() < target {
            let u = usable[cursor % usable.len()];
            cursor += 1;
            let samples = dataset::load_audio_16k_mono(&u.audio)?;
            if samples.is_empty() {
                continue;
            }
            let start = audio.len() as f64 / SAMPLE_RATE as f64;
            audio.extend_from_slice(&samples);
            speech_samples += samples.len();
            segments.push(LongSegment {
                start_sec: start,
                end_sec: audio.len() as f64 / SAMPLE_RATE as f64,
                text: u.reference.trim().to_string(),
                source: u.audio.display().to_string(),
            });

            // The pause after an utterance: on average such that the speech share comes out at the
            // target. Jitter ×[0.4, 1.6] — pauses in a conversation are not uniform, and it is
            // exactly a "long pause in the wrong place" that tears a window off a phrase boundary.
            let mean_gap = samples.len() as f64 * (1.0 / args.speech_ratio - 1.0);
            let gap = (mean_gap * rng.uniform(0.4, 1.6)) as usize;
            audio.extend(std::iter::repeat_n(
                0i16,
                gap.min(target.saturating_sub(audio.len())),
            ));
        }

        let name = format!("lf_{:03}.wav", file_idx + 1);
        let path = args.out.join(&name);
        dataset::write_wav_16k_mono(&path, &audio)?;
        let entry = LongEntry {
            audio_filepath: name.clone(),
            // The reference of a long recording is the references of the utterances glued in
            // order: exactly what the model is obliged to produce, and exactly what is lost in
            // the "porridge" on long windows.
            text: segments
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            duration: audio.len() as f64 / SAMPLE_RATE as f64,
            speech_ratio: speech_samples as f64 / audio.len() as f64,
            segments,
        };
        manifest.push_str(&serde_json::to_string(&entry)?);
        manifest.push('\n');
        println!(
            "{}: {:.1} min, utterances {}, speech {:.0} %",
            name,
            entry.duration / 60.0,
            entry.segments.len(),
            entry.speech_ratio * 100.0
        );
    }

    let mpath = args.out.join("longform.jsonl");
    std::fs::write(&mpath, manifest).with_context(|| format!("writing {}", mpath.display()))?;
    println!("\nmanifest: {}", mpath.display());
    println!(
        "run it: localvox-bench sweep {} --windows whole,15:8,30:15,180:150",
        mpath.display()
    );
    Ok(())
}
