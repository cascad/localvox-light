//! Kaldi features (fbank) — the input of the voice embedding model.
//!
//! **Why not our `mel.rs`.** It is computed for GigaAM: a Hann window, 64 mels, `n_fft`
//! 320, no pre-emphasis, no CMN. The embedders (3D-Speaker, WeSpeaker) are trained on
//! CLASSIC kaldi features: 80 mels, a Povey window, pre-emphasis 0.97, DC offset removal,
//! subtraction of the mean over time. Feeding the model «almost the same» features is not
//! «slightly worse quality». It is garbage vectors, collapsed clusters and speakers handed
//! out to the wrong people — WITHOUT A SINGLE ERROR in the log.
//!
//! That is why everything here repeats kaldi down to the last constant, and `tests/fbank.rs`
//! checks the result against a REFERENCE taken from the reference implementation. The
//! golden test needs no model on disk and therefore runs in the ordinary test run: a
//! divergence in the features must fail right away, not surface a week later as bad
//! labelling.

use std::f32::consts::PI;

pub const SAMPLE_RATE: f32 = 16_000.0;
pub const NUM_MEL_BINS: usize = 80;
/// A 25 ms window with a 10 ms shift — at 16 kHz that is 400 and 160 samples.
const FRAME_LENGTH: usize = 400;
const FRAME_SHIFT: usize = 160;
/// The FFT takes the nearest power of two (`round_to_power_of_two`).
const N_FFT: usize = 512;
const PREEMPH: f32 = 0.97;
const LOW_FREQ: f32 = 20.0;
/// `high_freq = -400` in the kaldi config means «Nyquist minus 400».
const HIGH_FREQ: f32 = SAMPLE_RATE / 2.0 - 400.0;
/// The logarithm floor — exactly the one in kaldi (`FLT_EPSILON`), otherwise silence gives
/// −inf or a different number, and the WHOLE matrix diverges.
const LOG_FLOOR: f32 = f32::EPSILON;

/// The feature matrix `[frames, 80]` for a piece of speech (PCM 16 kHz mono f32, −1..1).
///
/// Already with CMN: the mean over TIME has been subtracted. The model was trained on
/// exactly these.
pub fn compute(samples: &[f32]) -> Vec<Vec<f32>> {
    let n = num_frames(samples.len());
    if n == 0 {
        return Vec::new();
    }
    let window = povey(FRAME_LENGTH);
    let bank = mel_bank();
    let mut fft = realfft::RealFftPlanner::<f32>::new();
    let plan = fft.plan_fft_forward(N_FFT);

    let mut out: Vec<Vec<f32>> = Vec::with_capacity(n);
    let mut buf = vec![0.0f32; N_FFT];
    for f in 0..n {
        extract(samples, f, &mut buf[..FRAME_LENGTH]);
        buf[FRAME_LENGTH..].fill(0.0);

        // Kaldi's order: remove the DC offset → pre-emphasis → window. Swapping them around
        // means computing different features.
        let mean: f32 = buf[..FRAME_LENGTH].iter().sum::<f32>() / FRAME_LENGTH as f32;
        for x in &mut buf[..FRAME_LENGTH] {
            *x -= mean;
        }
        for i in (1..FRAME_LENGTH).rev() {
            buf[i] -= PREEMPH * buf[i - 1];
        }
        buf[0] -= PREEMPH * buf[0];
        for (x, w) in buf[..FRAME_LENGTH].iter_mut().zip(&window) {
            *x *= w;
        }

        let mut spectrum = plan.make_output_vec();
        let mut input = buf.clone();
        if plan.process(&mut input, &mut spectrum).is_err() {
            return Vec::new();
        }
        // Power, not amplitude: fbank in kaldi is `use_power` by default.
        let power: Vec<f32> = spectrum.iter().map(|c| c.re * c.re + c.im * c.im).collect();

        out.push(
            bank.iter()
                .map(|(offset, weights)| {
                    let e: f32 = weights
                        .iter()
                        .enumerate()
                        .map(|(i, w)| w * power[offset + i])
                        .sum();
                    e.max(LOG_FLOOR).ln()
                })
                .collect(),
        );
    }

    cmn(&mut out);
    out
}

/// Subtract the mean over TIME (for each mel separately).
///
/// This is not cosmetics: CMN removes the colouring of the channel — the microphone, the
/// room, the volume. Without it «the same person in a different room» stops being the same
/// person.
fn cmn(frames: &mut [Vec<f32>]) {
    let Some(width) = frames.first().map(Vec::len) else {
        return;
    };
    let n = frames.len() as f32;
    for m in 0..width {
        let mean: f32 = frames.iter().map(|f| f[m]).sum::<f32>() / n;
        for f in frames.iter_mut() {
            f[m] -= mean;
        }
    }
}

/// The number of frames at `snip_edges = false`: the signal is not cut off but REFLECTED at
/// the edges, so the number of frames is exactly «duration / shift».
fn num_frames(samples: usize) -> usize {
    if samples == 0 {
        return 0;
    }
    (samples + FRAME_SHIFT / 2) / FRAME_SHIFT
}

/// Frame `f` into the buffer. The edges are reflected (at `snip_edges = false` kaldi mirrors
/// the signal rather than padding it with zeros: a zero at the edge is a click that was not
/// there).
fn extract(samples: &[f32], f: usize, out: &mut [f32]) {
    let mid = (f * FRAME_SHIFT + FRAME_SHIFT / 2) as isize;
    let start = mid - (FRAME_LENGTH / 2) as isize;
    let len = samples.len() as isize;
    for (i, x) in out.iter_mut().enumerate() {
        let mut s = start + i as isize;
        while s < 0 || s >= len {
            s = if s < 0 { -s - 1 } else { 2 * len - 1 - s };
        }
        *x = samples[s as usize];
    }
}

/// The Povey window: Hann to the power of 0.85. Exactly that one, not Hann and not Hamming.
fn povey(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| {
            let a = 2.0 * PI * i as f32 / (n as f32 - 1.0);
            (0.5 - 0.5 * a.cos()).powf(0.85)
        })
        .collect()
}

fn hz_to_mel(f: f32) -> f32 {
    1127.0 * (1.0 + f / 700.0).ln()
}
/// Triangular filters: for every mel an offset and weights (kaldi stores them the same way,
/// sparsely: a filter covers only its own piece of the spectrum).
fn mel_bank() -> Vec<(usize, Vec<f32>)> {
    let bins = N_FFT / 2 + 1;
    let df = SAMPLE_RATE / N_FFT as f32;
    let (mel_low, mel_high) = (hz_to_mel(LOW_FREQ), hz_to_mel(HIGH_FREQ));
    let step = (mel_high - mel_low) / (NUM_MEL_BINS + 1) as f32;

    (0..NUM_MEL_BINS)
        .map(|m| {
            let left = mel_low + m as f32 * step;
            let center = left + step;
            let right = center + step;

            let mut offset = 0usize;
            let mut weights: Vec<f32> = Vec::new();
            for b in 0..bins {
                let mel = hz_to_mel(b as f32 * df);
                if mel <= left || mel >= right {
                    if weights.is_empty() {
                        offset = b + 1;
                    }
                    continue;
                }
                let w = if mel <= center {
                    (mel - left) / (center - left)
                } else {
                    (right - mel) / (right - center)
                };
                weights.push(w);
            }
            (offset, weights)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Silence must give neither −inf nor NaN: the logarithm floor is the same as in kaldi.
    /// Were it to diverge, the whole matrix would diverge — and silently.
    #[test]
    fn silence_gives_the_kaldi_log_floor_not_minus_infinity() {
        let f = compute(&vec![0.0; 16_000]);
        assert_eq!(f.len(), 100, "a second of speech is a hundred 10 ms frames");
        // After CMN silence must become zeros, not NaN.
        assert!(f.iter().flatten().all(|x| x.is_finite()));
        assert!(f.iter().flatten().all(|x| x.abs() < 1e-3));
    }

    #[test]
    fn the_frame_count_matches_kaldi_snip_edges_false() {
        // Exactly «duration / shift»: the edges are reflected, not cut off.
        assert_eq!(num_frames(16_000), 100);
        assert_eq!(num_frames(85_440), 534); // as in the reference
        assert_eq!(num_frames(0), 0);
    }

    /// The Povey window is Hann TO THE POWER of 0.85, not Hann. Zero at the edges, one in
    /// the middle; confusing it with Hann means computing different features.
    #[test]
    fn the_window_is_povey_not_hann() {
        let w = povey(400);
        assert!(w[0].abs() < 1e-6 && w[399].abs() < 1e-6);
        assert!((w[199] - 1.0).abs() < 0.01);
        let hann = 0.5 - 0.5 * (2.0 * PI * 100.0 / 399.0).cos();
        assert!(
            (w[100] - hann).abs() > 0.01,
            "the window coincided with Hann — so the power of 0.85 got lost"
        );
    }

    /// The filters cover only the given band: 20 Hz at the bottom, Nyquist−400 at the top.
    #[test]
    fn the_mel_bank_respects_the_frequency_limits() {
        let bank = mel_bank();
        assert_eq!(bank.len(), NUM_MEL_BINS);
        assert!(bank.iter().all(|(_, w)| !w.is_empty()), "an empty filter");
        let df = SAMPLE_RATE / N_FFT as f32;
        let (last_offset, last_w) = bank.last().unwrap();
        let top = (last_offset + last_w.len()) as f32 * df;
        assert!(top <= HIGH_FREQ + df, "the filters climbed above {HIGH_FREQ} Hz");
    }
}
