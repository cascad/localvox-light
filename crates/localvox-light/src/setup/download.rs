//! Streaming model transport. A caller publishes the verified .part, not this module.
use anyhow::{Context, Result};
use localvox_setup_ui::progress::{Phase, Progress};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::PathBuf,
    time::{Duration, Instant},
};

pub struct Request {
    pub urls: Vec<String>,
    pub output: PathBuf,
    pub sha256: Option<String>,
    pub size: Option<u64>,
}

pub fn fetch(request: &Request, mut emit: impl FnMut(&Progress)) -> Result<()> {
    anyhow::ensure!(!request.urls.is_empty(), "Не задан источник модели");
    anyhow::ensure!(
        request
            .output
            .file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with(".part")),
        "Загрузчик пишет только во временный файл .part"
    );
    if let Some(sha) = &request.sha256 {
        anyhow::ensure!(
            sha.len() == 64 && sha.bytes().all(|c| c.is_ascii_hexdigit()),
            "Неверный ожидаемый SHA-256"
        );
    }
    let urls: Vec<url::Url> = request
        .urls
        .iter()
        .map(|value| {
            let url = url::Url::parse(value).context("Неверный адрес источника")?;
            anyhow::ensure!(
                matches!(url.scheme(), "http" | "https")
                    && url.username().is_empty()
                    && url.password().is_none(),
                "Источник модели должен быть HTTP(S) без пароля в адресе"
            );
            Ok(url)
        })
        .collect::<Result<_>>()?;
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(30))
        .timeout_write(Duration::from_secs(30))
        .redirects(10)
        .user_agent("localvox-fetch-models/2.0")
        .build();
    let mut errors = Vec::new();
    for (index, url) in urls.iter().enumerate() {
        let file_name = url
            .path_segments()
            .and_then(|parts| parts.last())
            .filter(|s| !s.is_empty())
            .unwrap_or("модель");
        let display_name = if index == 0 {
            file_name.to_string()
        } else {
            format!("{file_name} · резервный источник")
        };
        let name = display_name.as_str();
        let started = Instant::now();
        emit(&Progress::stage(name, Phase::Connecting));
        let result = (|| {
            let response = agent
                .get(url.as_str())
                .set("Accept-Encoding", "identity")
                .call()
                .map_err(|error| match error {
                    ureq::Error::Status(status, _) => anyhow::anyhow!("HTTP {status}"),
                    ureq::Error::Transport(error) => anyhow::anyhow!("Сеть: {}", error.kind()),
                })?;
            anyhow::ensure!(
                response.status() == 200,
                "Неожиданный HTTP {}",
                response.status()
            );
            let declared = response
                .header("Content-Length")
                .map(str::parse::<u64>)
                .transpose()
                .context("Неверный Content-Length")?;
            if let (Some(expected), Some(actual)) = (request.size, declared) {
                anyhow::ensure!(
                    expected == actual,
                    "Размер ответа {actual}, ожидалось {expected}"
                );
            }
            let total = request.size.or(declared);
            let mut reader = response.into_reader();
            let mut file = File::create(&request.output)
                .context("Не удалось создать временный файл модели")?;
            let mut sha = Sha256::new();
            let mut count = 0u64;
            let mut last = Instant::now();
            let mut bytes = [0u8; 128 * 1024];
            emit(&Progress {
                name: name.into(),
                phase: Phase::Downloading,
                bytes: 0,
                total,
                elapsed_ms: started.elapsed().as_millis() as u64,
            });
            loop {
                let n = reader
                    .read(&mut bytes)
                    .context("Поток скачивания оборвался")?;
                if n == 0 {
                    break;
                }
                count = count
                    .checked_add(n as u64)
                    .context("Слишком большой ответ")?;
                anyhow::ensure!(
                    total.is_none_or(|total| count <= total),
                    "Ответ длиннее объявленного размера"
                );
                file.write_all(&bytes[..n])
                    .context("Не удалось записать модель на диск")?;
                sha.update(&bytes[..n]);
                if last.elapsed() >= Duration::from_millis(250) {
                    emit(&Progress {
                        name: name.into(),
                        phase: Phase::Downloading,
                        bytes: count,
                        total,
                        elapsed_ms: started.elapsed().as_millis() as u64,
                    });
                    last = Instant::now();
                }
            }
            anyhow::ensure!(
                total.is_none_or(|total| count == total),
                "Скачано {count} байт, ожидалось {total:?}"
            );
            emit(&Progress {
                name: name.into(),
                phase: Phase::Downloading,
                bytes: count,
                total,
                elapsed_ms: started.elapsed().as_millis() as u64,
            });
            emit(&Progress::stage(name, Phase::Verifying));
            let actual = format!("{:x}", sha.finalize());
            if let Some(expected) = &request.sha256 {
                anyhow::ensure!(
                    actual.eq_ignore_ascii_case(expected),
                    "SHA-256 не совпал: ожидался {expected}, получен {actual}"
                );
            }
            file.sync_all()
                .context("Не удалось сохранить скачанный файл")?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                emit(&Progress::stage(name, Phase::Complete));
                return Ok(());
            }
            Err(error) => {
                // The file name was explicitly requested as our .part; never remove a ready model.
                let _ = std::fs::remove_file(&request.output);
                errors.push(format!(
                    "{}: {error:#}",
                    url.host_str().unwrap_or("источник")
                ));
            }
        }
    }
    anyhow::bail!(
        "Не удалось скачать модель. Готовые модели сохранены. {}",
        errors.join("; ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::BufRead, net::TcpListener};

    fn server(responses: Vec<Vec<u8>>) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut input = std::io::BufReader::new(&stream);
                loop {
                    let mut line = String::new();
                    input.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                }
                stream.write_all(&response).unwrap();
            }
        });
        (base, worker)
    }
    #[test]
    fn native_download_checks_size_hash_fallback_and_unknown_length_without_publishing() {
        for response in [
            b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc".to_vec(),
            b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nabc".to_vec(),
        ] {
            let root = tempfile::tempdir().unwrap();
            let output = root.path().join("model.part");
            let (base, worker) = server(vec![
                b"HTTP/1.1 503 Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
                response,
            ]);
            let mut events = Vec::new();
            fetch(
                &Request {
                    urls: vec![format!("{base}/failed"), format!("{base}/model")],
                    output: output.clone(),
                    sha256: Some(format!("{:x}", Sha256::digest(b"abc"))),
                    size: None,
                },
                |event| events.push(event.clone()),
            )
            .unwrap();
            worker.join().unwrap();
            assert_eq!(std::fs::read(&output).unwrap(), b"abc");
            assert!(!root.path().join("model").exists());
            assert_eq!(events.last().unwrap().phase, Phase::Complete);
            assert!(events
                .iter()
                .any(|event| event.phase == Phase::Downloading && event.bytes == 3));
        }
    }
    #[test]
    fn redirects_keep_the_pinned_size_and_hash_contract() {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("model.part");
        let (base, worker) = server(vec![
            b"HTTP/1.1 302 Found\r\nLocation: /actual\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
            b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc".to_vec(),
        ]);
        fetch(
            &Request {
                urls: vec![format!("{base}/redirect")],
                output: output.clone(),
                size: Some(3),
                sha256: Some(format!("{:x}", Sha256::digest(b"abc"))),
            },
            |_| {},
        )
        .unwrap();
        worker.join().unwrap();
        assert_eq!(std::fs::read(output).unwrap(), b"abc");
    }

    #[test]
    fn a_short_body_or_wrong_hash_never_leaves_a_ready_file_or_part() {
        for (body, sha) in [
            (
                b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\nConnection: close\r\n\r\nabc".as_slice(),
                None,
            ),
            (
                b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc".as_slice(),
                Some("0".repeat(64)),
            ),
        ] {
            let root = tempfile::tempdir().unwrap();
            let output = root.path().join("model.part");
            std::fs::write(root.path().join("model"), b"previous ready model").unwrap();
            let (base, worker) = server(vec![body.to_vec()]);
            assert!(fetch(
                &Request {
                    urls: vec![base],
                    output: output.clone(),
                    sha256: sha,
                    size: None
                },
                |_| {}
            )
            .is_err());
            worker.join().unwrap();
            assert!(!output.exists());
            assert_eq!(
                std::fs::read(root.path().join("model")).unwrap(),
                b"previous ready model"
            );
        }
    }
}
