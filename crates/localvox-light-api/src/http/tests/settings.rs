//! Run settings in a copied test executable with a private cwd/.env. No write
//! in this suite can resolve to the developer's configuration or live daemon.
use super::*;
use localvox_light_client::{Client, Reply};
use std::{
    fs,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn wait_for(path: &Path, child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.is_file() {
        assert!(
            child.try_wait().unwrap().is_none(),
            "settings fixture exited before readiness"
        );
        assert!(Instant::now() < deadline, "settings fixture timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn settings_http_masks_unknown_values_and_reports_snapshot_restart_before_and_after_save() {
    let directory = tempfile::tempdir().unwrap();
    let exe = directory.path().join(if cfg!(windows) {
        "settings-test.exe"
    } else {
        "settings-test"
    });
    fs::copy(std::env::current_exe().unwrap(), &exe).unwrap();
    let env_path = directory.path().join(".env");
    fs::write(&env_path, "# keep this comment\nLOCALVOX_LLM_MODEL=before\nLOCALVOX_API_TOKEN=synthetic-known-secret\nLOCALVOX_NEW_CREDENTIAL=synthetic-unknown-secret\n").unwrap();
    fs::write(
        directory.path().join(".env.defaults"),
        "LOCALVOX_LLM_MODEL=preset-model\nLOCALVOX_LIGHT_CHUNK_SEC=420\n",
    )
    .unwrap();
    let mut command = Command::new(exe);
    command
        .args([
            "--ignored",
            "--exact",
            "http::tests::settings::settings_fixture",
            "--nocapture",
        ])
        .current_dir(directory.path())
        .env("LOCALVOX_SETTINGS_FIXTURE", directory.path())
        .env("LOCALVOX_LLM_MODEL", "before")
        .env_remove("LOCALVOX_LIGHT_MIC")
        .env_remove("LOCALVOX_LIGHT_LOOPBACK_DEVICE")
        .env_remove("LOCALVOX_LIGHT_INCREMENTAL_TRANSCRIBE")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = OwnedChild(command.spawn().unwrap());
    let ready = directory.path().join("address");
    wait_for(&ready, &mut child.0);
    let client = Client::new(
        fs::read_to_string(ready).unwrap().trim(),
        "synthetic-token".into(),
    )
    .unwrap();
    assert!(client.health().unwrap().compatible());
    let raw: Value = client
        .request(
            "GET",
            &["api", "settings"],
            &[],
            None,
            Duration::from_secs(3),
        )
        .unwrap();
    let wire = raw.to_string();
    assert!(!wire.contains("synthetic-known-secret") && !wire.contains("synthetic-unknown-secret"));
    assert_eq!(
        raw["extra"][0],
        json!({"key":"LOCALVOX_NEW_CREDENTIAL","set":true})
    );
    let doc = client.settings().unwrap();
    assert!(doc.settings.iter().any(|s| s.kind == "path"));
    assert!(doc.settings.iter().any(|s| s.kind == "bool"));
    assert!(doc.settings.iter().any(|s| s.kind == "number"));
    let live: Vec<_> = doc
        .settings
        .iter()
        .filter(|s| s.applies == "live")
        .map(|s| s.key.as_str())
        .collect();
    assert_eq!(
        live,
        [
            "LOCALVOX_LLM_SERVER_URLS",
            "LOCALVOX_LIGHT_INCREMENTAL_TRANSCRIBE",
            "LOCALVOX_LIGHT_AUTOCOOK_SUMMARY"
        ]
    );
    assert!(live.iter().all(|s| [
        "LOCALVOX_LIGHT_AUTOCOOK_SUMMARY",
        "LOCALVOX_LIGHT_INCREMENTAL_TRANSCRIBE",
        "LOCALVOX_LIGHT_AUTOCOOK_CLEANUP",
        "LOCALVOX_LIGHT_AUTOCOOK_REFINE",
        "LOCALVOX_LLM_SERVER_URLS"
    ]
    .contains(s)));
    let early = || {
        client
            .settings()
            .unwrap()
            .settings
            .into_iter()
            .find(|s| s.key == "LOCALVOX_LIGHT_INCREMENTAL_TRANSCRIBE")
            .unwrap()
    };
    let early_enabled = || {
        client
            .request::<Value>(
                "GET",
                &["fixture", "early-enabled"],
                &[],
                None,
                Duration::from_secs(3),
            )
            .unwrap()["enabled"]
            .as_bool()
            .unwrap()
    };
    assert_eq!(early().default_value.as_deref(), Some("on"));
    assert!(early().value.is_none() && early_enabled());
    for (value, expected) in [(Some("off"), false), (Some("on"), true), (None, true)] {
        let reply = client
            .save_settings(
                [(
                    "LOCALVOX_LIGHT_INCREMENTAL_TRANSCRIBE".into(),
                    value.map(str::to_owned),
                )]
                .into(),
            )
            .unwrap();
        assert!(reply.need_restart.is_empty());
        assert_eq!(
            early_enabled(),
            expected,
            "the actual scheduler flag must change live"
        );
        assert_eq!(early().value.as_deref(), value);
        assert_eq!(early().default_value.as_deref(), Some("on"));
        let saved = localvox_light_core::env_file::parse(&fs::read_to_string(&env_path).unwrap());
        assert_eq!(
            saved
                .get("LOCALVOX_LIGHT_INCREMENTAL_TRANSCRIBE")
                .map(String::as_str),
            value
        );
    }
    let token = doc
        .settings
        .iter()
        .find(|s| s.key == "LOCALVOX_API_TOKEN")
        .unwrap();
    assert!(token.set && token.value.is_none());
    assert!(token.default_value.is_none());
    let model = doc
        .settings
        .iter()
        .find(|s| s.key == "LOCALVOX_LLM_MODEL")
        .unwrap();
    assert_eq!(model.value.as_deref(), Some("before"));
    assert_eq!(model.default_value.as_deref(), Some("preset-model"));
    let chunk = || {
        client
            .settings()
            .unwrap()
            .settings
            .into_iter()
            .find(|s| s.key == "LOCALVOX_LIGHT_CHUNK_SEC")
            .unwrap()
    };
    assert_eq!(chunk().default_value.as_deref(), Some("420"));
    client
        .save_settings([("LOCALVOX_LIGHT_CHUNK_SEC".into(), Some("123".into()))].into())
        .unwrap();
    assert_eq!(chunk().value.as_deref(), Some("123"));
    client
        .save_settings([("LOCALVOX_LIGHT_CHUNK_SEC".into(), None)].into())
        .unwrap();
    assert!(chunk().value.is_none());
    assert_eq!(chunk().default_value.as_deref(), Some("420"));
    client.save_settings([
        ("LOCALVOX_LLM_SERVER_URLS".into(), Some(r#"["http://localhost:11434/v1/","http://localhost:11434/v1","https://models.example/v1"]"#.into())),
        ("LOCALVOX_LLM_BASE_URL".into(), Some("https://models.example/v1/".into())),
    ].into()).unwrap();
    let servers = client.settings().unwrap();
    let urls = servers
        .settings
        .iter()
        .find(|s| s.key == "LOCALVOX_LLM_BASE_URL")
        .unwrap();
    assert_eq!(urls.options.as_ref().unwrap().len(), 2);
    assert_eq!(urls.value.as_deref(), Some("https://models.example/v1"));
    let unchanged = fs::read(&env_path).unwrap();
    let error = client
        .save_settings(
            [(
                "LOCALVOX_LLM_BASE_URL".into(),
                Some("https://user:private-credential@host/v1".into()),
            )]
            .into(),
        )
        .unwrap_err();
    assert!(!format!("{error:#}").contains("private-credential"));
    assert_eq!(fs::read(&env_path).unwrap(), unchanged);
    client
        .save_settings([("LOCALVOX_LLM_BASE_URL".into(), None)].into())
        .unwrap();
    let original = fs::read(&env_path).unwrap();
    assert!(client
        .request::<Reply>(
            "POST",
            &["api", "settings"],
            &[],
            Some(json!({"set":{"PATH":"unsafe"}})),
            Duration::from_secs(3)
        )
        .is_err());
    assert_eq!(fs::read(&env_path).unwrap(), original);
    let reply = client
        .save_settings([("LOCALVOX_LLM_MODEL".into(), Some("after".into()))].into())
        .unwrap();
    assert!(!reply.need_restart.is_empty());
    let reply: Reply = client
        .request(
            "POST",
            &["api", "settings"],
            &[],
            Some(json!({"set":{
                "LOCALVOX_LIGHT_AUTOCOOK_SUMMARY":false,"LOCALVOX_LIGHT_AUTOCOOK_INTERVAL_SEC":7,
                "LOCALVOX_NEW_CREDENTIAL":null,"LOCALVOX_FUTURE_OPTION":"new"
            }})),
            Duration::from_secs(3),
        )
        .unwrap();
    assert!(reply
        .need_restart
        .iter()
        .any(|key| key == "LOCALVOX_FUTURE_OPTION"));
    let reply = client
        .save_settings([("LOCALVOX_LIGHT_AUTOCOOK_SUMMARY".into(), Some("off".into()))].into())
        .unwrap();
    assert!(
        reply.need_restart.is_empty(),
        "only post-processing flags apply without restart"
    );
    let imported = client
        .ingest("https://example.test/two-hour-meeting")
        .unwrap();
    let name = imported.session.unwrap();
    let queue = localvox_light_core::jobs::JobQueue::load(&directory.path().join("archive"));
    let job = queue.jobs().iter().find(|job| job.session == name).unwrap();
    assert!(job.follow_settings);
    assert_eq!(
        (job.summary, job.cleanup, job.refine),
        (false, false, false)
    );
    let progress = client.progress(&name).unwrap();
    assert_eq!(
        progress
            .stages
            .iter()
            .filter(|stage| stage.state == "skipped")
            .count(),
        3
    );
    let keys = ["SUMMARY"].map(|s| format!("LOCALVOX_LIGHT_AUTOCOOK_{s}"));
    client
        .save_settings(keys.iter().map(|key| (key.clone(), None)).collect())
        .unwrap();
    let default_session = client
        .ingest("https://example.test/default-policy")
        .unwrap()
        .session
        .unwrap();
    let default_queue =
        localvox_light_core::jobs::JobQueue::load(&directory.path().join("archive"));
    let defaults = default_queue
        .jobs()
        .iter()
        .find(|job| job.session == default_session)
        .unwrap();
    assert_eq!(
        (defaults.summary, defaults.cleanup, defaults.refine),
        (true, false, false)
    );
    client
        .save_settings(
            keys.into_iter()
                .map(|key| (key, Some("off".into())))
                .collect(),
        )
        .unwrap();
    let saved = fs::read_to_string(&env_path).unwrap();
    assert!(saved.starts_with("# keep this comment\n"));
    assert!(saved.contains("LOCALVOX_LLM_MODEL=after"));
    assert!(saved.contains("LOCALVOX_LIGHT_AUTOCOOK_SUMMARY=off"));
    assert!(saved.contains("LOCALVOX_LIGHT_AUTOCOOK_INTERVAL_SEC=7"));
    assert!(saved.contains("# LOCALVOX_NEW_CREDENTIAL="));
    let doc = client.settings().unwrap();
    assert_eq!(
        doc.settings
            .iter()
            .find(|s| s.key == "LOCALVOX_LLM_MODEL")
            .unwrap()
            .value
            .as_deref(),
        Some("after")
    );
    fs::remove_file(&env_path).unwrap();
    fs::create_dir(&env_path).unwrap();
    assert!(client
        .save_settings([("LOCALVOX_LLM_MODEL".into(), Some("must-not-apply".into()))].into())
        .is_err());
    fs::write(directory.path().join("stop"), "stop fixture only").unwrap();
    let result = directory.path().join("result.json");
    wait_for(&result, &mut child.0);
    let result: Value = serde_json::from_slice(&fs::read(result).unwrap()).unwrap();
    assert_eq!(
        result["model"], "after",
        "failed persistence must not mutate the child process environment"
    );
    assert!(child.0.wait().unwrap().success());
    assert_eq!(result["chunk"], "420");
}

#[test]
#[ignore = "runs only inside a copied executable/private environment"]
fn settings_fixture() {
    let Some(root) = std::env::var_os("LOCALVOX_SETTINGS_FIXTURE").map(std::path::PathBuf::from)
    else {
        return;
    };
    let exe = std::env::current_exe().unwrap();
    assert_eq!(exe.parent().unwrap(), root);
    assert_eq!(std::env::current_dir().unwrap(), root);
    localvox_light_core::presets::apply_missing().unwrap();
    assert!(std::env::var_os("LOCALVOX_LIGHT_MIC").is_none());
    assert!(std::env::var_os("LOCALVOX_LIGHT_LOOPBACK_DEVICE").is_none());
    let server = Server::http("127.0.0.1:0").unwrap();
    let config = cfg(Some("synthetic-token"));
    let archive = Archive::new(root.join("archive"));
    localvox_light_core::artifacts::atomic_write(
        &root.join("address"),
        format!("http://{}", server.server_addr()).as_bytes(),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !root.join("stop").exists() && Instant::now() < deadline {
        if let Some(request) = server.recv_timeout(Duration::from_millis(20)).unwrap() {
            // Test-only observation of the live scheduler, not the saved settings view.
            if request.url() == "/fixture/early-enabled" {
                request
                    .respond(Response::from_string(
                        json!({"enabled":localvox_light_core::jobs::prefix::enabled()}).to_string(),
                    ))
                    .unwrap();
                continue;
            }
            handle_request(&archive, &config, request);
        }
    }
    localvox_light_core::artifacts::atomic_write(
        &root.join("result.json"),
        json!({"model":std::env::var("LOCALVOX_LLM_MODEL").unwrap(),"chunk":std::env::var("LOCALVOX_LIGHT_CHUNK_SEC").unwrap()}).to_string().as_bytes(),
    )
    .unwrap();
}
