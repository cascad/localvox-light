//! Modular ASR engine trait + noise gate.
//! Currently implements Vosk; others (Whisper, GigaAM, Parakeet) can be added
//! by implementing the `AsrEngine` trait.

pub mod vosk;

use anyhow::Result;
use webrtc_vad::{SampleRate, Vad, VadMode};

/// Pluggable ASR engine. Receives f32 samples (16 kHz mono), returns text.
/// Must be Send + Sync so the engine can be shared across a worker pool.
pub trait AsrEngine: Send + Sync {
    #[allow(dead_code)]
    fn name(&self) -> &str;
    fn transcribe(&self, samples: &[f32]) -> Result<String>;
}

/// Compute speech ratio via VeryAggressive VAD. Returns 0.0..=1.0.
pub fn speech_ratio(samples: &[f32]) -> f32 {
    const FRAME: usize = 320;
    if samples.len() < FRAME {
        return 1.0;
    }
    let mut vad = Vad::new_with_rate_and_mode(SampleRate::Rate16kHz, VadMode::VeryAggressive);
    vad.set_sample_rate(SampleRate::Rate16kHz);
    let mut total = 0u32;
    let mut speech = 0u32;
    for frame in samples.chunks(FRAME) {
        if frame.len() < FRAME {
            break;
        }
        let i16s: Vec<i16> = frame
            .iter()
            .map(|&s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
            .collect();
        total += 1;
        if vad.is_voice_segment(&i16s).unwrap_or(true) {
            speech += 1;
        }
    }
    if total == 0 { 1.0 } else { speech as f32 / total as f32 }
}

/// Trim f32 samples to speech-only portions with padding.
/// Returns None if no speech found.
pub fn trim_to_speech(samples: &[f32], pad_ms: u32) -> Option<Vec<f32>> {
    const FRAME: usize = 320;
    if samples.len() < FRAME {
        return Some(samples.to_vec());
    }

    let mut vad = Vad::new_with_rate_and_mode(SampleRate::Rate16kHz, VadMode::VeryAggressive);
    vad.set_sample_rate(SampleRate::Rate16kHz);

    let flags: Vec<bool> = samples
        .chunks(FRAME)
        .filter(|c| c.len() == FRAME)
        .map(|frame| {
            let i16s: Vec<i16> = frame
                .iter()
                .map(|&s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
                .collect();
            vad.is_voice_segment(&i16s).unwrap_or(true)
        })
        .collect();

    if !flags.iter().any(|&s| s) {
        return None;
    }

    let pad_frames = ((pad_ms as usize) + 19) / 20;
    let num_frames = flags.len();
    let mut keep = vec![false; num_frames];
    for (i, &is_speech) in flags.iter().enumerate() {
        if is_speech {
            let start = i.saturating_sub(pad_frames);
            let end = (i + pad_frames + 1).min(num_frames);
            for k in &mut keep[start..end] {
                *k = true;
            }
        }
    }

    let mut result = Vec::new();
    for (i, &k) in keep.iter().enumerate() {
        if k {
            let start = i * FRAME;
            let end = ((i + 1) * FRAME).min(samples.len());
            result.extend_from_slice(&samples[start..end]);
        }
    }
    if result.is_empty() { None } else { Some(result) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speech_ratio_short_buffer_is_one() {
        let s = vec![0.0f32; 100];
        assert_eq!(speech_ratio(&s), 1.0);
    }

    #[test]
    fn trim_to_speech_short_returns_clone() {
        let s = vec![0.0f32; 100];
        let t = trim_to_speech(&s, 300).expect("some");
        assert_eq!(t.len(), s.len());
    }
}
