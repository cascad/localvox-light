//! The catalogue of settings the UI shows — and the only place that knows they exist.
//!
//! Adding a setting to the screen is ONE line here: the API serves this list and the UI renders it
//! generically. A list hardcoded in the frontend as well would be a second source of truth for the
//! same facts, and the two would drift the first time either was touched.
//!
//! WHAT THIS DELIBERATELY DOES NOT KNOW: the default values. They live at the call sites, as
//! `env::var(...).unwrap_or(default)`, and copying them here would create exactly the drift this
//! module exists to avoid — the catalogue would keep claiming `300` long after the code moved on.
//! So an unset key is reported as unset, and the screen says «по умолчанию» rather than inventing
//! a number. `hint` is documentation (what the code does when the key is absent), shown as
//! placeholder text — never as a value.
//!
//! The catalogue is a CURATED set, not all 89 keys. Everything else is reachable through the raw
//! key/value area: since every setting is just a `.env` key, that covers the rest without a
//! hand-written field for each.

use serde::Serialize;

/// How the screen should render the field. The kind is presentation, not validation: the value
/// always ends up as a string in `.env`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Text,
    /// A filesystem path — the screen can offer to check that it exists.
    Path,
    /// `on`/`off`. Written as the word, because that is what the readers accept.
    Bool,
    Number,
    /// Never echoed back to the screen once set — see `Setting::secret`.
    Secret,
}

#[derive(Debug, Clone, Serialize)]
pub struct Setting {
    pub key: &'static str,
    pub group: &'static str,
    pub label: &'static str,
    /// What happens if the key is left unset, in the owner's language. Placeholder text, not a
    /// value: the real default lives in the code.
    pub hint: &'static str,
    pub kind: Kind,
}

impl Setting {
    pub fn secret(&self) -> bool {
        matches!(self.kind, Kind::Secret)
    }
}

/// The curated catalogue. Order is the order on screen.
pub const CATALOGUE: &[Setting] = &[
    // ── Инструменты ──────────────────────────────────────────────────────────
    // The group that started all this: the daemon ingest resolved these to bare names and failed
    // with "not installed" while the binary sat in `bin/`.
    Setting {
        key: "LOCALVOX_LIGHT_YT_DLP",
        group: "Инструменты",
        label: "yt-dlp",
        hint: "не задано — ищем рядом с exe, затем в PATH",
        kind: Kind::Path,
    },
    Setting {
        key: "LOCALVOX_LIGHT_YT_FFMPEG",
        group: "Инструменты",
        label: "ffmpeg",
        hint: "не задано — ищем рядом с exe, затем в PATH",
        kind: Kind::Path,
    },
    Setting {
        key: "LOCALVOX_LIGHT_YT_JS_RUNTIME_PATH",
        group: "Инструменты",
        label: "node (для YouTube)",
        hint: "нужен не всегда: YouTube иногда требует JS для расшифровки ссылки",
        kind: Kind::Path,
    },
    // ── Устройства ───────────────────────────────────────────────────────────
    Setting {
        key: "LOCALVOX_LIGHT_MIC",
        group: "Устройства",
        label: "Микрофон",
        hint: "default — системный по умолчанию",
        kind: Kind::Text,
    },
    Setting {
        key: "LOCALVOX_LIGHT_LOOPBACK_DEVICE",
        group: "Устройства",
        label: "Системный звук (loopback)",
        hint: "default-output — то, что звучит в колонках",
        kind: Kind::Text,
    },
    // ── LLM ──────────────────────────────────────────────────────────────────
    Setting {
        key: "LOCALVOX_LLM_BASE_URL",
        group: "LLM",
        label: "Адрес сервера",
        hint: "http://localhost:11434/v1 — локальная Ollama",
        kind: Kind::Text,
    },
    Setting {
        key: "LOCALVOX_LLM_MODEL",
        group: "LLM",
        label: "Модель",
        hint: "модель для причёсывания и сводки",
        kind: Kind::Text,
    },
    // ── API ──────────────────────────────────────────────────────────────────
    Setting {
        key: "LOCALVOX_API_BIND",
        group: "API",
        label: "Адрес",
        hint: "127.0.0.1:3017 — только этот компьютер",
        kind: Kind::Text,
    },
    Setting {
        key: "LOCALVOX_API_TOKEN",
        group: "API",
        label: "Токен",
        hint: "обязателен, если адрес не localhost",
        kind: Kind::Secret,
    },
    // ── Автоварка ────────────────────────────────────────────────────────────
    Setting {
        key: "LOCALVOX_LIGHT_AUTOCOOK",
        group: "Автоварка",
        label: "Доваривать записи сама",
        hint: "включена",
        kind: Kind::Bool,
    },
    Setting {
        key: "LOCALVOX_LIGHT_AUTOCOOK_INTERVAL_SEC",
        group: "Автоварка",
        label: "Как часто искать недоваренное, сек",
        hint: "300",
        kind: Kind::Number,
    },
    Setting {
        key: "LOCALVOX_LIGHT_AUTOCOOK_QUIESCENT_SEC",
        group: "Автоварка",
        label: "Сессия «тихая» столько секунд → варим",
        hint: "60",
        kind: Kind::Number,
    },
    Setting {
        key: "LOCALVOX_LIGHT_AUTOCOOK_SUMMARY",
        group: "Автоварка",
        label: "Делать сводку",
        hint: "включено",
        kind: Kind::Bool,
    },
    Setting {
        key: "LOCALVOX_LIGHT_AUTOCOOK_CLEANUP",
        group: "Автоварка",
        label: "Делать читаемый текст",
        hint: "включено",
        kind: Kind::Bool,
    },
    Setting {
        key: "LOCALVOX_LIGHT_AUTOCOOK_REFINE",
        group: "Автоварка",
        label: "Причёсывать расшифровку",
        hint: "включено",
        kind: Kind::Bool,
    },
    // ── Язык ─────────────────────────────────────────────────────────────────
    Setting {
        key: "LOCALVOX_LANG",
        group: "Язык",
        label: "Язык записи",
        hint: "auto — язык текста определяем по самой расшифровке",
        kind: Kind::Text,
    },
];

/// Is this key ours to write at all?
///
/// The raw key/value area accepts ANY setting, which is the point — every setting is just a `.env`
/// key. But it must not become a way to write arbitrary environment for the daemon: a request that
/// set `PATH` would be a way to make it run someone else's binary. So the prefix is the boundary,
/// plus `RUST_LOG`, which is ours in practice and documented in `.env.example`.
pub fn writable(key: &str) -> bool {
    let k = key.trim();
    if k.is_empty() || k.len() > 128 {
        return false;
    }
    if !k
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    {
        return false;
    }
    k.starts_with("LOCALVOX_") || k == "RUST_LOG"
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The prefix is a security boundary, not tidiness: writing `PATH` into the daemon's `.env`
    /// would be a way to make it execute a different `ffmpeg` than the operator believes.
    #[test]
    fn only_our_own_keys_can_be_written() {
        assert!(writable("LOCALVOX_LIGHT_YT_DLP"));
        assert!(writable("RUST_LOG"));

        assert!(!writable("PATH"), "PATH would redirect every child process we spawn");
        assert!(!writable("OPENAI_API_KEY"));
        assert!(!writable(""));
        // No lowercase, spaces or separators — a key like this could smuggle a second assignment.
        assert!(!writable("LOCALVOX_A=B"));
        assert!(!writable("LOCALVOX_A B"));
        assert!(!writable("localvox_light_mic"));
        assert!(!writable("LOCALVOX_A\nPATH"));
    }

    /// The catalogue is served to the screen; a duplicate key would render two fields writing to
    /// one place, and the loser would silently win on save.
    #[test]
    fn the_catalogue_has_no_duplicate_keys() {
        let mut seen = std::collections::BTreeSet::new();
        for s in CATALOGUE {
            assert!(seen.insert(s.key), "duplicate key in the catalogue: {}", s.key);
        }
    }

    /// Everything in the catalogue must be writable, or the screen would offer a field that the
    /// save endpoint then refuses.
    #[test]
    fn every_catalogued_setting_can_actually_be_saved() {
        for s in CATALOGUE {
            assert!(writable(s.key), "catalogued but not writable: {}", s.key);
        }
    }
}
