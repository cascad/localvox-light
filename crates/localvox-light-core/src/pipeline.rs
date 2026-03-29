//! VAD-based segmentation: receives PCM chunks, writes WAV segments to disk,
//! notifies the ASR worker when a segment is ready.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crossbeam_channel::{Receiver, Sender as CrossbeamSender};
use tracing::debug;

use crate::events::{StructuredLog, UiMsg};
use hound::{SampleFormat, WavSpec, WavWriter};
use webrtc_vad::{SampleRate, Vad, VadMode};

use crate::audio::PcmChunk;

const FRAME_SAMPLES: usize = 320; // 20 ms at 16 kHz
const FRAME_BYTES: usize = FRAME_SAMPLES * 2;

/// Уведомление о готовом сегменте; упорядоченный ASR читает путь с диска, поля для отладки/будущего.
#[allow(dead_code)]
pub struct SegmentReady {
    pub path: PathBuf,
    pub source_id: u8,
    /// Длительность записанного сегмента (сек), 16 kHz mono.
    pub duration_sec: f64,
}

pub struct PipelineConfig {
    pub max_chunk_sec: f64,
    pub min_chunk_sec: f64,
    pub vad_silence_sec: f64,
    pub work_dir: PathBuf,
    /// После `open_new` первый новый файл будет `max(существующие seq)+1` для каждого источника.
    pub initial_seg_seq: [u32; 2],
}

struct SourceState {
    source_id: u8,
    vad: Vad,
    seq: u32,
    duration_sec: f64,
    silence_frames: u32,
    silence_threshold_frames: u32,
    writer: Option<WavWriter<File>>,
    part_path: Option<PathBuf>,
    pcm_remainder: Vec<u8>,
}

impl SourceState {
    /// `last_seq_on_disk` — максимальный уже существующий номер сегмента; следующий будет +1.
    fn new(source_id: u8, silence_threshold_frames: u32, last_seq_on_disk: u32) -> Self {
        let mut vad = Vad::new_with_rate_and_mode(SampleRate::Rate16kHz, VadMode::LowBitrate);
        vad.set_sample_rate(SampleRate::Rate16kHz);
        Self {
            source_id,
            vad,
            seq: last_seq_on_disk,
            duration_sec: 0.0,
            silence_frames: 0,
            silence_threshold_frames,
            writer: None,
            part_path: None,
            pcm_remainder: Vec::new(),
        }
    }

    fn feed(
        &mut self,
        samples: &[i16],
        cfg: &PipelineConfig,
    ) -> Vec<(PathBuf, f64, f64)> {
        let pcm_bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        self.pcm_remainder.extend_from_slice(&pcm_bytes);

        let chunk_sec = samples.len() as f64 / 16000.0;
        self.duration_sec += chunk_sec;

        let (_any_speech, should_flush) = self.run_vad();
        let flush_vad = should_flush
            && self.duration_sec >= cfg.min_chunk_sec
            && self.writer.is_some();
        let flush_time = self.duration_sec >= cfg.max_chunk_sec && self.writer.is_some();

        let mut completed = Vec::new();

        if flush_vad || flush_time {
            let seg_dur = self.duration_sec;
            self.write_samples(samples);
            let t_write = Instant::now();
            if let Some(p) = self.finalize(&cfg.work_dir) {
                let write_sec = t_write.elapsed().as_secs_f64();
                completed.push((p, seg_dur, write_sec));
            }
            self.duration_sec = 0.0;
            self.silence_frames = 0;
            self.open_new(&cfg.work_dir);
        } else {
            if self.writer.is_none() {
                self.open_new(&cfg.work_dir);
            }
            self.write_samples(samples);
        }
        completed
    }

    fn run_vad(&mut self) -> (bool, bool) {
        let mut any_speech = false;
        while self.pcm_remainder.len() >= FRAME_BYTES {
            let frame: Vec<u8> = self.pcm_remainder.drain(..FRAME_BYTES).collect();
            let i16_samples: Vec<i16> = frame
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect();
            let is_speech = self.vad.is_voice_segment(&i16_samples).unwrap_or(true);
            if is_speech {
                any_speech = true;
                self.silence_frames = 0;
            } else {
                self.silence_frames += 1;
            }
        }
        let should_flush = self.silence_frames >= self.silence_threshold_frames;
        (any_speech, should_flush)
    }

    fn open_new(&mut self, work_dir: &Path) {
        self.seq += 1;
        let path = work_dir.join(format!("src{}_{:06}.part", self.source_id, self.seq));
        let spec = WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        match File::create(&path).and_then(|f| {
            WavWriter::new(f, spec).map_err(std::io::Error::other)
        }) {
            Ok(w) => {
                self.writer = Some(w);
                self.part_path = Some(path);
            }
            Err(e) => tracing::error!("Failed to create segment file: {e}"),
        }
    }

    fn write_samples(&mut self, samples: &[i16]) {
        if let Some(ref mut w) = self.writer {
            for &s in samples {
                let _ = w.write_sample(s);
            }
        }
    }

    fn finalize(&mut self, _work_dir: &Path) -> Option<PathBuf> {
        let path = self.part_path.take()?;
        if let Some(mut w) = self.writer.take() {
            let _ = w.flush();
            drop(w);
        }
        let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if size <= 44 {
            let _ = fs::remove_file(&path);
            return None;
        }
        let final_path = path.with_extension("wav");
        if fs::rename(&path, &final_path).is_err() {
            return None;
        }
        Some(final_path)
    }

    fn flush(&mut self, work_dir: &Path) -> Option<(PathBuf, f64, f64)> {
        let dur = self.duration_sec;
        let t_write = Instant::now();
        let p = self.finalize(work_dir)?;
        let write_sec = t_write.elapsed().as_secs_f64();
        self.duration_sec = 0.0;
        self.silence_frames = 0;
        Some((p, dur, write_sec))
    }

    /// Пауза записи WAV: закрыть writer, удалить незавершённый `.part`, сбросить буфер VAD.
    fn discard_open_segment(&mut self) {
        self.writer.take();
        if let Some(p) = self.part_path.take() {
            let _ = fs::remove_file(&p);
        }
        self.duration_sec = 0.0;
        self.silence_frames = 0;
        self.pcm_remainder.clear();
    }
}

fn sync_pause_discard(
    sources: &mut [Option<SourceState>; 2],
    record_pcm: &Arc<AtomicBool>,
    prev_recording: &mut bool,
) {
    let cur = record_pcm.load(Ordering::Relaxed);
    if !cur && *prev_recording {
        for src in sources.iter_mut().flatten() {
            src.discard_open_segment();
        }
    }
    *prev_recording = cur;
}

fn feed_pcm_chunk(
    sources: &mut [Option<SourceState>; 2],
    chunk: PcmChunk,
    silence_frames: u32,
    cfg: &PipelineConfig,
    seg_tx: &CrossbeamSender<SegmentReady>,
    log_tx: &Option<CrossbeamSender<UiMsg>>,
) {
    let sid = chunk.source_id as usize;
    if sid > 1 {
        return;
    }

    let state = sources[sid].get_or_insert_with(|| {
        SourceState::new(
            chunk.source_id,
            silence_frames,
            cfg.initial_seg_seq[sid],
        )
    });

    let completed = state.feed(&chunk.samples, cfg);
    for (path, duration_sec, write_sec) in completed {
        let fname = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let _ = seg_tx.send(SegmentReady {
            path,
            source_id: chunk.source_id,
            duration_sec,
        });
        let proc_sec = write_sec;
        debug!(
            "Segment ready src{}: {}",
            chunk.source_id,
            fname
        );
        if let Some(ref tx) = log_tx {
            let _ = tx.send(UiMsg::Log(StructuredLog {
                stage: "segment".into(),
                source_id: chunk.source_id,
                chunk_sec: duration_sec,
                proc_sec,
                detail: fname,
                verbose_only: true,
            }));
        }
    }
}

/// Main pipeline loop: receives PCM from audio threads, segments via VAD,
/// writes WAV to disk, sends paths to ASR worker.
pub fn run(
    cfg: PipelineConfig,
    pcm_rx: Receiver<PcmChunk>,
    seg_tx: CrossbeamSender<SegmentReady>,
    running: Arc<AtomicBool>,
    record_pcm: Arc<AtomicBool>,
    log_tx: Option<CrossbeamSender<UiMsg>>,
) {
    let silence_frames =
        (16000.0 * cfg.vad_silence_sec / FRAME_SAMPLES as f64).ceil() as u32;

    let mut sources: [Option<SourceState>; 2] = [None, None];

    let timeout = std::time::Duration::from_millis(200);
    let mut prev_recording = true;

    while running.load(Ordering::Relaxed) {
        match pcm_rx.recv_timeout(timeout) {
            Ok(c) => {
                sync_pause_discard(&mut sources, &record_pcm, &mut prev_recording);
                if record_pcm.load(Ordering::Relaxed) {
                    feed_pcm_chunk(
                        &mut sources,
                        c,
                        silence_frames,
                        &cfg,
                        &seg_tx,
                        &log_tx,
                    );
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                sync_pause_discard(&mut sources, &record_pcm, &mut prev_recording);
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }

    // Остаток очереди после остановки. Дальше `pcm_rx` дропается при выходе — send в колбэках cpal
    // получит Disconnected и не будет ждать, пока поток захвата отпустит `drop(stream)` (на Windows это
    // часто десятки секунд). Раньше здесь был второй бесконечный цикл до Disconnected → зависание после Ctrl+C.
    while let Ok(c) = pcm_rx.try_recv() {
        sync_pause_discard(&mut sources, &record_pcm, &mut prev_recording);
        if record_pcm.load(Ordering::Relaxed) {
            feed_pcm_chunk(
                &mut sources,
                c,
                silence_frames,
                &cfg,
                &seg_tx,
                &log_tx,
            );
        }
    }

    for src in sources.iter_mut().flatten() {
        if let Some((p, duration_sec, write_sec)) = src.flush(&cfg.work_dir) {
            let fname = p.file_name().unwrap_or_default().to_string_lossy().to_string();
            let sid = src.source_id;
            let _ = seg_tx.send(SegmentReady {
                path: p,
                source_id: sid,
                duration_sec,
            });
            let proc_sec = write_sec;
            if let Some(ref tx) = log_tx {
                let _ = tx.send(UiMsg::Log(StructuredLog {
                    stage: "segment".into(),
                    source_id: sid,
                    chunk_sec: duration_sec,
                    proc_sec,
                    detail: format!("{fname} (flush)"),
                    verbose_only: true,
                }));
            }
        }
    }
    debug!("Pipeline stopped, flushed remaining segments");
}
