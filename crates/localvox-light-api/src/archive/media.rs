//! Lazy HTTP body: one bounded PCM window, independent of the requested range length.
use super::*;
use std::io::{self, Read};

const WINDOW_BYTES: usize = 1024 * 1024;

pub struct AudioStream {
    pub total: u64,
    pub start: u64,
    pub end: u64,
    dir: PathBuf,
    meta: localvox_light_core::chunks::SessionMeta,
    position: u64,
    buffer: Vec<u8>,
    consumed: usize,
    partial: Option<localvox_light_core::incremental::AudioReader>,
}
impl AudioStream {
    fn fill_buffer(&mut self) -> io::Result<()> {
        let end = self.end.min(self.position + WINDOW_BYTES as u64);
        let from = (self.position - 44) / 2;
        let to = (end - 44).div_ceil(2);
        let mut read_source = |id| {
            if let Some(reader) = &mut self.partial {
                return reader.range(id, from, to).map_err(io::Error::other);
            }
            let overlaps = self.meta.chunks.iter().any(|c| {
                c.source_id == id
                    && c.start_offset_sec < to as f64 / SR
                    && c.start_offset_sec + c.duration_sec > from as f64 / SR
            });
            if overlaps {
                source_pcm(&self.dir, &self.meta, id, from, to).map_err(io::Error::other)
            } else {
                Ok(Vec::new())
            }
        };
        let mic = read_source(0)?;
        let sys = read_source(1)?;
        let mixed = mix(&mic, &sys);
        let skip = ((self.position - 44) % 2) as usize;
        let count = (end - self.position) as usize;
        self.buffer.clear();
        self.buffer.extend(mixed.into_iter().skip(skip).take(count));
        if self.partial.is_some() {
            if self.buffer.len() != count {
                return Err(io::Error::other("snapshot audio coverage gap"));
            }
        } else {
            self.buffer.resize(count, 0);
        }
        self.consumed = 0;

        Ok(())
    }

    pub fn len(&self) -> u64 {
        self.end - self.start
    }
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}
impl Read for AudioStream {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() || self.position >= self.end {
            return Ok(0);
        }
        if self.position < 44 {
            let header = wav_header((self.total - 44) as u32);
            let count = out.len().min((self.end.min(44) - self.position) as usize);
            out[..count]
                .copy_from_slice(&header[self.position as usize..self.position as usize + count]);
            self.position += count as u64;
            return Ok(count);
        }
        if self.consumed == self.buffer.len() {
            self.fill_buffer()?;
        }
        let count = out.len().min(self.buffer.len() - self.consumed);
        out[..count].copy_from_slice(&self.buffer[self.consumed..self.consumed + count]);
        self.position += count as u64;
        self.consumed += count;
        Ok(count)
    }
}

impl Archive {
    /// The HTTP body is assembled as it is consumed. An open-ended Range does
    /// not allocate or decode the remaining recording before sending its header.
    pub fn audio_wav(
        &self,
        session: &str,
        range: Option<(u64, Option<u64>)>,
    ) -> Result<AudioStream> {
        self.audio_wav_snapshot(session, range, None)
    }
    pub fn audio_wav_snapshot(
        &self,
        session: &str,
        range: Option<(u64, Option<u64>)>,
        snapshot: Option<&str>,
    ) -> Result<AudioStream> {
        let dir = self.session_dir(session)?;
        let meta: localvox_light_core::chunks::SessionMeta =
            serde_json::from_slice(&fs::read(dir.join("meta.json")).context("нет meta.json")?)
                .context("meta.json не читается")?;
        let snapshot = snapshot
            .map(|id| localvox_light_core::incremental::Published::snapshot(&dir, id))
            .transpose()?;
        if snapshot.is_none() && self.partial_document(&dir)?.is_some() {
            anyhow::bail!("аудио записи доступно только по опубликованному снимку");
        }
        let duration = meta
            .chunks
            .iter()
            .map(|c| c.start_offset_sec + c.duration_sec)
            .fold(0.0_f64, f64::max);
        let pcm_len = snapshot
            .as_ref()
            .map_or((duration * SR) as u64, |s| s.audio_end);
        anyhow::ensure!(
            pcm_len <= (u32::MAX as u64 - 36) / 2,
            "запись превышает размер WAV RIFF"
        );
        let file_len = 44 + pcm_len * 2;
        anyhow::ensure!(
            snapshot.is_none() || range.is_none_or(|(start, _)| start < file_len),
            "запрошенное аудио ещё недоступно; граница снимка — {pcm_len} сэмплов"
        );
        let (start, end) = match range {
            Some((start, end)) => {
                let start = start.min(file_len);
                (
                    start,
                    end.map(|end| end.saturating_add(1).min(file_len))
                        .unwrap_or(file_len)
                        .max(start),
                )
            }
            None => (0, file_len),
        };
        let partial = snapshot.map(|s| s.audio_reader(&dir)).transpose()?;
        let mut stream = AudioStream {
            total: file_len,
            start,
            end,
            dir,
            meta,
            position: start,
            buffer: Vec::new(),
            consumed: 0,
            partial,
        };
        // Validate the first bounded PCM buffer before sending successful HTTP headers.
        if stream.partial.is_some() && stream.end > 44 {
            let start = stream.position;
            stream.position = start.max(44);
            stream.fill_buffer()?;
            stream.position = start;
        }
        Ok(stream)
    }

    pub fn peaks_snapshot(&self, name: &str, buckets: usize, id: &str) -> Result<Value> {
        let dir = self.session_dir(name)?;
        let snapshot = localvox_light_core::incremental::Published::snapshot(&dir, id)?;
        let end = snapshot.audio_end;
        let mut reader = snapshot.audio_reader(&dir)?;
        let buckets = buckets.clamp(32, 2000);
        let mut acc = vec![0f32; buckets];
        let mut from = 0;
        while from < end {
            let to = (from + 512 * 1024).min(end);
            for source in [0, 1] {
                let pcm = reader.range(source, from, to)?;
                for (i, sample) in pcm.chunks_exact(2).enumerate() {
                    let bucket =
                        (((from + i as u64) as u128 * buckets as u128) / end as u128) as usize;
                    acc[bucket.min(buckets - 1)] = acc[bucket.min(buckets - 1)].max(f32::from(
                        i16::from_le_bytes([sample[0], sample[1]]).saturating_abs(),
                    ));
                }
            }
            from = to;
        }
        let loudest = acc.iter().copied().fold(1f32, f32::max);
        Ok(
            json!({"duration_sec":end as f64/SR,"snapshot":id,"peaks":acc.into_iter().map(|v|(v/loudest).sqrt()).collect::<Vec<_>>()}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn open_ended_hour_is_lazy_and_pcm_windows_are_bounded() {
        let directory = tempfile::tempdir().unwrap();
        let session = directory.path().join("sessions/hour");
        fs::create_dir_all(session.join("audio")).unwrap();
        // A real sparse PCM file keeps the memory test honest without faking missing audio.
        let mut pcm = fs::File::create(session.join("audio/missing.wav")).unwrap();
        std::io::Write::write_all(&mut pcm, &wav_header(3600 * 32000)).unwrap();
        pcm.set_len(44 + 3600 * 32000).unwrap();
        drop(pcm);
        fs::write(session.join("meta.json"), json!({"started_at":"t", "sample_rate":16000,"chunks":[{"file":"missing","source_id":0,"start_offset_sec":0,"duration_sec":3600}]}).to_string()).unwrap();
        let archive = Archive::new(directory.path().into());
        let mut stream = archive
            .audio_wav("hour", Some((44 + 1800 * 32000, None)))
            .unwrap();
        assert_eq!(stream.len(), 1800 * 32000);
        assert!(stream.buffer.is_empty());
        let mut out = [1; 100];
        stream.read_exact(&mut out).unwrap();
        assert_eq!(out, [0; 100]);
        assert!(stream.buffer.len() <= WINDOW_BYTES);
        assert_eq!(stream.position, stream.start + 100);
    }

    #[test]
    fn unreadable_or_short_declared_audio_is_not_fabricated_as_silence() {
        for short in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let session = root.path().join("sessions/broken");
            fs::create_dir_all(session.join("audio")).unwrap();
            fs::write(session.join("meta.json"),json!({"started_at":"t","sample_rate":16000,"chunks":[{"file":"declared","source_id":0,"start_offset_sec":0,"duration_sec":1.0}]}).to_string()).unwrap();
            if short {
                fs::write(
                    session.join("audio/declared.wav"),
                    wav_16k_mono(&[1, 0, 2, 0]),
                )
                .unwrap();
            }
            let archive = Archive::new(root.path().into());
            let mut stream = archive.audio_wav("broken", None).unwrap();
            let mut bytes = Vec::new();
            assert!(stream.read_to_end(&mut bytes).is_err());
        }
    }
    #[test]
    fn odd_byte_ranges_and_small_reads_equal_the_full_wav_across_chunk_boundaries() {
        let directory = tempfile::tempdir().unwrap();
        let session = directory.path().join("sessions/pcm");
        fs::create_dir_all(session.join("audio")).unwrap();
        let a = (0..20_000i16)
            .flat_map(|s| s.to_le_bytes())
            .collect::<Vec<_>>();
        let b = (20_000..30_000i16)
            .flat_map(|s| s.to_le_bytes())
            .collect::<Vec<_>>();
        fs::write(session.join("audio/a.wav"), wav_16k_mono(&a)).unwrap();
        fs::write(session.join("audio/b.wav"), wav_16k_mono(&b)).unwrap();
        fs::write(session.join("meta.json"), json!({"started_at":"t","sample_rate":16000,"chunks":[{"file":"a","source_id":0,"start_offset_sec":0,"duration_sec":1.25},{"file":"b","source_id":0,"start_offset_sec":1.25,"duration_sec":0.625}]}).to_string()).unwrap();
        let archive = Archive::new(directory.path().into());
        let mut expected = Vec::new();
        expected.extend(&a);
        expected.extend(&b);
        let expected = wav_16k_mono(&expected);
        for (start, end) in [(0, 43), (3, 51), (45, 129), (40_041, 40_101), (0, 60_043)] {
            let mut stream = archive.audio_wav("pcm", Some((start, Some(end)))).unwrap();
            let mut actual = Vec::new();
            let mut bytes = [0; 7];
            loop {
                let n = stream.read(&mut bytes).unwrap();
                if n == 0 {
                    break;
                }
                actual.extend_from_slice(&bytes[..n]);
            }
            assert_eq!(actual, expected[start as usize..=end as usize]);
        }
    }
    #[test]
    #[ignore = "requires real ffmpeg; synthetic PCM only"]
    fn flac_range_is_sample_accurate_without_decoding_the_whole_output() {
        use localvox_light_process::{run, Limits};
        let directory = tempfile::tempdir().unwrap();
        let samples = (0..30_000i16)
            .flat_map(|n| n.to_le_bytes())
            .collect::<Vec<_>>();
        let wav = directory.path().join("test.wav");
        let flac = directory.path().join("test.flac");
        fs::write(&wav, wav_16k_mono(&samples)).unwrap();
        let mut command =
            std::process::Command::new(localvox_light_core::chunks::resolve_ffmpeg_for_decode());
        command
            .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-i"])
            .arg(&wav)
            .arg(&flac);
        let output = run(
            command,
            None,
            Limits::new(std::time::Duration::from_secs(10)),
            || false,
            |_, _| {},
        )
        .unwrap();
        assert!(output.termination.success());
        for (from, to) in [(1, 19000), (8001, 9601), (17999, 20001)] {
            let actual = chunk_pcm_range(directory.path(), "test", from, to).unwrap();
            assert_eq!(actual, samples[from as usize * 2..to as usize * 2]);
        }
    }
}

impl Archive {
    /// Audio clip of a fragment (F6 player): sample-accurately glues the needed range
    /// out of the source's chunks → WAV 16 kHz mono s16. For "found it in search →
    /// listened to it". `dur_sec` is bounded, the audio may have been deleted by
    /// retention. A piece of the recording as WAV.
    ///
    /// `source_id`: 0 — microphone, 1 — system audio, `None` — a MIX of both. The mix
    /// is needed so that the recording can simply be listened to: a conversation
    /// consists of both tracks, and listening to half a dialogue is pointless.
    pub fn audio_clip(
        &self,
        session: &str,
        source_id: Option<u8>,
        start_sec: f64,
        dur_sec: f64,
    ) -> Result<Vec<u8>> {
        let dir = self.session_dir(session)?;
        anyhow::ensure!(
            self.partial_document(&dir)?.is_none(),
            "во время ранней обработки используйте опубликованный audio snapshot"
        );
        let meta_txt = fs::read_to_string(dir.join("meta.json")).context("нет meta.json сессии")?;
        let meta: localvox_light_core::chunks::SessionMeta =
            serde_json::from_str(&meta_txt).context("meta.json повреждён")?;

        let dur = dur_sec.clamp(0.1, MAX_CLIP_SEC);
        let want_start = (start_sec.max(0.0) * SR) as u64;
        let want_end = ((start_sec.max(0.0) + dur) * SR) as u64;

        let pcm = match source_id {
            Some(id) => source_pcm(&dir, &meta, id, want_start, want_end)?,
            None => {
                // The mix: the tracks are glued sample-to-sample from the start of the
                // recording, so they add up position by position. A missing track (a
                // monologue into the microphone, audio without a microphone) is not an
                // error — we play the one that exists; it is an error only if there is
                // NOT A SINGLE one.
                let mic = source_pcm(&dir, &meta, 0, want_start, want_end).unwrap_or_default();
                let sys = source_pcm(&dir, &meta, 1, want_start, want_end).unwrap_or_default();
                anyhow::ensure!(
                    !mic.is_empty() || !sys.is_empty(),
                    "нет аудио для фрагмента ({:.1}–{:.1} c)",
                    start_sec.max(0.0),
                    start_sec.max(0.0) + dur
                );
                mix(&mic, &sys)
            }
        };
        Ok(wav_16k_mono(&pcm))
    }

    /// The shape of the recording: peak loudness per bucket, for the waveform in the player.
    ///
    /// It is REAL. A drawn wave (a sine, noise, anything "pretty") is worse than no wave at all:
    /// people navigate a recording by it — they aim at the loud part where the argument was — and
    /// a wave that does not match the sound sends them to the wrong place while looking exactly
    /// as trustworthy as one that does.
    ///
    /// One pass over the audio, cached in `peaks.json`. The cache is derived data and therefore
    /// disposable: delete it and it is recomputed from the chunks, which are the truth.
    pub fn peaks(&self, name: &str, buckets: usize) -> Result<Value> {
        let buckets = buckets.clamp(32, 2000);
        let dir = self.session_dir(name)?;
        let meta: localvox_light_core::chunks::SessionMeta =
            serde_json::from_slice(&fs::read(dir.join("meta.json")).context("нет meta.json")?)
                .context("meta.json не читается")?;

        let duration = meta
            .chunks
            .iter()
            .map(|c| c.start_offset_sec + c.duration_sec)
            .fold(0.0_f64, f64::max);
        if duration <= 0.0 {
            return Ok(json!({ "duration_sec": 0.0, "peaks": [] }));
        }

        let cache = dir.join("peaks.json");
        if let Some(v) = fs::read(&cache)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .filter(|v| v["buckets"] == json!(buckets))
            .filter(|v| (v["duration_sec"].as_f64().unwrap_or(0.0) - duration).abs() < 0.5)
        {
            return Ok(v);
        }

        // In windows, not all at once: an hour of a track is 115 MB, and holding two of them in
        // the daemon's memory to draw a strip 300 pixels wide is not an honest trade.
        const WINDOW_SEC: f64 = 60.0;
        let mut acc = vec![0f32; buckets];
        let mut from = 0.0_f64;
        while from < duration {
            let to = (from + WINDOW_SEC).min(duration);
            for source_id in 0..2u8 {
                // A chunk that cannot be read must not cost us the whole wave: the player is more
                // useful with a gap in the picture than absent because of one broken file.
                let pcm = source_pcm(&dir, &meta, source_id, (from * SR) as u64, (to * SR) as u64)
                    .unwrap_or_default();
                for (i, s) in pcm.chunks_exact(2).enumerate() {
                    let v = f32::from(i16::from_le_bytes([s[0], s[1]]).saturating_abs());
                    let at = from + i as f64 / SR;
                    let b = ((at / duration) * buckets as f64) as usize;
                    let b = b.min(buckets - 1);
                    // The two tracks are NOT summed but taken at their max: the wave answers
                    // "was it loud here", and a sum would make a moment where both speak at once
                    // look twice as loud as the argument that followed it.
                    if v > acc[b] {
                        acc[b] = v;
                    }
                }
            }
            from = to;
        }

        // Normalized against the loudest moment of THIS recording, and through a square root: a
        // quiet conversation must be visible as speech, not as a flat line at the bottom.
        let loudest = acc.iter().copied().fold(0f32, f32::max).max(1.0);
        let peaks: Vec<f32> = acc.iter().map(|v| (v / loudest).sqrt()).collect();

        let out = json!({ "duration_sec": duration, "buckets": buckets, "peaks": peaks });
        let _ = fs::write(&cache, serde_json::to_vec(&out).unwrap_or_default());
        Ok(out)
    }
}
