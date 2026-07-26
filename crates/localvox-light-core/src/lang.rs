//! The language of a recording.
//!
//! **The main idea: the language is part of the RECIPE.** It goes into the transcript
//! version label (`versions::cook_recipe`) and into the recipe of the LLM artifacts
//! (`processing::llm_recipe`). That is why «the user changed the language → reset
//! everything derived and re-cook it» needs neither a separate button nor cleanup code:
//! the recipe diverged, which means the session was cooked the old way, which means it
//! will be re-cooked — by the same archive migration mechanism that handles a change of
//! model or of windows. The reset happens BY CONSTRUCTION.
//!
//! **Where the language comes from** (the first non-empty one):
//! 1. the session's `meta.lang` — a human's explicit choice for this recording;
//! 2. `LOCALVOX_LANG` — the global choice (`auto` = no choice);
//! 3. auto-detection — only for TEXT (see below);
//! 4. [`DEFAULT`] — the language of the model that ships with the product.
//!
//! **Why auto-detection does not pick the ASR model.** The language can only be determined
//! from text, and the text is produced by a model — which still has to be picked. The
//! chicken and the egg. Worse: a Russian model does not «fail to recognize» English
//! speech, it produces plausible Russian mush, and any detector will honestly say «this is
//! Russian». That is why auto-detection works where the text ALREADY exists: choosing the
//! templates and the language of the LLM's answer. To RECOGNIZE another language it has to
//! be named explicitly — and then a model for it is needed. Determining the language from
//! the sound itself (audio LID) is possible, but that is a separate model; for now it is
//! more honest to say so than to pretend.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

/// The language «out of the box»: the bundled ASR model, the templates, the dictionaries.
pub const DEFAULT: &str = "ru";

/// How many words are needed to even attempt detecting the language. On three words any
/// statistical detector is a coin toss, and a mistake here is expensive: the wrong
/// template, the wrong answer language, a needless re-cook.
const MIN_WORDS_TO_DETECT: usize = 20;

// ─────────────────────────── codes ───────────────────────────

/// An ISO-639 code → the canonical two-letter one. We take the list of languages from
/// `isolang` rather than inventing our own: «ru», «rus», «RU» are one and the same,
/// «эльфийский» is an error.
pub fn normalize(code: &str) -> Result<String> {
    let c = code.trim().to_lowercase();
    let lang = isolang::Language::from_639_1(&c)
        .or_else(|| isolang::Language::from_639_3(&c))
        .with_context(|| format!("unknown language code «{code}» (an ISO-639 one is needed: ru, en, de…)"))?;
    lang.to_639_1()
        .map(str::to_string)
        .with_context(|| format!("language «{code}» has no two-letter ISO-639-1 code"))
}

/// The language of a text — or `None` if there is nothing to say it by (too little text)
/// or the detector itself is unsure. `None` is more honest than a guess: a guess would
/// travel into the recipe and drag a re-cook along.
pub fn detect(text: &str) -> Option<String> {
    if text.split_whitespace().count() < MIN_WORDS_TO_DETECT {
        return None;
    }
    let info = whatlang::detect(text)?;
    if !info.is_reliable() {
        return None;
    }
    isolang::Language::from_639_3(info.lang().code())
        .and_then(|l| l.to_639_1())
        .map(str::to_string)
}

// ─────────────────────────── the choice ───────────────────────────

/// The global choice: `LOCALVOX_LANG=en`. Empty or `auto` — there is no choice.
pub fn global() -> Option<String> {
    let raw = std::env::var("LOCALVOX_LANG").ok()?;
    let raw = raw.trim();
    if raw.is_empty() || raw.eq_ignore_ascii_case("auto") {
        return None;
    }
    match normalize(raw) {
        Ok(code) => Some(code),
        Err(e) => {
            tracing::warn!("LOCALVOX_LANG: {e} — working as if it were auto");
            None
        }
    }
}

/// The explicit choice for this session — and ONLY its own, from `meta.json`.
///
/// We DELIBERATELY do not peek into the environment (`LOCALVOX_LANG`) from here. The
/// language is part of the recipe, so a variable affecting already recorded sessions would
/// act retroactively: set `LOCALVOX_LANG=en` for the sake of one call — and the whole
/// Russian archive becomes «cooked the wrong way», which means it would go for a re-cook
/// with an English model, losing `best`. The global choice is a choice for NEW recordings;
/// it is fixed in `meta.json` at the moment the session is created
/// ([`crate::chunks::create_session_dir`]).
pub fn explicit(session_dir: &Path) -> Option<String> {
    meta_field(session_dir, "lang")
}

/// The language we RECOGNIZE with. Auto-detection takes no part here (see the module
/// header): with no explicit choice we take the language of the default model.
pub fn asr(session_dir: &Path) -> String {
    explicit(session_dir).unwrap_or_else(|| DEFAULT.to_string())
}

/// The language we TALK to the LLM in (the template, the answer language, the
/// dictionaries). Here auto-detection is appropriate: the text already exists and is right
/// before our eyes.
pub fn text(session_dir: &Path) -> String {
    explicit(session_dir)
        .or_else(|| meta_field(session_dir, "lang_detected"))
        .unwrap_or_else(|| DEFAULT.to_string())
}

/// The transcript version label for a language — and, through it, the cook recipe.
///
/// It depends ONLY on the language: the recipe must be computable without going to disk
/// (discovery does that for every session in every cycle). And one more thing: for
/// [`DEFAULT`] the label stays the historical one (`gigaam-int8`), so adding a language
/// does NOT re-cook the whole already cooked Russian archive — the recipe is byte for byte
/// the same.
pub fn label(lang: &str) -> String {
    if lang == DEFAULT {
        crate::versions::DEFAULT_COOK_LABEL.to_string()
    } else {
        format!("asr-{lang}")
    }
}

// ─────────────────────────── the model ───────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    /// An ONNX model (GigaAM v3 and its kin).
    Onnx,
    /// A Vosk model (a directory with `am/`, `conf/`).
    Vosk,
}

#[derive(Debug)]
pub struct AsrModel {
    pub engine: Engine,
    pub dir: PathBuf,
}

/// The model directory of the default language: `LOCALVOX_ASR_MODEL_DIR` → next to the exe
/// → in the current directory.
///
/// «Next to the exe» is not a decoration: on Windows autostart the daemon's working
/// directory is `system32`, and a relative `models/…` will not be found there.
pub fn default_model_dir() -> Option<PathBuf> {
    const DEFAULT_MODEL: &str = "gigaam-v3-e2e-ctc";
    if let Some(d) = std::env::var_os("LOCALVOX_ASR_MODEL_DIR") {
        return Some(PathBuf::from(d));
    }
    let near_exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .map(|d| d.join("models").join(DEFAULT_MODEL))
        .filter(|p| p.is_dir());
    if near_exe.is_some() {
        return near_exe;
    }
    let in_cwd = PathBuf::from("models").join(DEFAULT_MODEL);
    in_cwd.is_dir().then_some(in_cwd)
}

/// A model that is not the recogniser: it stands NEXT TO one and is found by the same rule.
///
/// The pair travels together on purpose. The override variable and the directory name were
/// written out separately in every place that needed them, and the doctor would have been one
/// more — a check spelling a directory differently from the code it checks is worse than no
/// check, because it reports on something that does not exist.
#[derive(Debug, Clone, Copy)]
pub struct SideModel {
    pub env: &'static str,
    pub dir: &'static str,
}

/// «Кто говорит» — сегментация + вектор голоса.
pub const DIARIZE: SideModel = SideModel {
    env: "LOCALVOX_DIARIZE_MODEL_DIR",
    dir: "diarize",
};

/// GLiNER — проверка имён в ответах LLM.
pub const NER: SideModel = SideModel {
    env: "LOCALVOX_NER_MODEL_DIR",
    dir: "ner-gliner",
};

/// Where a SIDE model lives (diarization, NER): `<env>` → next to the ASR model → `models/<name>`.
///
/// One rule, one implementation. It used to be written out twice — in `ner` and in
/// `diarize::segment` — and the second copy carried a comment saying «the same way of searching as
/// NER's, and for the same reason», which is a duplicate announcing itself. The third copy would
/// have been the doctor's.
///
/// «Next to the ASR model», not «by cwd»: on Windows autostart the working directory is
/// `system32`, and a model looked up by cwd is silently not found — the name check then simply
/// does not happen, and nothing says so.
pub fn sibling_model_dir(m: SideModel, asr_model_dir: Option<&Path>) -> Option<PathBuf> {
    let (env_var, dir_name) = (m.env, m.dir);
    if let Some(d) = std::env::var_os(env_var) {
        let d = PathBuf::from(d);
        return d.is_dir().then_some(d);
    }
    // First the model the caller named, then the one we can find ourselves.
    let roots = [asr_model_dir.map(Path::to_path_buf), default_model_dir()];
    for root in roots.into_iter().flatten() {
        if let Some(p) = root.parent().map(|p| p.join(dir_name)) {
            if p.is_dir() {
                return Some(p);
            }
        }
    }
    let in_cwd = PathBuf::from("models").join(dir_name);
    in_cwd.is_dir().then_some(in_cwd)
}

/// The model for a language. `default_dir` is the model of the default language (the
/// caller knows its path: next to the exe in a built application, `models/…` in
/// development).
///
/// The order: `LOCALVOX_ASR_MODEL_DIR_<LANGUAGE>` → `<models root>/asr-<language>` →
/// (for [`DEFAULT`] only) `default_dir`.
///
/// The models root is taken FROM `default_dir`, not from the current directory. Otherwise
/// on autostart (cwd = `system32`) the Russian model would be found by an absolute path
/// while the English one would be looked for in `C:\Windows\system32\models\asr-en` — and
/// «there is no model» would be a lie while the model lies next to the exe.
///
/// No model — a **loud error**, not a silent cook with the Russian model: silently
/// emitting plausible mush is worse than saying «there is no model».
pub fn asr_model(lang: &str, default_dir: &Path) -> Result<AsrModel> {
    let dir = model_dir_for(lang, default_dir).ok_or_else(|| {
        anyhow::anyhow!(
            "there is no ASR model for language «{lang}»: put it next to the default model \
             ({}) under the name asr-{lang}, or set LOCALVOX_ASR_MODEL_DIR_{} (a Vosk model \
             of this language will do, or an ONNX one). The default language is «{DEFAULT}»",
            default_dir.display(),
            lang.to_uppercase(),
        )
    })?;
    let engine = sniff(&dir)?;
    Ok(AsrModel { engine, dir })
}

/// Whether there is a model for a language at all. It is needed BEFORE destroying the
/// derived data: offering a human a language we cannot recognize means promising the
/// impossible.
pub fn has_model(lang: &str, default_dir: &Path) -> bool {
    model_dir_for(lang, default_dir).is_some_and(|d| sniff(&d).is_ok())
}

/// The languages a model may be found for: the default language + everything lying in
/// `<models root>/asr-*` + everything the `LOCALVOX_ASR_MODEL_DIR_*` variables point at.
///
/// The list is not invented («well, let there be ru, en, de»), it is COLLECTED from the
/// facts: offering a human a language we cannot recognize means promising the impossible
/// and destroying the derived data on the very first click.
pub fn candidates(default_dir: &Path) -> Vec<String> {
    let mut out = vec![DEFAULT.to_string()];

    let models_root = default_dir.parent().unwrap_or(Path::new("models"));
    if let Ok(entries) = std::fs::read_dir(models_root) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(code) = name.strip_prefix("asr-") {
                if let Ok(code) = normalize(code) {
                    out.push(code);
                }
            }
        }
    }
    for (key, _) in std::env::vars_os() {
        let key = key.to_string_lossy();
        if let Some(code) = key.strip_prefix("LOCALVOX_ASR_MODEL_DIR_") {
            if let Ok(code) = normalize(code) {
                out.push(code);
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn model_dir_for(lang: &str, default_dir: &Path) -> Option<PathBuf> {
    if let Some(d) = std::env::var_os(format!("LOCALVOX_ASR_MODEL_DIR_{}", lang.to_uppercase())) {
        return Some(PathBuf::from(d));
    }
    let models_root = default_dir.parent().unwrap_or(Path::new("models"));
    let by_convention = models_root.join(format!("asr-{lang}"));
    if by_convention.is_dir() {
        return Some(by_convention);
    }
    (lang == DEFAULT).then(|| default_dir.to_path_buf())
}

/// What kind of model lies in the directory. We do not ask the human — we look ourselves:
/// `*.onnx` → ONNX, `am/` + `conf/` → Vosk.
fn sniff(dir: &Path) -> Result<Engine> {
    if !dir.is_dir() {
        bail!("the ASR model directory was not found: {}", dir.display());
    }
    let has_onnx = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .any(|e| e.path().extension().and_then(|x| x.to_str()) == Some("onnx"))
        })
        .unwrap_or(false);
    if has_onnx {
        return Ok(Engine::Onnx);
    }
    if dir.join("am").is_dir() || dir.join("conf").is_dir() {
        return Ok(Engine::Vosk);
    }
    bail!(
        "in {} we can see neither an ONNX model (*.onnx) nor a Vosk model (am/, conf/)",
        dir.display()
    )
}

// ─────────────────────────── recording the choice ───────────────────────────

/// Record the session language (`None` — back to auto). Returns `true` if the choice
/// really changed.
///
/// The derived data we do NOT touch here: the language is part of the recipes, so
/// everything derived has already become «cooked by another recipe» — it will be
/// re-cooked. Erasing the files is a separate decision of the caller (in the UI we erase
/// them right away, so that a human does not read a summary in a language he has just
/// rejected).
pub fn set(session_dir: &Path, lang: Option<&str>) -> Result<bool> {
    let code = lang.map(normalize).transpose()?;
    let path = session_dir.join("meta.json");
    let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    // We edit the JSON as a value rather than through our own struct: meta has fields this
    // code does not know about (and more will appear), and they must not be lost.
    let mut meta: serde_json::Value =
        serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))?;
    let obj = meta.as_object_mut().context("meta.json is not an object")?;

    let was = obj.get("lang").and_then(|v| v.as_str()).map(str::to_string);
    if was == code {
        return Ok(false);
    }
    match &code {
        Some(c) => {
            obj.insert("lang".into(), serde_json::Value::String(c.clone()));
        }
        None => {
            obj.remove("lang");
        }
    }
    // What was detected earlier is about the previous text, obtained by the previous model.
    obj.remove("lang_detected");

    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&meta)?)
        .and_then(|()| std::fs::rename(&tmp, &path))
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(true)
}

/// Remember the language detected from the text (only when there is no explicit choice).
pub fn remember_detected(session_dir: &Path, code: &str) {
    let path = session_dir.join("meta.json");
    let Ok(bytes) = std::fs::read(&path) else {
        return;
    };
    let Ok(mut meta) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return;
    };
    let Some(obj) = meta.as_object_mut() else {
        return;
    };
    obj.insert(
        "lang_detected".into(),
        serde_json::Value::String(code.to_string()),
    );
    let tmp = path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, serde_json::to_vec_pretty(&meta).unwrap_or_default())
        .and_then(|()| std::fs::rename(&tmp, &path))
    {
        tracing::warn!("the language was not written into meta: {e}");
    }
}

fn meta_field(session_dir: &Path, field: &str) -> Option<String> {
    let bytes = std::fs::read(session_dir.join("meta.json")).ok()?;
    let meta: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let raw = meta.get(field)?.as_str()?;
    normalize(raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn session(meta: &str) -> tempfile::TempDir {
        let d = tempdir().unwrap();
        std::fs::write(d.path().join("meta.json"), meta).unwrap();
        d
    }

    #[test]
    fn codes_are_canonical_and_bogus_ones_are_rejected() {
        assert_eq!(normalize("RU").unwrap(), "ru");
        assert_eq!(normalize("rus").unwrap(), "ru");
        assert_eq!(normalize(" en ").unwrap(), "en");
        assert!(normalize("эльфийский").is_err());
    }

    #[test]
    fn a_language_change_changes_the_recipe() {
        // The whole construction rests on this: the recipe diverged → the session is
        // «cooked the old way» → it will be re-cooked. No separate reset of the derived
        // data is needed.
        let ru = crate::versions::cook_recipe(&label("ru"), 15.0, 8.0, 500, false);
        let en = crate::versions::cook_recipe(&label("en"), 15.0, 8.0, 500, false);
        assert_ne!(ru, en);
    }

    #[test]
    fn adding_languages_does_not_recook_the_russian_archive() {
        // The Russian label stayed the historical one — the recipe is byte for byte the
        // same. About diarization we ask in the SAME place the cook asks: otherwise the
        // test would start failing for exactly the person who has the diarization model
        // installed — that is, for the user, not for us.
        assert_eq!(label("ru"), crate::versions::DEFAULT_COOK_LABEL);
        assert_eq!(
            crate::versions::cook_recipe(
                &label("ru"),
                15.0,
                8.0,
                500,
                crate::diarize::enabled()
            ),
            crate::versions::default_cook_recipe()
        );
    }

    #[test]
    fn the_session_choice_beats_the_global_one() {
        let d = session(r#"{"started_at":"t","sample_rate":16000,"chunks":[],"lang":"en"}"#);
        assert_eq!(asr(d.path()), "en");
    }

    #[test]
    fn without_a_choice_we_speak_the_language_we_heard() {
        let d =
            session(r#"{"started_at":"t","sample_rate":16000,"chunks":[],"lang_detected":"en"}"#);
        // we recognized with the default model…
        assert_eq!(asr(d.path()), DEFAULT);
        // …but we talk to the LLM in the language of the text
        assert_eq!(text(d.path()), "en");
    }

    #[test]
    fn a_guess_is_not_made_from_three_words() {
        // On three words the detector is a coin. Silence is more honest: the language would
        // travel into the recipe and drag a re-cook of everything derived along with it.
        assert!(detect("Привет, как дела?").is_none());

        let ru = "Мы обсудили план работ на следующую неделю и договорились встретиться \
                  в понедельник, чтобы пройти по оставшимся вопросам проекта и по бюджету, \
                  который пока не согласован с финансами";
        assert_eq!(detect(ru).as_deref(), Some("ru"));

        let en = "We discussed the plan for the next week and agreed to meet on Monday \
                  to go through the remaining questions about the project and about the budget, \
                  which is still not approved by finance";
        assert_eq!(detect(en).as_deref(), Some("en"));
    }

    #[test]
    fn setting_a_language_forgets_what_we_guessed_before() {
        let d =
            session(r#"{"started_at":"t","sample_rate":16000,"chunks":[],"lang_detected":"ru"}"#);
        assert!(set(d.path(), Some("en")).unwrap());
        assert_eq!(asr(d.path()), "en");
        // the guess was about the PREVIOUS text, obtained by the PREVIOUS model
        let meta: serde_json::Value =
            serde_json::from_slice(&std::fs::read(d.path().join("meta.json")).unwrap()).unwrap();
        assert!(meta.get("lang_detected").is_none());
        // setting the same one again — not a change
        assert!(!set(d.path(), Some("en")).unwrap());
        // and back to auto
        assert!(set(d.path(), None).unwrap());
        assert!(meta_field(d.path(), "lang").is_none());
    }

    #[test]
    fn setting_a_language_keeps_the_rest_of_meta() {
        let d = session(
            r#"{"started_at":"t","sample_rate":16000,"chunks":[],"stopped_reason":"вручную","что_то_новое":1}"#,
        );
        set(d.path(), Some("en")).unwrap();
        let meta: serde_json::Value =
            serde_json::from_slice(&std::fs::read(d.path().join("meta.json")).unwrap()).unwrap();
        assert_eq!(meta["stopped_reason"], "вручную");
        assert_eq!(meta["что_то_новое"], 1);
    }

    #[test]
    fn a_missing_model_is_loud() {
        let e = asr_model("de", Path::new("models/gigaam-v3-e2e-ctc")).unwrap_err();
        let msg = format!("{e}");
        assert!(msg.contains("asr-de"), "{msg}");
    }

    #[test]
    fn a_language_model_lives_next_to_the_default_one_not_next_to_the_cwd() {
        // On Windows autostart the daemon's working directory is system32. The Russian
        // model was found by an absolute path while the English one was looked for in
        // C:\Windows\system32\models\asr-en — and «there is no model» was a lie while the
        // model lay next to the exe.
        let root = tempdir().unwrap();
        let models = root.path().join("models");
        let default_dir = models.join("gigaam-v3-e2e-ctc");
        std::fs::create_dir_all(&default_dir).unwrap();
        let en = models.join("asr-en");
        std::fs::create_dir_all(en.join("am")).unwrap(); // looks like Vosk
        std::fs::create_dir_all(en.join("conf")).unwrap();

        let m = asr_model("en", &default_dir)
            .expect("a model next to the default one MUST be found");
        assert_eq!(m.engine, Engine::Vosk);
        assert_eq!(m.dir, en);
        assert!(has_model("en", &default_dir));
        assert!(!has_model("de", &default_dir));
    }

    #[test]
    fn we_only_offer_languages_we_can_actually_recognise() {
        let root = tempdir().unwrap();
        let models = root.path().join("models");
        let default_dir = models.join("gigaam-v3-e2e-ctc");
        std::fs::create_dir_all(&default_dir).unwrap();
        std::fs::create_dir_all(models.join("asr-en").join("am")).unwrap();

        let langs = candidates(&default_dir);
        assert!(langs.contains(&"ru".to_string()));
        assert!(langs.contains(&"en".to_string()));
        assert!(
            !langs.contains(&"de".to_string()),
            "we offered a language we cannot handle: {langs:?}"
        );
    }

    #[test]
    fn the_global_setting_does_not_rewrite_the_past() {
        // The language is part of the recipe. If LOCALVOX_LANG acted on already recorded
        // sessions, a single variable would declare the WHOLE archive «cooked the wrong
        // way» — and it would be re-cooked by a foreign model, losing best. The global
        // choice is a choice for NEW recordings; it is fixed in meta when the session is
        // created.
        let d = session(r#"{"started_at":"t","sample_rate":16000,"chunks":[]}"#);
        std::env::set_var("LOCALVOX_LANG", "en");
        let seen = asr(d.path());
        std::env::remove_var("LOCALVOX_LANG");
        assert_eq!(
            seen, DEFAULT,
            "the variable rewrote the language of an already recorded session"
        );
    }
}
