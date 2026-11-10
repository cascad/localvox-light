use super::*;
use crate::chunks::{codec, resolve_ffmpeg_for_decode};

#[derive(Debug, Default, Serialize)]
pub struct AudioStats {
    pub chunks_read: usize,
    pub samples_supplied: u64,
    pub windows: usize,
    pub peak_window_samples: usize,
}

/// The caller chooses a fixed horizon and a previously committed end per source.
/// A container is opened/decoded once per horizon, never once for each ASR window.
pub fn consume_windows(
    session: &Path,
    horizon: &Horizon,
    after: &BTreeMap<u8, u64>,
    cancelled: impl Fn() -> bool,
    mut consume: impl FnMut(VerifiedWindow<'_>) -> Result<()>,
) -> Result<AudioStats> {
    let mut stats = AudioStats::default();
    let ffmpeg = PathBuf::from(resolve_ffmpeg_for_decode());
    for (id, all) in &horizon.windows {
        let end = after.get(id).copied().unwrap_or(0);
        ensure!(
            end == 0 || all.iter().any(|w| w.end_sample == end),
            "incremental start is not a window boundary"
        );
        let windows: Vec<_> = all.iter().filter(|w| w.end_sample > end).collect();
        let Some(first) = windows.first() else {
            continue;
        };
        let last = windows.last().unwrap().end_sample;
        let mut at = first.start_sample;
        let mut index = 0;
        let capacity = windows
            .iter()
            .map(|w| (w.end_sample - w.start_sample) as usize)
            .max()
            .unwrap_or(0);
        let mut buffer = Vec::with_capacity(capacity);
        let source = &horizon.input.sources[id];
        for range in source
            .ranges
            .iter()
            .take(horizon.plan.sources[id].sequence as usize)
        {
            if range.kind == capture::Kind::Empty {
                continue;
            }
            let to = last.min(range.start_sample + range.samples);
            let from = at.max(range.start_sample);
            if to <= from {
                continue;
            }
            ensure!(from == at, "sealed input gap while assembling ASR windows");
            ensure!(!cancelled(), "incremental audio cancelled");
            codec::with_read_cancelled(
                &session.join("audio"),
                &range.stem(*id),
                &cancelled,
                |path| {
                    ensure!(
                        codec::samples(path)? == range.samples,
                        "sealed input sample count changed"
                    );
                    if path.extension().and_then(|s| s.to_str()) == Some("flac") {
                        codec::verify_conversion_cancelled(
                            &session.join("audio"),
                            &range.stem(*id),
                            range.samples,
                            horizon.generation(),
                            &cancelled,
                        )?;
                    }
                    codec::stream_range(
                        path,
                        &ffmpeg,
                        from - range.start_sample,
                        to - range.start_sample,
                        &cancelled,
                        |mut bytes| {
                            while !bytes.is_empty() {
                                ensure!(!cancelled(), "incremental audio cancelled");
                                let window = windows
                                    .get(index)
                                    .context("audio passed the planned horizon")?;
                                ensure!(
                                    at >= window.start_sample && at < window.end_sample,
                                    "window assembly discontinuity"
                                );
                                let count =
                                    (window.end_sample - at).min(bytes.len() as u64 / 2) as usize;
                                buffer.extend(
                                    bytes[..count * 2]
                                        .chunks_exact(2)
                                        .map(|b| i16::from_le_bytes([b[0], b[1]])),
                                );
                                stats.samples_supplied += count as u64;
                                at += count as u64;
                                stats.peak_window_samples =
                                    stats.peak_window_samples.max(buffer.len());
                                bytes = &bytes[count * 2..];
                                if at == window.end_sample {
                                    consume(VerifiedWindow::new(window, &buffer)?)?;
                                    stats.windows += 1;
                                    buffer.clear();
                                    index += 1;
                                }
                            }
                            Ok(())
                        },
                    )
                },
            )?;
            stats.chunks_read += 1;
            if at == last {
                break;
            }
        }
        ensure!(
            at == last && buffer.is_empty() && index == windows.len(),
            "sealed horizon did not cover all ASR windows"
        );
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::incremental::test_support::Fixture;
    #[test]
    fn windows_cross_transport_seams_and_a_later_horizon_reads_only_its_bounded_suffix() {
        let f = Fixture::with_transport([&[10, 20], &[8, 20]], true, 7);
        let mut seen = Vec::new();
        let stats = consume_windows(
            &f.session,
            &f.horizon,
            &BTreeMap::new(),
            || false,
            |w| {
                seen.push((
                    w.descriptor().source,
                    w.descriptor().start_sample,
                    w.pcm().to_vec(),
                ));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(stats.chunks_read, 6);
        assert_eq!(stats.windows, 4);
        assert_eq!(stats.samples_supplied, 40);
        assert_eq!(stats.peak_window_samples, 12);
        assert_eq!(seen[0].2, f.pcm[&0][..10]);
        assert_eq!(seen[1].2, f.pcm[&0][10..]);
        fs::remove_file(f.session.join("audio/src0_chunk0001.wav")).unwrap();
        let stats = consume_windows(
            &f.session,
            &f.horizon,
            &[(0, 10), (1, 8)].into(),
            || false,
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(stats.windows, 2);
        assert_eq!(stats.chunks_read, 4);
        assert_eq!(stats.samples_supplied, 22);
        assert!(
            consume_windows(
                &f.session,
                &f.horizon,
                &BTreeMap::new(),
                || false,
                |_| Ok(())
            )
            .is_err(),
            "full final verification must notice missing old input"
        );
    }
    #[test]
    fn loss_changed_pcm_nonboundary_start_and_cancellation_never_create_a_fake_window() {
        let f = Fixture::with_transport([&[10, 20], &[10, 20]], true, 7);
        assert!(consume_windows(
            &f.session,
            &f.horizon,
            &[(0, 3)].into(),
            || false,
            |_| Ok(())
        )
        .is_err());
        let path = f.session.join("audio/src0_chunk0002.wav");
        let original = fs::read(&path).unwrap();
        let mut corrupt = original.clone();
        corrupt[44] ^= 1;
        fs::write(&path, corrupt).unwrap();
        let mut calls = 0;
        assert!(consume_windows(
            &f.session,
            &f.horizon,
            &BTreeMap::new(),
            || false,
            |_| {
                calls += 1;
                Ok(())
            }
        )
        .is_err());
        assert_eq!(calls, 0);
        fs::write(&path, original).unwrap();
        let cancelled = std::cell::Cell::new(false);
        assert!(consume_windows(
            &f.session,
            &f.horizon,
            &BTreeMap::new(),
            || cancelled.get(),
            |_| {
                cancelled.set(true);
                Ok(())
            }
        )
        .is_err());
        fs::remove_file(path).unwrap();
        assert!(consume_windows(
            &f.session,
            &f.horizon,
            &BTreeMap::new(),
            || false,
            |_| panic!("window across missing PCM must not be emitted")
        )
        .is_err());
    }
    #[test]
    #[ignore = "requires real ffmpeg, generated PCM only"]
    fn real_flac_suffix_crosses_seams_without_one_decode_for_every_window() {
        let f = Fixture::with_transport([&[5000, 10000, 16017], &[]], true, 7000);
        for seq in 1..=3 {
            codec::convert(
                &PathBuf::from(resolve_ffmpeg_for_decode()),
                &f.session.join(format!("audio/src0_chunk{seq:04}.wav")),
                || false,
                std::time::Duration::from_secs(10),
            )
            .unwrap();
        }
        let mut seen = Vec::new();
        let stats = consume_windows(
            &f.session,
            &f.horizon,
            &[(0, 5000)].into(),
            || false,
            |w| {
                seen.extend_from_slice(w.pcm());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(seen, f.pcm[&0][5000..]);
        assert_eq!(stats.chunks_read, 3);
        assert_eq!(stats.windows, 2);
        assert!(stats.peak_window_samples <= 6017);
    }
}
