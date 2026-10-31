use super::*;
use localvox_light_core::{
    jobs::discovery,
    processing,
    versions::{VersionEntry, VersionStore},
};
use std::{fs, sync::mpsc, time::Instant};

fn policy() -> (bool, bool, bool) {
    (true, false, false)
}
fn wanted() -> jobs::Wanted {
    jobs::Wanted::new(true, false, None, "fixture")
}
fn probe(root: &Path) {
    fs::create_dir_all(root.join("sessions/probe")).unwrap();
    JobQueue::mutate(root, |queue| {
        assert!(queue.enqueue_cook("probe", false, false, false));
    });
}

fn while_scan_is_paused(post: bool) {
    let root = tempfile::tempdir().unwrap();
    probe(root.path());
    let work_dir = root.path();
    let (entered_tx, entered) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let (done_tx, done) = mpsc::channel();
    std::thread::scope(|scope| {
        let scan = scope.spawn(move || {
            discover_with(
                work_dir,
                || {
                    entered_tx.send(()).unwrap();
                    gate.recv_timeout(Duration::from_secs(10)).unwrap();
                    discovery::scan(work_dir, 0, &wanted())
                },
                policy,
            )
        });
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        let request = scope.spawn(move || {
            if post {
                use localvox_light_api::{
                    archive::Archive,
                    http::{respond, HttpConfig},
                };
                let archive = Archive::new(work_dir.to_path_buf());
                let config = HttpConfig {
                    token: Some("synthetic-token".into()),
                    bind: "127.0.0.1:0".into(),
                };
                let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
                let address = format!("http://{}", server.server_addr());
                std::thread::scope(|scope| {
                    let serving = scope.spawn(|| {
                        let mut request = server
                            .recv_timeout(Duration::from_secs(8))
                            .unwrap()
                            .unwrap();
                        let mut body = String::new();
                        request.as_reader().read_to_string(&mut body).unwrap();
                        let (status, json) = respond(
                            &archive,
                            &config,
                            request.method(),
                            request.url(),
                            Some("Bearer synthetic-token"),
                            true,
                            &body,
                        );
                        request
                            .respond(
                                tiny_http::Response::from_string(json.to_string())
                                    .with_status_code(status),
                            )
                            .unwrap();
                    });
                    let client =
                        localvox_light_client::Client::new(&address, "synthetic-token".into())
                            .unwrap();
                    let accepted = client
                        .ingest("https://example.test/while-scanning")
                        .unwrap();
                    assert!(accepted.session.is_some());
                    serving.join().unwrap();
                });
            } else {
                JobQueue::mutate(work_dir, |queue| {
                    assert!(queue.start_phase(0, &[JobPhase::Prepare]).is_some());
                });
            }
            done_tx.send(()).unwrap();
        });
        let progressed_before_release = done.recv_timeout(Duration::from_secs(2)).is_ok();
        release.send(()).unwrap();
        scan.join().unwrap();
        request.join().unwrap();
        assert!(
            progressed_before_release,
            "queue work must finish while archive discovery is still paused"
        );
    });
}

#[test]
fn a_worker_claim_does_not_wait_for_archive_discovery() {
    while_scan_is_paused(false);
}

#[test]
fn an_actual_import_post_does_not_wait_for_archive_discovery() {
    while_scan_is_paused(true);
}

fn archive(root: &Path, count: usize) {
    let body = (0..7200).map(|second| serde_json::json!({"source_id":0,"start_sec":second,"end_sec":second+1,"text":"Synthetic meeting words for isolated queue discovery measurement."}).to_string()+"\n").collect::<String>();
    for index in 0..count {
        let dir = root.join("sessions").join(format!("archive-{index:02}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("meta.json"), b"{}").unwrap();
        let store = VersionStore::open(&dir).unwrap();
        let (id, path) = store.next_version("synthetic").unwrap();
        fs::write(&path, &body).unwrap();
        store
            .commit(VersionEntry {
                id,
                file: path.file_name().unwrap().to_string_lossy().into_owned(),
                label: "raw".into(),
                model: "fixture".into(),
                params: serde_json::json!({}),
                created_at: "2026-10-31T00:00:00Z".into(),
                parents: vec![],
            })
            .unwrap();
        fs::write(dir.join("summary.md"), "Synthetic summary").unwrap();
        for kind in [processing::TRANSCRIPT, processing::SUMMARY] {
            processing::record(
                &dir,
                kind,
                "fixture",
                processing::Outcome::Ok,
                None,
                Some(id),
            )
            .unwrap();
        }
    }
}

#[test]
#[ignore = "synthetic queue/discovery timing profile; no models or user archive"]
fn profile_queue_discovery() {
    let mut measurements = vec![];
    for count in [4, 16] {
        let root = tempfile::tempdir().unwrap();
        probe(root.path());
        archive(root.path(), count);
        let work_dir = root.path();
        let mut samples = vec![];
        for sample in 0..4 {
            JobQueue::mutate(root.path(), |queue| {
                queue.reclaim_session("probe");
            });
            let (entered_tx, entered) = mpsc::channel();
            let (elapsed_tx, elapsed) = mpsc::channel();
            std::thread::scope(|scope| {
                let scan = scope.spawn(move || {
                    discover_with(
                        work_dir,
                        || {
                            let began = Instant::now();
                            entered_tx.send(()).unwrap();
                            let found = discovery::scan(work_dir, 0, &wanted());
                            elapsed_tx
                                .send(began.elapsed().as_secs_f64() * 1000.0)
                                .unwrap();
                            found
                        },
                        policy,
                    )
                });
                entered.recv_timeout(Duration::from_secs(10)).unwrap();
                let began = Instant::now();
                JobQueue::mutate(root.path(), |queue| {
                    assert!(queue.start_phase(0, &[JobPhase::Prepare]).is_some());
                });
                let claim_ms = began.elapsed().as_secs_f64() * 1000.0;
                let queued = scan.join().unwrap();
                assert_eq!(queued, 0);
                let scan_ms = elapsed.recv().unwrap();
                if sample > 0 {
                    samples.push((claim_ms, scan_ms));
                }
            });
        }
        let measure = |mut values: Vec<f64>| {
            values.sort_by(f64::total_cmp);
            serde_json::json!({"median":values[1],"min":values[0],"max":values[2]})
        };
        measurements.push(
            serde_json::json!({"sessions":count,"transcript_seconds_per_session":7200,
            "claim_ms": measure(samples.iter().map(|s|s.0).collect()),
            "scan_ms": measure(samples.iter().map(|s|s.1).collect())}),
        );
    }
    let report =
        serde_json::json!({"profile":"dev","samples":3,"warmup":1,"measurements":measurements});
    if let Some(path) = std::env::var_os("LOCALVOX_DISCOVERY_PROFILE_OUTPUT") {
        fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
