//! The golden test of the features: our kaldi-fbank against the reference one.
//!
//! **Why it exists.** The voice embedding model is trained on SPECIFIC features. Compute
//! them «almost the same way» — a Hann window instead of Povey, no pre-emphasis, no mean
//! subtraction — and the model will not crash, will not complain and will not return an
//! error. It will return plausible numbers that mean nothing: the voices stop clustering,
//! the lines scatter among random people, and the only way to notice is with your own eyes,
//! a week later, on a bad speaker labelling.
//!
//! This has already happened (int8 mDeBERTa: the scores collapsed fourfold and the NER
//! «found nothing»). The price of such a mistake is not a bug — it is weeks of distrust in
//! your own system.
//!
//! The reference was taken ONCE with the reference implementation (kaldi-native-fbank, C++,
//! through Python) on real Russian speech from our own benchmark. At runtime neither Python
//! nor C++ is needed — only the numbers lie on disk. The test does NOT REQUIRE a model and
//! therefore runs in the normal test pass: a divergence of the features must fail
//! immediately.

#![cfg(feature = "onnx")]

use std::path::{Path, PathBuf};

use localvox_light_core::diarize::fbank;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn read_f32(path: &Path) -> Vec<f32> {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}

fn wav_16k_mono(path: &Path) -> Vec<f32> {
    let mut r = hound::WavReader::open(path).expect("the golden wav");
    let spec = r.spec();
    assert_eq!(spec.sample_rate, 16_000, "the golden file is not recorded at 16 kHz");
    assert_eq!(spec.channels, 1);
    r.samples::<i16>()
        .map(|s| f32::from(s.unwrap()) / 32768.0)
        .collect()
}

#[test]
fn our_fbank_matches_the_reference_implementation() {
    let samples = wav_16k_mono(&fixtures().join("golden_input.wav"));
    let expected = read_f32(&fixtures().join("golden_fbank.f32"));

    let ours = fbank::compute(&samples);
    let frames = ours.len();
    assert_eq!(
        frames * fbank::NUM_MEL_BINS,
        expected.len(),
        "кадров {frames}, а в эталоне {} — разошлась сама РАЗМЕТКА времени, \
         а не значения (snip_edges/шаг/длина окна)",
        expected.len() / fbank::NUM_MEL_BINS
    );

    let flat: Vec<f32> = ours.into_iter().flatten().collect();
    let n = flat.len() as f64;
    let mae: f64 = flat
        .iter()
        .zip(&expected)
        .map(|(a, b)| f64::from((a - b).abs()))
        .sum::<f64>()
        / n;
    let max = flat
        .iter()
        .zip(&expected)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);

    eprintln!("MAE {mae:.3e}, max {max:.3e}, frames {frames}, reference spread {:.2}", {
        let (lo, hi) = expected.iter().fold((f32::MAX, f32::MIN), |(l, h), &x| (l.min(x), h.max(x)));
        hi - lo
    });

    // The thresholds come from the reference measurement: a divergence is allowed only at
    // the level of f32 arithmetic, not at the level of «a different formula».
    assert!(
        mae < 1e-3,
        "средняя ошибка фичей {mae:.2e} (порог 1e-3) — фичи считаются НЕ ТАК, \
         как их считали при обучении модели"
    );
    assert!(max < 1e-2, "maximum divergence {max:.2e} (threshold 1e-2)");
}

/// The same reference, but end to end: features → model → voice vector. Runs only if the
/// model is on disk (CI has none — it is not in the repository).
#[test]
fn the_voice_vector_matches_the_reference_when_the_model_is_installed() {
    let Some(dir) = localvox_light_core::diarize::model_dir() else {
        eprintln!("no diarization model — the end-to-end check is skipped");
        return;
    };
    let Ok(voice) = localvox_light_core::diarize::embed::Voice::open(&dir) else {
        eprintln!("the embedding model did not open — the end-to-end check is skipped");
        return;
    };

    let samples = wav_16k_mono(&fixtures().join("golden_input.wav"));
    let expected = read_f32(&fixtures().join("golden_embedding.f32"));
    let ours = voice.embed(&samples).expect("the voice vector");

    assert_eq!(ours.len(), expected.len(), "a different vector dimensionality");
    let dot: f64 = ours
        .iter()
        .zip(&expected)
        .map(|(a, b)| f64::from(*a) * f64::from(*b))
        .sum();
    let na: f64 = ours.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>().sqrt();
    let nb: f64 = expected
        .iter()
        .map(|x| f64::from(*x).powi(2))
        .sum::<f64>()
        .sqrt();
    let cosine = dot / (na * nb);

    // The cosine is exactly what we later tell people apart by. Let it diverge — and «the
    // same person» stops being the same person.
    assert!(
        cosine >= 0.999,
        "the voice vector diverged from the reference: cosine {cosine:.5} (threshold 0.999)"
    );
}
