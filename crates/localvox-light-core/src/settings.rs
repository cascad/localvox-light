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

/// WHEN a saved value starts working. Per setting, because the answer differs per setting and a
/// blanket «перезапустите демон» was a lie in both directions: it made live settings look dead,
/// and it hid the ones that genuinely cannot change under a running capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Applies {
    /// Read at the moment it is used, so saving is enough. The autocook flags are consulted on
    /// every cycle, the tool paths on every ingest, the LLM address on every job.
    Live,
    /// Held by the capture threads. Saving updates them and re-opens the streams — no restart,
    /// but there IS a visible seam: the microphone closes and opens again.
    Capture,
    /// Read ONCE, when the engine or the socket came up. A ring buffer already allocated cannot
    /// change its length under a running recording, and a bound port cannot move. We say so
    /// instead of pretending.
    Restart,
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
    pub applies: Applies,
}

impl Setting {
    pub fn secret(&self) -> bool {
        matches!(self.kind, Kind::Secret)
    }
}

/// What a key needs before it works. Unknown keys — the ones a person adds by hand in «остальное
/// в файле» — are assumed to need a restart: we do not know who reads them, and promising they
/// are live would be a guess presented as a fact.
pub fn applies(key: &str) -> Applies {
    CATALOGUE
        .iter()
        .find(|s| s.key == key)
        .map(|s| s.applies)
        .unwrap_or(Applies::Restart)
}

/// Put saved values into the RUNNING process, and report what is actually in force now.
///
/// Everything in this system that reads a setting reads it from the environment — the tool
/// resolvers on every ingest, `post_processing_from_env` on every autocook cycle, the LLM client
/// on every job. So updating the environment IS the hot reload for them: no watcher, no reload
/// signal, no second source of truth to drift from `.env`.
///
/// It is deliberately NOT a promise about everything. A value the engine read once, into a ring
/// buffer or a bound socket, does not change because the variable did — those are `Restart`, and
/// the return value says which ones the person still has to restart for.
///
/// Windows only, in practice: `SetEnvironmentVariable` is safe to call while other threads read.
/// On glibc the same call races `getenv` — if this daemon is ever ported, the environment stops
/// being an acceptable channel and this becomes an in-process overlay that readers consult.
pub fn apply_live(changes: &[(String, Option<String>)]) -> Vec<&'static str> {
    let mut need_restart: Vec<&'static str> = Vec::new();
    let mut touched_capture = false;
    for (key, value) in changes {
        match applies(key) {
            Applies::Live | Applies::Capture => {
                match value {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
                touched_capture |= applies(key) == Applies::Capture;
            }
            Applies::Restart => {
                // The variable is set anyway, so that a component reading it LATER (a job that
                // has not started yet) sees the new value rather than the old one. What cannot
                // be updated is what already holds a copy — and that is what we report.
                match value {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
                if let Some(s) = CATALOGUE.iter().find(|s| s.key == key) {
                    if !need_restart.contains(&s.label) {
                        need_restart.push(s.label);
                    }
                }
            }
        }
    }
    // The devices move NOW — the streams close and open on the new endpoint. If nobody registered
    // the controls (a CLI run, no engine in this process), the promise cannot be kept, and the
    // person is told the truth instead: these need a restart after all.
    if touched_capture && !crate::audio::reload_capture_from_env() {
        for s in CATALOGUE.iter().filter(|s| s.applies == Applies::Capture) {
            if changes.iter().any(|(k, _)| k == s.key) && !need_restart.contains(&s.label) {
                need_restart.push(s.label);
            }
        }
    }
    need_restart
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
        applies: Applies::Live,
    },
    Setting {
        key: "LOCALVOX_LIGHT_YT_FFMPEG",
        group: "Инструменты",
        label: "ffmpeg",
        hint: "не задано — ищем рядом с exe, затем в PATH",
        kind: Kind::Path,
        applies: Applies::Live,
    },
    Setting {
        key: "LOCALVOX_LIGHT_YT_JS_RUNTIME_PATH",
        group: "Инструменты",
        label: "node (для YouTube)",
        hint: "нужен не всегда: YouTube иногда требует JS для расшифровки ссылки",
        kind: Kind::Path,
        applies: Applies::Live,
    },
    // ── Устройства ───────────────────────────────────────────────────────────
    Setting {
        key: "LOCALVOX_LIGHT_MIC",
        group: "Устройства",
        label: "Микрофон",
        hint: "default — системный по умолчанию",
        kind: Kind::Text,
        applies: Applies::Capture,
    },
    Setting {
        key: "LOCALVOX_LIGHT_LOOPBACK_DEVICE",
        group: "Устройства",
        label: "Системный звук (loopback)",
        hint: "default-output — то, что звучит в колонках",
        kind: Kind::Text,
        applies: Applies::Capture,
    },
    // ── Запись ───────────────────────────────────────────────────────────────
    Setting {
        key: "LOCALVOX_LIGHT_PREROLL_SEC",
        group: "Запись",
        label: "Буфер прошлого, сек",
        // The hint states the DEFAULT'S consequence, not the number alone: this buffer is the one
        // setting here that costs RAM continuously, and it costs it whether or not anything is
        // being recorded. A person raising it to an hour deserves to know that before, not after.
        hint: "300 — пять минут, ≈19 МБ памяти на две дорожки",
        kind: Kind::Number,
        applies: Applies::Restart,
    },
    Setting {
        key: "LOCALVOX_LIGHT_CHUNK_SEC",
        group: "Запись",
        label: "Ротация файлов аудио, сек",
        // Not a quality knob: neighbouring chunks join sample-exactly, so this changes how the
        // recording is SPLIT ON DISK and nothing about what is recorded.
        hint: "300 — новый файл каждые пять минут",
        kind: Kind::Number,
        applies: Applies::Restart,
    },
    Setting {
        key: "LOCALVOX_LIGHT_AUTOSTOP_SEC",
        group: "Запись",
        label: "Автостоп по тишине, сек",
        hint: "900 — пятнадцать минут тишины на обеих дорожках; 0 — никогда",
        kind: Kind::Number,
        applies: Applies::Restart,
    },
    // ── LLM ──────────────────────────────────────────────────────────────────
    Setting {
        key: "LOCALVOX_LLM_BASE_URL",
        group: "LLM",
        label: "Адрес сервера",
        hint: "http://localhost:11434/v1 — локальная Ollama",
        kind: Kind::Text,
        applies: Applies::Live,
    },
    Setting {
        key: "LOCALVOX_LLM_MODEL",
        group: "LLM",
        label: "Модель",
        hint: "модель для причёсывания и сводки",
        kind: Kind::Text,
        applies: Applies::Live,
    },
    // Only the SUMMARY may go to Claude; the line-by-line cleanup stays local (it is high-volume and
    // would burn the subscription). Live: the cook is spawned fresh and reads this from the env on
    // every run, so a saved value reaches the NEXT cook without a restart. Existing summaries keep
    // their model until re-cooked («Переварить») — a switch never re-cooks the archive on its own.
    Setting {
        key: "LOCALVOX_SUMMARY_PROVIDER",
        group: "LLM",
        label: "Сводку делает",
        hint: "локальная модель (Ollama)",
        kind: Kind::Text,
        applies: Applies::Live,
    },
    // ── API ──────────────────────────────────────────────────────────────────
    Setting {
        key: "LOCALVOX_API_BIND",
        group: "API",
        label: "Адрес",
        hint: "127.0.0.1:3017 — только этот компьютер",
        kind: Kind::Text,
        applies: Applies::Restart,
    },
    Setting {
        key: "LOCALVOX_API_TOKEN",
        group: "API",
        label: "Токен",
        hint: "обязателен, если адрес не localhost",
        kind: Kind::Secret,
        applies: Applies::Restart,
    },
    // ── Автоварка ────────────────────────────────────────────────────────────
    Setting {
        key: "LOCALVOX_LIGHT_AUTOCOOK",
        group: "Автоварка",
        label: "Доваривать записи сама",
        hint: "включена",
        kind: Kind::Bool,
        applies: Applies::Live,
    },
    Setting {
        key: "LOCALVOX_LIGHT_AUTOCOOK_INTERVAL_SEC",
        group: "Автоварка",
        label: "Как часто искать недоваренное, сек",
        hint: "300",
        kind: Kind::Number,
        applies: Applies::Live,
    },
    Setting {
        key: "LOCALVOX_LIGHT_AUTOCOOK_QUIESCENT_SEC",
        group: "Автоварка",
        label: "Сессия «тихая» столько секунд → варим",
        hint: "60",
        kind: Kind::Number,
        applies: Applies::Live,
    },
    Setting {
        key: "LOCALVOX_LIGHT_AUTOCOOK_SUMMARY",
        group: "Автоварка",
        label: "Делать сводку",
        hint: "включено",
        kind: Kind::Bool,
        applies: Applies::Live,
    },
    Setting {
        key: "LOCALVOX_LIGHT_AUTOCOOK_CLEANUP",
        group: "Автоварка",
        label: "Делать «Текст»",
        hint: "включено",
        kind: Kind::Bool,
        applies: Applies::Live,
    },
    Setting {
        key: "LOCALVOX_LIGHT_AUTOCOOK_REFINE",
        group: "Автоварка",
        label: "Причёсывать реплики",
        hint: "включено",
        kind: Kind::Bool,
        applies: Applies::Live,
    },
    // ── Язык ─────────────────────────────────────────────────────────────────
    Setting {
        key: "LOCALVOX_LANG",
        group: "Язык",
        label: "Язык записи",
        hint: "auto — язык текста определяем по самой расшифровке",
        kind: Kind::Text,
        applies: Applies::Live,
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
