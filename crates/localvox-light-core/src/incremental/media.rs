//! Window-proved PCM for one immutable HTTP audio response.
use super::*;
use crate::chunks::codec::PcmReader;

pub struct AudioReader {
    session: PathBuf,
    horizon: Horizon,
    end: u64,
    containers: BTreeMap<u8, (u32, PcmReader)>,
    windows: BTreeMap<u8, (u64, Vec<i16>)>,
}
impl Snapshot {
    pub fn audio_reader(&self, session: &Path) -> Result<AudioReader> {
        let horizon = self.input.horizon(session)?;
        ensure!(
            self.audio_end <= self.checkpoint.common_end,
            "invalid snapshot media boundary"
        );
        Ok(AudioReader {
            session: session.into(),
            horizon,
            end: self.audio_end,
            containers: BTreeMap::new(),
            windows: BTreeMap::new(),
        })
    }
}
impl AudioReader {
    pub fn range(&mut self, id: u8, from: u64, to: u64) -> Result<Vec<u8>> {
        ensure!(
            from <= to && to <= self.end && to - from <= 512 * 1024 + 1,
            "media read exceeds snapshot boundary/buffer"
        );
        let plan = self
            .horizon
            .plan
            .sources
            .get(&id)
            .context("unknown media source")?;
        let planned_end = plan.closed_end;
        ensure!(
            to <= planned_end || plan.eof,
            "snapshot crosses an unclosed source"
        );
        let mut at = from;
        let mut result = Vec::with_capacity(((to - from) * 2) as usize);
        while at < to.min(planned_end) {
            let descriptor = self.horizon.windows[&id]
                .iter()
                .find(|w| w.start_sample <= at && w.end_sample > at)
                .context("media window gap")?
                .clone();
            if !self
                .windows
                .get(&id)
                .is_some_and(|(start, _)| *start == descriptor.start_sample)
            {
                let mut pcm =
                    Vec::with_capacity((descriptor.end_sample - descriptor.start_sample) as usize);
                let mut next = descriptor.start_sample;
                for range in &self.horizon.input.sources[&id].ranges {
                    let a = next.max(range.start_sample);
                    let b = descriptor
                        .end_sample
                        .min(range.start_sample + range.samples);
                    if a >= b {
                        continue;
                    }
                    ensure!(
                        range.kind == capture::Kind::Audio && a == next,
                        "unconfirmed media source range"
                    );
                    if !self
                        .containers
                        .get(&id)
                        .is_some_and(|(sequence, _)| *sequence == range.sequence)
                    {
                        self.containers.remove(&id);
                        let reader = PcmReader::open(
                            &self.session.join("audio"),
                            &range.stem(id),
                            range.samples,
                            self.horizon.generation(),
                            &PathBuf::from(crate::chunks::resolve_ffmpeg_for_decode()),
                        )
                        .with_context(|| {
                            format!(
                                "audio source {id} range {}..{}",
                                range.start_sample,
                                range.start_sample + range.samples
                            )
                        })?;
                        self.containers.insert(id, (range.sequence, reader));
                    }
                    pcm.extend(
                        self.containers
                            .get_mut(&id)
                            .unwrap()
                            .1
                            .range(a - range.start_sample, b - range.start_sample)?,
                    );
                    next = b;
                    if next == descriptor.end_sample {
                        break;
                    }
                }
                VerifiedWindow::new(&descriptor, &pcm).with_context(|| {
                    format!(
                        "media source {id} window {}..{}",
                        descriptor.start_sample, descriptor.end_sample
                    )
                })?;
                self.windows.insert(id, (descriptor.start_sample, pcm));
            }
            let (start, pcm) = &self.windows[&id];
            let until = to.min(descriptor.end_sample);
            for s in &pcm[(at - *start) as usize..(until - *start) as usize] {
                result.extend_from_slice(&s.to_le_bytes());
            }
            at = until;
        }
        // A shorter source is padded only after its explicitly proved EOF.
        if at < to {
            ensure!(
                self.horizon.plan.sources[&id].eof,
                "missing media is not silence"
            );
            result.resize(((to - from) * 2) as usize, 0);
        }
        Ok(result)
    }
}
