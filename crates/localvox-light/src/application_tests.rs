use super::*;

#[test]
fn daemon_reports_actual_cleanup_outcome_before_releasing_the_http_response() {
    for failed in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let final_chunk = root.path().join("closed-chunk");
        let output = final_chunk.clone();
        let running = Arc::new(AtomicBool::new(true));
        let application = localvox_light_api::application::ApplicationControl::new(running.clone());
        let stop = running.clone();
        let engine = std::thread::spawn(move || {
            while stop.load(Ordering::SeqCst) {
                std::thread::yield_now();
            }
            std::fs::write(output, b"saved").unwrap();
        });
        let daemon = Daemon {
            running,
            application: application.clone(),
            record_pcm: Arc::new(AtomicBool::new(true)),
            api_addr: None,
            work_dir: root.path().to_string_lossy().into_owned(),
            fatal: Arc::new(std::sync::Mutex::new(
                failed.then(|| "capture finalization failed".into()),
            )),
            engine,
            autocook: Some(std::thread::spawn(|| {})),
            voice: None,
            ask_worker: std::thread::spawn(|| {}),
        };
        let request = application.request().unwrap();
        let thread = std::thread::spawn(move || daemon.shutdown());
        let result = request.wait(std::time::Duration::from_secs(3));
        assert_eq!(result.is_err(), failed);
        assert_eq!(std::fs::read(final_chunk).unwrap(), b"saved");
        assert!(
            !thread.is_finished(),
            "shutdown must leave time for HTTP response"
        );
        drop(request);
        assert_eq!(thread.join().unwrap().is_err(), failed);
    }
}
