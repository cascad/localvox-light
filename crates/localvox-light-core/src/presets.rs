//! Env data is the single source for catalogue defaults, including packaged fallbacks.
use anyhow::{Context, Result};
use std::{collections::BTreeMap, path::PathBuf};

const PACKAGED: &str = include_str!("../../../.env.defaults");
pub const LLM_SERVERS: &str = "LOCALVOX_LLM_SERVER_URLS";

pub fn baked(key: &str) -> &'static str {
    PACKAGED
        .lines()
        .filter_map(|line| line.split_once('='))
        .find_map(|(k, value)| (k.trim() == key).then_some(value.trim()))
        .unwrap_or("")
}
pub fn number<T: std::str::FromStr>(key: &str) -> T {
    baked(key)
        .parse()
        .ok()
        .expect("invalid numeric value in packaged .env.defaults")
}
pub struct Presets {
    pub path: PathBuf,
    pub packaged: bool,
    pub values: BTreeMap<String, String>,
}
fn parse(content: &str) -> Result<BTreeMap<String, String>> {
    let mut seen = std::collections::BTreeSet::new();
    for line in content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let (key, _) = line
            .split_once('=')
            .context("ожидалось KEY=value в .env.defaults")?;
        anyhow::ensure!(seen.insert(key.trim()), "повтор ключа в .env.defaults");
        anyhow::ensure!(
            crate::settings::CATALOGUE
                .iter()
                .any(|s| s.key == key.trim()),
            "неизвестный ключ в .env.defaults"
        );
    }
    let values = crate::env_file::parse(content);
    for setting in crate::settings::CATALOGUE {
        if let Some(value) = values.get(setting.key) {
            match setting.kind {
                crate::settings::Kind::Number => anyhow::ensure!(
                    value.parse::<f64>().is_ok_and(|n| n.is_finite() && n >= 0.),
                    "неверное число {} в .env.defaults",
                    setting.key
                ),
                crate::settings::Kind::Bool => anyhow::ensure!(
                    matches!(value.as_str(), "on" | "off" | "true" | "false" | "1" | "0"),
                    "неверный переключатель {} в .env.defaults",
                    setting.key
                ),
                crate::settings::Kind::Servers => {
                    server_urls(value)?;
                }
                crate::settings::Kind::Secret => {
                    anyhow::ensure!(value.is_empty(), "секреты задаются в пользовательском .env")
                }
                _ => {}
            }
        }
    }
    Ok(values)
}
pub fn load() -> Result<Presets> {
    let path = crate::env_file::resolve_path().with_file_name(".env.defaults");
    let cwd = std::env::current_dir()?.join(".env.defaults");
    load_paths(&[path, cwd])
}
fn load_paths(paths: &[PathBuf]) -> Result<Presets> {
    let mut values = parse(PACKAGED)?;
    for path in paths {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                values.extend(parse(&content)?);
                return Ok(Presets {
                    path: path.clone(),
                    packaged: false,
                    values,
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("не удалось прочитать .env.defaults"),
        }
    }
    Ok(Presets {
        path: paths
            .first()
            .context("не задан путь предустановок")?
            .clone(),
        packaged: true,
        values,
    })
}
pub fn default_value(key: &str) -> Option<String> {
    load()
        .ok()
        .and_then(|p| p.values.get(key).cloned())
        .or_else(|| (!baked(key).is_empty()).then(|| baked(key).to_owned()))
        .filter(|value| !value.is_empty())
}
pub fn apply_missing() -> Result<()> {
    let defaults = load()?;
    for (key, value) in defaults.values {
        if !value.is_empty() && std::env::var_os(&key).is_none_or(|v| v.is_empty()) {
            std::env::set_var(key, value);
        }
    }
    Ok(())
}
pub fn normalize_server(value: &str) -> Result<String> {
    let url = url::Url::parse(value.trim()).context("нужен HTTP(S) адрес LLM-сервера")?;
    anyhow::ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "нужен HTTP(S) адрес без токена, логина, query или фрагмента"
    );
    anyhow::ensure!(value.len() <= 2048, "слишком длинный адрес сервера");
    Ok(url.to_string().trim_end_matches('/').into())
}
pub fn server_urls(value: &str) -> Result<Vec<String>> {
    let values: Vec<String> =
        serde_json::from_str(value).context("нужен список адресов LLM-серверов")?;
    anyhow::ensure!(values.len() <= 32, "не больше 32 LLM-серверов");
    let mut result = vec![];
    for value in values {
        let value = normalize_server(&value)?;
        if !result.contains(&value) {
            result.push(value);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_catalogue_setting_has_a_single_env_default() {
        let values = parse(PACKAGED).unwrap();
        assert_eq!(values.len(), crate::settings::CATALOGUE.len());
        for setting in crate::settings::CATALOGUE {
            assert!(values.contains_key(setting.key), "{}", setting.key);
        }
        assert_eq!(number::<u64>("LOCALVOX_LIGHT_AUTOCOOK_INTERVAL_SEC"), 5);
        assert_eq!(baked("LOCALVOX_API_TOKEN"), "");
        assert!(parse("LOCALVOX_LIGHT_PREROLL_SEC=oops").is_err());
        assert!(parse("LOCALVOX_API_TOKEN=secret").is_err());
    }
    #[test]
    fn server_urls_are_normalized_deduplicated_and_do_not_accept_credentials() {
        assert_eq!(
            server_urls(r#"["http://localhost:11434/v1/","http://localhost:11434/v1"]"#).unwrap(),
            vec!["http://localhost:11434/v1"]
        );
        for url in [
            "file:///tmp/model",
            "https://user:secret@host/v1",
            "https://host/v1?key=secret",
        ] {
            assert!(normalize_server(url).is_err());
        }
    }
    #[test]
    fn preset_files_have_explicit_precedence_and_io_failures_are_not_defaults() {
        let root = tempfile::tempdir().unwrap();
        let adjacent = root.path().join("adjacent.env");
        let cwd = root.path().join("cwd.env");
        std::fs::write(&cwd, "LOCALVOX_LLM_MODEL=cwd-model").unwrap();
        let paths = [adjacent.clone(), cwd.clone()];
        assert_eq!(
            load_paths(&paths).unwrap().values["LOCALVOX_LLM_MODEL"],
            "cwd-model"
        );
        std::fs::write(&adjacent, "LOCALVOX_LLM_MODEL=adjacent-model").unwrap();
        assert_eq!(
            load_paths(&paths).unwrap().values["LOCALVOX_LLM_MODEL"],
            "adjacent-model"
        );
        std::fs::remove_file(&adjacent).unwrap();
        std::fs::create_dir(&adjacent).unwrap();
        assert!(load_paths(&paths).is_err());
        assert!(load_paths(&[root.path().join("missing")]).unwrap().packaged);
        assert!(parse("LOCALVOX_LANG=auto\nLOCALVOX_LANG=en").is_err());
    }
}
