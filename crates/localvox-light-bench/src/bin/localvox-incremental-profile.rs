//! Finite native reference on an explicitly selected public PCM. No devices/daemon/LLM.
use anyhow::{ensure, Context, Result};
use clap::Parser;
use localvox_light_core::{
    artifacts,
    chunks::{self, capture, planning, ChunkParams, ChunkRecorder, SessionMeta},
    versions::VersionStore,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
#[path = "profile/resources.rs"]
mod resources;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    model: PathBuf,
    #[arg(long)]
    worker: PathBuf,
    /// A new isolated directory. Never an existing archive.
    #[arg(long)]
    output: PathBuf,
    /// Explicit short probe; omitted for the fixed whole-recording reference.
    #[arg(long)]
    limit_sec: Option<u64>,
    #[arg(long, default_value_t = 3)]
    pairs: usize,
    /// Explicit comparison mode; ordinary incremental runs reuse one owned ASR server.
    #[arg(long)]
    one_shot: bool,
    /// Additional comparison against current capture/planner with ASR only after Stop.
    /// Omitted preserves the original legacy-offline reference; never changes its gate.
    #[arg(long)]
    current_capture_baseline: bool,
}
fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn file_sha(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buf = [0; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        digest.update(&buf[..n]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
fn save(path: &Path, value: &Value) -> Result<()> {
    artifacts::atomic_write(path, &serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

struct Recording {
    root: PathBuf,
    session: PathBuf,
    recorders: [ChunkRecorder; 2],
    owner: Option<Arc<Mutex<capture::Writer>>>,
    planner: Option<planning::Handle>,
    planners: planning::Planners,
}
impl Recording {
    fn new(root: &Path, incremental: bool, generation: usize) -> Result<Self> {
        let session = root.join("sessions/recorded");
        fs::create_dir_all(session.join("audio"))?;
        let meta = Arc::new(Mutex::new(SessionMeta {
            sample_rate: 16000,
            lang: Some("ru".into()),
            capture_generation: incremental
                .then(|| format!("0199a774-0000-7000-8000-{generation:012x}")),
            ..Default::default()
        }));
        chunks::save_meta_checked(&session.join("meta.json"), &meta.lock().unwrap())?;
        let owner = if incremental {
            Some(Arc::new(Mutex::new(capture::Writer::begin(
                &session,
                meta.lock().unwrap().capture_generation.clone().unwrap(),
            )?)))
        } else {
            None
        };
        let planners = planning::Planners::default();
        let planner = owner
            .as_ref()
            .map(|owner| planners.attach(owner.clone(), "unused-WAV".into()))
            .transpose()?;
        let params = Arc::new(ChunkParams {
            audio_dir: session.join("audio"),
            meta_path: session.join("meta.json"),
            chunk_sec: 300.0,
            flac: false,
            ffmpeg: "unused-WAV".into(),
        });
        let recorders = std::array::from_fn(|id| match &owner {
            Some(owner) => ChunkRecorder::for_capture(
                id as u8,
                params.clone(),
                meta.clone(),
                owner.clone(),
                Arc::new(chunks::codec::Conversions::default()),
            ),
            None => ChunkRecorder::new(id as u8, params.clone(), meta.clone()),
        });
        Ok(Self {
            root: root.into(),
            session,
            owner,
            planner,
            planners,
            recorders,
        })
    }
    fn feed(&mut self, pcm: &[i16], seal: bool) -> Result<()> {
        self.recorders[0].feed(pcm);
        self.recorders[1].feed(&vec![0; pcm.len()]);
        if seal {
            // ChunkRecorder already rotates exactly at 300 s; do not create extra empty seals.
            if self.owner.is_some() {
                self.wait_plan(false)?;
            }
        }
        Ok(())
    }
    fn wait_plan(&self, complete: bool) -> Result<()> {
        let revision = capture::read(&self.session)?.revision;
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Ok(h) = fs::read(self.session.join(planning::FILE))
                .map_err(anyhow::Error::from)
                .and_then(|b| Ok(serde_json::from_slice::<planning::Snapshot>(&b)?))
            {
                if h.capture_revision >= revision && (!complete || h.complete) {
                    break;
                }
            }
            ensure!(
                Instant::now() < deadline,
                "planner did not reach revision {revision}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    }
    fn stop(&mut self) -> Result<()> {
        for r in &mut self.recorders {
            r.finalize_current();
        }
        if let Some(owner) = &self.owner {
            owner.lock().unwrap().end(
                "native reference Stop",
                false,
                [
                    (0, self.recorders[0].end_samples()),
                    (1, self.recorders[1].end_samples()),
                ]
                .into(),
            )?;
            self.planner.take();
            self.wait_plan(true)?;
        }
        // Finalized recorders contain no live file; planner is joined outside the capture path.
        drop(std::mem::take(&mut self.planners));
        // Recorders retain their capture references until this Recording is dropped.
        Ok(())
    }
}
fn worker(
    args: &Args,
    root: &Path,
    session: &Path,
    prefix: bool,
    number: usize,
    resident: Option<&mut Option<localvox_light_process::StdioSession>>,
) -> Result<Value> {
    if let Some(resident) = resident {
        return resident_worker(args, root, session, prefix, number, resident);
    }
    let mut command = std::process::Command::new(&args.worker);
    command
        .arg(session)
        .arg("--work-dir")
        .arg(root)
        .arg("--model-dir")
        .arg(&args.model)
        .current_dir(root)
        .env("LOCALVOX_DIARIZE", "off")
        .env("LOCALVOX_LANG", "ru");
    if prefix {
        command.arg("--worker-prefix");
    } else {
        command.env("LOCALVOX_PROFILE_WINDOWS", root.join("windows.jsonl"));
    }
    let stopped = Arc::new(AtomicBool::new(false));
    let mut monitor = None;
    let result = localvox_light_process::run(
        command,
        None,
        localvox_light_process::Limits::new(Duration::from_secs(7200)),
        || false,
        |pid, _| {
            if monitor.is_none() {
                let stopped = stopped.clone();
                monitor = Some(std::thread::spawn(move || {
                    resources::observe(pid, &stopped)
                }));
            }
        },
    );
    stopped.store(true, Ordering::Relaxed);
    let metrics = monitor
        .map(|t| t.join().unwrap())
        .unwrap_or_else(resources::unavailable);
    let output = result?;
    fs::write(root.join(format!("worker-{number}.stdout")), &output.stdout)?;
    fs::write(root.join(format!("worker-{number}.stderr")), &output.stderr)?;
    ensure!(
        output.termination.success(),
        "native worker failed: {:?}: {}",
        output.termination,
        String::from_utf8_lossy(&output.stderr)
    );
    let report = if prefix {
        serde_json::from_slice(&output.stdout)?
    } else {
        Value::Null
    };
    Ok(
        json!({"prefix":prefix,"spawned":true,"wall_sec":output.elapsed.as_secs_f64(),"resources":metrics,"report":report}),
    )
}

fn resident_worker(
    args: &Args,
    root: &Path,
    session: &Path,
    prefix: bool,
    number: usize,
    resident: &mut Option<localvox_light_process::StdioSession>,
) -> Result<Value> {
    use localvox_light_core::jobs::asr_worker::{Reply, Request};
    let start = Instant::now();
    let spawned = resident.is_none();
    if spawned {
        let mut command = std::process::Command::new(&args.worker);
        command
            .arg("--worker-serve")
            .arg("--work-dir")
            .arg(root)
            .current_dir(root)
            .env("LOCALVOX_DIARIZE", "off")
            .env("LOCALVOX_LANG", "ru")
            .env("LOCALVOX_PROFILE_WINDOWS", root.join("windows.jsonl"));
        *resident = Some(localvox_light_process::StdioSession::spawn(command)?);
    }
    let server = resident.as_mut().context("native resident missing")?;
    let pid = server.pid();
    let before = if spawned {
        json!({"cpu_sec":0.0,"read_bytes":0.0,"write_bytes":0.0})
    } else {
        resources::Process::open(pid)
            .map(|p| p.read())
            .unwrap_or_else(resources::unavailable)
    };
    let stopped = Arc::new(AtomicBool::new(false));
    let signal = stopped.clone();
    let observer = std::thread::spawn(move || resources::observe(pid, &signal));
    let mut request = Request::new(number as u64 + 1, session.into(), args.model.clone());
    request.prefix = prefix;
    let result = server.request(
        &serde_json::to_vec(&request)?,
        localvox_light_process::Limits::new(Duration::from_secs(7200)),
        || false,
        |_, _| {},
    );
    let active_wall = start.elapsed().as_secs_f64();
    stopped.store(true, Ordering::Relaxed);
    let mut metrics = observer.join().unwrap();
    for field in ["cpu_sec", "read_bytes", "write_bytes"] {
        metrics[field] = metrics[field]
            .as_f64()
            .zip(before[field].as_f64())
            .map(|(a, b)| json!((a - b).max(0.0)))
            .unwrap_or(Value::Null);
    }
    metrics["scope"] =
        json!("resident_process_request_delta; peaks are process lifetime high-water");
    let output = result?;
    fs::write(root.join(format!("worker-{number}.stdout")), &output.stdout)?;
    fs::write(root.join(format!("worker-{number}.stderr")), &output.stderr)?;
    let reply: Reply = serde_json::from_slice(&output.stdout)?;
    reply.validate(request.id)?;
    let report = reply.result.map_err(anyhow::Error::msg)?;
    Ok(
        json!({"prefix":prefix,"spawned":spawned,"model_loads":reply.model_loads,"pid":pid,"wall_sec":active_wall,"resources":metrics,"report":report}),
    )
}
fn trace(root: &Path) -> Result<Vec<Value>> {
    fs::read_to_string(root.join("windows.jsonl"))?
        .lines()
        .map(|l| Ok(serde_json::from_str(l)?))
        .collect()
}
fn cached_trace(session: &Path, identity: &str) -> Result<Vec<Value>> {
    use localvox_light_core::incremental::{consume_windows, Cache, Horizon, Identity};
    let saved: Identity = serde_json::from_slice(&fs::read(
        session.join(format!("incremental/{identity}/identity.json")),
    )?)?;
    ensure!(
        saved.fingerprint()? == identity,
        "cache identity differs from final receipt"
    );
    let cache = Cache::open(session, &saved)?;
    let horizon = Horizon::read(session)?;
    // Audit actual PCM against both legacy JSON and framed results outside the
    // measured operation. No checkpoint adoption and no missing result repair.
    let mut rows = Vec::new();
    consume_windows(
        session,
        &horizon,
        &BTreeMap::new(),
        || false,
        |window| {
            let record = cache
                .lookup(&window)?
                .context("final ASR cache result is missing")?;
            let d = record.descriptor();
            rows.push(json!({"source":d.source,"start":d.start_sample,"end":d.end_sample,"kind":d.kind,"pcm_sha256":d.pcm_sha256}));
            Ok(())
        },
    )?;
    rows.sort_by_key(|r| (r["source"].as_u64(), r["start"].as_u64()));
    Ok(rows)
}
fn number(value: &Value, key: &str) -> f64 {
    value[key].as_f64().unwrap_or(0.0)
}
fn run(
    args: &Args,
    name: &str,
    incremental: bool,
    batch: usize,
    n: u64,
    generation: usize,
) -> Result<Value> {
    let root = args.output.join(name);
    fs::create_dir(&root)?;
    let mut recording = Recording::new(
        &root,
        incremental || args.current_capture_baseline,
        generation,
    )?;
    let process = resources::Process::open(std::process::id());
    let before = process
        .as_ref()
        .map(|p| p.read())
        .unwrap_or_else(resources::unavailable);
    let start = Instant::now();
    let mut producer_wall = 0.0;
    let mut workers = Vec::new();
    let mut resident = None;
    let mut from = 0;
    let mut chunks = 0;
    while from < n {
        let step = Instant::now();
        let to = (from + 300 * 16000).min(n);
        let bytes = chunks::codec::range(&args.input, from, to, Path::new("unused-WAV"))?;
        let pcm: Vec<_> = bytes
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        let seal = to - from == 300 * 16000;
        recording.feed(&pcm, seal)?;
        producer_wall += step.elapsed().as_secs_f64();
        chunks += usize::from(seal);
        from = to;
        if incremental && seal && chunks % batch == 0 {
            let out = worker(
                args,
                &recording.root,
                &recording.session,
                true,
                workers.len(),
                (!args.one_shot).then_some(&mut resident),
            )?;
            eprintln!(
                "{name}: {}/{} s · prefix {:.2} s",
                from / 16000,
                n / 16000,
                out["wall_sec"].as_f64().unwrap()
            );
            workers.push(out);
        }
    }
    let stop = Instant::now();
    let close = Instant::now();
    recording.stop()?;
    producer_wall += close.elapsed().as_secs_f64();
    // Standalone final CLI needs the real capture lease released, just like after Stop.
    let session = recording.session.clone();
    let root = recording.root.clone();
    drop(recording);
    let out = worker(
        args,
        &root,
        &session,
        false,
        workers.len(),
        (incremental && !args.one_shot).then_some(&mut resident),
    )?;
    workers.push(out);
    let stop_ready = stop.elapsed().as_secs_f64();
    let wall = start.elapsed().as_secs_f64();
    let after = process
        .as_ref()
        .map(|p| p.read())
        .unwrap_or_else(resources::unavailable);
    let manifest = VersionStore::open(&session)?.load();
    ensure!(manifest.versions.len() == 1, "unexpected final versions");
    let version = &manifest.versions[0];
    let cache = &version.params["asr_cache"];
    let (_, lines) = artifacts::source_transcript(&session)?;
    let final_text: Vec<_> = lines
        .iter()
        .map(|l| json!([l.source_id, l.start_sec, l.end_sec, l.text]))
        .collect();
    let actual = trace(&root)?;
    let mut coverage = BTreeMap::<u64, u64>::new();
    for window in &actual {
        let end = coverage
            .entry(window["source"].as_u64().context("trace source")?)
            .or_default();
        ensure!(
            window["start"].as_u64() == Some(*end),
            "actual full-cook window gap/overlap"
        );
        *end = window["end"].as_u64().context("trace end")?;
    }
    ensure!(
        coverage == [(0, n), (1, n)].into(),
        "final window coverage differs from input"
    );
    if incremental {
        ensure!(
            cached_trace(
                &session,
                cache["recognizer"].as_str().context("cache recognizer")?
            )? == actual,
            "early cache and actual full-cook PCM windows differ"
        );
    }
    let early_calls = workers
        .iter()
        .filter_map(|w| w["report"]["asr_calls"].as_u64())
        .sum::<u64>();
    let final_calls = cache["calls"].as_u64().context("final ASR counter")?;
    let worker_wall = workers.iter().map(|w| number(w, "wall_sec")).sum::<f64>();
    let cpu_workers: Option<f64> = workers
        .iter()
        .map(|w| w["resources"]["cpu_sec"].as_f64())
        .sum();
    let delta = |key: &str| {
        after[key]
            .as_f64()
            .zip(before[key].as_f64())
            .map(|(after, before)| after - before)
    };
    let producer_cpu = delta("cpu_sec");
    let worker_io = |key: &str| {
        workers
            .iter()
            .map(|w| w["resources"][key].as_f64())
            .sum::<Option<f64>>()
    };
    let loads = if incremental && !args.one_shot {
        workers
            .iter()
            .map(|w| w["model_loads"].as_u64().unwrap())
            .sum::<u64>() as usize
    } else {
        workers
            .iter()
            .filter(|w| w["report"]["model_loaded"] == true)
            .count()
            + usize::from(final_calls > 0)
    };
    let starts = workers.iter().filter(|w| w["spawned"] == true).count();
    let result = json!({"name":name,"incremental":incremental,"batch_chunks":batch,"input_samples":n,"producer_wall_sec":producer_wall,"worker_wall_sec":worker_wall,"active_wall_sec":producer_wall+worker_wall,"observed_run_wall_sec":wall,"stop_to_ready_sec":stop_ready,
        "producer_cpu_sec":producer_cpu,"worker_cpu_sec":cpu_workers,"cpu_sec":producer_cpu.zip(cpu_workers).map(|(a,b)|a+b),"producer_logical_read_bytes":delta("read_bytes"),"producer_logical_write_bytes":delta("write_bytes"),"producer_process_after":after,
        "worker_logical_read_bytes":worker_io("read_bytes"),"worker_logical_write_bytes":worker_io("write_bytes"),"worker_starts":starts,"model_loads":loads,
        "early_asr_calls":early_calls,"final_asr_calls":final_calls,"asr_calls":early_calls+final_calls,"final_cache_hits":cache["hits"],"windows":actual.len(),"window_sha256":sha(&serde_json::to_vec(&actual)?),"text_sha256":sha(&serde_json::to_vec(&final_text)?),"text_lines":lines.len(),"workers":workers});
    save(&root.join("text.json"), &json!(final_text))?;
    save(&root.join("run-report.json"), &result)?;
    Ok(result)
}

fn main() -> Result<()> {
    std::env::set_var("LOCALVOX_LANG", "ru");
    // The receipt and both worker paths must fingerprint the same recipe.
    std::env::set_var("LOCALVOX_DIARIZE", "off");
    let mut args = Args::parse();
    args.input = args.input.canonicalize()?;
    args.model = args.model.canonicalize()?;
    args.worker = args.worker.canonicalize()?;
    ensure!(
        !args.output.exists(),
        "output already exists; use a new isolated reference workspace"
    );
    fs::create_dir_all(&args.output)?;
    args.output = args.output.canonicalize()?;
    fs::write(
        args.output.join(".incremental-profile-workspace"),
        "localvox-incremental-profile/v1\n",
    )?;
    ensure!(
        (1..=3).contains(&args.pairs) && (args.limit_sec.is_some() || args.pairs == 3),
        "whole reference requires three fixed pairs"
    );
    let all = chunks::codec::samples(&args.input)?;
    let n = args.limit_sec.map_or(all, |s| all.min(s * 16000));
    ensure!(n > 0, "empty reference PCM");
    let selection = localvox_light_core::cook::model::Selection::resolve(
        &args.output,
        &localvox_light_core::cook::CookParams {
            model_dir: args.model.clone(),
            ..Default::default()
        },
        || false,
    )?;
    let plan = json!({"input":args.input,"input_file_sha256":file_sha(&args.input)?,"samples":n,"whole_source_samples":all,"worker":args.worker,"worker_sha256":file_sha(&args.worker)?,"offline_capture":if args.current_capture_baseline {"current capture and planner; ASR only after Stop"} else {"legacy capture without ledger/planner; original reference"},"worker_lifecycle":if args.one_shot {"one-shot"} else {"resident incremental; one-shot offline"},"model":selection.identity,"window_params":{"max_sec":15,"min_sec":8,"silence_ms":500},"transport_samples":300*16000,"source1":"actual zero PCM","diarization":"off; independent hardware/model acceptance","schedule":["offline-1","incremental-1-steady","offline-2","incremental-2-burst3","offline-3","incremental-3-steady"],"gate":{"active_wall_overhead":0.20,"every_pair":true,"aggregate":true},"cpu_count":std::thread::available_parallelism()?.get()});
    save(&args.output.join("plan.json"), &plan)?;
    let mut runs = Vec::new();
    let mut pairs = Vec::new();
    for pair in 0..args.pairs {
        let batch = if pair == 1 { 3 } else { 1 };
        let offline = run(
            &args,
            &format!("offline-{}", pair + 1),
            false,
            1,
            n,
            pair * 2 + 1,
        )?;
        runs.push(offline.clone());
        save(
            &args.output.join("report.json"),
            &json!({"complete":false,"runs":runs,"pairs":pairs}),
        )?;
        let early = run(
            &args,
            &format!("incremental-{}", pair + 1),
            true,
            batch,
            n,
            pair * 2 + 2,
        )?;
        ensure!(
            early["window_sha256"] == offline["window_sha256"],
            "native PCM windows differ"
        );
        let same_text = early["text_sha256"] == offline["text_sha256"];
        let same_calls = early["asr_calls"] == offline["asr_calls"];
        let overhead =
            number(&early, "active_wall_sec") / number(&offline, "active_wall_sec") - 1.0;
        let result = json!({"pair":pair+1,"batch_chunks":batch,"overhead":overhead,"pass":overhead<=0.20 && same_text && same_calls,"stop_to_ready_sec":early["stop_to_ready_sec"],"same_windows":true,"same_text":same_text,"same_asr_calls":same_calls});
        eprintln!("pair {}: {}", pair + 1, result);
        runs.push(early);
        pairs.push(result);
        save(
            &args.output.join("report.json"),
            &json!({"complete":false,"runs":runs,"pairs":pairs}),
        )?;
    }
    let full = runs
        .iter()
        .filter(|r| r["incremental"] == false)
        .map(|r| number(r, "active_wall_sec"))
        .sum::<f64>();
    let early = runs
        .iter()
        .filter(|r| r["incremental"] == true)
        .map(|r| number(r, "active_wall_sec"))
        .sum::<f64>();
    let pass = pairs.iter().all(|p| p["pass"] == true) && early <= full * 1.20;
    ensure!(
        file_sha(&args.input)? == plan["input_file_sha256"],
        "public input changed during reference"
    );
    ensure!(
        file_sha(&args.worker)? == plan["worker_sha256"],
        "worker changed during reference"
    );
    let current = localvox_light_core::cook::model::Selection::resolve(
        &args.output,
        &localvox_light_core::cook::CookParams {
            model_dir: args.model.clone(),
            ..Default::default()
        },
        || false,
    )?;
    ensure!(
        serde_json::to_value(current.identity)? == plan["model"],
        "model dependencies changed during reference"
    );
    save(
        &args.output.join("report.json"),
        &json!({"complete":true,"reference":args.limit_sec.is_none(),"pass":pass,"aggregate_overhead":early/full-1.0,"runs":runs,"pairs":pairs}),
    )?;
    println!(
        "reference complete; pass={pass}; output={}",
        args.output.display()
    );
    Ok(())
}
