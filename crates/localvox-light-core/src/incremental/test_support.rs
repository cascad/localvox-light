use super::*;
use crate::chunks::{ChunkParams, ChunkRecorder, SessionMeta};
use std::sync::{Arc, Mutex};

pub(super) struct Fixture {
    pub(super) root: tempfile::TempDir,
    pub(super) session: PathBuf,
    pub(super) horizon: Horizon,
    pub(super) pcm: BTreeMap<u8, Vec<i16>>,
    pub(super) identity: Identity,
}
impl Fixture {
    pub(super) fn new(ends: [&[u64]; 2], terminal: bool) -> Self {
        Self::with_transport(ends, terminal, usize::MAX)
    }
    pub(super) fn with_transport(ends: [&[u64]; 2], terminal: bool, chunk_samples: usize) -> Self {
        let root = tempfile::tempdir().unwrap();
        let session = root.path().join("session");
        fs::create_dir_all(session.join("audio")).unwrap();
        let generation = uuid::Uuid::now_v7().to_string();
        let meta = SessionMeta {
            capture_generation: Some(generation.clone()),
            sample_rate: 16000,
            ..Default::default()
        };
        fs::write(
            session.join("meta.json"),
            serde_json::to_vec(&meta).unwrap(),
        )
        .unwrap();
        let writer = Arc::new(Mutex::new(
            capture::Writer::begin(&session, generation.clone()).unwrap(),
        ));
        let shared_meta = Arc::new(Mutex::new(meta));
        let params = Arc::new(ChunkParams {
            audio_dir: session.join("audio"),
            meta_path: session.join("meta.json"),
            chunk_sec: 300.0,
            flac: false,
            ffmpeg: "unused".into(),
        });
        let recipe = planning::Recipe::default();
        let fingerprint = recipe.fingerprint().unwrap();
        let mut sources = BTreeMap::new();
        let mut pcm = BTreeMap::new();
        for id in [0, 1] {
            let n = ends[id as usize].last().copied().unwrap_or(0);
            let samples: Vec<_> = (0..n)
                .map(|i| ((i * 7 + id as u64 * 10) % 32000) as i16)
                .collect();
            let mut rec = ChunkRecorder::for_capture(
                id,
                params.clone(),
                shared_meta.clone(),
                writer.clone(),
                Arc::new(crate::chunks::codec::Conversions::default()),
            );
            for piece in samples.chunks(chunk_samples.max(1)) {
                rec.feed(piece);
                rec.finalize_current();
            }
            let mut at = 0;
            let mut refs = Vec::new();
            for end in ends[id as usize] {
                let d = planning::Descriptor {
                    version: 1,
                    generation: generation.clone(),
                    recipe: fingerprint.clone(),
                    source: id,
                    start_sample: at,
                    end_sample: *end,
                    kind: if id == 1 {
                        WindowKind::Silence
                    } else {
                        WindowKind::Speech
                    },
                    pcm_sha256: pcm_hash(&samples[at as usize..*end as usize]),
                };
                let bytes = serde_json::to_vec(&d).unwrap();
                let path = session.join(d.path());
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, &bytes).unwrap();
                refs.push(planning::Reference {
                    start_sample: at,
                    end_sample: *end,
                    sha256: hash(&bytes),
                });
                at = *end;
            }
            sources.insert(
                id,
                planning::SourcePlan {
                    sequence: capture::read(&session).unwrap().sources[&id].ranges.len() as u32,
                    input_end: n,
                    closed_end: n,
                    eof: terminal,
                    windows: refs,
                },
            );
            pcm.insert(id, samples);
        }
        if terminal {
            writer
                .lock()
                .unwrap()
                .end(
                    "test",
                    false,
                    pcm.iter().map(|(id, s)| (*id, s.len() as u64)).collect(),
                )
                .unwrap();
        }
        let input = capture::read(&session).unwrap();
        let common_end = common_frontier(sources.values().map(|s| (s.closed_end, s.eof)));
        let plan = planning::Snapshot {
            version: 1,
            generation,
            capture_revision: input.revision,
            recipe,
            sources,
            common_end,
            complete: terminal,
        };
        fs::create_dir_all(session.join("capture/plan")).unwrap();
        fs::write(
            session.join(planning::FILE),
            serde_json::to_vec(&plan).unwrap(),
        )
        .unwrap();
        let horizon = Horizon::read(&session).unwrap();
        let model = root.path().join("model.bin");
        let vocab = root.path().join("vocab.txt");
        fs::write(&model, b"synthetic fixture weights").unwrap();
        fs::write(&vocab, b"fixture vocabulary").unwrap();
        let identity = Identity::from_files(
            "fixture".into(),
            "ru".into(),
            "default recipe".into(),
            [("model".into(), model), ("vocab".into(), vocab)].into(),
            || false,
        )
        .unwrap();
        Self {
            root,
            session,
            horizon,
            pcm,
            identity,
        }
    }
    pub(super) fn window(&self, id: u8, n: usize) -> VerifiedWindow<'_> {
        let d = &self.horizon.windows[&id][n];
        VerifiedWindow::new(
            d,
            &self.pcm[&id][d.start_sample as usize..d.end_sample as usize],
        )
        .unwrap()
    }
    pub(super) fn lease(&self) -> File {
        crate::jobs::try_execution_lock(&self.session)
            .unwrap()
            .unwrap()
    }
}
