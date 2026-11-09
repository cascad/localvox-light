use super::*;
use sha2::{Digest, Sha256};
#[path = "fixtures.rs"]
mod fixtures;

#[derive(Debug, PartialEq, Eq)]
struct SavedWindow {
    start: u64,
    kind: WindowKind,
    pcm: Vec<i16>,
}

fn collect(
    f: &fixtures::Fixture,
    mut next_size: impl FnMut() -> usize,
    native: bool,
) -> Vec<SavedWindow> {
    use webrtc_vad::{SampleRate, Vad, VadMode};
    let mut vad = Vad::new_with_rate_and_mode(SampleRate::Rate16kHz, VadMode::LowBitrate);
    let mut classified = 0;
    let mut classify = |frame: &[i16]| {
        assert_eq!(frame.len(), FRAME_SAMPLES);
        // All native/synthetic classifiers see exactly the same full PCM frames.
        assert_eq!(frame, &f.pcm[classified..classified + FRAME_SAMPLES]);
        classified += FRAME_SAMPLES;
        if native {
            vad.is_voice_segment(frame).unwrap_or(true)
        } else {
            frame.iter().any(|s| *s != 0)
        }
    };
    let mut w = Windower::new(f.max, f.min, f.silence_ms);
    let mut windows = Vec::new();
    let mut emit = |window: Window<'_>| {
        windows.push(SavedWindow {
            start: window.start_sample,
            kind: window.kind,
            pcm: window.pcm.to_vec(),
        });
        Ok(())
    };
    let mut at = 0;
    while at < f.pcm.len() {
        let end = f.pcm.len().min(at + next_size().max(1));
        w.feed_all(&f.pcm[at..end], &mut classify, &mut emit)
            .unwrap();
        assert_eq!(w.input_end(), end as u64);
        assert!(w.pending.len() < FRAME_SAMPLES);
        assert!(w.buf.len() < (f.max * SAMPLE_RATE as f64).ceil() as usize + FRAME_SAMPLES);
        // No new PCM / wait / empty transport messages do not flush the tail.
        let closed = w.closed_end();
        w.feed_all(&[], &mut classify, &mut emit).unwrap();
        assert_eq!(w.closed_end(), closed);
        at = end;
    }
    w.finish_all(&mut emit).unwrap();
    w.finish_all(&mut emit).unwrap(); // no duplicate EOF publication
    assert_eq!(classified, f.pcm.len() / FRAME_SAMPLES * FRAME_SAMPLES);
    assert_eq!(w.closed_end(), f.pcm.len() as u64);
    assert_eq!(w.input_end(), f.pcm.len() as u64);
    let mut covered = 0;
    for window in &windows {
        assert_eq!(window.start, covered);
        assert!(!window.pcm.is_empty());
        let end = covered as usize + window.pcm.len();
        assert_eq!(window.pcm, f.pcm[covered as usize..end]);
        covered = end as u64;
    }
    assert_eq!(covered, f.pcm.len() as u64);
    windows
}

#[test]
fn extracted_planner_matches_the_pre_extraction_pcm_oracle() {
    // Captured by the actual old cook::Windower at 00188a0 before extraction.
    // This checked-in oracle is never updated by a test run.
    let oracle: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("legacy-windows.json")).unwrap();
    for (f, old) in fixtures::fixtures().into_iter().zip(&oracle) {
        assert_eq!(old["name"], f.name);
        let windows = collect(&f, || usize::MAX / 2, false);
        let actual: Vec<_> = windows.iter().filter(|w| w.kind == WindowKind::Speech).map(|w| {
            let bytes: Vec<_> = w.pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
            serde_json::json!({"start":w.start,"samples":w.pcm.len(),"sha256":format!("{:x}",Sha256::digest(bytes))})
        }).collect();
        assert_eq!(serde_json::json!(actual), old["windows"], "{}", f.name);
    }
    assert_eq!(oracle.len(), fixtures::fixtures().len());
}

#[test]
fn random_transport_partitions_preserve_every_window_and_silent_outcome() {
    for f in fixtures::fixtures() {
        let expected = collect(&f, || usize::MAX / 2, false);
        for seed in 0..32u32 {
            let mut state = seed;
            let actual = collect(
                &f,
                || {
                    state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                    1 + state as usize % 8192
                },
                false,
            );
            assert_eq!(actual, expected, "{} / seed {seed}", f.name);
        }
        for size in [1, 319, 320, 321, 16000 + 73] {
            assert_eq!(
                collect(&f, || size, false),
                expected,
                "{} / partition {size}",
                f.name
            );
        }
    }
}

#[test]
fn native_vad_windows_are_identical_across_file_and_partial_frame_boundaries() {
    for f in fixtures::fixtures() {
        let expected = collect(&f, || usize::MAX / 2, true);
        for seed in 0..12u32 {
            let mut state = seed;
            assert_eq!(
                collect(
                    &f,
                    || {
                        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                        1 + state as usize % 16001
                    },
                    true
                ),
                expected,
                "{} / native {seed}",
                f.name
            );
        }
    }
}

#[test]
fn a_word_across_a_file_seam_waits_for_more_pcm_or_true_eof() {
    let mut w = Windower::new(15.0, 8.0, 500);
    let mut windows = Vec::new();
    let mut classify = |_: &[i16]| true;
    let mut emit = |window: Window<'_>| {
        windows.push((window.start_sample, window.pcm.to_vec()));
        Ok(())
    };
    let first = vec![37; 16000 + 17];
    w.feed_all(&first, &mut classify, &mut emit).unwrap();
    assert_eq!(w.closed_end(), 0);
    assert_eq!(w.input_end(), first.len() as u64);
    w.feed_all(&[], &mut classify, &mut emit).unwrap();
    assert_eq!(w.closed_end(), 0);
    let second = vec![93; 8000 + 59];
    w.feed_all(&second, &mut classify, &mut emit).unwrap();
    assert_eq!(w.closed_end(), 0);
    w.finish_all(&mut emit).unwrap();
    assert_eq!(windows, vec![(0, [first, second].concat())]);
    assert!(w
        .feed_all(&[1; 320], &mut classify, &mut |_| Ok(()))
        .is_err());
}

#[test]
fn refused_publication_requires_replay_instead_of_skipping_or_reemitting_a_window() {
    for terminal in [false, true] {
        let mut w = Windower::new(1.0, 5.0, 500);
        let pcm = vec![1; if terminal { 400 } else { 16000 }];
        let mut refuse = |_: Window<'_>| Err(anyhow::anyhow!("disk refusal"));
        let result = w.feed_all(&pcm, &mut |_| true, &mut refuse);
        if terminal {
            result.unwrap();
            assert!(w.finish_all(&mut refuse).is_err());
        } else {
            assert!(result.is_err());
        }
        let mut calls = 0;
        let mut emit = |_: Window<'_>| {
            calls += 1;
            Ok(())
        };
        assert!(w.feed_all(&pcm, &mut |_| true, &mut emit).is_err());
        assert!(w.finish_all(&mut emit).is_err());
        assert_eq!(calls, 0);
    }
}
