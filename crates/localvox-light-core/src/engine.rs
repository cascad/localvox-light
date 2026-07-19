//! Capture → pipeline → Vosk ASR → `transcript.jsonl`. Shared logic for the TUI and headless.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait};
use crossbeam_channel::Sender;
use hound::WavReader;
use tracing::{debug, info};

use crate::asr::{speech_ratio, trim_to_speech, AsrEngine};
use crate::audio;
use crate::cli::{normalized_model_path, Cli};
use crate::events::{StructuredLog, UiMsg};
use crate::light_config::LightDeviceConfig;
use crate::pipeline::{PipelineConfig, SegmentPayload, SegmentReady};
use crate::session;
use crate::transcript::{TranscriptEntry, TranscriptWriter};

#[allow(clippy::too_many_arguments)]
pub fn run_engine(
    cli: Cli,
    audio_devices: Arc<RwLock<LightDeviceConfig>>,
    ui_tx: Option<Sender<UiMsg>>,
    reset_rx: crossbeam_channel::Receiver<()>,
    running: Arc<AtomicBool>,
    record_pcm: Arc<AtomicBool>,
    reload_gen: Arc<AtomicU64>,
    transcript_hook: Option<crate::events::TranscriptHook>,
) -> Result<()> {
    let work_dir = PathBuf::from(&cli.audio_dir);
    std::fs::create_dir_all(&work_dir)?;
    info!("Workspace: {}", work_dir.display());

    // One writing process per work_dir: a second instance would wreck the live
    // .part chunks of the first one with its recovery. The lock is held until run_engine ends.
    let _instance_lock = match session::try_instance_lock(&work_dir) {
        Ok(Some(lock)) => lock,
        Ok(None) => anyhow::bail!(
            "the workspace directory {} is already taken by another localvox-light — we do not start a second instance",
            work_dir.display()
        ),
        Err(e) => anyhow::bail!("workspace directory lock: {e}"),
    };

    session::remove_orphan_part_files(&work_dir);

    // The live length of the segment queue (RAM mode leaves no files to count).
    let pending_segments = Arc::new(AtomicUsize::new(0));

    // F8: the session chunks are the primary artifact. Unfinished ones after a crash — recover;
    // old ones — sweep by the retention policy (P3).
    let recovered_chunks = crate::chunks::recover_orphan_chunks(&work_dir);
    if recovered_chunks > 0 {
        info!("Recovery: recovered {recovered_chunks} audio chunks (.part → .wav)");
    }
    crate::chunks::sweep_audio_retention(&work_dir, cli.retention_audio_days);
    // 10 min grace: a parallel ingest may have just created a session
    session::sweep_empty_session_dirs(&work_dir, Duration::from_secs(600));

    if let Some(ref t) = ui_tx {
        let _ = t.send(UiMsg::WorkspacePaths {
            workspace_dir: work_dir.clone(),
            dump_dir: cli.transcript_dump_dir.clone(),
        });
        let _ = t.send(UiMsg::Status(format!("Данные: {}", work_dir.display())));
    }

    if let Some(ref t) = ui_tx {
        let sd_stats = work_dir.clone();
        let tx_stats = t.clone();
        let run_stats = running.clone();
        let pending_stats = Arc::clone(&pending_segments);
        thread::Builder::new()
            .name("queue-stats".into())
            .spawn(move || {
                while run_stats.load(Ordering::Relaxed) {
                    let (_, umb, smb) = session::workspace_queue_stats(&sd_stats);
                    let _ = tx_stats.send(UiMsg::QueuePending {
                        unprocessed_wavs: pending_stats.load(Ordering::Relaxed),
                        unprocessed_mb: umb,
                        workspace_total_mb: smb,
                    });
                    thread::sleep(Duration::from_secs(1));
                }
            })
            .expect("spawn queue-stats");
    }

    if let Some(ref t) = ui_tx {
        let entries = TranscriptWriter::read_all_entries(&work_dir);
        let n = entries.len();
        let rows: Vec<_> = entries
            .into_iter()
            .map(|e| {
                let time = format_entry_time_local(&e.timestamp);
                (time, e.source_id, e.text)
            })
            .collect();
        let _ = t.send(UiMsg::TranscriptHistory(rows));
        let _ = t.send(UiMsg::Log(StructuredLog {
            stage: "hydrate".into(),
            source_id: 0,
            chunk_sec: n as f64,
            proc_sec: 0.0,
            detail: if n > 0 {
                format!("TUI ← transcript.jsonl ({n} lines)")
            } else {
                "TUI ← transcript.jsonl (empty)".into()
            },
            verbose_only: true,
        }));
    }

    let model_path_buf = PathBuf::from(normalized_model_path(&cli));
    let noise_gate = cli.noise_gate;

    // UNBOUNDED, and this is not laziness but a deliberate choice.
    //
    // It used to be `bounded(1024)` = 33 seconds of buffer for both sources, and on overflow
    // audio was SILENTLY DROPPED. Under load (the cook + the LLM eat the cores) this single
    // consumer thread starves — and the recording is lost. Measured on a live archive: in one
    // session 55 % of the time survived, the file played twice as fast, the ASR received mush.
    //
    // Audio is the ONLY unrecoverable artifact. Everything else (the transcript, the summary,
    // the indices) is recreated from it. That is why back-pressure here is paid for with
    // MEMORY, not with loss: 32 KB/s per source, a minute-long stall — 2 MB.
    // The queue depth is watched by the watchdog below: staying silent about a stall is not an
    // option either.
    let (pcm_tx, pcm_rx) = crossbeam_channel::unbounded::<audio::PcmChunk>();
    let (seg_tx, seg_rx) = crossbeam_channel::unbounded::<SegmentReady>();
    let (engine_tx, engine_rx) =
        crossbeam_channel::bounded::<Result<crate::asr::vosk::VoskEngine, String>>(1);

    // Recovery: all unprocessed WAVs from disk → into the channel BEFORE the pipeline (sorted
    // by src/seq).
    let recovery = session::recover_unprocessed(&work_dir);
    if !recovery.is_empty() {
        let n = recovery.len();
        debug!("Recovery: {n} unprocessed WAV(s) → channel");
        if let Some(ref t) = ui_tx {
            let _ = t.send(UiMsg::Log(StructuredLog {
                stage: "recover".into(),
                source_id: 0,
                chunk_sec: n as f64,
                proc_sec: 0.0,
                detail: format!("{n} WAV queued for re-processing"),
                verbose_only: true,
            }));
        }
        for (path, source_id) in recovery {
            let duration_sec = wav_duration_sec(&path).unwrap_or(0.0);
            let seg_id = path
                .file_stem()
                .and_then(|x| x.to_str())
                .unwrap_or_default()
                .to_string();
            pending_segments.fetch_add(1, Ordering::Relaxed);
            let _ = seg_tx.send(SegmentReady {
                seg_id,
                payload: SegmentPayload::Disk(path),
                source_id,
                duration_sec,
                from_recovery: true,
            });
        }
    }

    let pipeline_cfg = PipelineConfig {
        max_chunk_sec: cli.max_chunk_sec,
        min_chunk_sec: cli.min_chunk_sec,
        vad_silence_sec: cli.vad_silence_sec,
        work_dir: work_dir.clone(),
        initial_seg_seq: [
            session::max_segment_seq_on_disk(&work_dir, 0),
            session::max_segment_seq_on_disk(&work_dir, 1),
        ],
        segments_to_disk: cli.segments_to_disk,
        mic_silence_warn_sec: cli.mic_silence_warn_sec,

        // THE ENGINE NO LONGER CREATES A SESSION ON STARTUP. Launching the daemon is not a
        // decision to record: capture runs, the pre-roll ring fills, but the disk stays clean
        // until a human presses "record" (or says so). The pipeline owns the session lifecycle
        // and creates the directory at that moment.
        session_chunks: !cli.no_session_chunks,
        chunk_sec: cli.chunk_sec,
        chunk_flac: cli.chunk_flac,
        ffmpeg: std::env::var("LOCALVOX_LIGHT_YT_FFMPEG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("ffmpeg")),
        preroll_sec: cli.preroll_sec,
        autostop_sec: cli.autostop_sec,
    };
    if cli.no_session_chunks {
        info!("Session chunks are disabled (--no-session-chunks)");
    } else {
        info!(
            "Ready to record: the pre-roll ring holds {:.0} s; nothing is written until you start",
            cli.preroll_sec
        );
    }

    // The detection channel: call detection → the pipeline (the sole writer of the meta). It
    // marks a running recording; it does not start one.
    let (session_tx, session_rx) = crossbeam_channel::unbounded::<crate::pipeline::SessionSignal>();

    // F3: automatic call detection (Windows) — a status in the UI + marks in the session meta.
    #[cfg(windows)]
    let detect_handle = {
        let off = matches!(
            std::env::var("LOCALVOX_LIGHT_CALL_DETECT")
                .unwrap_or_default()
                .to_lowercase()
                .as_str(),
            "0" | "off" | "false" | "no"
        );
        if !off {
            let ignore: Vec<String> = std::env::var("LOCALVOX_LIGHT_CALL_IGNORE")
                .map(|v| v.split(',').map(String::from).collect())
                .unwrap_or_default();
            crate::detect::spawn_call_detect(
                running.clone(),
                ui_tx.clone(),
                session_tx.clone(),
                ignore,
            )
        } else {
            None
        }
    };
    drop(session_tx); // the other Senders live in detect; we close our own end

    let pipeline_running = running.clone();
    let pipeline_record_pcm = Arc::clone(&record_pcm);
    let log_tx = ui_tx.clone();
    let pipeline_seg_tx = seg_tx.clone();
    let pipeline_pending = Arc::clone(&pending_segments);
    drop(seg_tx);
    let pipeline_handle = thread::Builder::new()
        .name("pipeline".into())
        .spawn(move || {
            crate::pipeline::run(
                pipeline_cfg,
                Some(session_rx),
                pcm_rx,
                pipeline_seg_tx,
                pipeline_pending,
                pipeline_running,
                pipeline_record_pcm,
                log_tx,
            )
        })?;

    let ui_load = ui_tx.clone();
    let load_stats_dir = work_dir.clone();
    thread::Builder::new()
        .name("vosk-load".into())
        .spawn(move || {
            if let Some(ref t) = ui_load {
                let _ = t.send(UiMsg::Log(StructuredLog {
                    stage: "load".into(),
                    source_id: 0,
                    chunk_sec: 0.0,
                    proc_sec: 0.0,
                    detail: model_path_buf.display().to_string(),
                    verbose_only: true,
                }));
            }
            // Load progress: libvosk gives no callbacks, so the percentage comes from the
            // duration of the previous load (stored in model-load.json).
            let expected = read_model_load_sec(&load_stats_dir);
            let t0 = Instant::now();
            let loading = Arc::new(AtomicBool::new(true));
            if let Some(ref t) = ui_load {
                let t = t.clone();
                let loading = Arc::clone(&loading);
                thread::Builder::new()
                    .name("vosk-load-progress".into())
                    .spawn(move || {
                        while loading.load(Ordering::Relaxed) {
                            let elapsed = t0.elapsed().as_secs_f64();
                            let msg = match expected {
                                Some(exp) if exp > 1.0 => format!(
                                    "Загрузка модели Vosk {} {elapsed:.0} с из ~{exp:.0} с",
                                    progress_bar(elapsed / exp, 20)
                                ),
                                _ => format!(
                                    // with no history — an asymptotic fill: the bar is alive
                                    // but promises no 100%
                                    "Загрузка модели Vosk {} {elapsed:.0} с (первый запуск)",
                                    progress_bar(1.0 - (-elapsed / 20.0).exp(), 20)
                                ),
                            };
                            let _ = t.send(UiMsg::Status(msg));
                            thread::sleep(Duration::from_millis(500));
                        }
                    })
                    .ok();
            }
            let r = crate::asr::vosk::VoskEngine::new(&model_path_buf).map_err(|e| e.to_string());
            loading.store(false, Ordering::Relaxed);
            let proc = t0.elapsed().as_secs_f64();
            if r.is_ok() {
                write_model_load_sec(&load_stats_dir, proc);
            }
            if let Some(ref t) = ui_load {
                match &r {
                    Ok(_) => {
                        let _ = t.send(UiMsg::Log(StructuredLog {
                            stage: "load".into(),
                            source_id: 0,
                            chunk_sec: 0.0,
                            proc_sec: proc,
                            detail: "Vosk ready".into(),
                            verbose_only: true,
                        }));
                        let _ = t.send(UiMsg::Status(format!(
                            "Запись — модель загружена за {proc:.0} с"
                        )));
                    }
                    Err(e) => {
                        let _ = t.send(UiMsg::Log(StructuredLog {
                            stage: "load".into(),
                            source_id: 0,
                            chunk_sec: 0.0,
                            proc_sec: proc,
                            detail: format!("FAILED: {e}"),
                            verbose_only: false,
                        }));
                    }
                }
            }
            let _ = engine_tx.send(r);
        })?;

    let asr_work_dir = work_dir.clone();
    let asr_running = running.clone();
    let ui_asr = ui_tx.clone();
    let asr_pending = Arc::clone(&pending_segments);
    let num_workers = cli.asr_workers;
    let asr_handle = thread::Builder::new()
        .name("asr-pool".into())
        .spawn(move || {
            asr_worker_pool(
                engine_rx,
                noise_gate,
                seg_rx,
                asr_work_dir,
                asr_running,
                reset_rx,
                ui_asr,
                num_workers,
                transcript_hook,
                asr_pending,
            );
        })?;

    {
        let cfg = audio_devices
            .read()
            .map_err(|e| anyhow::anyhow!("devices: the lock is poisoned: {e}"))?;
        let _ = audio::resolve_mic(&cfg.mic).map_err(|e| {
            let msg = format!("Микрофон: {e:#}");
            if let Some(ref t) = ui_tx {
                let _ = t.send(UiMsg::EngineFatal {
                    message: msg.clone(),
                });
            }
            eprintln!("localvox-light: {msg}");
            running.store(false, Ordering::SeqCst);
            e
        })?;
    }

    let mic_tx = pcm_tx.clone();
    let mic_running = running.clone();
    let mic_ui = ui_tx.clone();
    let devices_mic = Arc::clone(&audio_devices);
    let reload_mic = Arc::clone(&reload_gen);
    let mic_handle = thread::Builder::new()
        .name("mic-capture".into())
        .spawn(move || {
            // F9: the microphone priority list — the primary one + the fallbacks from env.
            let fallbacks: Vec<String> = std::env::var("LOCALVOX_LIGHT_MIC_FALLBACKS")
                .map(|v| {
                    v.split(',')
                        .map(|x| x.trim().to_string())
                        .filter(|x| !x.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            let mut ever_captured = false;
            let mut warned_fallback: Option<String> = None;
            let mut warned_output_clash = false;
            while mic_running.load(Ordering::Relaxed) {
                let cfg = match devices_mic.read() {
                    Ok(g) => g.clone(),
                    Err(_) => break,
                };
                let mut resolved: Option<(cpal::Device, bool)> = None;
                match audio::resolve_mic(&cfg.mic) {
                    Ok(d) => resolved = Some((d, false)),
                    Err(_) => {
                        for q in &fallbacks {
                            if let Ok(d) = audio::resolve_mic(q) {
                                resolved = Some((d, true));
                                break;
                            }
                        }
                    }
                }
                let Some((mic_device, via_fallback)) = resolved else {
                    let extras = if fallbacks.is_empty() {
                        String::new()
                    } else {
                        format!(" (запасные тоже: {})", fallbacks.join(", "))
                    };
                    let msg = format!("Микрофон недоступен: «{}»{extras}", cfg.mic);
                    if !ever_captured {
                        // a broken config at startup — an honest fatal error
                        if let Some(ref t) = mic_ui {
                            let _ = t.send(UiMsg::EngineFatal {
                                message: msg.clone(),
                            });
                        }
                        eprintln!("localvox-light: {msg}");
                        mic_running.store(false, Ordering::SeqCst);
                        break;
                    }
                    // the device vanished mid-run — we wait for it to come back, we do not drop
                    // the recording
                    if let Some(ref t) = mic_ui {
                        let _ = t.send(UiMsg::Status(format!("⚠ {msg} — жду устройство")));
                    }
                    thread::sleep(Duration::from_secs(3));
                    continue;
                };
                let mic_name = mic_device
                    .description()
                    .map(|d| d.name().to_string())
                    .unwrap_or_else(|_| "unknown".into());
                if via_fallback {
                    if warned_fallback.as_deref() != Some(mic_name.as_str()) {
                        warned_fallback = Some(mic_name.clone());
                        let m = format!(
                            "⚠ микрофон «{}» недоступен — использую запасной: {mic_name}",
                            cfg.mic
                        );
                        tracing::warn!("{m}");
                        if let Some(ref t) = mic_ui {
                            let _ = t.send(UiMsg::Status(m));
                        }
                    }
                } else {
                    warned_fallback = None;
                }
                // sanity (the rake stepped on in Summit 1.9.1): microphone == the output device
                // means almost certainly the wrong device was picked
                if !warned_output_clash {
                    let out_name = cpal::default_host()
                        .default_output_device()
                        .and_then(|d| d.description().ok().map(|x| x.name().to_string()));
                    if out_name.as_deref() == Some(mic_name.as_str()) {
                        warned_output_clash = true;
                        let m = format!(
                            "⚠ выбранный микрофон совпадает с устройством вывода ({mic_name}) — проверьте выбор (F2)"
                        );
                        tracing::warn!("{m}");
                        if let Some(ref t) = mic_ui {
                            let _ = t.send(UiMsg::Status(m));
                        }
                    }
                }
                info!("Mic: {mic_name}");
                ever_captured = true;
                let g = reload_mic.load(Ordering::SeqCst);
                if let Err(e) = audio::mic_capture(
                    mic_device,
                    0,
                    mic_tx.clone(),
                    mic_running.clone(),
                    mic_ui.clone(),
                    Some(Arc::clone(&reload_mic)),
                    g,
                ) {
                    tracing::error!("Mic capture error: {e}");
                    eprintln!("localvox-light: microphone — capture error: {e:#}");
                    thread::sleep(Duration::from_millis(400));
                }
                if !mic_running.load(Ordering::Relaxed) {
                    break;
                }
            }
        })?;

    let lb_tx = pcm_tx.clone();
    let lb_running = running.clone();
    let lb_ui = ui_tx.clone();
    let devices_lb = Arc::clone(&audio_devices);
    let reload_lb = Arc::clone(&reload_gen);
    let loopback_handle = thread::Builder::new()
        .name("loopback-capture".into())
        .spawn(move || {
            let mut lb_fail_streak: u32 = 0;
            while lb_running.load(Ordering::Relaxed) {
                let cfg = match devices_lb.read() {
                    Ok(g) => g.clone(),
                    Err(_) => break,
                };
                if !cfg.loopback {
                    lb_fail_streak = 0;
                    let base = reload_lb.load(Ordering::SeqCst);
                    while lb_running.load(Ordering::Relaxed)
                        && reload_lb.load(Ordering::SeqCst) == base
                    {
                        thread::sleep(Duration::from_millis(100));
                    }
                    continue;
                }
                let q = cfg.loopback_device.clone();
                let g = reload_lb.load(Ordering::SeqCst);
                match audio::loopback_capture(
                    &q,
                    lb_tx.clone(),
                    lb_running.clone(),
                    lb_ui.clone(),
                    Some(Arc::clone(&reload_lb)),
                    g,
                ) {
                    Ok(()) => lb_fail_streak = 0,
                    Err(e) => {
                        let es = format!("{e:#}");
                        if lb_fail_streak == 0 {
                            tracing::warn!("Loopback capture error: {es}");
                            eprintln!("localvox-light: loopback — capture error: {es}");
                            if let Some(ref t) = lb_ui {
                                let _ = t.send(UiMsg::Status(
                                    "Loopback: ошибка захвата (повторы без спама в stderr). F2 — устройство или выключите loopback; --no-loopback"
                                        .into(),
                                ));
                            }
                        } else if lb_fail_streak == 1 || lb_fail_streak.is_power_of_two() {
                            tracing::debug!(target: "localvox_light_core::loopback", "loopback capture still failing: {es}");
                        }
                        lb_fail_streak = lb_fail_streak.saturating_add(1);
                        let ms = (600u64)
                            .saturating_mul(lb_fail_streak as u64)
                            .min(12_000)
                            .max(500);
                        thread::sleep(Duration::from_millis(ms));
                    }
                }
                if !lb_running.load(Ordering::Relaxed) {
                    break;
                }
            }
        })?;

    // FOLLOW THE DEFAULT DEVICE.
    //
    // The capture streams bind to a device ONCE, at start. Set to "default" they remember whatever
    // was default AT THAT MOMENT — and then you join a meeting, a headset connects, Windows moves
    // the default, the call app follows it, and we keep listening to the old, now-idle endpoint.
    // Loopback sends no packets when nothing plays there, so we pad with silence and record
    // nothing, silently. Measured on the owner's recording (16.07.2026): the other side audible
    // for 2 minutes, then 27 minutes of silence on a full-length track — and it had happened
    // before, every time he joined a call.
    //
    // The restart machinery already existed (`reload_gen` — the device screen switches sources
    // live with it). What was missing was someone to NOTICE. This is that someone: it watches the
    // system default and bumps the counter when it moves; the supervisors above re-open their
    // streams on the new device by themselves. No restart, no re-plugging, no human.
    //
    // Only for sources set to "default". A pinned device is the human's decision.
    let watch_running = running.clone();
    let watch_devices = Arc::clone(&audio_devices);
    let watch_reload = Arc::clone(&reload_gen);
    let watch_ui = ui_tx.clone();
    let device_watch = thread::Builder::new()
        .name("device-watch".into())
        .spawn(move || {
            // The FIRST poll is the baseline, not a change: we have just bound to these.
            let mut last_render = crate::audio::default_render_id();
            let mut last_capture = crate::audio::default_capture_id();
            while watch_running.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_secs(2));
                if !watch_running.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(cfg) = watch_devices.read().map(|g| g.clone()) else {
                    break;
                };
                let mut moved: Option<String> = None;

                if cfg.loopback && crate::audio::follows_default(&cfg.loopback_device) {
                    let now = crate::audio::default_render_id();
                    // `None` means "could not ask" — not "the device is gone". Restarting on a
                    // failed query would fight the OS during its own hiccup.
                    if now.is_some() && now != last_render {
                        moved = Some("системный звук".into());
                        last_render = now;
                    }
                }
                if crate::audio::follows_default(&cfg.mic) {
                    let now = crate::audio::default_capture_id();
                    if now.is_some() && now != last_capture {
                        moved = Some(match moved {
                            Some(prev) => format!("{prev} и микрофон"),
                            None => "микрофон".into(),
                        });
                        last_capture = now;
                    }
                }

                if let Some(what) = moved {
                    // Loud on purpose: the sound the person expects to be recorded has just moved
                    // to another device. They must be able to learn this from the log, not from a
                    // silent recording two days later.
                    tracing::warn!(
                        "устройство по умолчанию сменилось ({what}) — перезапускаю захват на новом"
                    );
                    if let Some(ref t) = watch_ui {
                        let _ = t.send(UiMsg::Status(format!(
                            "Устройство сменилось ({what}) — перехожу на новое"
                        )));
                    }
                    watch_reload.fetch_add(1, Ordering::SeqCst);
                }
            }
        })?;

    drop(pcm_tx);
    info!("Recording (WAV → disk); Vosk loads in parallel. Ctrl+C to stop.");
    if let Some(ref t) = ui_tx {
        let _ = t.send(UiMsg::Status(
            "Recording — WAV на диск, модель грузится…".into(),
        ));
    }

    // The pipeline first: drain pcm before all the Senders in mic/loopback are released
    // (otherwise a bounded send in the cpal callback hangs).
    pipeline_handle.join().ok();
    mic_handle.join().ok();
    loopback_handle.join().ok();
    device_watch.join().ok();
    asr_handle.join().ok();
    // detection closes an open meeting in the meta on exit — wait for it
    #[cfg(windows)]
    if let Some(h) = detect_handle {
        h.join().ok();
    }

    info!("Workspace saved: {}", work_dir.display());
    Ok(())
}

/// The pool coordinator: waits for the model, opens the TranscriptWriter, starts N workers.
/// The recovery files are already in the channel (sent in run_engine before the pipeline started).
#[allow(clippy::too_many_arguments)]
fn asr_worker_pool(
    engine_rx: crossbeam_channel::Receiver<Result<crate::asr::vosk::VoskEngine, String>>,
    noise_gate: f32,
    seg_rx: crossbeam_channel::Receiver<SegmentReady>,
    work_dir: PathBuf,
    running: Arc<AtomicBool>,
    reset_rx: crossbeam_channel::Receiver<()>,
    ui_tx: Option<Sender<UiMsg>>,
    num_workers: usize,
    transcript_hook: Option<crate::events::TranscriptHook>,
    pending: Arc<AtomicUsize>,
) {
    let engine = match engine_rx.recv() {
        Ok(Ok(e)) => e,
        Ok(Err(e)) => {
            tracing::error!("Vosk model failed to load: {e}");
            let msg = format!("Модель Vosk: {e}");
            if let Some(ref t) = ui_tx {
                let _ = t.send(UiMsg::EngineFatal {
                    message: msg.clone(),
                });
            }
            eprintln!("localvox-light: {msg}");
            ui_log(
                ui_tx.as_ref(),
                StructuredLog {
                    stage: "load".into(),
                    source_id: 0,
                    chunk_sec: 0.0,
                    proc_sec: 0.0,
                    detail: format!("abort: {e}"),
                    verbose_only: false,
                },
            );
            running.store(false, Ordering::SeqCst);
            return;
        }
        Err(_) => {
            tracing::error!("Vosk load channel closed");
            let msg = "Загрузка модели: канал закрыт до ответа".to_string();
            if let Some(ref t) = ui_tx {
                let _ = t.send(UiMsg::EngineFatal {
                    message: msg.clone(),
                });
            }
            eprintln!("localvox-light: {msg}");
            running.store(false, Ordering::SeqCst);
            return;
        }
    };

    let tw = match TranscriptWriter::open(&work_dir) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("Failed to open transcript: {e}");
            let msg = format!("transcript.jsonl: {e:#}");
            if let Some(ref t) = ui_tx {
                let _ = t.send(UiMsg::EngineFatal {
                    message: msg.clone(),
                });
            }
            eprintln!("localvox-light: {msg}");
            running.store(false, Ordering::SeqCst);
            return;
        }
    };

    let engine = Arc::new(engine);
    let tw = Arc::new(Mutex::new(tw));
    let num = num_workers.max(1);
    debug!("Starting {num} ASR worker thread(s)");

    // Sharding by source: every segment of one source_id goes to one worker strictly in order —
    // the voice hook and the transcript see the phrases in the order they were spoken (a shared
    // MPMC channel let two workers swap phrases around: a short segment overtook a long one).
    // mic and loopback are still parallel.
    let mut worker_txs: Vec<crossbeam_channel::Sender<SegmentReady>> = Vec::with_capacity(num);
    let mut worker_rxs = Vec::with_capacity(num);
    for _ in 0..num {
        let (tx, rx) = crossbeam_channel::unbounded::<SegmentReady>();
        worker_txs.push(tx);
        worker_rxs.push(rx);
    }
    thread::Builder::new()
        .name("asr-router".into())
        .spawn(move || {
            while let Ok(seg) = seg_rx.recv() {
                let idx = seg.source_id as usize % worker_txs.len();
                if worker_txs[idx].send(seg).is_err() {
                    break;
                }
            }
            // the txs get dropped — the workers will finish once they drain their queues
        })
        .expect("spawn asr-router");

    let mut handles = Vec::with_capacity(num);
    let mut reset_rx_slot = Some(reset_rx);

    for (i, seg_rx) in worker_rxs.into_iter().enumerate() {
        let engine = Arc::clone(&engine);
        let tw = Arc::clone(&tw);
        let running = Arc::clone(&running);
        let ui_tx = ui_tx.clone();
        let work_dir = work_dir.clone();
        let worker_reset_rx = if i == 0 { reset_rx_slot.take() } else { None };
        let hook = transcript_hook.clone();
        let pending = Arc::clone(&pending);

        let h = thread::Builder::new()
            .name(format!("asr-{i}"))
            .spawn(move || {
                asr_thread_loop(
                    i,
                    engine,
                    seg_rx,
                    tw,
                    running,
                    ui_tx,
                    work_dir,
                    noise_gate,
                    worker_reset_rx,
                    hook,
                    pending,
                );
            })
            .expect("spawn asr worker");
        handles.push(h);
    }

    for h in handles {
        h.join().ok();
    }
    debug!("ASR worker pool stopped ({num} threads)");
}

#[allow(clippy::too_many_arguments)]
fn asr_thread_loop(
    id: usize,
    engine: Arc<crate::asr::vosk::VoskEngine>,
    seg_rx: crossbeam_channel::Receiver<SegmentReady>,
    tw: Arc<Mutex<TranscriptWriter>>,
    running: Arc<AtomicBool>,
    ui_tx: Option<Sender<UiMsg>>,
    work_dir: PathBuf,
    noise_gate: f32,
    reset_rx: Option<crossbeam_channel::Receiver<()>>,
    transcript_hook: Option<crate::events::TranscriptHook>,
    pending: Arc<AtomicUsize>,
) {
    loop {
        if let Some(ref rx) = reset_rx {
            let mut do_reset = false;
            while rx.try_recv().is_ok() {
                do_reset = true;
            }
            if do_reset {
                match TranscriptWriter::reopen_truncated(&work_dir) {
                    Ok(n) => {
                        *tw.lock().unwrap() = n;
                        debug!("transcript.jsonl cleared (a fresh sheet)");
                    }
                    Err(e) => tracing::error!("Failed to truncate transcript: {e}"),
                }
                if let Some(ref t) = ui_tx {
                    let _ = t.send(UiMsg::ClearTranscript);
                    let _ = t.send(UiMsg::Status(
                        "Запись — транскрипт обнулён (x), продолжаем".into(),
                    ));
                }
            }
        }

        match seg_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(seg) => {
                // the queue counter goes down on any outcome of the processing
                let _dec = DecOnDrop(&pending);
                if let SegmentPayload::Disk(ref p) = seg.payload {
                    if !p.exists() {
                        continue;
                    }
                }
                // Shutdown: we do not start a heavy ASR. The disk will be picked up by recover;
                // a RAM draft simply disappears — the slow lane will cook it from the chunks.
                if !running.load(Ordering::Relaxed) {
                    continue;
                }
                if let Some(entry) =
                    process_segment(&*engine, &seg, noise_gate, ui_tx.as_ref(), &running)
                {
                    if let Some(ref t) = ui_tx {
                        if t.send(UiMsg::Transcript {
                            source_id: entry.source_id,
                            text: entry.text.clone(),
                            time: None,
                        })
                        .is_err()
                        {
                            tracing::warn!("TUI channel closed; the line goes only to transcript.jsonl");
                        }
                    }
                    // the guard is released before remove_file and the hook — otherwise the whole
                    // pool stands on the Mutex while (for example) the antivirus scans the WAV
                    let appended = tw.lock().unwrap().append(&entry);
                    match appended {
                        Ok(()) => {
                            if let SegmentPayload::Disk(ref p) = seg.payload {
                                let _ = std::fs::remove_file(p);
                            }
                            // we do not hand recovery segments from a previous run to the voice
                            // hook: yesterday's «запиши…» must not be executed all over again
                            if !seg.from_recovery {
                                if let Some(ref h) = transcript_hook {
                                    h(entry.source_id, &entry.text);
                                }
                            }
                        }
                        Err(e) => tracing::error!("Failed to write transcript: {e}"),
                    }
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                if !running.load(Ordering::Relaxed) && seg_rx.is_empty() {
                    break;
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }
    debug!("asr-{id} stopped");
}

/// A textual fill bar for the status line: `[██████░░░░░░]`.
fn progress_bar(frac: f64, width: usize) -> String {
    let filled = (frac.clamp(0.0, 0.99) * width as f64).round() as usize;
    format!("[{}{}]", "█".repeat(filled), "░".repeat(width - filled))
}

/// The duration of the previous model load (sec) — for the progress bar.
fn read_model_load_sec(work_dir: &Path) -> Option<f64> {
    let text = std::fs::read_to_string(work_dir.join("model-load.json")).ok()?;
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()?
        .get("vosk_load_sec")?
        .as_f64()
        .filter(|s| *s > 0.0)
}

fn write_model_load_sec(work_dir: &Path, sec: f64) {
    let body = serde_json::json!({ "vosk_load_sec": sec });
    let _ = std::fs::write(work_dir.join("model-load.json"), body.to_string());
}

/// Decrement of the queue counter on leaving the scope — this covers `continue` as well.
struct DecOnDrop<'a>(&'a AtomicUsize);
impl Drop for DecOnDrop<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

fn format_entry_time_local(rfc: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(rfc.trim())
        .map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|_| chrono::Local::now().format("%H:%M:%S").to_string())
}

fn ui_log(tx: Option<&Sender<UiMsg>>, log: StructuredLog) {
    if let Some(t) = tx {
        let _ = t.send(UiMsg::Log(log));
    }
}

fn wav_duration_sec(path: &Path) -> Option<f64> {
    let r = WavReader::open(path).ok()?;
    let spec = r.spec();
    let n = r.len();
    if spec.sample_rate == 0 {
        return None;
    }
    Some(n as f64 / spec.sample_rate as f64)
}

fn process_segment(
    engine: &impl AsrEngine,
    seg: &SegmentReady,
    noise_gate: f32,
    ui_tx: Option<&Sender<UiMsg>>,
    running: &AtomicBool,
) -> Option<TranscriptEntry> {
    let source_id = seg.source_id;
    let pipeline_chunk_sec = Some(seg.duration_sec);
    let samples: Vec<f32> = match &seg.payload {
        SegmentPayload::Ram(raw) => raw.iter().map(|&v| f32::from(v) / 32768.0).collect(),
        SegmentPayload::Disk(path) => match wav_to_f32(path) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Failed to read {}: {e}", path.display());
                return None;
            }
        },
    };
    if samples.is_empty() {
        discard_payload(seg);
        return None;
    }

    let duration_sec = samples.len() as f64 / 16000.0;

    let samples = if noise_gate > 0.0 {
        let t_gate = Instant::now();
        let ratio = speech_ratio(&samples);
        if ratio == 0.0 {
            debug!(
                "src{}: {:.1}s -> (noise, 0% speech, skipped)",
                source_id, duration_sec
            );
            discard_payload(seg);
            return None;
        }
        if ratio < noise_gate {
            match trim_to_speech(&samples, 300) {
                Some(trimmed) => {
                    let proc = t_gate.elapsed().as_secs_f64();
                    let new_dur = trimmed.len() as f64 / 16000.0;
                    debug!(
                        "src{}: {:.1}s -> trimmed to {:.1}s (speech {:.0}%)",
                        source_id,
                        duration_sec,
                        new_dur,
                        ratio * 100.0
                    );
                    ui_log(
                        ui_tx,
                        StructuredLog {
                            stage: "gate".into(),
                            source_id,
                            chunk_sec: duration_sec,
                            proc_sec: proc,
                            detail: format!("trim → {:.1}s speech {:.0}%", new_dur, ratio * 100.0),
                            verbose_only: true,
                        },
                    );
                    trimmed
                }
                None => {
                    debug!(
                        "src{}: {:.1}s -> gate trim → empty (silence) [{:.3}s]",
                        source_id,
                        duration_sec,
                        t_gate.elapsed().as_secs_f64()
                    );
                    discard_payload(seg);
                    return None;
                }
            }
        } else {
            let proc = t_gate.elapsed().as_secs_f64();
            ui_log(
                ui_tx,
                StructuredLog {
                    stage: "gate".into(),
                    source_id,
                    chunk_sec: duration_sec,
                    proc_sec: proc,
                    detail: format!("pass speech {:.0}%", ratio * 100.0),
                    verbose_only: true,
                },
            );
            samples
        }
    } else {
        samples
    };

    let final_dur = samples.len() as f64 / 16000.0;
    if !running.load(Ordering::Relaxed) {
        return None;
    }
    let t0 = Instant::now();
    let text = match engine.transcribe(&samples) {
        Ok(t) => t,
        Err(e) => {
            let proc = t0.elapsed().as_secs_f64();
            tracing::warn!("ASR error for {}: {e}", seg.seg_id);
            ui_log(
                ui_tx,
                StructuredLog {
                    stage: "asr".into(),
                    source_id,
                    chunk_sec: final_dur,
                    proc_sec: proc,
                    detail: format!("ERROR: {e}"),
                    verbose_only: false,
                },
            );
            return None;
        }
    };
    let elapsed = t0.elapsed().as_secs_f64();

    let pipe_hint = pipeline_chunk_sec
        .map(|s| format!(" pipeline_seg={s:.2}s"))
        .unwrap_or_default();

    if text.is_empty() {
        debug!(
            "src{}: {:.1}s -> (no speech) [{:.3}s]{}",
            source_id, final_dur, elapsed, pipe_hint
        );
        discard_payload(seg);
        return None;
    }

    let display_text = if text.chars().count() > 80 {
        let truncated: String = text.chars().take(77).collect();
        format!("{truncated}...")
    } else {
        text.clone()
    };
    debug!(
        "src{}: {:.1}s -> {} [{:.1}s]",
        source_id, final_dur, display_text, elapsed
    );
    ui_log(
        ui_tx,
        StructuredLog {
            stage: "asr".into(),
            source_id,
            chunk_sec: final_dur,
            proc_sec: elapsed,
            detail: format!("{display_text}{pipe_hint}"),
            verbose_only: true,
        },
    );

    Some(TranscriptEntry {
        seg_id: seg.seg_id.clone(),
        source_id,
        text,
        duration_sec: final_dur,
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
}

/// Rejecting a segment: RAM — just drop the buffer; disk — delete the WAV.
fn discard_payload(seg: &SegmentReady) {
    if let SegmentPayload::Disk(ref p) = seg.payload {
        let _ = std::fs::remove_file(p);
    }
}

fn wav_to_f32(path: &Path) -> Result<Vec<f32>> {
    let reader = WavReader::open(path)?;
    let spec = reader.spec();
    if spec.channels != 1 || spec.bits_per_sample != 16 {
        anyhow::bail!(
            "expected 16-bit mono WAV, got {}ch {}bit",
            spec.channels,
            spec.bits_per_sample
        );
    }
    let samples: Vec<f32> = reader
        .into_samples::<i16>()
        .filter_map(Result::ok)
        .map(|s| s as f32 / 32768.0)
        .collect();
    Ok(samples)
}

#[cfg(test)]
mod tests {
    use super::wav_duration_sec;
    use hound::{SampleFormat, WavSpec, WavWriter};
    use tempfile::tempdir;

    #[test]
    fn wav_duration_sec_mono_16k() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.wav");
        let spec = WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut w = WavWriter::create(&path, spec).unwrap();
        for _ in 0..8000 {
            w.write_sample(0i16).unwrap();
        }
        w.finalize().unwrap();
        let d = wav_duration_sec(&path).expect("duration");
        assert!((d - 0.5).abs() < 1e-6);
    }
}
