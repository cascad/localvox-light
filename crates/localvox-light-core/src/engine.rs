//! Захват → пайплайн → Vosk ASR → `transcript.jsonl`. Общая логика для TUI и headless.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossbeam_channel::Sender;
use hound::WavReader;
use tracing::{debug, info};
use cpal::traits::DeviceTrait;

use crate::asr::{speech_ratio, trim_to_speech, AsrEngine};
use crate::audio;
use crate::cli::{normalized_model_path, Cli};
use crate::events::{StructuredLog, UiMsg};
use crate::light_config::LightDeviceConfig;
use crate::pipeline::{PipelineConfig, SegmentReady};
use crate::session;
use crate::transcript::{TranscriptEntry, TranscriptWriter};

pub fn run_engine(
    cli: Cli,
    audio_devices: Arc<RwLock<LightDeviceConfig>>,
    ui_tx: Option<Sender<UiMsg>>,
    reset_rx: crossbeam_channel::Receiver<()>,
    running: Arc<AtomicBool>,
    record_pcm: Arc<AtomicBool>,
    reload_gen: Arc<AtomicU64>,
) -> Result<()> {
    let work_dir = PathBuf::from(&cli.audio_dir);
    std::fs::create_dir_all(&work_dir)?;
    info!("Workspace: {}", work_dir.display());

    session::remove_orphan_part_files(&work_dir);

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
        thread::Builder::new()
            .name("queue-stats".into())
            .spawn(move || {
                while run_stats.load(Ordering::Relaxed) {
                    let (n, umb, smb) = session::workspace_queue_stats(&sd_stats);
                    let _ = tx_stats.send(UiMsg::QueuePending {
                        unprocessed_wavs: n,
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
                format!("TUI ← transcript.jsonl ({n} строк)")
            } else {
                "TUI ← transcript.jsonl (пусто)".into()
            },
            verbose_only: true,
        }));
    }

    let model_path_buf = PathBuf::from(normalized_model_path(&cli));
    let noise_gate = cli.noise_gate;

    let (pcm_tx, pcm_rx) = crossbeam_channel::bounded::<audio::PcmChunk>(1024);
    let (seg_tx, seg_rx) = crossbeam_channel::unbounded::<SegmentReady>();
    let (engine_tx, engine_rx) =
        crossbeam_channel::bounded::<Result<crate::asr::vosk::VoskEngine, String>>(1);

    // Recovery: все необработанные WAV с диска → в канал ДО пайплайна (отсортированные по src/seq).
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
            let _ = seg_tx.send(SegmentReady {
                path,
                source_id,
                duration_sec,
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
    };
    let pipeline_running = running.clone();
    let pipeline_record_pcm = Arc::clone(&record_pcm);
    let log_tx = ui_tx.clone();
    let pipeline_seg_tx = seg_tx.clone();
    drop(seg_tx);
    let pipeline_handle = thread::Builder::new()
        .name("pipeline".into())
        .spawn(move || {
            crate::pipeline::run(
                pipeline_cfg,
                pcm_rx,
                pipeline_seg_tx,
                pipeline_running,
                pipeline_record_pcm,
                log_tx,
            )
        })?;

    let ui_load = ui_tx.clone();
    thread::Builder::new()
        .name("vosk-load".into())
        .spawn(move || {
            if let Some(ref t) = ui_load {
                let _ = t.send(UiMsg::Status("Loading Vosk model…".into()));
                let _ = t.send(UiMsg::Log(StructuredLog {
                    stage: "load".into(),
                    source_id: 0,
                    chunk_sec: 0.0,
                    proc_sec: 0.0,
                    detail: model_path_buf.display().to_string(),
                    verbose_only: true,
                }));
            }
            let t0 = Instant::now();
            let r = crate::asr::vosk::VoskEngine::new(&model_path_buf).map_err(|e| e.to_string());
            let proc = t0.elapsed().as_secs_f64();
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
                        let _ = t.send(UiMsg::Status("Recording — Vosk ready".into()));
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
            );
        })?;

    {
        let cfg = audio_devices
            .read()
            .map_err(|e| anyhow::anyhow!("устройства: блокировка повреждена: {e}"))?;
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
            while mic_running.load(Ordering::Relaxed) {
                let cfg = match devices_mic.read() {
                    Ok(g) => g.clone(),
                    Err(_) => break,
                };
                let mic_device = match audio::resolve_mic(&cfg.mic) {
                    Ok(d) => d,
                    Err(e) => {
                        let msg = format!("Микрофон: {e:#}");
                        if let Some(ref t) = mic_ui {
                            let _ = t.send(UiMsg::EngineFatal {
                                message: msg.clone(),
                            });
                        }
                        eprintln!("localvox-light: {msg}");
                        mic_running.store(false, Ordering::SeqCst);
                        break;
                    }
                };
                let mic_name = mic_device
                    .description()
                    .map(|d| d.name().to_string())
                    .unwrap_or_else(|_| "unknown".into());
                info!("Mic: {mic_name}");
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
                    eprintln!("localvox-light: микрофон — ошибка захвата: {e:#}");
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
            while lb_running.load(Ordering::Relaxed) {
                let cfg = match devices_lb.read() {
                    Ok(g) => g.clone(),
                    Err(_) => break,
                };
                if !cfg.loopback {
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
                if let Err(e) = audio::loopback_capture(
                    &q,
                    lb_tx.clone(),
                    lb_running.clone(),
                    lb_ui.clone(),
                    Some(Arc::clone(&reload_lb)),
                    g,
                ) {
                    tracing::error!("Loopback capture error: {e}");
                    eprintln!("localvox-light: loopback — ошибка захвата: {e:#}");
                    thread::sleep(Duration::from_millis(400));
                }
                if !lb_running.load(Ordering::Relaxed) {
                    break;
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

    // Сначала pipeline: дренаж pcm до отпускания всех Sender в mic/loopback (иначе bounded send в колбэке cpal зависает).
    pipeline_handle.join().ok();
    mic_handle.join().ok();
    loopback_handle.join().ok();
    asr_handle.join().ok();

    info!("Workspace saved: {}", work_dir.display());
    Ok(())
}

/// Координатор пула: ждёт модель, открывает TranscriptWriter, запускает N воркеров.
/// Recovery-файлы уже в канале (отправлены в run_engine до старта пайплайна).
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

    let mut handles = Vec::with_capacity(num);
    let mut reset_rx_slot = Some(reset_rx);

    for i in 0..num {
        let engine = Arc::clone(&engine);
        let seg_rx = seg_rx.clone();
        let tw = Arc::clone(&tw);
        let running = Arc::clone(&running);
        let ui_tx = ui_tx.clone();
        let work_dir = work_dir.clone();
        let worker_reset_rx = if i == 0 { reset_rx_slot.take() } else { None };

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
                        debug!("transcript.jsonl cleared (новый лист)");
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
                if !seg.path.exists() {
                    continue;
                }
                // Остановка: не стартуем новый тяжёлый ASR — WAV на диске, подхватит recover.
                if !running.load(Ordering::Relaxed) {
                    continue;
                }
                if let Some(entry) = process_segment(
                    &*engine,
                    &seg.path,
                    seg.source_id,
                    noise_gate,
                    ui_tx.as_ref(),
                    Some(seg.duration_sec),
                    &running,
                ) {
                    if let Some(ref t) = ui_tx {
                        if t
                            .send(UiMsg::Transcript {
                                source_id: entry.source_id,
                                text: entry.text.clone(),
                                time: None,
                            })
                            .is_err()
                        {
                            tracing::warn!("TUI channel closed; строка только в transcript.jsonl");
                        }
                    }
                    match tw.lock().unwrap().append(&entry) {
                        Ok(()) => {
                            let _ = std::fs::remove_file(&seg.path);
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

fn format_entry_time_local(rfc: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(rfc.trim())
        .map(|dt| dt.with_timezone(&chrono::Local).format("%H:%M:%S").to_string())
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
    path: &Path,
    source_id: u8,
    noise_gate: f32,
    ui_tx: Option<&Sender<UiMsg>>,
    pipeline_chunk_sec: Option<f64>,
    running: &AtomicBool,
) -> Option<TranscriptEntry> {
    let samples = match wav_to_f32(path) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Failed to read {}: {e}", path.display());
            return None;
        }
    };
    if samples.is_empty() {
        let _ = std::fs::remove_file(path);
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
            let _ = std::fs::remove_file(path);
            return None;
        }
        if ratio < noise_gate {
            match trim_to_speech(&samples, 300) {
                Some(trimmed) => {
                    let proc = t_gate.elapsed().as_secs_f64();
                    let new_dur = trimmed.len() as f64 / 16000.0;
                    debug!(
                        "src{}: {:.1}s -> trimmed to {:.1}s (speech {:.0}%)",
                        source_id, duration_sec, new_dur, ratio * 100.0
                    );
                    ui_log(
                        ui_tx,
                        StructuredLog {
                            stage: "gate".into(),
                            source_id,
                            chunk_sec: duration_sec,
                            proc_sec: proc,
                            detail: format!(
                                "trim → {:.1}s speech {:.0}%",
                                new_dur,
                                ratio * 100.0
                            ),
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
                    let _ = std::fs::remove_file(path);
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
            tracing::warn!("ASR error for {}: {e}", path.display());
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
            source_id,
            final_dur,
            elapsed,
            pipe_hint
        );
        let _ = std::fs::remove_file(path);
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

    let seg_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    Some(TranscriptEntry {
        seg_id,
        source_id,
        text,
        duration_sec: final_dur,
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
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
