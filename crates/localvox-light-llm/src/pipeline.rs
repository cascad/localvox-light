//! Processing a session: the best version of the transcript → the glossary → the LLM →
//! `processed.md` / `summary.md` next to the source (P2: the sources are not mutated).
//! Self-contained (P5): the input is only the session's files.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use localvox_light_core::versions::{
    now_rfc3339, read_transcript_lines, TranscriptLine, VersionEntry, VersionStore,
};

use crate::glossary::Glossary;
use crate::grounding;
use crate::{templates, user, LlmClient};

pub enum Task {
    Cleanup,
    Summary,
}

impl Task {
    /// The name of the artifact in the processing journal.
    pub fn artifact(&self) -> &'static str {
        match self {
            Task::Cleanup => localvox_light_core::processing::PROCESSED,
            Task::Summary => localvox_light_core::processing::SUMMARY,
        }
    }
    fn out_file(&self) -> &'static str {
        match self {
            Task::Cleanup => "processed.md",
            Task::Summary => "summary.md",
        }
    }
    /// A result that did not pass the grounding check. A separate file rather than a
    /// silent loss: we do not throw the model's work away, but we do not pass it off as
    /// fact either.
    fn unverified_file(&self) -> &'static str {
        match self {
            Task::Cleanup => "processed.unverified.md",
            Task::Summary => "summary.unverified.md",
        }
    }
}

pub struct ProcessParams {
    pub glossary_dir: PathBuf,
    pub templates_dir: Option<PathBuf>,
    /// The entity tagger — **not a generative model** (GLiNER: an encoder, spans of the
    /// input). Present — names/organizations/dates/amounts are checked by context, without
    /// a single list of names. Absent — they are not checked, and we SAY SO.
    pub entities: Option<std::sync::Arc<dyn grounding::Entities>>,
    /// A template explicitly chosen by the human for Task::Summary (`video-notes-ru`, a
    /// custom one). `None` — pick by the LANGUAGE of the recording and its length
    /// (a summary or a note).
    ///
    /// `None` by default precisely, and not «summary-ru»: the language of the recording is
    /// not ours to decide for the human, but there is no point asking them every time
    /// either.
    pub summary_template: Option<String>,
    /// The map-reduce threshold: transcripts longer than this (in characters) are cut into
    /// parts, each is processed separately, and for a summary there is a final reduce pass.
    pub map_reduce_chars: usize,
}

impl Default for ProcessParams {
    fn default() -> Self {
        Self {
            glossary_dir: PathBuf::from("assets/glossary"),
            templates_dir: None,
            entities: None,
            summary_template: None,
            map_reduce_chars: 24_000,
        }
    }
}

/// Below this is a short recording: a voice note, a thought out loud, a quick call. It
/// gets the `note-ru` template (a 1–3 sentence digest) instead of a summary with the
/// sections «Решения» and «Поручения»: a layout the model is told to fill in is a layout
/// the model WILL fill in — and it will fill it with inventions if there is no material.
///
/// This is a choice of the FORM of the answer, not a skipping of the processing: a short
/// thought is the most valuable thing said all day, and discarding it because of its
/// length is not allowed. What protects against inventions is not a threshold but
/// `grounding::check` — checking the answer against the transcript.
const SHORT_RECORD_WORDS: usize = 60;

pub struct ProcessOutcome {
    pub out_path: PathBuf,
    pub replacements: usize,
    pub llm_calls: usize,
    pub wall_sec: f64,
    /// There is too little speech in the session — the LLM was not called, no file was
    /// written. Not an error: a quiet session is normal for an always-on recording.
    pub skipped: Option<String>,
    /// The result is not confirmed by the recording: it landed in `*.unverified.md` with a
    /// header. Neither an error nor a loss — a signal saying «check this yourself before
    /// forwarding it».
    pub unverified: Option<String>,
    /// The version of the transcript the artifact was made from — it goes into the journal.
    /// It later shows whether the artifact is stale or simply failed the check.
    pub source: Option<u32>,
}

/// A request to the model with a grounding check of the answer.
///
/// The model gets ONE chance to correct itself: it is confronted with the specific words
/// that are not in the recording. If it insists — the answer is returned anyway, but
/// marked as unverified.
///
/// We throw nothing away. The model's work is not lost: an unverified result lands in a
/// separate `*.unverified.md` file with a header listing exactly what is not confirmed by
/// the recording. An invention is not passed off as fact — but it does not silently
/// disappear either: it is the human who decides, not our heuristic.
///
/// The check is deterministic: it does not depend on the model, on the wording of the
/// prompt, or on the length of the recording (see `grounding`).
fn chat_grounded(
    client: &LlmClient,
    prompt: &str,
    source: &str,
    speech: &str,
    template: &str,
    answer_keeps_markup: bool,
    ner: Option<&dyn grounding::Entities>,
) -> Result<(String, usize, grounding::Ungrounded)> {
    let check = |a: &str| {
        // The cleanup PRESERVES the structure of the transcript — the labels and the
        // timecodes. There is nothing to compare their numbers against: the timecodes are
        // cut out of the speech (otherwise «(00:15)» would ground any invented deadline).
        // Acceptance run 13.07.2026: an excellent cleaned-up text went into quarantine
        // over «invented» 4, 6, 7, 22 — and those were the `(04:06)` and `(07:22)` from
        // its own line headers. We strip the markup from the answer with the same code as
        // from the speech: we compare speech against speech.
        let cleaned;
        let a = if answer_keeps_markup {
            cleaned = speech_only(a);
            cleaned.as_str()
        } else {
            a
        };
        // Words may be grounded by the template (we gave them to the model), NUMBERS only
        // by the speech: the «1–3 предложения» from our own prompt used to ground the
        // invented «3 задачи» in the answer.
        let lex = localvox_light_core::lexicon::active();
        match ner {
            Some(n) => grounding::check_with_entities(lex, source, speech, a, n),
            None => grounding::check_parts(lex, source, speech, a),
        }
    };
    let answer = client.chat(&[user(prompt.to_string())])?;
    let bad = check(&answer);
    if bad.is_empty() {
        return Ok((answer, 1, bad));
    }
    tracing::warn!(
        "the LLM invented things that are not in the recording ({}) — asking again",
        bad.describe()
    );

    // We do NOT show the model the list of caught words. The review caught the
    // «laundering»: given a list of tokens, the model simply crossed them out — and a
    // fabricated summary without the «150» and the «Иван» passed the check and was saved
    // as honest. We ask it to restate everything from scratch off the transcript: deleting
    // specific pieces of evidence is easy, but reassembling the document so that nothing
    // spurious ends up in it again is a different kind of work.
    let fix = "Твой ответ содержит утверждения, которых в расшифровке нет.\n\
               Сделай заново: перечитай расшифровку выше и изложи ТОЛЬКО то, что в \
               ней действительно сказано. Не переноси формулировки из прошлого \
               ответа — начни с чистого листа. Ни одного имени, числа, срока или \
               названия, которого нет в записи. Если содержания в записи нет, верни \
               ровно одну строку:\nСодержательной речи в записи не распознано."
        .to_string();
    let retry = client.chat(&[
        user(prompt.to_string()),
        crate::assistant(answer.clone()),
        user(fix),
    ])?;
    // A RETREAT INTO A REFUSAL IS NOT AN ANSWER, and it must never win.
    //
    // The re-ask prompt itself offers the model an escape hatch: «if there is no content,
    // return exactly one line: Содержательной речи в записи не распознано». A model that has
    // just been scolded for inventing things grabs that line like a life buoy — and the
    // refusal passes the grounding check perfectly (it contains nothing at all, so there is
    // nothing to invent).
    //
    // Caught live (13.07.2026): a 50-minute conversation, 223 lines, a good first answer with
    // a couple of flagged names — and after the re-ask the archive got NOTHING. Neither the
    // cleaned-up text nor the summary. The owner saw an empty session and was right to be
    // furious.
    //
    // A draft with a couple of flagged names is incomparably more valuable than emptiness:
    // the human sees the warning and decides for himself. Emptiness he cannot even check.
    let retry_is_refusal = !grounding::says_something_about(speech, &retry, template);
    let first_says_something = grounding::says_something_about(speech, &answer, template);
    if retry_is_refusal && first_says_something {
        tracing::warn!(
            "the LLM backed off into a refusal after the re-ask — keeping the first answer              and marking it: a flagged draft is better than an empty archive"
        );
        return Ok((answer, 2, bad));
    }

    let still_bad = check(&retry);
    if still_bad.is_empty() {
        tracing::info!("the LLM restated the answer off the recording — no inventions");
        return Ok((retry, 2, still_bad));
    }
    // The second answer is no better than the first — we hand over the one with FEWER
    // inventions and mark it honestly. Losing both is not allowed: they may contain a
    // correct part.
    if still_bad.count() <= bad.count() {
        Ok((retry, 2, still_bad))
    } else {
        Ok((answer, 2, bad))
    }
}

/// Erase both artifacts of the task. Called on EVERY early exit: if an old summary
/// survives a re-cook, the archive shows a document assembled from a different (noisy, or
/// even fabricated) transcript — and passes it off as the current one. «There is nothing
/// to show» MUST mean emptiness, not a stale lie.
fn drop_stale(session_dir: &Path, task: &Task) {
    let _ = fs::remove_file(session_dir.join(task.out_file()));
    let _ = fs::remove_file(session_dir.join(task.unverified_file()));
}

/// Where the result goes — and it ALWAYS goes into the real file.
///
/// **The check is no longer a gatekeeper.** It used to be able to throw the document into
/// quarantine («a draft not confirmed by the recording»), and that was wrong twice over:
///
/// * it compares LITERALLY, so an ordinary synonym becomes an «invention» — the recording
///   says «Питер», the summary writes «Санкт-Петербург». Measured on a live recording:
///   of the words it flagged, «Сергей» and «муж» had both been said out loud;
/// * a document in quarantine is a document the human does not read. A 50-minute
///   conversation ended up with an empty session — and that is worse than any inaccuracy.
///
/// So now: the document is written, and the doubts are written down NEXT TO IT — «worth
/// double-checking, may be a lie or an imprecision». The human reads it and decides; he
/// can listen to the recording, the machine cannot. And when he says «it is fine», his
/// verdict is REMEMBERED (`Outcome::Confirmed`) — a warning that comes back after it has
/// been answered stops being read at all.
fn place_result(session_dir: &Path, task: &Task) -> PathBuf {
    // The drafts of the old quarantine — out. There is no such thing any more.
    let _ = fs::remove_file(session_dir.join(task.unverified_file()));
    session_dir.join(task.out_file())
}

/// Drop the sections that SAY NOTHING ABOUT THE RECORDING.
///
/// «## Договорённости \n В записи отсутствуют договорённости.» is not a section but a
/// polite nothing. The absence of a section IS the answer: a lecture has no action items,
/// idle chatter has no decisions.
///
/// The template is told this in plain words («a section with no material for it — DO NOT
/// PRINT AT ALL»), and the model ignores the instruction. Which means the prompt is not
/// the tool here.
///
/// The sign of emptiness is NOT a search for the words «отсутствуют»/«нет»: the model
/// changes the wording however it likes, and catching it by its words is a game lost in
/// advance. The sign is a zero overlap of content with the recording (the words of our own
/// template are subtracted). There is nothing to fake it with: for a section to pass, it
/// must contain at least one word that was actually spoken.
fn drop_empty_sections(text: &str, speech: &str, template: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    // Section boundaries: lines of the form «## Заголовок».
    let heads: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.trim_start().starts_with("## "))
        .map(|(i, _)| i)
        .collect();
    if heads.is_empty() {
        return text.to_string();
    }

    let mut out: Vec<&str> = lines[..heads[0]].to_vec(); // the preamble — as is
    let mut dropped = Vec::new();
    for (n, &start) in heads.iter().enumerate() {
        let end = heads.get(n + 1).copied().unwrap_or(lines.len());
        let body = lines[start + 1..end].join("\n");
        if grounding::says_something_about(speech, &body, template) {
            out.extend_from_slice(&lines[start..end]);
        } else {
            dropped.push(lines[start].trim().to_string());
        }
    }
    if !dropped.is_empty() {
        tracing::info!("empty sections dropped: {}", dropped.join(", "));
    }
    out.join("\n").trim_end().to_string() + "\n"
}

/// Only the spoken words out of the rendered transcript: «[Я] (00:47) текст» → «текст».
///
/// Stripping the label AND THE TIMECODE is mandatory. Our own timecode «(00:15)» is,
/// firstly, a spurious «word» (the `words == 0` threshold never once fired in prod), and
/// secondly, it «grounded» any number from 0 to 59: an invented deadline «до 15 числа»
/// found itself support in the timecode of a neighbouring line. This de-energized the
/// check completely.
fn speech_only(transcript: &str) -> String {
    let mut out = String::with_capacity(transcript.len());
    for line in transcript.lines() {
        let rest = line.split(']').next_back().unwrap_or(line);
        // «(00:47) текст» → «текст»
        let rest = match rest.trim().strip_prefix('(') {
            Some(after) => after.split_once(')').map(|(_, t)| t).unwrap_or(after),
            None => rest,
        };
        out.push_str(rest.trim());
        out.push('\n');
    }
    out
}

/// The number of words in the speech. «.», «Т.», «э-э», «а-а» are not speech but
/// recognition noise: a single letter, or a repetition of one and the same letter, does
/// not count as a word.
fn speech_words(transcript: &str) -> usize {
    speech_only(transcript)
        .split_whitespace()
        .filter(|w| {
            let letters: Vec<char> = w
                .chars()
                .filter(|c| c.is_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect();
            letters.len() >= 2 && letters.iter().any(|c| *c != letters[0])
        })
        .count()
}

/// Process a session: read the best version of the transcript, run the task through the LLM.
pub fn process_session(
    session_dir: &Path,
    task: &Task,
    client: &LlmClient,
    p: &ProcessParams,
) -> Result<ProcessOutcome> {
    let t0 = std::time::Instant::now();

    // Which version of the transcript we build the artifact from. It goes into the journal
    // and distinguishes «the summary is stale» (the recording was re-cooked) from «the
    // summary failed the check» (the text is the same). The first is a reason to delete it,
    // the second is not.
    let source = VersionStore::open(session_dir)
        .ok()
        .and_then(|s| s.best())
        .map(|v| v.id);

    // The entity tagger — if there is one. Names, organizations, dates and amounts are
    // checked by IT (by context, without a single list of names); it does not touch the
    // numbers — they are deterministic and do not depend on the model.
    let ner: Option<&dyn grounding::Entities> = p.entities.as_deref();

    let transcript = render_transcript(session_dir)?;
    let words = speech_words(&transcript);
    // The only threshold that is not a heuristic but a fact: there is nothing to recognize.
    if words == 0 {
        drop_stale(session_dir, task);
        return Ok(ProcessOutcome {
            out_path: session_dir.join(task.out_file()),
            replacements: 0,
            llm_calls: 0,
            wall_sec: t0.elapsed().as_secs_f64(),
            skipped: Some("there is no recognized speech in the recording".into()),
            unverified: None,
            source,
        });
    }

    let glossary = Glossary::load_dir(&p.glossary_dir)?;
    let (transcript, replacements) = glossary.apply(&transcript);
    let glossary_block = glossary.prompt_block(&transcript);

    // The form of the answer follows the input: a short recording gets a digest, not a
    // summary with sections that there is nothing to fill in with (except inventions).
    //
    // But we do not override an EXPLICITLY chosen template: if the human asked for
    // `--summary-template video-notes-ru`, they know what they want, and silently giving
    // them something else is worse than giving them a somewhat empty outline.
    //
    // The language of the recording picks the template: a Russian recording gets a Russian
    // summary, an English one gets an English summary. Here too an explicit template
    // outranks the language.
    let lang = localvox_light_core::lang::text(session_dir);
    let dir = p.templates_dir.as_deref();
    // An empty template means «not set» (an empty environment variable). The same rule
    // governs processing::llm_style, which computes the recipe: if we diverged from it
    // here, the artifact would be rebuilt forever.
    let explicit = p
        .summary_template
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let (template_name, template) = match (task, explicit) {
        (Task::Cleanup, _) => templates::for_lang("cleanup", &lang, dir)?,
        (Task::Summary, Some(explicit)) => (explicit.to_string(), templates::load(explicit, dir)?),
        (Task::Summary, None) if words < SHORT_RECORD_WORDS => {
            tracing::info!("the recording is short ({words} words) — a note instead of a summary");
            templates::for_lang("note", &lang, dir)?
        }
        (Task::Summary, None) => templates::for_lang("summary", &lang, dir)?,
    };
    tracing::debug!("language «{lang}», template «{template_name}»");

    // The base of the grounding check — one for the whole run and for all the branches:
    // the template (we gave the model its words ourselves) + the glossary (the canonical
    // «capacity» is also ours, not invented) + ONLY THE SPEECH, without labels and
    // timecodes, otherwise «(00:15)» grounds any invented number up to 59.
    let speech = speech_only(&transcript);
    let base = format!("{template}\n{glossary_block}\n{speech}");
    // The cleanup returns the transcript in the same shape — with labels and timecodes;
    // a summary is free-form markdown. The markup must be stripped from the answer,
    // otherwise its numbers («(07:22)» → 7 and 22) look like inventions.
    let keeps_markup = matches!(task, Task::Cleanup);

    let parts = split_for_map_reduce(&transcript, p.map_reduce_chars);
    let mut llm_calls = 0usize;
    let mut outputs: Vec<String> = Vec::with_capacity(parts.len());
    for part in &parts {
        let prompt = templates::render(&template, part, &glossary_block);
        let (answer, calls, _) = chat_grounded(client, &prompt, &base, &speech, &template, keeps_markup, ner)?;
        llm_calls += calls;
        outputs.push(answer);
    }

    let result = if outputs.len() == 1 {
        outputs.pop().unwrap()
    } else {
        match task {
            // cleanup — a concatenation of the cleaned-up parts
            Task::Cleanup => outputs.join("\n\n"),
            // summary — a reduction of the per-part results into a single document
            Task::Summary => {
                let joined = outputs.join("\n\n---\n\n");
                let prompt = format!(
                    "Ниже — результаты обработки последовательных частей одного \
                     длинного материала. Сведи их в один цельный документ той же \
                     структуры и на том же языке, без дублей и повторов разделов; \
                     слова «части» и «протокол частей» в тексте не упоминай.\n\n{joined}"
                );
                llm_calls += 1;
                client.chat(&[user(prompt)])?
            }
        }
    };

    // An answer that overlaps with the recording in NOT A SINGLE meaningful word is not an
    // analysis of the recording but a message saying that there is nothing to analyse (no
    // matter how the model worded it). It does not become an artifact: we do not write the
    // file, otherwise the archive would hold a «summary» consisting of the single phrase
    // «no speech recognized».
    //
    // The sign is the overlap of content, not a search for the words «не распознано»: the
    // model changes the wording, but the fact «there is not a single word from the
    // recording in the answer» cannot be faked.
    if !grounding::says_something_about(&speech, &result, &template) {
        drop_stale(session_dir, task);
        return Ok(ProcessOutcome {
            out_path: session_dir.join(task.out_file()),
            replacements: replacements.len(),
            llm_calls,
            wall_sec: t0.elapsed().as_secs_f64(),
            skipped: Some(
                "the answer says nothing about this recording — there was nothing to analyse"
                    .into(),
            ),
            unverified: None,
            source,
        });
    }

    // Empty sections — out. «## Задачи \n В записи отсутствуют задачи» is not a section but
    // a polite nothing: a lecture has no action items, idle chatter has no decisions.
    //
    // The template is told NOT TO PRINT such sections — and the model ignores that
    // instruction (acceptance run 13.07.2026: a lecture about Oblomov got three empty
    // sections in a row). This cannot be cured with a prompt, so we cure it
    // deterministically.
    //
    // The sign is the same as for a wholly empty answer: a ZERO OVERLAP OF CONTENT with the
    // recording, not a search for the words «отсутствуют»/«нет». The model changes the
    // wording however it likes, while the fact «there is not a single word from the
    // recording in this section» is impossible to fake.
    let result = if matches!(task, Task::Summary) {
        drop_empty_sections(&result, &speech, &template)
    } else {
        result
    };

    // The final check against the same base: the map-reduce reduction is also a call to the
    // model, and it too can bring in something of its own.
    let for_check = if keeps_markup {
        speech_only(&result)
    } else {
        result.clone()
    };
    let lex = localvox_light_core::lexicon::active();
    let ungrounded = match ner {
        Some(n) => grounding::check_with_entities(lex, &base, &speech, &for_check, n),
        None => grounding::check_parts(lex, &base, &speech, &for_check),
    };

    let out_path = place_result(session_dir, task);
    // The doubts live in the header COMMENT: the app needs them (it shows the mark and the
    // «it is fine» button), but they must not disfigure the document a human forwards to
    // colleagues. A warning across the whole page over a «Питер» that became a
    // «Санкт-Петербург» is exactly the fussing that makes people stop reading warnings.
    let doubts = if ungrounded.is_empty() {
        String::new()
    } else {
        format!(" | стоит перепроверить: {}", ungrounded.describe())
    };
    let header = format!(
        "<!-- localvox: {} | модель {} | глоссарий: {} замен | {}{doubts} -->

",
        template_name,
        client.model(),
        replacements.iter().map(|r| r.count).sum::<usize>(),
        localvox_light_core::versions::now_rfc3339(),
    );
    fs::write(&out_path, header + &result)
        .with_context(|| format!("writing {}", out_path.display()))?;

    Ok(ProcessOutcome {
        out_path,
        replacements: replacements.len(),
        llm_calls,
        wall_sec: t0.elapsed().as_secs_f64(),
        skipped: None,
        unverified: (!ungrounded.is_empty()).then(|| ungrounded.describe()),
        source,
    })
}

pub struct RefineOutcome {
    pub version_id: u32,
    pub file: PathBuf,
    pub lines: usize,
    /// How many lines the LLM actually changed.
    pub changed: usize,
    /// Lines sent to the LLM but never returned (they stayed as the original) — a signal
    /// that the model swallowed the batch, not that no corrections were needed.
    pub omitted: usize,
    pub llm_calls: usize,
    pub wall_sec: f64,
    /// true — best was already `refined`, we did nothing (idempotency).
    pub skipped: bool,
}

/// Cleaning up the transcript with the LLM while preserving the structure (F1/«the refined
/// version»): every line of the best version is corrected separately (recognition errors,
/// punctuation), the timecodes and the split into lines are NOT changed — the result is
/// committed as a new version `refined` (parents = best) and becomes best. That is why
/// search/export/the player use the cleaned-up variant right away, while the raw one stays
/// unchanged (P2) and switchable.
pub fn refine_session(
    session_dir: &Path,
    client: &LlmClient,
    p: &ProcessParams,
    force: bool,
) -> Result<RefineOutcome> {
    let t0 = std::time::Instant::now();
    let store = VersionStore::open(session_dir)?;
    let best = store
        .best()
        .context("no transcript versions — cook it first (localvox-process)")?;
    // Idempotency (without --force): we do not run the LLM again if the result already
    // exists. best is already cleaned up — reuse it; best was switched back to the raw one
    // (--set-best) but it has a cleaned-up descendant — just make that one best.
    if !force {
        let reuse_id = if best.label == "refined" {
            Some(best.id)
        } else {
            store
                .load()
                .versions
                .iter()
                .find(|v| v.label == "refined" && v.parents == [best.id])
                .map(|v| v.id)
        };
        if let Some(id) = reuse_id {
            if id != best.id {
                store.set_best(id)?;
            }
            return Ok(RefineOutcome {
                version_id: id,
                file: store.resolve(id).unwrap_or_default(),
                lines: 0,
                changed: 0,
                omitted: 0,
                llm_calls: 0,
                wall_sec: t0.elapsed().as_secs_f64(),
                skipped: true,
            });
        }
    }
    let src_path = store
        .resolve(best.id)
        .context("the file of the best version was not found")?;
    let orig = read_transcript_lines(&src_path)?;
    if orig.is_empty() {
        bail!("the transcript is empty — there is nothing to clean up");
    }
    // The cleanup runs line by line and does not touch the timecodes, so there is nothing
    // here to invent a «meeting» with — we run it even for a short note. We cut off only on
    // the fact: there are no recognized words at all («Т.», «.»), there is nothing to fix.
    let words: usize = orig.iter().map(|l| speech_words(&l.text)).sum();
    if words == 0 {
        return Ok(RefineOutcome {
            version_id: best.id,
            file: src_path,
            lines: orig.len(),
            changed: 0,
            omitted: 0,
            llm_calls: 0,
            wall_sec: t0.elapsed().as_secs_f64(),
            skipped: true,
        });
    }

    // A deterministic glossary pass over every line — before the LLM.
    let glossary = Glossary::load_dir(&p.glossary_dir)?;
    // Terms — only those that actually occur: otherwise the list goes into the context and
    // the model takes it for content (see prompt_block).
    let all_text = orig
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let glossary_block = glossary.prompt_block(&all_text);
    let mut lines: Vec<TranscriptLine> = orig.clone();
    for l in &mut lines {
        let (t, _) = glossary.apply(&l.text);
        l.text = t;
    }

    // Batched correction: we number the lines [1..N] and cut them by a character budget.
    let mut corrected: std::collections::HashMap<usize, String> = std::collections::HashMap::new();
    let mut llm_calls = 0usize;
    let mut batch: Vec<(usize, &str)> = Vec::new();
    let mut batch_chars = 0usize;
    let flush = |batch: &mut Vec<(usize, &str)>,
                 corrected: &mut std::collections::HashMap<usize, String>,
                 llm_calls: &mut usize|
     -> Result<()> {
        if batch.is_empty() {
            return Ok(());
        }
        let mut prompt = String::from(REFINE_PROMPT);
        if !glossary_block.is_empty() {
            prompt.push_str("\n\nТермины (правильное написание):\n");
            prompt.push_str(&glossary_block);
        }
        prompt.push_str("\n\n");
        for (n, text) in batch.iter() {
            prompt.push_str(&format!("[{n}] {text}\n"));
        }
        // The numbers of this batch: foreign/renumbered answers from the LLM ([1..] instead
        // of [81..]) must not overwrite the corrections of another batch — we take only
        // those sent in THIS batch; the remaining lines will stay as the original.
        let sent: std::collections::HashSet<usize> = batch.iter().map(|(n, _)| *n).collect();
        let answer = client.chat(&[user(prompt)])?;
        *llm_calls += 1;
        for (n, text) in parse_numbered(&answer) {
            if sent.contains(&n) && !text.trim().is_empty() {
                corrected.insert(n, text);
            }
        }
        batch.clear();
        Ok(())
    };
    for (i, l) in lines.iter().enumerate() {
        let n = i + 1;
        if !batch.is_empty() && batch_chars + l.text.len() > p.map_reduce_chars {
            flush(&mut batch, &mut corrected, &mut llm_calls)?;
            batch_chars = 0;
        }
        batch.push((n, &l.text));
        batch_chars += l.text.len() + 8;
    }
    flush(&mut batch, &mut corrected, &mut llm_calls)?;

    // We assemble the cleaned-up lines: where the LLM gave a replacement — we take it,
    // otherwise the original (after the glossary). We do not touch the timecodes/source.
    //
    // EVERY corrected line is checked against ITS OWN original. The cleanup MUST REPAIR
    // what was recognized, not ADD to it: a name or a number that was not in the line is
    // the most dangerous thing here — the refined version becomes best, that is, THE
    // transcript itself, and from then on everything the model added is «grounded» by
    // definition for the summary, the search and the export. The review called this the
    // worst hole in WP-C16. A line that fails the check is rolled back to the original.
    let n_lines = lines.len();
    let mut changed = 0usize;
    let mut matched = 0usize;
    let mut rejected = 0usize;
    let refined: Vec<TranscriptLine> = lines
        .into_iter()
        .enumerate()
        .map(|(i, mut l)| {
            if let Some(fixed) = corrected.get(&(i + 1)) {
                matched += 1;
                let bad = grounding::check(&l.text, fixed);
                if !bad.is_empty() {
                    tracing::warn!(
                        "refine: the line at {:.0} s was added to ({}) — rolling back to \
                         the original",
                        l.start_sec,
                        bad.describe()
                    );
                    rejected += 1;
                    return l;
                }
                if fixed.trim() != l.text.trim() {
                    changed += 1;
                }
                l.text = fixed.trim().to_string();
            }
            l
        })
        .collect();
    if rejected > 0 {
        tracing::warn!(
            "refine: {rejected} lines rolled back — the model was adding, not repairing"
        );
    }
    let omitted = n_lines - matched; // the lines the LLM did not answer for

    // The mutation of versions.json runs under the same cross-process `.cook.lock` as the
    // cook: otherwise a simultaneous cook/refine of one session would hit an id collision
    // in the two-phase commit. The expensive LLM pass is already behind us and needed no
    // lock.
    let cook_lock = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(session_dir.join(".cook.lock"))
        .context("the session's version lock")?;
    cook_lock.lock().context("acquiring the version lock")?;

    // A new version `refined` (parents = best) → we make it best.
    let (id, path) = store.next_version("refined")?;
    {
        let mut w = std::io::BufWriter::new(fs::File::create(&path)?);
        for l in &refined {
            writeln!(w, "{}", serde_json::to_string(l)?)?;
        }
    }
    store.commit(VersionEntry {
        id,
        label: "refined".into(),
        file: path.file_name().unwrap().to_string_lossy().into(),
        model: client.model().to_string(),
        params: serde_json::json!({ "task": "refine-lines", "parent_label": best.label }),
        created_at: now_rfc3339(),
        parents: vec![best.id],
    })?;
    store.set_best(id)?;
    drop(cook_lock);

    Ok(RefineOutcome {
        version_id: id,
        file: path,
        lines: refined.len(),
        changed,
        omitted,
        llm_calls,
        wall_sec: t0.elapsed().as_secs_f64(),
        skipped: false,
    })
}

const REFINE_PROMPT: &str = "\
Ниже — реплики расшифровки речи, каждая пронумерована [N]. Исправь в каждой \
ошибки автоматического распознавания, опечатки и пунктуацию; сохрани смысл и \
разговорный стиль. НЕ объединяй и НЕ разбивай реплики, НЕ меняй их количество и \
порядок. Верни РОВНО те же номера [N], по одной исправленной реплике в строке, \
без каких-либо пояснений. Если реплика — бессвязный шум, верни её без изменений.";

/// Parsing an LLM answer of the form `[N] текст`. A line that the LLM wrapped across
/// several physical lines is glued back into one (otherwise the tail would be lost); the
/// preamble before the first `[N]` is dropped.
fn parse_numbered(answer: &str) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    for line in answer.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix('[') {
            if let Some(close) = rest.find(']') {
                if let Ok(n) = rest[..close].trim().parse::<usize>() {
                    out.push((n, rest[close + 1..].trim().to_string()));
                    continue;
                }
            }
        }
        // The continuation of a wrapped line — we append it to the current one.
        if let Some((_, text)) = out.last_mut() {
            let cont = line.trim();
            if !cont.is_empty() {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(cont);
            }
        }
    }
    out
}

/// The best version's jsonl → the text «[label] (timecode) line», sorted by time.
/// Public — the invention benchmark (`localvox-bench llm`) must assemble the prompt from
/// exactly the same text as prod, otherwise it measures something other than our product.
pub fn render_transcript(session_dir: &Path) -> Result<String> {
    let store = VersionStore::open(session_dir)?;
    let Some(best) = store.best() else {
        bail!(
            "there are no transcript versions in {} — run localvox-process (the cook) first",
            session_dir.display()
        );
    };
    let path = store
        .resolve(best.id)
        .context("the file of the best version was not found")?;
    let lines = read_transcript_lines(&path)?;

    let mut out = String::new();
    for l in &lines {
        // WHO said it. If diarization separated the voices — its answer; if a person said
        // a name — the name. If we do not know — we honestly name the source of the sound
        // rather than invent a participant: attributing someone else's words to a living
        // person is not a typo but slander.
        let who: &str = match l.speaker.as_deref() {
            Some(name) => name,
            None if l.source_id == 0 => "Я",
            None => "Собеседники",
        };
        let mins = (l.start_sec / 60.0) as u64;
        let secs = l.start_sec as u64 % 60;
        out.push_str(&format!("[{who}] ({mins:02}:{secs:02}) {}\n", l.text));
    }
    Ok(out)
}

/// Cuts the text along its lines into parts of ≤ max_chars (a single line longer than the
/// limit stays whole — we do not tear a line in half).
fn split_for_map_reduce(text: &str, max_chars: usize) -> Vec<String> {
    if text.len() <= max_chars {
        return vec![text.to_string()];
    }
    let mut parts = Vec::new();
    let mut cur = String::new();
    for line in text.lines() {
        if !cur.is_empty() && cur.len() + line.len() + 1 > max_chars {
            parts.push(std::mem::take(&mut cur));
        }
        cur.push_str(line);
        cur.push('\n');
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    /// THE DOCUMENT IS ALWAYS WRITTEN, and the doubts are only a mark.
    ///
    /// The quarantine («a draft not confirmed by the recording») cost the owner a whole
    /// 50-minute conversation: the check found two words, the model was asked to redo it, it
    /// backed off into a refusal, and the archive got NOTHING — neither the cleaned-up text
    /// nor the summary. Meanwhile the check itself was wrong: of the words it flagged,
    /// «Сергей» and «муж» had been said out loud, and «Санкт-Петербург» was the model's
    /// synonym for the «Питер» that was said.
    ///
    /// A document with an inaccuracy the human can read and correct. Emptiness he cannot.
    #[test]
    fn a_doubtful_result_is_still_written_as_the_real_document() {
        let dir = tempfile::tempdir().unwrap();
        // A leftover from the old quarantine — must be swept away, there is no such thing now.
        std::fs::write(dir.path().join("summary.unverified.md"), "old draft").unwrap();

        let out = place_result(dir.path(), &Task::Summary);

        assert_eq!(out, dir.path().join("summary.md"), "the result went into quarantine again");
        assert!(
            !dir.path().join("summary.unverified.md").exists(),
            "the draft of the old quarantine stayed in the archive"
        );
    }

    /// The canonical form from the glossary is not an invention: it was WE who gave the
    /// model «capacity» instead of the heard «как посетит». The final check MUST see the
    /// glossary, otherwise an honest summary goes into the draft over our own correction.
    #[test]
    fn glossary_canon_is_grounded_in_the_final_check() {
        let template = "Составь протокол.\n{{glossary}}\n{{transcript}}";
        let glossary_block = "- capacity (в записи может звучать как: как посетит)";
        let transcript = "[0s] а как посетит команды на этот спринт?";
        let answer = "## Ключевые выводы\nОбсуждали capacity команды на спринт.";

        let without = grounding::check(&format!("{template}\n{transcript}"), answer);
        assert!(
            !without.is_empty(),
            "the test is meaningless if it is already empty anyway"
        );

        let with = grounding::check(
            &format!("{template}\n{glossary_block}\n{transcript}"),
            answer,
        );
        assert!(
            with.is_empty(),
            "the glossary canon was taken for an invention: {with:?}"
        );
    }

    /// «There was nothing to analyse» is determined NOT by the wording of the answer but by
    /// the overlap of content: an answer that reused not a single word of the recording is
    /// not talking about it. The model changes the wording however it likes (during the
    /// acceptance run it reworded the prescribed line, and the exact comparison did not
    /// fire) — while a zero overlap is impossible to fake.
    #[test]
    fn an_answer_that_says_nothing_about_the_recording_is_not_an_artifact() {
        // a real session from the acceptance run: fragments with no content
        let tpl = templates::load("note-ru", None).unwrap();
        let speech = "Уже закричал.\nГоворю несколько пауз.\nВа!\nРазбиваю шесть яиц и виваю.";
        for refusal in [
            "Содержательной речи в записи не распознано.",
            "В записи не распознано ни одной законченной осмысленной фразы.",
            "Запись не содержит ничего, что можно изложить.",
        ] {
            assert!(
                !grounding::says_something_about(speech, refusal, &tpl),
                "a refusal was taken for a summary: {refusal}"
            );
        }
        // whereas an analysis of the same recording reuses its words, so it is about it
        assert!(grounding::says_something_about(
            speech,
            "Автор говорит о паузах и разбивает яйца.",
            &tpl
        ));
    }

    /// The words OF A REFUSAL are dictated by our own template — they cannot be proof that
    /// the model is talking about the recording. The review reproduced it: the recording
    /// «Проверяю распознавание речи» overlapped with the refusal «Содержательной речи в
    /// записи не распознано» and went into the archive as a summary.
    #[test]
    fn refusal_words_come_from_our_template_and_prove_nothing() {
        let tpl = templates::load("note-ru", None).unwrap();
        let speech = "Проверяю распознавание речи. Раз, два, три.";
        assert!(
            !grounding::says_something_about(
                speech,
                "Содержательной речи в записи не распознано.",
                &tpl
            ),
            "a refusal squeezed through on the words of our own template"
        );
    }

    /// A short thought is a legitimate artifact: its analysis overlaps with the recording.
    #[test]
    fn a_short_thought_still_produces_an_artifact() {
        let tpl = templates::load("note-ru", None).unwrap();
        let speech = "Отметь мысль: надо переписать варку на пул воркеров.";
        assert!(grounding::says_something_about(
            speech,
            "Предложено переписать варку на пул воркеров.",
            &tpl
        ));
    }

    /// The cleaned-up text keeps the labels and the timecodes — their numbers are not an
    /// invention. Acceptance run 13.07.2026: an excellent `processed.md` went into
    /// quarantine with the verdict «the numbers 4, 6, 7, 22… are invented», and those were
    /// the `(04:06)` and `(07:22)` from its own line headers. Speech must be checked against
    /// speech, with the markup stripped from both sides.
    #[test]
    fn timecodes_in_the_cleaned_transcript_are_not_invented_numbers() {
        let transcript = "[Я] (04:06) Отметь мысль.\n[Я] (07:22) Давай во вторник планёрку.\n";
        let speech = speech_only(transcript);
        // the model returned the same format — with the timecodes
        let answer = "[Я] (04:06) Отметь мысль.\n[Я] (07:22) Давай во вторник проведём планёрку.\n";

        let raw = grounding::check(&speech, answer);
        assert!(
            !raw.numbers.is_empty(),
            "the test is meaningless if it is clean even without stripping the markup"
        );

        let cleaned = grounding::check(&speech, &speech_only(answer));
        assert!(
            cleaned.numbers.is_empty(),
            "the timecodes were taken for invented numbers: {cleaned:?}"
        );
    }

    /// A timecode is neither a word nor a fact. On the prod format («[Я] (00:47) .») the
    /// first version counted it as a word: the `words == 0` threshold never fired, and the
    /// number 47 «grounded» an invented deadline.
    #[test]
    fn timecodes_are_stripped_from_speech() {
        assert_eq!(speech_words("[Я] (00:47) э-э.\n"), 0);
        assert_eq!(speech_words("[Я] (00:15) .\n[Собеседники] (01:02) Т.\n"), 0);
        assert!(!speech_only("[Я] (00:47) привет мир\n").contains("47"));
        assert_eq!(speech_only("[Я] (00:47) привет мир\n").trim(), "привет мир");
    }

    /// The promise «we lose nothing»: the model's answer is ALWAYS saved.
    /// If it failed the check — it lands as a draft with a header, it does not disappear.

    /// An empty section is not a section. Taken from a REAL summary out of the archive
    /// (the lecture about Oblomov, 13.07.2026): the template is told not to print empty
    /// sections, and the model printed three in a row.
    ///
    /// We catch it NOT by the words «отсутствуют»/«нет» — the model changes the wording
    /// however it likes. We catch it by the zero overlap of content with the recording.
    #[test]
    fn a_section_that_says_nothing_about_the_recording_is_dropped() {
        let speech = "Обломовка это место вечного лета, где нет власти и соседей. \
                      Добролюбов заклеймил Обломова, назвав его лишним человеком.";
        let template = templates::load("summary-ru", None).unwrap();
        let answer = "## О чём запись\n\
                      Разбор романа Гончарова «Обломов» и образа Обломовки.\n\n\
                      ## Главное\n\
                      Добролюбов заклеймил Обломова как лишнего человека.\n\n\
                      ## Договорённости\n\
                      В записи отсутствуют договорённости.\n\n\
                      ## Открытые вопросы\n\
                      Не выявлено.\n\n\
                      ## Задачи\n\
                      Нет.\n";

        let out = drop_empty_sections(answer, speech, &template);
        assert!(out.contains("## О чём запись"), "{out}");
        assert!(out.contains("## Главное"), "{out}");
        assert!(
            !out.contains("## Договорённости"),
            "an empty section stayed: {out}"
        );
        assert!(!out.contains("## Открытые вопросы"), "{out}");
        assert!(!out.contains("## Задачи"), "{out}");
    }

    /// A non-empty section, on the other hand, MUST NOT be touched, even if it is short.
    #[test]
    fn a_section_with_real_content_survives() {
        let speech = "Через три года после романа появится Базаров, а через четыре — Рахметов.";
        let template = templates::load("summary-ru", None).unwrap();
        let answer = "## Главное\n\
                      Разговор о романе.\n\n\
                      ## Детали и цифры\n\
                      Через три года появится Базаров, через четыре — Рахметов.\n";
        let out = drop_empty_sections(answer, speech, &template);
        assert!(
            out.contains("Базаров"),
            "a section with real content was dropped: {out}"
        );
        assert!(out.contains("## Детали и цифры"), "{out}");
    }

    /// A STALE summary (made from a different transcript) MUST go: it talks about a
    /// different text, and keeping it means passing a stale lie off as the truth.

    /// A CONFIRMED summary about THE SAME transcript, on the other hand, MUST NOT be
    /// deleted.
    ///
    /// These are different reasons: «stale» is a legitimate one, «failed the check» is not.
    /// A change of model plus one false positive of the check erased a good document, and
    /// there was nowhere to get it back from. And our check has been wrong three times.

    /// A draft must not survive a successful run: otherwise unverified text would stay in
    /// the archive next to the verified one.

    /// Humming and fragments are ZERO words, not «too few words». The only case where there
    /// is nothing to process as a matter of fact, not as a matter of heuristics.
    #[test]
    fn babbling_has_no_words_at_all() {
        assert_eq!(speech_words("Т."), 0);
        assert_eq!(speech_words("."), 0);
        assert_eq!(speech_words("[0s] Т.\n[15s] э-э, а."), 0);
        assert_eq!(speech_words("[120s] [Я] ."), 0);
    }

    /// A short thought is a legitimate input: it gets the digest template but is NOT
    /// discarded. The length threshold picks the form of the answer, not the right to
    /// exist: «отметь мысль: переписать варку на пул воркеров» is the most valuable thing
    /// said all day, and losing it over eight words is not allowed.
    #[test]
    fn a_short_thought_is_real_speech_and_gets_the_note_shape() {
        let note = "[3s] [Я] Отметь мысль: надо переписать варку на пул воркеров.";
        let words = speech_words(note);
        assert!(words > 0, "the thought was lost as «no speech»");
        assert!(
            words < SHORT_RECORD_WORDS,
            "a short recording must get a digest, not a summary: {words}"
        );
    }

    #[test]
    fn a_real_meeting_gets_the_protocol_shape() {
        let line = "[0s] Обсудили сроки релиза и договорились перенести выкладку \
                    на следующую неделю, потому что тесты ещё не прошли до конца.";
        let t = (0..4).map(|_| line).collect::<Vec<_>>().join("\n");
        assert!(
            speech_words(&t) >= SHORT_RECORD_WORDS,
            "words: {}",
            speech_words(&t)
        );
    }

    #[test]
    fn split_keeps_lines_whole_and_covers_everything() {
        let text = (0..100)
            .map(|i| format!("строка номер {i} с текстом"))
            .collect::<Vec<_>>()
            .join("\n");
        let parts = split_for_map_reduce(&text, 300);
        assert!(parts.len() > 1);
        let rejoined: String = parts.join("");
        assert_eq!(rejoined.lines().count(), 100);
        for p in &parts[..parts.len() - 1] {
            assert!(p.len() <= 300 + 40);
        }
    }

    #[test]
    fn short_text_is_single_part() {
        assert_eq!(split_for_map_reduce("abc", 100), vec!["abc".to_string()]);
    }

    #[test]
    fn parse_numbered_maps_lines_and_skips_preamble() {
        // The preamble before the first [N] is dropped; the numbers are global.
        let ans = "Вот исправления:\n[1] Привет, как дела?\n [2]  Нормально.\n[10] Десятая.";
        let got = parse_numbered(ans);
        assert_eq!(got.len(), 3);
        assert_eq!(got[0], (1, "Привет, как дела?".to_string()));
        assert_eq!(got[1], (2, "Нормально.".to_string()));
        assert_eq!(got[2], (10, "Десятая.".to_string()));
    }

    #[test]
    fn parse_numbered_joins_wrapped_reply() {
        // A line that the LLM wrapped across two lines is glued back, not cut.
        let ans = "[3] Первая часть реплики,\nвторая часть реплики.";
        let got = parse_numbered(ans);
        assert_eq!(
            got,
            vec![(3, "Первая часть реплики, вторая часть реплики.".to_string())]
        );
    }
}
