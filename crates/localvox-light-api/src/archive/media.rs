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
}
impl AudioStream {
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
            let end = self.end.min(self.position + WINDOW_BYTES as u64);
            let from = (self.position - 44) / 2;
            let to = (end - 44).div_ceil(2);
            let mic = source_pcm(&self.dir, &self.meta, 0, from, to).unwrap_or_default();
            let sys = source_pcm(&self.dir, &self.meta, 1, from, to).unwrap_or_default();
            let mixed = mix(&mic, &sys);
            let skip = ((self.position - 44) % 2) as usize;
            let count = (end - self.position) as usize;
            self.buffer.clear();
            self.buffer.extend(mixed.into_iter().skip(skip).take(count));
            self.buffer.resize(count, 0);
            self.consumed = 0;
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
        let dir = self.session_dir(session)?;
        let meta: localvox_light_core::chunks::SessionMeta =
            serde_json::from_slice(&fs::read(dir.join("meta.json")).context("нет meta.json")?)
                .context("meta.json не читается")?;
        let duration = meta
            .chunks
            .iter()
            .map(|c| c.start_offset_sec + c.duration_sec)
            .fold(0.0_f64, f64::max);
        let pcm_len = (duration * SR) as u64;
        anyhow::ensure!(
            pcm_len <= (u32::MAX as u64 - 36) / 2,
            "запись превышает размер WAV RIFF"
        );
        let file_len = 44 + pcm_len * 2;
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
        Ok(AudioStream {
            total: file_len,
            start,
            end,
            dir,
            meta,
            position: start,
            buffer: Vec::new(),
            consumed: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn open_ended_hour_is_lazy_and_pcm_windows_are_bounded() {
        let directory = tempfile::tempdir().unwrap();
        let session = directory.path().join("sessions/hour");
        fs::create_dir_all(&session).unwrap();
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
