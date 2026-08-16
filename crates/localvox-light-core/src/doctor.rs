//! `--doctor`: продукт проверяет сам себя.
//!
//! ПОЧЕМУ ЭТО ЗДЕСЬ, А НЕ В СКРИПТЕ УСТАНОВКИ. Скрипт знает разложенный им каталог; демон
//! знает, где он ИЩЕТ. Это разные вопросы, и они уже разошлись: модель Vosk может лежать на
//! другом диске по `LOCALVOX_LIGHT_MODEL`, и установщик, проверяющий свой `models/`, честно
//! скажет «нет модели» там, где всё работает. Скрипт, заново выводящий правила поиска, устареет
//! молча и соврёт ровно тогда, когда его позовут разбираться.
//!
//! Поэтому проверки зовут ТЕ ЖЕ функции разрешения путей, что и рабочий код:
//! [`crate::lang::default_model_dir`], [`crate::diarize::model_dir`],
//! [`crate::ner::model_dir_near`], [`crate::cli::validate_vosk_model_dir`]. Третьей копии правил
//! не существует.
//!
//! Вся энтропия (переменные среды, текущий каталог, путь к exe) снимается в [`Layout::from_env`]
//! — на композиционном корне. [`inspect`] уже чистая функция от разложенного: те же пути → тот
//! же вывод, и её можно проверить тестом на временном каталоге.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Работает.
    Ok,
    /// Продукт запустится, но чего-то не будет уметь. Человек должен знать ЧЕГО.
    Warn,
    /// Не заработает. Без этого запускаться бессмысленно.
    Fail,
}

impl State {
    pub fn mark(self) -> &'static str {
        match self {
            State::Ok => "OK  ",
            State::Warn => "ЖДЁТ",
            State::Fail => "НЕТ ",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Finding {
    pub what: String,
    pub state: State,
    pub detail: String,
    /// Что сделать человеку. Диагноз без назначения — это жалоба, а не помощь.
    pub fix: Option<String>,
}

impl Finding {
    fn ok(what: &str, detail: impl Into<String>) -> Self {
        Finding { what: what.into(), state: State::Ok, detail: detail.into(), fix: None }
    }
    fn bad(what: &str, state: State, detail: impl Into<String>, fix: impl Into<String>) -> Self {
        Finding {
            what: what.into(),
            state,
            detail: detail.into(),
            fix: Some(fix.into()),
        }
    }
}

/// Куда всё разрешилось. Строится из среды один раз; ниже — только чистые проверки.
#[derive(Debug, Clone)]
pub struct Layout {
    pub vosk_model: PathBuf,
    pub gigaam: Option<PathBuf>,
    pub diarize: Option<PathBuf>,
    pub ner: Option<PathBuf>,
    /// Повар рядом с нами. `None` — автоварка выключится, архив не расшифруется никогда.
    pub cook: Option<PathBuf>,
    pub work_dir: PathBuf,
    pub llm_base_url: String,
    pub llm_model: String,
    pub api_bind: String,
}

/// Имя бинаря повара: суффикс только там, где его требует ОС.
pub fn cook_exe_name() -> &'static str {
    if cfg!(windows) {
        "localvox-process.exe"
    } else {
        "localvox-process"
    }
}

impl Layout {
    /// Единственное место, где читается среда. Дальше — детерминированно.
    pub fn from_env(vosk_model: PathBuf, work_dir: PathBuf) -> Self {
        let gigaam = crate::lang::default_model_dir();
        let cook = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join(cook_exe_name())))
            .filter(|p| p.is_file());
        // The SAME rule the loaders use — `ner` itself sits behind the `onnx` feature, which the
        // daemon does not build, and a doctor spelling the directory on its own would report on
        // a place nothing looks at.
        Layout {
            vosk_model,
            diarize: crate::lang::sibling_model_dir(crate::lang::DIARIZE, gigaam.as_deref()),
            ner: crate::lang::sibling_model_dir(crate::lang::NER, gigaam.as_deref()),
            gigaam,
            cook,
            work_dir,
            llm_base_url: std::env::var("LOCALVOX_LLM_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:11434/v1".into()),
            llm_model: std::env::var("LOCALVOX_LLM_MODEL").unwrap_or_else(|_| "qwen3.5:9b".into()),
            api_bind: std::env::var("LOCALVOX_API_BIND")
                .unwrap_or_else(|_| "127.0.0.1:3017".into()),
        }
    }
}

/// Все файлы на месте? Возвращает имя первого недостающего.
fn first_missing<'a>(dir: &Path, files: &[&'a str]) -> Option<&'a str> {
    files.iter().copied().find(|f| !dir.join(f).is_file())
}

/// Полный разбор — чистая функция от разложенных путей.
pub fn inspect(l: &Layout) -> Vec<Finding> {
    let mut out = Vec::new();

    // ── Живое распознавание. Без него демон не стартует вообще: validate_vosk_model
    //    вызывается в main() безусловно.
    out.push(match crate::cli::validate_vosk_model_dir(&l.vosk_model) {
        Ok(()) => Finding::ok("Vosk (живое распознавание)", l.vosk_model.display().to_string()),
        Err(e) => Finding::bad(
            "Vosk (живое распознавание)",
            State::Fail,
            format!("{}: {e}", l.vosk_model.display()),
            "scripts/fetch-models.sh --only vosk (или fetch-models.ps1 -Only vosk); \
             путь задаётся LOCALVOX_LIGHT_MODEL",
        ),
    });

    // ── Расшифровка архива.
    out.push(match &l.gigaam {
        Some(d) => match first_missing(
            d,
            &["v3_e2e_ctc.int8.onnx", "v3_e2e_ctc.yaml", "v3_e2e_ctc_vocab.txt"],
        ) {
            None => Finding::ok("GigaAM (расшифровка архива)", d.display().to_string()),
            Some(f) => Finding::bad(
                "GigaAM (расшифровка архива)",
                State::Fail,
                format!("в {} нет {f}", d.display()),
                "scripts/fetch-models.sh --only gigaam",
            ),
        },
        None => Finding::bad(
            "GigaAM (расшифровка архива)",
            State::Fail,
            "каталог модели не найден",
            "scripts/fetch-models.sh --only gigaam; путь задаётся LOCALVOX_ASR_MODEL_DIR",
        ),
    });

    // ── Повар. Его отсутствие — самая тихая из поломок: демон пишет звук, автоварка молча
    //    выключается, и человек узнаёт об этом по пустому архиву через неделю.
    out.push(match &l.cook {
        Some(p) => Finding::ok("Повар (localvox-process)", p.display().to_string()),
        None => Finding::bad(
            "Повар (localvox-process)",
            State::Fail,
            format!("нет {} рядом с демоном", cook_exe_name()),
            "положить его рядом с localvox-light: без него автоварка выключается и архив \
             не расшифровывается НИКОГДА",
        ),
    });

    // ── Необязательное: продукт работает, но чего-то не умеет. Именно поэтому ЖДЁТ, а не НЕТ.
    out.push(match &l.diarize {
        Some(d) => match first_missing(d, &["segmentation.onnx", "embedding.onnx"]) {
            None => Finding::ok("Диаризация (кто говорит)", d.display().to_string()),
            Some(f) => Finding::bad(
                "Диаризация (кто говорит)",
                State::Warn,
                format!("в {} нет {f}", d.display()),
                "scripts/fetch-models.sh --only diarize",
            ),
        },
        None => Finding::bad(
            "Диаризация (кто говорит)",
            State::Warn,
            "модели нет — записи расшифруются, но без разметки говорящих",
            "scripts/fetch-models.sh --only diarize",
        ),
    });

    out.push(match &l.ner {
        Some(d) => match first_missing(d, &["tokenizer.json", "gliner_config.json"]) {
            None => Finding::ok("GLiNER (проверка имён)", d.display().to_string()),
            Some(f) => Finding::bad(
                "GLiNER (проверка имён)",
                State::Warn,
                format!("в {} нет {f}", d.display()),
                "scripts/fetch-models.sh --only ner",
            ),
        },
        None => Finding::bad(
            "GLiNER (проверка имён)",
            State::Warn,
            "модели нет — имена в ответах LLM не проверяются (числа проверяются всегда)",
            "scripts/fetch-models.sh --only ner",
        ),
    });

    // ── Архив. Каталог создаётся при первой записи, поэтому «его ещё нет» это не отказ;
    //    отказ — это когда он есть и НЕ КАТАЛОГ.
    out.push(if l.work_dir.is_dir() {
        Finding::ok("Архив", l.work_dir.display().to_string())
    } else if l.work_dir.exists() {
        Finding::bad(
            "Архив",
            State::Fail,
            format!("{} — это файл, а не каталог", l.work_dir.display()),
            "поправить LOCALVOX_LIGHT_AUDIO_DIR",
        )
    } else {
        Finding::bad(
            "Архив",
            State::Warn,
            format!("{} ещё нет — создастся при первой записи", l.work_dir.display()),
            "ничего не делать, если путь верный",
        )
    });

    out
}

/// yt-dlp ломается вслед за YouTube каждые несколько недель (HTTP 403 на скачивании). Бинарь старше
/// этого — кандидат на поломку, и доктор должен предупредить ДО того, как ингест по ссылке отвалится.
pub const YT_DLP_STALE_DAYS: i64 = 30;

/// Дата сборки из строки версии yt-dlp: «2026.08.16» или «2026.08.16.020253». Чистая, тестируемая.
pub fn parse_yt_dlp_date(version: &str) -> Option<chrono::NaiveDate> {
    let mut it = version.trim().split('.');
    let y = it.next()?.parse::<i32>().ok()?;
    let m = it.next()?.parse::<u32>().ok()?;
    let d = it.next()?.parse::<u32>().ok()?;
    chrono::NaiveDate::from_ymd_opt(y, m, d)
}

/// Находка про yt-dlp — чистая. «Сейчас» и запуск бинаря снимаются вызывающим (см. `probe_yt_dlp`
/// в демоне); сюда приходят уже разрешённый путь, версия (`None` — не найден/не ответил) и возраст
/// в днях. Устаревший или отсутствующий yt-dlp — это ЖДЁТ, а не отказ: запись и варка от него не
/// зависят, без него не работает только «запись по ссылке».
pub fn yt_dlp_finding(resolved: &str, version: Option<&str>, age_days: Option<i64>) -> Finding {
    match (version, age_days) {
        (None, _) => Finding::bad(
            "yt-dlp (запись по ссылке)",
            State::Warn,
            format!("{resolved}: не найден или не отвечает"),
            "поставить yt-dlp (nightly) и указать путь в LOCALVOX_LIGHT_YT_DLP; \
             без него запись по ссылке не работает",
        ),
        (Some(v), Some(age)) if age > YT_DLP_STALE_DAYS => Finding::bad(
            "yt-dlp (запись по ссылке)",
            State::Warn,
            format!("{v} — {age} дн.: YouTube ломает старые версии (HTTP 403)"),
            format!(
                "обновить по месту: localvox-light --update-yt-dlp  (или напрямую: {resolved} --update-to nightly)"
            ),
        ),
        (Some(v), _) => Finding::ok("yt-dlp (запись по ссылке)", format!("{v}  ({resolved})")),
    }
}

/// Худшее из найденного — им и определяется код возврата.
pub fn worst(findings: &[Finding]) -> State {
    if findings.iter().any(|f| f.state == State::Fail) {
        State::Fail
    } else if findings.iter().any(|f| f.state == State::Warn) {
        State::Warn
    } else {
        State::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }

    fn layout(root: &Path) -> Layout {
        Layout {
            vosk_model: root.join("vosk"),
            gigaam: None,
            diarize: None,
            ner: None,
            cook: None,
            work_dir: root.join("arch"),
            llm_base_url: "http://localhost:11434/v1".into(),
            llm_model: "m".into(),
            api_bind: "127.0.0.1:3017".into(),
        }
    }

    fn find<'a>(fs: &'a [Finding], what: &str) -> &'a Finding {
        fs.iter().find(|f| f.what.starts_with(what)).expect(what)
    }

    /// Пустая установка: обязательное — отказ, необязательное — предупреждение. Разница
    /// не косметическая: по ней человек решает, чинить сейчас или можно работать.
    #[test]
    fn an_empty_install_fails_on_the_required_and_only_warns_on_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let fs = inspect(&layout(tmp.path()));

        assert_eq!(find(&fs, "Vosk").state, State::Fail);
        assert_eq!(find(&fs, "GigaAM").state, State::Fail);
        assert_eq!(find(&fs, "Повар").state, State::Fail);
        assert_eq!(find(&fs, "Диаризация").state, State::Warn);
        assert_eq!(find(&fs, "GLiNER").state, State::Warn);
        assert_eq!(worst(&fs), State::Fail);
    }

    /// Каждая находка не-Ok обязана нести назначение. Диагноз без «что делать» человек
    /// прочитает как «сломалось, разбирайся сам» — а доктор ровно затем и зовётся.
    #[test]
    fn every_complaint_says_what_to_do_about_it() {
        let tmp = tempfile::tempdir().unwrap();
        for f in inspect(&layout(tmp.path())) {
            if f.state != State::Ok {
                assert!(f.fix.is_some(), "без назначения: {} — {}", f.what, f.detail);
            }
        }
    }

    /// Неполный комплект ловится так же, как отсутствующий: каталог есть, а модель в нём
    /// половинчатая — это не «почти работает», это не работает.
    #[test]
    fn half_a_model_is_not_a_model() {
        let tmp = tempfile::tempdir().unwrap();
        let g = tmp.path().join("gigaam");
        touch(&g.join("v3_e2e_ctc.int8.onnx"));
        touch(&g.join("v3_e2e_ctc.yaml")); // словаря нет

        let mut l = layout(tmp.path());
        l.gigaam = Some(g);
        let fs = inspect(&l);
        let f = find(&fs, "GigaAM");
        assert_eq!(f.state, State::Fail);
        assert!(f.detail.contains("v3_e2e_ctc_vocab.txt"), "{}", f.detail);
    }

    /// Строка версии yt-dlp разбирается в дату; лишний хвост (`.020253`) и мусор не мешают.
    #[test]
    fn yt_dlp_version_parses_to_a_date() {
        use chrono::Datelike;
        let d = parse_yt_dlp_date("2026.08.16.020253").unwrap();
        assert_eq!((d.year(), d.month(), d.day()), (2026, 8, 16));
        assert_eq!(parse_yt_dlp_date("2026.07.04").unwrap().day(), 4);
        assert!(parse_yt_dlp_date("nightly").is_none());
        assert!(parse_yt_dlp_date("").is_none());
    }

    /// Свежий — OK; старше порога — ЖДЁТ с командой обновления; отсутствующий — ЖДЁТ, но не отказ
    /// (запись и варка от yt-dlp не зависят).
    #[test]
    fn yt_dlp_finding_flags_stale_and_missing_but_never_fails() {
        let fresh = yt_dlp_finding("bin/yt-dlp.exe", Some("2026.08.16"), Some(1));
        assert_eq!(fresh.state, State::Ok);

        let stale = yt_dlp_finding("bin/yt-dlp.exe", Some("2026.07.04"), Some(43));
        assert_eq!(stale.state, State::Warn);
        assert!(stale.fix.as_deref().unwrap().contains("--update-yt-dlp"));

        let missing = yt_dlp_finding("yt-dlp", None, None);
        assert_eq!(missing.state, State::Warn);
        assert_ne!(missing.state, State::Fail, "нет yt-dlp — это не отказ продукта");
    }

    /// Каталога архива ещё нет — это НЕ отказ: он создаётся при первой записи. А вот файл
    /// на его месте — отказ, и молчать о нём нельзя.
    #[test]
    fn a_missing_archive_waits_but_a_file_in_its_place_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let mut l = layout(tmp.path());
        assert_eq!(find(&inspect(&l), "Архив").state, State::Warn);

        let f = tmp.path().join("occupied");
        touch(&f);
        l.work_dir = f;
        assert_eq!(find(&inspect(&l), "Архив").state, State::Fail);
    }
}
