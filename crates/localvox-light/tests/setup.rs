//! Real filesystem, subprocess and HTTP boundaries; no devices or developer models.
use localvox_light::setup::{execute, plan::*, probe, prompt};
use localvox_light_core::doctor::Layout;
use localvox_light_core::env_file::Snapshot;
use std::{
    io::Cursor,
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

fn layout(root: &Path) -> Layout {
    Layout {
        vosk_model: root.join("external/vosk"),
        gigaam: Some(root.join("external/gigaam")),
        diarize: None,
        ner: None,
        cook: None,
        work_dir: root.join("archive"),
        llm_base_url: "http://127.0.0.1:1/v1".into(),
        llm_model: "fixture:tiny".into(),
        summary_enabled: true,
        summary_provider: "ollama".into(),
        api_bind: "127.0.0.1:0".into(),
    }
}
fn touch(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"fixture").unwrap();
}
fn ready(l: &Layout) {
    touch(&l.vosk_model.join("am/final.mdl"));
    for f in [
        "v3_e2e_ctc.int8.onnx",
        "v3_e2e_ctc.yaml",
        "v3_e2e_ctc_vocab.txt",
    ] {
        touch(&l.gigaam.as_ref().unwrap().join(f));
    }
}
fn snapshot(root: &Path) -> Vec<(String, Vec<u8>, std::time::SystemTime)> {
    let mut out = Vec::new();
    fn visit(base: &Path, path: &Path, out: &mut Vec<(String, Vec<u8>, std::time::SystemTime)>) {
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(base, &path, out);
            } else {
                out.push((
                    path.strip_prefix(base).unwrap().to_string_lossy().into(),
                    std::fs::read(&path).unwrap(),
                    path.metadata().unwrap().modified().unwrap(),
                ));
            }
        }
    }
    visit(root, root, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

struct Server {
    url: String,
    pulls: Arc<AtomicUsize>,
    gets: Arc<AtomicUsize>,
    stop: Arc<AtomicUsize>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    fn new(present: bool, failure: bool) -> Self {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", server.server_addr());
        let pulls = Arc::new(AtomicUsize::new(0));
        let gets = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicUsize::new(0));
        let (p, g, s) = (pulls.clone(), gets.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            let mut present = present;
            while s.load(Ordering::SeqCst) == 0 {
                if let Some(mut req) = server.recv_timeout(Duration::from_millis(20)).unwrap() {
                    let body = match req.url() {
                        "/api/tags" => {
                            g.fetch_add(1, Ordering::SeqCst);
                            if present {
                                r#"{"models":[{"name":"fixture:tiny"}]}"#.to_string()
                            } else {
                                r#"{"models":[]}"#.to_string()
                            }
                        }
                        "/api/pull" => {
                            p.fetch_add(1, Ordering::SeqCst);
                            let mut body = String::new();
                            req.as_reader().read_to_string(&mut body).unwrap();
                            assert_eq!(
                                serde_json::from_str::<serde_json::Value>(&body).unwrap()["model"],
                                "fixture:tiny"
                            );
                            if failure {
                                "{\"error\":\"fixture refused\"}\n".into()
                            } else {
                                present = true;
                                "{\"status\":\"pulling manifest\"}\n{\"status\":\"success\"}\n"
                                    .into()
                            }
                        }
                        other => panic!("Unexpected endpoint {other}"),
                    };
                    req.respond(tiny_http::Response::from_string(body)).unwrap();
                }
            }
        });
        Self {
            url,
            pulls,
            gets,
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(1, Ordering::SeqCst);
        self.thread.take().unwrap().join().unwrap();
    }
}

#[test]
fn cancel_and_noop_do_not_even_create_a_lock_or_env() {
    let tmp = tempfile::tempdir().unwrap();
    let l = layout(tmp.path());
    ready(&l);
    let before = snapshot(tmp.path());
    let mut output = Vec::new();
    assert!(
        prompt::choose(&l, tmp.path(), &mut Cursor::new(b"q\n"), &mut output)
            .unwrap()
            .is_none()
    );
    assert!(
        prompt::choose(&l, tmp.path(), &mut Cursor::new(b""), &mut output)
            .unwrap()
            .is_none()
    );
    let plan = prompt::choose(&l, tmp.path(), &mut Cursor::new(b"\n\n\n\n\n"), &mut output)
        .unwrap()
        .unwrap();
    assert!(plan.actions.is_empty() && plan.changes.is_empty());
    let settings = Snapshot::read(&tmp.path().join(".env")).unwrap();
    execute::apply(&plan, &l, tmp.path(), &settings).unwrap();
    // Declining a concrete settings change also has no side effects.
    assert!(prompt::choose(
        &l,
        tmp.path(),
        &mut Cursor::new(b"\n\n\n\n3\nno\n"),
        &mut output
    )
    .unwrap()
    .is_none());
    assert_eq!(snapshot(tmp.path()), before);
    assert!(!l.work_dir.exists());
}

#[test]
fn external_paths_and_manifest_downloads_reuse_complete_models() {
    let tmp = tempfile::tempdir().unwrap();
    let l = layout(tmp.path());
    ready(&l);
    let tags = [probe::Tag {
        name: l.llm_model.clone(),
    }];
    let plan = build(
        &l,
        &[
            (
                Component::Gigaam,
                ModelChoice::Existing(l.gigaam.clone().unwrap()),
            ),
            (
                Component::Vosk,
                ModelChoice::Download {
                    root: tmp.path().into(),
                    destination: l.vosk_model.clone(),
                },
            ),
        ],
        LlmChoice::Ollama {
            base: l.llm_base_url.clone(),
            model: l.llm_model.clone(),
            install: false,
            gguf: None,
        },
        &tags,
    )
    .unwrap();
    assert!(plan.actions.is_empty() && plan.changes.is_empty());
    assert!(build(
        &l,
        &[(
            Component::Gigaam,
            ModelChoice::Existing(tmp.path().join("missing"))
        )],
        LlmChoice::Keep,
        &[]
    )
    .is_err());
    assert!(build(
        &l,
        &[],
        LlmChoice::Server {
            base: "https://u:secret@host/v1".into(),
            model: "m".into()
        },
        &[]
    )
    .is_err());
    assert!(valid_tag("-f").is_err());
    assert!(valid_tag("model\nFROM bad").is_err());
}

#[test]
fn ollama_report_distinguishes_api_from_selected_model_and_has_unique_ids() {
    let tmp = tempfile::tempdir().unwrap();
    let mut l = layout(tmp.path());
    let server = Server::new(false, false);
    l.llm_base_url = server.url.clone();
    let findings = localvox_light::setup::report(&l);
    let state = |id: &str| findings.iter().find(|f| f.id == id).unwrap().state;
    assert_eq!(state("ollama_api"), localvox_light_core::doctor::State::Ok);
    assert_eq!(state("llm_model"), localvox_light_core::doctor::State::Warn);
    assert!(findings.iter().any(|f| f.id == "ollama_cli"));
    let ids: std::collections::HashSet<_> = findings.iter().map(|f| &f.id).collect();
    assert_eq!(ids.len(), findings.len());
    assert_eq!(server.pulls.load(Ordering::SeqCst), 0);
    assert_eq!(snapshot(tmp.path()), vec![]);
}

#[test]
fn gguf_requires_header_local_server_and_a_new_tag() {
    let tmp = tempfile::tempdir().unwrap();
    let l = layout(tmp.path());
    let file = tmp.path().join("a model.gguf");
    std::fs::write(&file, b"GGUFfixture").unwrap();
    let choice = |base: &str| LlmChoice::Ollama {
        base: base.into(),
        model: "new:tiny".into(),
        install: false,
        gguf: Some(file.clone()),
    };
    assert!(build(&l, &[], choice(&l.llm_base_url), &[]).is_ok());
    assert!(build(&l, &[], choice("https://remote.example/v1"), &[]).is_err());
    assert!(build(
        &l,
        &[],
        choice(&l.llm_base_url),
        &[probe::Tag {
            name: "new:tiny".into()
        }]
    )
    .is_err());
    std::fs::write(&file, b"not gguf").unwrap();
    assert!(build(&l, &[], choice(&l.llm_base_url), &[]).is_err());
}

#[test]
fn pull_only_once_and_never_import_over_a_tag_that_appeared_later() {
    let tmp = tempfile::tempdir().unwrap();
    let server = Server::new(false, false);
    execute::ensure_tag(&server.url, "fixture:tiny", None, tmp.path()).unwrap();
    // Same recheck applies to GGUF; a retry must not replace an existing tag.
    execute::ensure_tag(
        &server.url,
        "fixture:tiny",
        Some(&tmp.path().join("must-not-open.gguf")),
        tmp.path(),
    )
    .unwrap();
    assert_eq!(server.pulls.load(Ordering::SeqCst), 1);
    assert_eq!(server.gets.load(Ordering::SeqCst), 3);
    assert_eq!(snapshot(tmp.path()), vec![]);
}

#[test]
fn failed_pull_keeps_config_and_next_attempt_can_retry() {
    let tmp = tempfile::tempdir().unwrap();
    let mut l = layout(tmp.path());
    let server = Server::new(false, true);
    l.llm_base_url = server.url.clone();
    let env = tmp.path().join(".env");
    let before = "# owner comment\nLOCALVOX_LLM_MODEL=old\n";
    std::fs::write(&env, before).unwrap();
    let plan = Plan {
        actions: vec![Action::EnsureTag {
            base: server.url.clone(),
            model: "fixture:tiny".into(),
            gguf: None,
        }],
        changes: vec![("LOCALVOX_LLM_MODEL".into(), Some("fixture:tiny".into()))],
    };
    let settings = Snapshot::read(&env).unwrap();
    assert!(execute::apply(&plan, &l, tmp.path(), &settings).is_err());
    assert_eq!(std::fs::read_to_string(&env).unwrap(), before);
    assert_eq!(server.pulls.load(Ordering::SeqCst), 1);
}

#[test]
fn settings_publish_preserves_comments_and_concurrent_edits_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let l = layout(tmp.path());
    let env = tmp.path().join(".env");
    let before = "# keep me\nLOCALVOX_LLM_MODEL=old\nOTHER=value\n";
    std::fs::write(&env, before).unwrap();
    let plan = Plan {
        actions: vec![],
        changes: vec![("LOCALVOX_LLM_MODEL".into(), Some("new".into()))],
    };
    let settings = Snapshot::read(&env).unwrap();
    execute::apply(&plan, &l, tmp.path(), &settings).unwrap();
    let after = std::fs::read_to_string(&env).unwrap();
    assert_eq!(after, before.replace("=old", "=new"));
    let mtime = env.metadata().unwrap().modified().unwrap();
    let updated = Snapshot::read(&env).unwrap();
    execute::apply(&plan, &l, tmp.path(), &updated).unwrap();
    assert_eq!(env.metadata().unwrap().modified().unwrap(), mtime);
    assert!(execute::apply(&plan, &l, tmp.path(), &settings).is_err());
    let lock = localvox_light_core::session::try_instance_lock(&l.work_dir)
        .unwrap()
        .unwrap();
    assert!(execute::apply(&plan, &l, tmp.path(), &updated).is_err());
    drop(lock);
}

#[test]
fn stream_error_and_truncation_never_count_as_success() {
    for body in [
        "{\"status\":\"pulling\"}\n",
        "{\"status\":\"success\"}\n{\"error\":\"bad\"}\n",
        "garbage\n",
    ] {
        assert!(execute::pull_progress(Cursor::new(body), &mut Vec::new()).is_err());
    }
    assert!(execute::pull_progress(
        Cursor::new(format!("{}\n", "x".repeat(1024 * 1024 + 1))),
        &mut Vec::new()
    )
    .is_err());
}

#[test]
fn doctor_cli_json_uses_configured_external_paths_without_changing_files() {
    let tmp = tempfile::tempdir().unwrap();
    let mut l = layout(tmp.path());
    ready(&l);
    let server = Server::new(true, false);
    l.llm_base_url = server.url.clone();
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_localvox-light"));
    cmd.current_dir(tmp.path())
        .args(["--doctor", "--doctor-json", "--model"])
        .arg(&l.vosk_model)
        .env("LOCALVOX_ASR_MODEL_DIR", l.gigaam.as_ref().unwrap())
        .env("LOCALVOX_LLM_BASE_URL", &server.url)
        .env("LOCALVOX_LLM_MODEL", "fixture:tiny")
        .env("LOCALVOX_LIGHT_AUDIO_DIR", &l.work_dir)
        .env("LOCALVOX_LIGHT_YT_DLP", tmp.path().join("absent-yt-dlp"));
    let before = snapshot(tmp.path());
    let out = cmd.output().unwrap();
    assert!(
        out.status.code().is_some_and(|n| (0..=2).contains(&n)),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["schema"], 1);
    let findings = report["findings"].as_array().unwrap();
    for id in ["vosk", "gigaam", "ollama_api", "llm_model"] {
        assert_eq!(
            findings.iter().find(|f| f["id"] == id).unwrap()["state"],
            "ok",
            "{id}"
        );
    }
    assert_eq!(server.pulls.load(Ordering::SeqCst), 0);
    assert_eq!(snapshot(tmp.path()), before);
    assert!(!l.work_dir.exists());
}

#[test]
fn fetch_subprocess_repairs_missing_files_and_second_apply_skips_it() {
    let tmp = tempfile::tempdir().unwrap();
    let l = layout(tmp.path());
    let scripts = tmp.path().join("scripts");
    std::fs::create_dir(&scripts).unwrap();
    let script = if cfg!(windows) {
        "fetch-models.ps1"
    } else {
        "fetch-models.sh"
    };
    let text = if cfg!(windows) {
        r##"param($Root,$Only)
Add-Content -LiteralPath (Join-Path $Root 'calls') -Value $Only
$d=Join-Path $Root 'external/gigaam'; New-Item -ItemType Directory -Force $d | Out-Null
'v3_e2e_ctc.int8.onnx','v3_e2e_ctc.yaml','v3_e2e_ctc_vocab.txt' | ForEach-Object { [IO.File]::WriteAllText((Join-Path $d $_),'fixture') }
if (Test-Path -LiteralPath (Join-Path $Root 'inject-edit')) { [IO.File]::WriteAllText((Join-Path $Root '.env'), "# external edit`nOTHER=external`n") }
"##
    } else {
        r#"set -eu
root=$2
echo call >> "$root/calls"
mkdir -p "$root/external/gigaam"
for f in v3_e2e_ctc.int8.onnx v3_e2e_ctc.yaml v3_e2e_ctc_vocab.txt; do echo fixture > "$root/external/gigaam/$f"; done
if [ -f "$root/inject-edit" ]; then printf '# external edit\nOTHER=external\n' > "$root/.env"; fi
"#
    };
    std::fs::write(scripts.join(script), text).unwrap();
    let plan = Plan {
        actions: vec![Action::Download {
            component: Component::Gigaam,
            root: tmp.path().into(),
            destination: l.gigaam.clone().unwrap(),
            repair_zip: false,
        }],
        changes: vec![],
    };
    for _ in 0..2 {
        let settings = Snapshot::read(&tmp.path().join(".env")).unwrap();
        execute::apply(&plan, &l, tmp.path(), &settings).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("calls"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    assert!(!tmp.path().join(".env").exists());

    // Simulate a separate editor during a real fetch subprocess. The files it
    // completed remain ready, but setup must not publish its old settings view.
    let env = tmp.path().join(".env");
    let settings = Snapshot::read(&env).unwrap();
    std::fs::remove_file(l.gigaam.as_ref().unwrap().join("v3_e2e_ctc_vocab.txt")).unwrap();
    touch(&tmp.path().join("inject-edit"));
    let mut retry = plan.clone();
    retry.changes = vec![("LOCALVOX_LLM_MODEL".into(), Some("new".into()))];
    assert!(execute::apply(&retry, &l, tmp.path(), &settings).is_err());
    assert_eq!(
        std::fs::read_to_string(&env).unwrap(),
        "# external edit\nOTHER=external\n"
    );
    Component::Gigaam
        .validate(&l, l.gigaam.as_ref().unwrap())
        .unwrap();
    let updated = Snapshot::read(&env).unwrap();
    execute::apply(&retry, &l, tmp.path(), &updated).unwrap();
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("calls"))
            .unwrap()
            .lines()
            .count(),
        2
    );
    let saved = std::fs::read_to_string(&env).unwrap();
    assert!(saved.starts_with("# external edit\nOTHER=external\n"));
    assert!(saved.contains("LOCALVOX_LLM_MODEL=new"));
}
