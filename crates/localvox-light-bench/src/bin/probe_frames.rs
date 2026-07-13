//! A one-off timecode diagnostic: do the logit frames add up with the audio duration.
fn main() -> anyhow::Result<()> {
    let wav = std::env::args().nth(1).expect("a path to a wav is required");
    let mut r = hound::WavReader::open(&wav)?;
    let all: Vec<i16> = r.samples::<i16>().collect::<Result<_, _>>()?;
    let off_sec: f64 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    let from = ((off_sec * 16_000.0) as usize).min(all.len());
    let take = (from + 16_000 * 15).min(all.len());
    let pcm: Vec<f32> = all[from..take]
        .iter()
        .map(|&s| f32::from(s) / 32768.0)
        .collect();
    println!(
        "window {off_sec:.1}..{:.1} s of the file",
        off_sec + (take - from) as f64 / 16000.0
    );
    let audio_sec = pcm.len() as f64 / 16_000.0;

    let dir = std::path::Path::new("models/gigaam-v3-e2e-ctc");
    let model = std::fs::read_dir(dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("onnx"))
        .min_by_key(|p| !p.to_string_lossy().contains("int8"))
        .expect("no onnx");
    let adapter = localvox_light_core::asr::onnx::adapters::GigaamV3E2eCtc::from_model_dir(dir)?;
    let eng = localvox_light_core::asr::onnx::OnnxEngine::new(&model, adapter)?;

    let words = eng.transcribe_words_pcm_16k_mono_f32(&pcm)?;
    println!("audio {audio_sec:.2} s, words {}", words.len());
    for w in words.iter().take(14) {
        println!("  {:6.2}-{:6.2}  {}", w.start_sec, w.end_sec, w.text);
    }
    if let Some(last) = words.last() {
        println!(
            "the last word ends at {:.2} s while the audio is {:.2} s",
            last.end_sec, audio_sec
        );
    }
    Ok(())
}
