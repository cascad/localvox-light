//! Read-only discovery. No model loading, downloads, service starts, or filesystem writes.
use anyhow::{Context, Result};
use localvox_light_core::doctor::{Finding, Layout, State};
use std::{io::Read, path::PathBuf, time::Duration};

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Tag {
    pub name: String,
}

pub fn cli() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "ollama.exe"
    } else {
        "ollama"
    };
    let mut paths: Vec<PathBuf> =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|p| p.join(name))
            .collect();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        paths.push(PathBuf::from(local).join("Programs/Ollama/ollama.exe"));
    }
    #[cfg(target_os = "macos")]
    {
        paths.push(PathBuf::from(
            "/Applications/Ollama.app/Contents/Resources/ollama",
        ));
        if let Some(home) = std::env::var_os("HOME") {
            paths.push(
                PathBuf::from(home).join("Applications/Ollama.app/Contents/Resources/ollama"),
            );
        }
    }
    paths.into_iter().find(|p| p.is_file())
}

pub fn endpoint(base: &str, suffix: &str) -> Result<String> {
    let base = localvox_light_core::presets::normalize_server(base)?;
    let base = base.strip_suffix("/v1").unwrap_or(&base);
    Ok(format!("{base}/{suffix}"))
}

pub fn local(base: &str) -> bool {
    url::Url::parse(base).ok().is_some_and(|u| match u.host() {
        Some(url::Host::Domain(s)) => s.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    })
}

pub fn tags(base: &str) -> Result<Vec<Tag>> {
    let url = endpoint(base, "api/tags")?;
    let response = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(3))
        .redirects(0)
        .build()
        .get(&url)
        .call()
        .map_err(|_| anyhow::anyhow!("Ollama API не ответил на /api/tags"))?;
    // Bound untrusted server output; discovery must not consume unbounded memory.
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() <= 4 * 1024 * 1024,
        "Слишком большой ответ /api/tags"
    );
    #[derive(serde::Deserialize)]
    struct Tags {
        models: Vec<Tag>,
    }
    Ok(serde_json::from_slice::<Tags>(&bytes)
        .context("Некорректный ответ /api/tags")?
        .models)
}

pub fn has_tag(tags: &[Tag], name: &str) -> bool {
    let canonical = |s: &str| {
        if s.rsplit('/').next().unwrap_or(s).contains(':') {
            s.to_string()
        } else {
            format!("{s}:latest")
        }
    };
    tags.iter().any(|t| canonical(&t.name) == canonical(name))
}

pub fn ollama_findings(layout: &Layout) -> Vec<Finding> {
    let mut out = vec![match cli() {
        Some(path) => Finding::ok(
            "ollama_cli",
            "Ollama · программа",
            path.display().to_string(),
        ),
        None => Finding::bad(
            "ollama_cli",
            "Ollama · программа",
            State::Warn,
            "CLI не найдена (для удалённого сервера не нужна)",
            "localvox-light --setup",
        ),
    }];
    match tags(&layout.llm_base_url) {
        Ok(found) => {
            out.push(Finding::ok(
                "ollama_api",
                "Ollama · API",
                "Список моделей получен без запуска inference",
            ));
            out.push(if has_tag(&found, &layout.llm_model) {
                Finding::ok("llm_model", "LLM · выбранная модель", &layout.llm_model)
            } else {
                Finding::bad(
                    "llm_model",
                    "LLM · выбранная модель",
                    State::Warn,
                    format!("{} отсутствует", layout.llm_model),
                    "Выбрать или скачать модель: localvox-light --setup",
                )
            });
        }
        Err(e) => {
            out.push(Finding::bad("ollama_api", "Ollama · API", State::Warn, e.to_string(),
                "Запустить Ollama; для другого OpenAI-совместимого сервера проверка /api/tags неприменима"));
            out.push(Finding::bad(
                "llm_model",
                "LLM · выбранная модель",
                State::Warn,
                format!("{}: наличие не проверено", layout.llm_model),
                "Проверить сервер и модель в настройках",
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_and_tag_identity_do_not_depend_on_spelling_latest() {
        assert_eq!(
            endpoint("http://[::1]:11434/v1/", "api/tags").unwrap(),
            "http://[::1]:11434/api/tags"
        );
        assert!(endpoint("http://user:secret@localhost/v1", "api/tags").is_err());
        assert!(has_tag(
            &[Tag {
                name: "bge-m3:latest".into()
            }],
            "bge-m3"
        ));
        assert!(!has_tag(
            &[Tag {
                name: "m:small".into()
            }],
            "m"
        ));
        assert!(local("http://[::1]:11434/v1"));
        assert!(!local("http://localhost.example/v1"));
    }
}
