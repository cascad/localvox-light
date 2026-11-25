//! Verified, versioned sidecar. Never updates a package-manager executable in place.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const DAY: u64 = 86400;
const BACKOFF: u64 = 3600;
const API: &str = "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest";
const RELEASES: &str = "https://github.com/yt-dlp/yt-dlp/releases/download";

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct State {
    checked: u64,
    attempted: u64,
    tag: String,
    sha256: String,
    directory: String,
}

fn asset(os: &str, arch: &str) -> Result<&'static str> {
    Ok(match (os, arch) {
        ("windows", "x86_64") => "yt-dlp.exe",
        ("macos", "x86_64" | "aarch64") => "yt-dlp_macos",
        ("linux", "x86_64") => "yt-dlp_linux",
        ("linux", "aarch64") => "yt-dlp_linux_aarch64",
        _ => anyhow::bail!("Нет официального standalone yt-dlp для {os}/{arch}"),
    })
}
fn tag_ok(tag: &str) -> bool {
    (4..40).contains(&tag.len())
        && tag.as_bytes()[0].is_ascii_digit()
        && tag
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}
fn recent(at: u64, now: u64, age: u64) -> bool {
    at != 0 && now >= at && now - at < age
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn cached(root: &Path, state: &State, name: &str) -> Option<PathBuf> {
    if !tag_ok(&state.tag) || state.sha256.len() != 64 {
        return None;
    }
    if !tag_ok(&state.directory) {
        return None;
    }
    let path = root.join(&state.directory).join(name);
    if !path
        .metadata()
        .ok()
        .is_some_and(|m| m.len() > 0 && m.len() <= 100 * 1024 * 1024)
    {
        return None;
    }
    // A cache file is never trusted merely because its name or state survived.
    let bytes = fs::read(&path).ok()?;
    (digest(&bytes) == state.sha256).then_some(path)
}
fn save(root: &Path, state: &State) -> Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(root)?;
    serde_json::to_writer(file.as_file_mut(), state)?;
    file.as_file().sync_all()?;
    file.persist(root.join("state.json")).map_err(|e| e.error)?;
    Ok(())
}

trait Fetch {
    fn get(&self, url: &str, limit: u64) -> Result<Vec<u8>>;
}
struct Https(ureq::Agent);
impl Fetch for Https {
    fn get(&self, url: &str, limit: u64) -> Result<Vec<u8>> {
        let response = self
            .0
            .get(url)
            .call()
            .context("Официальный сервер обновлений yt-dlp недоступен")?;
        anyhow::ensure!(
            response.status() == 200,
            "Сервер обновлений вернул HTTP {}",
            response.status()
        );
        let mut data = Vec::new();
        response
            .into_reader()
            .take(limit + 1)
            .read_to_end(&mut data)?;
        anyhow::ensure!(
            data.len() as u64 <= limit,
            "Ответ обновления превышает допустимый размер"
        );
        Ok(data)
    }
}
fn checksum(sums: &[u8], name: &str) -> Result<String> {
    let text = std::str::from_utf8(sums)?;
    let mut matching = text.lines().filter_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name).then_some(hash)
    });
    let hash = matching
        .next()
        .context("Официальный SHA-256 загрузчика отсутствует")?;
    anyhow::ensure!(
        matching.next().is_none()
            && hash.len() == 64
            && hash.bytes().all(|b| b.is_ascii_hexdigit()),
        "Неоднозначный или неверный SHA-256 загрузчика"
    );
    Ok(hash.to_ascii_lowercase())
}
fn verify(path: &Path, tag: &str) -> Result<()> {
    use localvox_light_process::{Limits, Scope};
    let mut command = std::process::Command::new(path);
    command.arg("--version");
    let mut limits = Limits::new(Duration::from_secs(15));
    if std::env::var(localvox_light_process::CONTAINED_ENV).as_deref() == Ok("1") {
        limits.scope = Scope::Inherit;
    }
    let result = localvox_light_process::run(command, None, limits, || false, |_, _| {})?;
    anyhow::ensure!(
        result.termination.success() && String::from_utf8_lossy(&result.stdout).trim() == tag,
        "Новая копия yt-dlp не подтвердила версию"
    );
    Ok(())
}
fn update(
    root: &Path,
    state: &mut State,
    name: &str,
    now: u64,
    fetch: &impl Fetch,
    validate: &impl Fn(&Path, &str) -> Result<()>,
    emit: &mut impl FnMut(&str),
) -> Result<PathBuf> {
    emit("Проверка официальной версии yt-dlp");
    let release: serde_json::Value = serde_json::from_slice(&fetch.get(API, 1024 * 1024)?)?;
    let tag = release
        .get("tag_name")
        .and_then(|v| v.as_str())
        .context("В ответе нет версии yt-dlp")?;
    anyhow::ensure!(tag_ok(tag), "Неверная версия yt-dlp");
    if state.tag == tag {
        if let Some(path) = cached(root, state, name) {
            state.checked = now;
            save(root, state)?;
            return Ok(path);
        }
    }
    let sums = fetch.get(&format!("{RELEASES}/{tag}/SHA2-256SUMS"), 1024 * 1024)?;
    let sha = checksum(&sums, name)?;
    emit(&format!("Скачивание yt-dlp {tag}"));
    let bytes = fetch.get(&format!("{RELEASES}/{tag}/{name}"), 100 * 1024 * 1024)?;
    anyhow::ensure!(
        digest(&bytes) == sha,
        "SHA-256 новой копии yt-dlp не совпал; рабочая копия сохранена"
    );
    let staging = tempfile::Builder::new()
        .prefix(&format!("{tag}-"))
        .tempdir_in(root)?;
    let exe = staging.path().join(name);
    fs::write(&exe, bytes)?;
    fs::write(staging.path().join(".localvox-managed"), b"1")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o755))?;
    }
    validate(&exe, tag)?;
    let previous = state.directory.clone();
    let directory = staging
        .path()
        .file_name()
        .context("Папка загрузчика")?
        .to_string_lossy()
        .to_string();
    let destination = staging.keep();
    let next = State {
        checked: now,
        attempted: now,
        tag: tag.into(),
        sha256: sha,
        directory,
    };
    save(root, &next)?;
    *state = next;
    // Retain current + previous. Only directories bearing our receipt are removed.
    for entry in fs::read_dir(root).into_iter().flatten().flatten() {
        let label = entry.file_name();
        let label = label.to_string_lossy();
        if label != state.directory
            && label != previous
            && tag_ok(&label)
            && entry
                .file_type()
                .is_ok_and(|t| t.is_dir() && !t.is_symlink())
            && entry.path().join(".localvox-managed").is_file()
        {
            if root
                .canonicalize()
                .ok()
                .zip(entry.path().canonicalize().ok())
                .is_some_and(|(root, path)| path.starts_with(&root) && path != root)
            {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }
    emit(&format!("yt-dlp {tag} готов"));
    Ok(destination.join(name))
}

fn maintain(
    root: &Path,
    fallback: &str,
    now: u64,
    name: &str,
    fetch: &impl Fetch,
    validate: &impl Fn(&Path, &str) -> Result<()>,
    mut emit: impl FnMut(&str),
) -> Result<String> {
    fs::create_dir_all(root)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(root.join("update.lock"))?;
    let start = std::time::Instant::now();
    loop {
        match lock.try_lock() {
            Ok(()) => break,
            Err(std::fs::TryLockError::WouldBlock) => {}
            Err(std::fs::TryLockError::Error(error)) => {
                return Err(error).context("Блокировка обновления yt-dlp недоступна")
            }
        }
        if start.elapsed() > Duration::from_secs(20) {
            emit("Обновление yt-dlp выполняется другим импортом; используется рабочая копия");
            let state = read_state(root);
            return Ok(cached(root, &state, name)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| fallback.into()));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut state = read_state(root);
    let old = cached(root, &state, name);
    if recent(state.checked, now, DAY) && old.is_some() || recent(state.attempted, now, BACKOFF) {
        return Ok(old
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| fallback.into()));
    }
    state.attempted = now;
    save(root, &state)?;
    match update(root, &mut state, name, now, fetch, validate, &mut emit) {
        Ok(path) => Ok(path.display().to_string()),
        Err(error) => {
            emit(&format!("Обновление yt-dlp не завершено: {error:#}. Используется рабочая копия; повтор проверки позже"));
            Ok(old
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| fallback.into()))
        }
    }
}
fn read_state(root: &Path) -> State {
    let read = || -> Result<State> {
        let file = fs::File::open(root.join("state.json"))?;
        let mut bytes = Vec::new();
        file.take(65537).read_to_end(&mut bytes)?;
        anyhow::ensure!(bytes.len() <= 65536, "state too large");
        Ok(serde_json::from_slice(&bytes)?)
    };
    read().unwrap_or_default()
}

/// Network access occurs only for actual URL ingestion, never in doctor/path discovery.
pub fn ensure(fallback: &str, enabled: bool, mut emit: impl FnMut(&str)) -> Result<String> {
    if !enabled {
        return Ok(fallback.into());
    }
    let Some(home) = std::env::home_dir() else {
        emit("Нет пользовательской папки для обновления yt-dlp; используется заданная копия");
        return Ok(fallback.into());
    };
    ensure_at(&home.join(".localvox/tools/yt-dlp"), fallback, true, emit)
}

pub fn ensure_at(
    root: &Path,
    fallback: &str,
    enabled: bool,
    mut emit: impl FnMut(&str),
) -> Result<String> {
    if !enabled {
        return Ok(fallback.into());
    }
    let root = root.join(format!(
        "{}-{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    let name = asset(std::env::consts::OS, std::env::consts::ARCH)?;
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(45))
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(15))
        .user_agent("localvox-ytdlp-update/1")
        .build();
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    match maintain(
        &root,
        fallback,
        now,
        name,
        &Https(agent),
        &verify,
        &mut emit,
    ) {
        Ok(path) => Ok(path),
        Err(error) => {
            emit(&format!("Не удалось подготовить обновление yt-dlp: {error:#}; используется заданный загрузчик"));
            Ok(fallback.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    struct Fake {
        tag: &'static str,
        bad: bool,
        calls: AtomicUsize,
    }
    impl Fetch for Fake {
        fn get(&self, url: &str, _: u64) -> Result<Vec<u8>> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            assert!(
                url.starts_with("https://api.github.com/repos/yt-dlp/yt-dlp/")
                    || url.starts_with(RELEASES)
            );
            if url == API {
                return Ok(serde_json::to_vec(
                    &serde_json::json!({"tag_name":self.tag}),
                )?);
            }
            if url.ends_with("SHA2-256SUMS") {
                return Ok(format!(
                    "{}  yt-dlp.exe\n",
                    if self.bad {
                        "0".repeat(64)
                    } else {
                        digest(b"verified executable")
                    }
                )
                .into_bytes());
            }
            Ok(b"verified executable".to_vec())
        }
    }
    fn fake(tag: &'static str, bad: bool) -> Fake {
        Fake {
            tag,
            bad,
            calls: AtomicUsize::new(0),
        }
    }
    fn accept(path: &Path, _: &str) -> Result<()> {
        anyhow::ensure!(fs::read(path)? == b"verified executable", "wrong file");
        Ok(())
    }
    #[test]
    fn verified_publish_cache_and_external_tool_are_independent() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("managed");
        let external = temp.path().join("external.exe");
        fs::write(&external, b"external unchanged").unwrap();
        let fetch = fake("2026.11.24", false);
        let selected = maintain(
            &root,
            external.to_str().unwrap(),
            100000,
            "yt-dlp.exe",
            &fetch,
            &accept,
            |_| {},
        )
        .unwrap();
        assert_eq!(fs::read(&selected).unwrap(), b"verified executable");
        assert_eq!(fs::read(external).unwrap(), b"external unchanged");
        assert_eq!(fetch.calls.load(Ordering::Relaxed), 3);
        assert_eq!(
            maintain(
                &root,
                "fallback",
                100001,
                "yt-dlp.exe",
                &fetch,
                &accept,
                |_| {}
            )
            .unwrap(),
            selected
        );
        assert_eq!(
            fetch.calls.load(Ordering::Relaxed),
            3,
            "cache must avoid all network calls"
        );
        maintain(
            &root,
            "fallback",
            200000,
            "yt-dlp.exe",
            &fetch,
            &accept,
            |_| {},
        )
        .unwrap();
        assert_eq!(
            fetch.calls.load(Ordering::Relaxed),
            4,
            "same release needs only metadata"
        );
    }
    #[test]
    fn bad_checksum_or_failed_launch_never_replaces_working_version_and_has_backoff() {
        for fail_launch in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path();
            let first = fake("2026.11.24", false);
            let old = maintain(
                &root,
                "fallback",
                100000,
                "yt-dlp.exe",
                &first,
                &accept,
                |_| {},
            )
            .unwrap();
            let next = fake("2026.11.25", !fail_launch);
            let validate = |path: &Path, tag: &str| {
                accept(path, tag)?;
                anyhow::bail!("launch rejected")
            };
            let mut notes = vec![];
            assert_eq!(
                maintain(
                    root,
                    "fallback",
                    200000,
                    "yt-dlp.exe",
                    &next,
                    &validate,
                    |s| notes.push(s.to_string())
                )
                .unwrap(),
                old
            );
            assert_eq!(
                fs::read_dir(root)
                    .unwrap()
                    .flatten()
                    .filter(|e| e.file_type().unwrap().is_dir())
                    .count(),
                1,
                "no rejected candidate may remain published"
            );
            assert_eq!(read_state(root).tag, "2026.11.24");
            assert!(notes.iter().any(|s| s.contains("не завершено")));
            let calls = next.calls.load(Ordering::Relaxed);
            maintain(
                root,
                "fallback",
                200001,
                "yt-dlp.exe",
                &next,
                &accept,
                |_| {},
            )
            .unwrap();
            assert_eq!(next.calls.load(Ordering::Relaxed), calls);
        }
    }
    #[test]
    fn concurrent_imports_publish_once_and_never_observe_a_partial_file() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_owned();
        let fetch = Arc::new(fake("2026.11.24", false));
        let handles = (0..2)
            .map(|_| {
                let root = root.clone();
                let fetch = fetch.clone();
                std::thread::spawn(move || {
                    maintain(
                        &root,
                        "fallback",
                        100000,
                        "yt-dlp.exe",
                        fetch.as_ref(),
                        &accept,
                        |_| {},
                    )
                    .unwrap()
                })
            })
            .collect::<Vec<_>>();
        let paths = handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(paths[0], paths[1]);
        assert_eq!(fetch.calls.load(Ordering::Relaxed), 3);
        assert_eq!(fs::read(&paths[0]).unwrap(), b"verified executable");
    }
    #[test]
    fn disabled_update_and_platform_mapping_and_untrusted_tags() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("absent");
        assert_eq!(
            ensure_at(&root, "custom", false, |_| panic!("disabled emitted work")).unwrap(),
            "custom"
        );
        assert!(!root.exists());
        for (os, arch, name) in [
            ("windows", "x86_64", "yt-dlp.exe"),
            ("macos", "aarch64", "yt-dlp_macos"),
            ("macos", "x86_64", "yt-dlp_macos"),
            ("linux", "x86_64", "yt-dlp_linux"),
            ("linux", "aarch64", "yt-dlp_linux_aarch64"),
        ] {
            assert_eq!(asset(os, arch).unwrap(), name);
        }
        assert!(asset("windows", "arm").is_err());
        for tag in ["../outside", "/absolute", "2026/11", "bad\nname", ""] {
            assert!(!tag_ok(tag));
        }
        assert!(checksum(b"00  yt-dlp.exe\n", "yt-dlp.exe").is_err());
    }
    #[test]
    fn missing_network_keeps_fallback_and_tampering_is_not_trusted() {
        struct Offline;
        impl Fetch for Offline {
            fn get(&self, _: &str, _: u64) -> Result<Vec<u8>> {
                anyhow::bail!("offline")
            }
        }
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(
            maintain(
                temp.path(),
                "external",
                100000,
                "yt-dlp.exe",
                &Offline,
                &accept,
                |_| {}
            )
            .unwrap(),
            "external"
        );
        let first = fake("2026.11.24", false);
        let file = maintain(
            temp.path(),
            "external",
            200000,
            "yt-dlp.exe",
            &first,
            &accept,
            |_| {},
        )
        .unwrap();
        fs::write(&file, b"tampered").unwrap();
        assert_eq!(
            maintain(
                temp.path(),
                "external",
                200001,
                "yt-dlp.exe",
                &Offline,
                &accept,
                |_| {}
            )
            .unwrap(),
            "external"
        );
        let repaired = maintain(
            temp.path(),
            "external",
            300000,
            "yt-dlp.exe",
            &first,
            &accept,
            |_| {},
        )
        .unwrap();
        assert_ne!(repaired, file);
        assert_eq!(fs::read(repaired).unwrap(), b"verified executable");
    }

    #[test]
    fn real_transport_bounds_response_size_and_waiting_time() {
        use std::{io::Write, net::TcpListener};
        for slow in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/test", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                let mut request = [0; 2048];
                let _ = socket.read(&mut request);
                if slow {
                    std::thread::sleep(Duration::from_millis(150));
                }
                let _ = socket.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nlong",
                );
            });
            let fetch = Https(
                ureq::AgentBuilder::new()
                    .timeout(Duration::from_millis(50))
                    .build(),
            );
            let error = fetch.get(&url, if slow { 10 } else { 2 }).unwrap_err();
            if !slow {
                assert!(error.to_string().contains("размер"));
            }
            server.join().unwrap();
        }
    }

    #[test]
    fn retention_keeps_two_owned_versions_and_leaves_unrelated_files() {
        let temp = tempfile::tempdir().unwrap();
        let stranger = temp.path().join("2020.01.01-user");
        fs::create_dir(&stranger).unwrap();
        fs::write(stranger.join("keep"), b"user").unwrap();
        let mut first = String::new();
        for (index, tag) in ["2026.11.23", "2026.11.24", "2026.11.25"]
            .into_iter()
            .enumerate()
        {
            let path = maintain(
                temp.path(),
                "external",
                100000 + (index as u64) * 100000,
                "yt-dlp.exe",
                &fake(tag, false),
                &accept,
                |_| {},
            )
            .unwrap();
            if index == 0 {
                first = path;
            }
        }
        assert!(!Path::new(&first).exists());
        assert!(stranger.join("keep").exists());
        assert_eq!(
            fs::read_dir(temp.path())
                .unwrap()
                .flatten()
                .filter(|e| e.path().join(".localvox-managed").is_file())
                .count(),
            2
        );
        assert!(
            !recent(200000, 100000, DAY),
            "future cache timestamps cannot suppress updates indefinitely"
        );
    }
    #[test]
    #[ignore = "explicit official network/binary smoke, temporary directory only"]
    fn official_release_smoke() {
        let temp = tempfile::tempdir().unwrap();
        let path = ensure_at(temp.path(), "missing-fallback", true, |s| eprintln!("{s}")).unwrap();
        assert_ne!(path, "missing-fallback");
        let state = read_state(&temp.path().join(format!(
            "{}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )));
        verify(Path::new(&path), &state.tag).unwrap();
    }
}
