//! Shared continuous PCM window planner; independent of ASR/ONNX.
use anyhow::{ensure, Result};

pub const SAMPLE_RATE: u32 = 16_000;
pub const FRAME_SAMPLES: usize = 320;

/// Canonical little-endian PCM identity shared by capture planning and ASR reuse.
pub fn pcm_sha256(pcm: &[i16]) -> String {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    let mut bytes = [0; 8192];
    for samples in pcm.chunks(bytes.len() / 2) {
        for (sample, pair) in samples.iter().zip(bytes.chunks_exact_mut(2)) {
            pair.copy_from_slice(&sample.to_le_bytes());
        }
        digest.update(&bytes[..samples.len() * 2]);
    }
    format!("{:x}", digest.finalize())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowKind {
    Speech,
    Silence,
}

/// A closed, contiguous piece of the input; silent pieces advance coverage too.
pub struct Window<'a> {
    pub start_sample: u64,
    pub pcm: &'a [i16],
    pub kind: WindowKind,
}

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
    finished: bool,
    failed: bool,
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
            finished: false,
            failed: false,
        }
    }

    pub fn feed<C, E>(&mut self, samples: &[i16], classify: &mut C, emit: &mut E) -> Result<()>
    where
        C: FnMut(&[i16]) -> bool,
        E: FnMut(u64, &[i16]) -> Result<()>,
    {
        self.feed_all(samples, classify, &mut |window| match window.kind {
            WindowKind::Speech => emit(window.start_sample, window.pcm),
            WindowKind::Silence => Ok(()),
        })
    }

    /// Feed sealed PCM without treating the transport boundary as EOF.
    /// Memory is bounded by one window plus a single incomplete 20 ms frame.
    pub fn feed_all<C, E>(
        &mut self,
        mut samples: &[i16],
        classify: &mut C,
        emit: &mut E,
    ) -> Result<()>
    where
        C: FnMut(&[i16]) -> bool,
        E: FnMut(Window<'_>) -> Result<()>,
    {
        ensure!(!self.finished, "window planner already finished");
        ensure!(
            !self.failed,
            "window planner failed; replay confirmed PCM to recover"
        );
        self.failed = true; // an emitter refusal requires replay, never guess how much committed
        if !self.pending.is_empty() {
            let take = samples.len().min(FRAME_SAMPLES - self.pending.len());
            self.pending.extend_from_slice(&samples[..take]);
            samples = &samples[take..];
            if self.pending.len() == FRAME_SAMPLES {
                let mut frame = [0; FRAME_SAMPLES];
                frame.copy_from_slice(&self.pending);
                self.pending.clear();
                self.frame(&frame, classify, emit)?;
            }
        }
        let mut frames = samples.chunks_exact(FRAME_SAMPLES);
        for frame in &mut frames {
            self.frame(frame, classify, emit)?;
        }
        self.pending.extend_from_slice(frames.remainder());
        self.failed = false;
        Ok(())
    }

    fn frame<C, E>(&mut self, frame: &[i16], classify: &mut C, emit: &mut E) -> Result<()>
    where
        C: FnMut(&[i16]) -> bool,
        E: FnMut(Window<'_>) -> Result<()>,
    {
        let is_speech = classify(frame);
        let frame_start = self.buf.len();
        self.buf.extend_from_slice(frame);
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
        Ok(())
    }

    /// End of the stream: flush the remainder (including an incomplete frame).
    pub fn finish<E>(&mut self, emit: &mut E) -> Result<()>
    where
        E: FnMut(u64, &[i16]) -> Result<()>,
    {
        self.finish_all(&mut |window| match window.kind {
            WindowKind::Speech => emit(window.start_sample, window.pcm),
            WindowKind::Silence => Ok(()),
        })
    }

    /// Call only after confirmed source EOF. Waiting/pause never calls this method.
    pub fn finish_all<E>(&mut self, emit: &mut E) -> Result<()>
    where
        E: FnMut(Window<'_>) -> Result<()>,
    {
        ensure!(
            !self.failed,
            "window planner failed; replay confirmed PCM to recover"
        );
        if self.finished {
            return Ok(());
        }
        self.failed = true;
        let tail: Vec<i16> = std::mem::take(&mut self.pending);
        self.buf.extend_from_slice(&tail);
        self.flush(emit)?;
        self.finished = true;
        self.failed = false;
        Ok(())
    }

    /// End of emitted speech/silence coverage, not the end of the buffered tail.
    pub fn closed_end(&self) -> u64 {
        self.buf_start
    }

    pub fn input_end(&self) -> u64 {
        self.buf_start + self.buf.len() as u64 + self.pending.len() as u64
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
        E: FnMut(Window<'_>) -> Result<()>,
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
        emit(Window {
            start_sample: self.buf_start,
            pcm: &self.buf[..cut],
            kind: if self.had_speech {
                WindowKind::Speech
            } else {
                WindowKind::Silence
            },
        })?;
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
        E: FnMut(Window<'_>) -> Result<()>,
    {
        if !self.buf.is_empty() {
            // Keep silent coverage; the legacy speech-only adapter skips its ASR call.
            emit(Window {
                start_sample: self.buf_start,
                pcm: &self.buf,
                kind: if self.had_speech {
                    WindowKind::Speech
                } else {
                    WindowKind::Silence
                },
            })?;
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
mod tests;
