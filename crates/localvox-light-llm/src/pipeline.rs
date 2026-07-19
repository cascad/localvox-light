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
            // Data, not a page: a delta over the transcript version. See `core::readable`.
            Task::Cleanup => localvox_light_core::readable::FILE,
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
/// ONLY THE SUMMARY comes through here. The readable text does not: it is not one document the
/// model wrote, it is our transcript with the model's wording looked up line by line, and each
/// line is judged against its own original inside `cleanup_by_lines`.
fn chat_grounded(
    client: &LlmClient,
    prompt: &str,
    source: &str,
    speech: &str,
    template: &str,
    ner: Option<&dyn grounding::Entities>,
) -> Result<(String, usize, grounding::Ungrounded)> {
    let check = |a: &str| {
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
    // NO ESCAPE HATCH. The prompt used to end with «if there is no content, return exactly one
    // line: Содержательной речи в записи не распознано» — and a model that has just been accused
    // of lying grabs that line like a life buoy. It then passes the check perfectly, because an
    // empty answer has nothing in it to be ungrounded. We handed it both the way out and the
    // reward for taking it.
    //
    // Whether the recording is empty is OUR question, and we answer it deterministically from the
    // transcript — not by asking the model to confess.
    let fix = "Твой ответ содержит утверждения, которых в расшифровке нет.\n\
               Сделай заново: перечитай расшифровку выше и изложи ТОЛЬКО то, что в \
               ней действительно сказано. Не переноси формулировки из прошлого \
               ответа — начни с чистого листа. Ни одного имени, числа, срока или \
               названия, которого нет в записи."
        .to_string();
    let retry = client.chat(&[
        user(prompt.to_string()),
        crate::assistant(answer.clone()),
        user(fix),
    ])?;
    // AN ANSWER THAT SAYS NOTHING ABOUT THE RECORDING NEVER WINS — no matter how clean it looks.
    //
    // This is the trap the whole thing kept falling into: emptiness passes the check perfectly,
    // because there is nothing in it to be ungrounded. So «I found nothing» always scores better
    // than a real document with one flagged word — and the archive ends up with nothing.
    //
    // Caught twice, live. 13.07.2026: a 50-minute conversation, 223 lines, a good first answer,
    // and after the re-ask the session was left EMPTY. 14.07.2026: a 102-minute recording lost its
    // readable text the same way.
    //
    // A document with a couple of flagged words is incomparably more valuable than emptiness: the
    // human sees the mark and decides. Emptiness he cannot even check.
    if !grounding::says_something_about(speech, &retry, template) {
        tracing::warn!(
            "the LLM backed off into an empty answer after the re-ask — keeping the first one and \
             marking it: a flagged document beats an empty archive"
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
///
/// FILTERING THE ANSWER BY SHAPE USED TO LIVE HERE, and it is gone on purpose. It kept only lines
/// matching `[метка] (таймкод) реплика`, which correctly killed the essays the model opened the
/// readable text with — and, on 16.07.2026, killed 120 of 149 real lines along with one, because
/// an essay was the model's ENTIRE answer for the first of two parts. A filter cannot tell «the
/// model added noise» from «the model replaced the document with noise»; both look like text of
/// the wrong shape. `cleanup_by_lines` removes the question: the model never supplies markup, so
/// there is nothing to filter and nothing to lose.
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
        // The readable text has no template FILE: its prompt is a const, exactly like refine's.
        // The lines are ours and so is the markup — there is nothing left for a template to
        // shape, and a template file that shapes nothing is a lie sitting in the repo.
        (Task::Cleanup, _) => ("cleanup-lines".to_string(), CLEANUP_PROMPT.to_string()),
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
    // THE READABLE TEXT LEAVES HERE, and it leaves as DATA rather than as a page.
    //
    // Everything below this point is about a document the model composed: grounding it against
    // the whole recording, dropping its empty sections, stamping a header comment onto its
    // markdown. None of that applies to the readable text — it is not a composition but a delta:
    // for each line of the transcript, what the cleanup made of it. Its guards are per-line and
    // live inside `assemble_readable`, where a cleaned line is judged against its OWN original.
    //
    // Storing it as data is what made renaming a speaker free. The old file baked `[Я]` into the
    // text at cook time, so «Я» could only become «Арсен Маркарян» by running the whole recording
    // through an LLM again. The name is not in this artifact at all: it is joined in at render
    // time by whichever client is drawing it.
    if matches!(task, Task::Cleanup) {
        let (mut readable, calls) =
            cleanup_by_lines(session_dir, client, &glossary, &glossary_block)?;
        readable.provenance = localvox_light_core::provenance::Provenance {
            template: template_name,
            model: client.model().to_string(),
            glossary: replacements.iter().map(|r| r.count).sum(),
            at: now_rfc3339(),
            doubts: None,
        };
        // The old quarantine draft, if one is still lying about — there is no quarantine any more.
        let _ = fs::remove_file(session_dir.join(task.unverified_file()));
        let out_path = localvox_light_core::readable::save(session_dir, &readable)?;
        return Ok(ProcessOutcome {
            out_path,
            replacements: replacements.len(),
            llm_calls: calls,
            wall_sec: t0.elapsed().as_secs_f64(),
            skipped: None,
            unverified: None,
            source,
        });
    }

    let speech = speech_only(&transcript);
    let base = format!("{template}\n{glossary_block}\n{speech}");
    let (result, llm_calls) = match task {
        // Handled above — it is not a document the model wrote.
        Task::Cleanup => unreachable!("the readable text is written as data, above"),
        // The summary is free-form: it is the model's document, and the map-reduce over parts
        // with a final reduce is the right shape for it. Nothing to key it by — it has no records.
        Task::Summary => {
            let parts = split_for_map_reduce(&transcript, p.map_reduce_chars);
            let mut calls_total = 0usize;
            let mut outputs: Vec<String> = Vec::with_capacity(parts.len());
            for part in &parts {
                let prompt = templates::render(&template, part, &glossary_block);
                let (answer, calls, _) =
                    chat_grounded(client, &prompt, &base, &speech, &template, ner)?;
                calls_total += calls;
                outputs.push(answer);
            }
            let joined = if outputs.len() == 1 {
                outputs.pop().unwrap()
            } else {
                let joined = outputs.join("\n\n---\n\n");
                let prompt = format!(
                    "Ниже — результаты обработки последовательных частей одного \
                     длинного материала. Сведи их в один цельный документ той же \
                     структуры и на том же языке, без дублей и повторов разделов; \
                     слова «части» и «протокол частей» в тексте не упоминай.\n\n{joined}"
                );
                calls_total += 1;
                client.chat(&[user(prompt)])?
            };
            (joined, calls_total)
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

    // The doubt check is for the SUMMARY ONLY — a free composition the model writes, where a name
    // or a number that was never said can slip in and only a whole-document pass will catch it.
    // (The map-reduce reduction is itself a model call and can add something of its own; this is
    // where it is caught.)
    //
    // THE READABLE TEXT DOES NOT GET THIS CHECK, and must not. It is assembled line by line from
    // OUR list (`cleanup_by_lines`): every line is either the transcript verbatim or a cleaning
    // already checked against its OWN original by `grounding::check` (nothing added) and
    // `keeps_what_was_said` (nothing replaced). A word in it is, by construction, a word from the
    // recording — there is nowhere for an invented name to enter.
    //
    // Running the document-level NER check on it anyway adds no safety and manufactures false
    // doubts. It tags the SAME words with a context-dependent model twice — once in the readable
    // text, once in the transcript — and flags any disagreement between the two passes. Measured
    // 17.07.2026, session 20260714_181036: «Кадыйлят», a standalone ASR garble at 58:43, sits
    // verbatim in BOTH the readable text and the transcript, and was still flagged «стоит
    // перепроверить: кадыйлят» because the NER tagged it as a name in one document and not the
    // other. A doubt on a word the reader can see in the very same text is worse than no doubt: it
    // teaches them to stop reading the marks that do matter.
    let ungrounded = if matches!(task, Task::Summary) {
        let lex = localvox_light_core::lexicon::active();
        match ner {
            Some(n) => grounding::check_with_entities(lex, &base, &speech, &result, n),
            None => grounding::check_parts(lex, &base, &speech, &result),
        }
    } else {
        grounding::Ungrounded::default()
    };

    let out_path = place_result(session_dir, task);
    // The doubts live in the header COMMENT: the app needs them (it shows the mark and the
    // «it is fine» button), but they must not disfigure the document a human forwards to
    // colleagues. A warning across the whole page over a «Питер» that became a
    // «Санкт-Петербург» is exactly the fussing that makes people stop reading warnings.
    let doubts = (!ungrounded.is_empty()).then(|| ungrounded.describe());
    let header = localvox_light_core::provenance::render(
        &template_name,
        &client.model(),
        replacements.iter().map(|r| r.count).sum::<usize>(),
        &localvox_light_core::versions::now_rfc3339(),
        doubts.as_deref(),
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
    /// true — there was no speech to clean up (a silent recording). This is DONE, not failed:
    /// the caller records it as `Outcome::Nothing` and never counts it as a post-processing
    /// error — that miscount was the re-cook loop that churned every silent session forever.
    pub nothing: bool,
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
                nothing: false, // already refined — done, not "nothing to do"
            });
        }
    }
    let src_path = store
        .resolve(best.id)
        .context("the file of the best version was not found")?;
    let orig = read_transcript_lines(&src_path)?;
    // An EMPTY transcript is a SILENT recording — "nothing to clean up", not a failure.
    //
    // This was the re-cook loop the owner kept hitting: a silent session has no transcript, refine
    // used to `bail!` here, the process exited 2 ("post-processing error"), the daemon marked the
    // job Failed, and `revive_failed` brought it back to Pending on every restart. Cleanup and
    // summary already treat "no speech" as skipped; refine now does the same — a done, not a
    // failure — so the session settles instead of churning forever.
    //
    // Below, a transcript that has lines but no recognized WORDS («Т.», «.») is skipped for the
    // same reason; an empty one is just the extreme of that.
    let words: usize = orig.iter().map(|l| speech_words(&l.text)).sum();
    if orig.is_empty() || words == 0 {
        return Ok(RefineOutcome {
            version_id: best.id,
            file: src_path,
            lines: orig.len(),
            changed: 0,
            omitted: 0,
            llm_calls: 0,
            wall_sec: t0.elapsed().as_secs_f64(),
            skipped: true,
            nothing: true, // silent recording — recorded as Outcome::Nothing, never a failure
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
                // The other direction: not what the model added, but whether the speech survived.
                // A line it REPLACED rather than repaired is an invention that `grounding::check`
                // cannot see — it has no name and no number to catch.
                if !keeps_what_was_said(&l.text, fixed) {
                    tracing::warn!(
                        "refine: the line at {:.0} s was replaced, not repaired — rolling back \
                         to the original. was: {:?}, model: {:?}",
                        l.start_sec,
                        l.text,
                        fixed
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
        nothing: false,
    })
}

const REFINE_PROMPT: &str = "\
Ниже — реплики расшифровки речи, каждая пронумерована [N]. Исправь в каждой \
ошибки автоматического распознавания, опечатки и пунктуацию; сохрани смысл и \
разговорный стиль. НЕ объединяй и НЕ разбивай реплики, НЕ меняй их количество и \
порядок. Верни РОВНО те же номера [N], по одной исправленной реплике в строке, \
без каких-либо пояснений. Если реплика — бессвязный шум, верни её без изменений.";

/// The words of a line, for comparing one against the other. Case and punctuation are noise here:
/// «Кнопка,» and «кнопка» are the same word surviving.
fn spoken_words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

/// A REPAIR KEEPS WHAT WAS SAID.
///
/// The prompt asks the model to fix recognition errors and keep the meaning; it never asks it to
/// shorten, and never to answer. On a real recording of the owner's it did answer: the line
/// «Ну, я тебя вижу, но не слышу. Вижу, что ты микрофон включил. У тебя, может, кнопка нажата?
/// ИНе вижу.» came back as «Какая нахуй кнопка, блядь?» — a plausible reply to the question,
/// written over the question.
///
/// Nothing caught it. `grounding::check` looks for names and numbers the model ADDED, and this
/// invention contained neither; the one word it kept («кнопка») came from the original. That check
/// guards one direction only, and the damage came from the other: not what was added, but what was
/// thrown away.
///
/// This matters more here than anywhere else in the pipeline. The refined version becomes `best` —
/// that is, THE transcript — so from that moment the invention is grounded by definition for the
/// summary, the search and the export, and the audio is the only thing left that disagrees.
///
/// Rolling back costs a repair we could have had. Keeping a fabrication costs the archive its
/// truth, permanently and invisibly. The measurement is set for that asymmetry, not for balance.
fn keeps_what_was_said(orig: &str, fixed: &str) -> bool {
    let o = spoken_words(orig);
    // Too short to measure: a mangled «ориентируйсям сво» → «ориентируйся сам» is a legitimate
    // repair that keeps almost no word intact, and a ratio over three words means nothing.
    // Inventions of that size are still caught by `grounding::check`.
    if o.len() < 4 {
        return true;
    }
    let f: std::collections::HashSet<String> = spoken_words(fixed).into_iter().collect();
    // A repair does not halve the utterance — it is the same speech with the errors taken out.
    let kept = o.iter().filter(|w| f.contains(*w)).count();
    kept * 2 >= o.len()
}

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

    // WHO said it. Diarization's answer if it separated the voices; otherwise the label of the
    // audio SOURCE — and that label is not «Я» by default any more. For a downloaded video source
    // 0 is whoever was in it, so «Я» would put the owner's name on words spoken by someone else,
    // and the summary would then say it in prose. One place decides: `chunks::source_label`.
    let meta: localvox_light_core::chunks::SessionMeta =
        std::fs::read(session_dir.join("meta.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();

    let mut out = String::new();
    for l in &lines {
        let who = match l.speaker.clone() {
            Some(name) => name,
            None => localvox_light_core::chunks::source_label(&meta, l.source_id),
        };
        let mins = (l.start_sec / 60.0) as u64;
        let secs = l.start_sec as u64 % 60;
        out.push_str(&format!("[{who}] ({mins:02}:{secs:02}) {}\n", l.text));
    }
    Ok(out)
}

/// How many transcript lines go to the model in one cleanup request.
///
/// This is a QUALITY knob, not a correctness one: `cleanup_by_lines` rebuilds the document from
/// OUR line list, so a batch the model mangles costs those lines their cleanup — never their
/// existence. It is here because a request the model can actually obey is a better request.
///
/// Measured 16.07.2026 on the owner's session 20260716_164443 (149 lines): the old cleanup sent
/// the transcript in two 24 000-byte parts. On the big part the model wrote an essay instead of
/// the lines, and the whole part was dropped by the shape filter — 120 of 149 lines vanished from
/// the readable text. On the small tail part it obeyed. Short batches are obeyed; long ones are
/// summarised.
const CLEANUP_BATCH_LINES: usize = 12;

/// …and a ceiling in CHARACTERS, because lines are not a unit of work.
///
/// A line count says nothing about how much the model has to read and write. A meeting's lines
/// have a median of 64 characters, so 40 of them are 2.5 KB; a refined video's have a median of
/// 247, so the same 40 lines are 9.8 KB — four times the work under the same knob.
///
/// THE VALUE IS MEASURED, not chosen. Once the context window stopped truncating the answer, the
/// model answered for every line — and mostly echoed it back unchanged. Yield against batch size,
/// 19.07.2026, two of the owner's sessions, counting lines the model actually rewrote:
///
/// ```text
///     video (245-char lines)     710 chars → 10/12    1421 → 7/12    2843 → 1/12
///     meeting (139-char lines)  1108 chars → 20/24    1727 → 15/24   3435 → 1/24
/// ```
///
/// The collapse above ~1700 characters is the same on both, so it is a property of the model's
/// attention over the request, not of one recording. And a batch of ONE is not the answer either
/// — 6/12 on the video: a line with no neighbours has no context to repair a misheard word with,
/// and the model falls back on «бессвязный шум — верни без изменений».
const CLEANUP_BATCH_CHARS: usize = 1200;

/// Cut the lines into batches bounded by BOTH: at most `CLEANUP_BATCH_LINES` lines and at most
/// `CLEANUP_BATCH_CHARS` characters. A single line longer than the budget goes alone rather than
/// being split — half a line is not a line.
fn cleanup_batches(texts: &[String]) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut chars = 0usize;
    for (i, t) in texts.iter().enumerate() {
        let would = chars + t.len();
        if i > start && (i - start >= CLEANUP_BATCH_LINES || would > CLEANUP_BATCH_CHARS) {
            out.push(start..i);
            start = i;
            chars = 0;
        }
        chars += t.len();
    }
    if start < texts.len() {
        out.push(start..texts.len());
    }
    out
}

const CLEANUP_PROMPT: &str = "\
Ниже — реплики автоматической расшифровки речи, каждая пронумерована [N]. Приведи каждую \
к читаемому виду: убери слова-паразиты («эээ», «ну», «как бы», «короче»), фальстарты и \
повторы; почини явные ошибки распознавания по контексту. Сохрани смысл и порядок; ничего \
не сокращай по содержанию и ничего не выдумывай — это чистка, а не пересказ. НЕ объединяй \
и НЕ разбивай реплики, НЕ меняй их количество. Верни РОВНО те же номера [N], по одной \
реплике в строке, без пояснений и преамбул. Если реплика — бессвязный шум, верни её без \
изменений.";

// The label of a readable line used to be built HERE, at cook time, and baked into the file. It
// is not built here any more and not stored at all: `readable::render_markdown` joins it in when
// someone asks for the text. See `core/src/readable.rs`.

/// The readable text, built line by line: the model may only REPHRASE lines we hand it, never
/// decide what the document contains.
///
/// THE INCIDENT THIS EXISTS FOR (16.07.2026, session 20260716_164443). The old cleanup sent the
/// transcript as free text and took the model's answer AS the document. On a 27 006-byte
/// transcript it split into two parts; on the big one the model returned «Вот структурированный
/// анализ… ### Логистика…» instead of the lines. `keep_only_transcript_lines` then stripped
/// everything that was not shaped like a transcript line — correctly killing the essay, and with
/// it all 120 lines of that part. Its emptiness guard («kept nothing → keep the answer») looked at
/// the WHOLE joined document, and the 29 lines of the surviving tail satisfied it. The readable
/// text of a 30-minute recording became its last five minutes, silently.
///
/// The lesson is not «split smaller» — a smaller part is a hope, and the model can disobey on any
/// size. It is that the OUTPUT MUST NOT BE THE SPINE. Here our own line list is: we walk it, look
/// the model's answer up by line number, and take it only if it passes the same two guards refine
/// uses. A line the model did not return keeps its original wording; a line it invented has
/// nowhere to land; an essay parses as no line numbers at all and changes nothing. The markup is
/// ours (`line_prefix`), so the model cannot inject a heading into the document even if it tries.
///
/// Returns the rendered text, the number of LLM calls, and how many lines the model never answered
/// for — that last one is the honest measure of how much cleanup actually happened.
fn cleanup_by_lines(
    session_dir: &Path,
    client: &LlmClient,
    glossary: &Glossary,
    glossary_block: &str,
) -> Result<(localvox_light_core::readable::Readable, usize)> {
    let store = VersionStore::open(session_dir)?;
    let best = store
        .best()
        .context("there are no transcript versions — run localvox-process (the cook) first")?;
    let src = store
        .resolve(best.id)
        .context("the file of the best version was not found")?;
    let lines = read_transcript_lines(&src)?;

    // The deterministic pass first: the glossary's canonical spelling is ours, not the model's.
    let texts: Vec<String> = lines.iter().map(|l| glossary.apply(&l.text).0).collect();

    let ranges = cleanup_batches(&texts);
    let batches = ranges.len();
    let mut cleaned: std::collections::HashMap<usize, String> = std::collections::HashMap::new();
    let mut llm_calls = 0usize;

    for (bi, range) in ranges.into_iter().enumerate() {
        let first = range.start;
        let chunk = &texts[range];
        let mut prompt = String::from(CLEANUP_PROMPT);
        if !glossary_block.is_empty() {
            prompt.push_str("\n\nТермины (правильное написание):\n");
            prompt.push_str(glossary_block);
        }
        prompt.push_str("\n\n");
        for (i, t) in chunk.iter().enumerate() {
            prompt.push_str(&format!("[{}] {t}\n", first + i + 1));
        }
        // Only the numbers of THIS batch. A model that renumbers its answer [1..] instead of
        // [81..] must not overwrite another batch's lines.
        let sent: std::collections::HashSet<usize> = (first + 1..=first + chunk.len()).collect();
        let answer = client.chat(&[user(prompt)])?;
        llm_calls += 1;
        let mut got = 0usize;
        for (n, text) in parse_numbered(&answer) {
            if sent.contains(&n) && !text.trim().is_empty() {
                cleaned.insert(n, text);
                got += 1;
            }
        }
        // The one number that says whether the model did the work or waved us off. Silence here
        // was what let 120 lines disappear without a trace.
        if got < chunk.len() {
            tracing::warn!(
                "cleanup: batch {}/{batches} — the model answered for {got} of {} lines; the rest \
                 keep the transcript's own wording",
                bi + 1,
                chunk.len()
            );
        }
    }

    let (edits, omitted, rejected) = assemble_readable(&lines, &texts, &cleaned);
    if omitted > 0 || rejected > 0 {
        tracing::info!(
            "cleanup: {} lines — {omitted} the model did not answer for, {rejected} rolled back; \
             those keep the transcript's own wording",
            lines.len()
        );
    }
    Ok((
        localvox_light_core::readable::Readable {
            version_id: best.id,
            provenance: Default::default(),
            edits,
            omitted,
            rejected,
        },
        llm_calls,
    ))
}

/// Build the delta by walking OUR lines and looking the model's answers up by number.
///
/// This is the whole invariant, and it is pure so it can be held to it: the result has at most one
/// entry per input line, keyed by ITS index, no matter what the model said. `cleaned` is a lookup
/// table, never an iteration source — that single choice is the difference between «the model
/// rephrased 3 of 40 lines» and «117 lines are gone».
///
/// A line that ends up saying exactly what the transcript says is NOT recorded: its absence is the
/// record that nothing happened to it, and that is also what makes the artifact a delta rather
/// than a second copy of the speech.
///
/// Returns the edits, how many lines the model never answered for, and how many of its answers
/// were rolled back by the guards.
fn assemble_readable(
    lines: &[TranscriptLine],
    texts: &[String],
    cleaned: &std::collections::HashMap<usize, String>,
) -> (std::collections::BTreeMap<usize, String>, usize, usize) {
    let mut omitted = 0usize;
    let mut rejected = 0usize;
    let mut edits = std::collections::BTreeMap::new();
    for (i, l) in lines.iter().enumerate() {
        let original = texts[i].as_str();
        let text = match cleaned.get(&(i + 1)) {
            Some(fixed) => {
                // The same two guards refine uses, and for the same reason. `check` catches what
                // the model ADDED (a name, a number that was never said); `keeps_what_was_said`
                // catches what it THREW AWAY — a line answered instead of repaired, which `check`
                // cannot see because an invention made of ordinary words has nothing to flag.
                let bad = grounding::check(&l.text, fixed);
                if !bad.is_empty() {
                    tracing::warn!(
                        "cleanup: the line at {:.0} s was added to ({}) — keeping the original",
                        l.start_sec,
                        bad.describe()
                    );
                    rejected += 1;
                    original
                } else if !keeps_what_was_said(&l.text, fixed) {
                    tracing::warn!(
                        "cleanup: the line at {:.0} s was replaced, not cleaned — keeping the \
                         original. was: {:?}, model: {:?}",
                        l.start_sec,
                        l.text,
                        fixed
                    );
                    rejected += 1;
                    original
                } else {
                    fixed.trim()
                }
            }
            None => {
                omitted += 1;
                original
            }
        };
        // The glossary counts as a change too: its canonical spelling is ours, applied before the
        // model ever sees the line, and a reader comparing against the transcript must be able to
        // see that the wording moved.
        if text != l.text {
            edits.insert(i, text.to_string());
        }
    }
    (edits, omitted, rejected)
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

    fn line(n: u64, source_id: u8, text: &str) -> TranscriptLine {
        TranscriptLine {
            source_id,
            start_sec: n as f64,
            end_sec: n as f64 + 1.0,
            text: text.to_string(),
            speaker: None,
        }
    }

    /// THE INCIDENT, 16.07.2026, session 20260716_164443.
    ///
    /// The model was handed the transcript as free text and answered the first of two parts with
    /// «Вот структурированный анализ… ### Логистика…» instead of the lines. The shape filter then
    /// stripped everything that did not look like a transcript line — the essay, and with it all
    /// 120 real lines of that part. The readable text of a 30-minute recording became its last five
    /// minutes, and nothing said a word about it.
    ///
    /// The knob used to be a line count alone, and lines are not a unit of work: 40 of the
    /// owner's meeting lines are 2.5 KB, 40 of his refined video's are 9.8 KB.
    #[test]
    fn a_batch_is_bounded_by_characters_as_well_as_by_lines() {
        // Lines of 1000 characters: six of them already exceed the character budget.
        let fat: Vec<String> = (0..12).map(|_| "п".repeat(1000)).collect();
        let ranges = cleanup_batches(&fat);
        assert!(ranges.len() > 1, "a 12 KB request went out as one batch");
        for r in &ranges {
            let chars: usize = fat[r.clone()].iter().map(String::len).sum();
            assert!(chars <= CLEANUP_BATCH_CHARS + 1000, "batch of {chars} chars");
        }
        // Every line lands in exactly one batch, in order — the invariant the whole cleanup rests
        // on. A line dropped here would simply never be offered to the model.
        let covered: Vec<usize> = ranges.iter().flat_map(|r| r.clone()).collect();
        assert_eq!(covered, (0..12).collect::<Vec<_>>());
    }

    /// Short lines still batch by count, or a chat-like meeting would go out in dozens of tiny
    /// requests, each paying the model's load time again.
    #[test]
    fn short_lines_still_fill_a_batch_by_count() {
        let thin: Vec<String> = (0..100).map(|_| "да".to_string()).collect();
        let ranges = cleanup_batches(&thin);
        assert_eq!(ranges.len(), 100usize.div_ceil(CLEANUP_BATCH_LINES), "{ranges:?}");
        assert_eq!(ranges[0], 0..CLEANUP_BATCH_LINES);
    }

    /// A single line longer than the whole budget is not split: half a line is not a line, and
    /// the delta is keyed by line index.
    #[test]
    fn one_oversized_line_goes_alone_rather_than_in_half() {
        let one = vec!["a".repeat(20_000), "b".to_string()];
        let ranges = cleanup_batches(&one);
        assert_eq!(ranges[0], 0..1);
        assert_eq!(ranges.len(), 2);
    }

    /// An essay carries no `[N]` markers, so it parses to NOTHING. That is the whole test: an empty
    /// lookup table must cost the cleanup, never the text.
    ///
    /// Since the artifact became a delta, «the text survives» is structural: an empty delta
    /// overrides nothing, and every line is rendered from the transcript that owns it. So what is
    /// asserted here is that the essay contributes NOTHING — not one entry, and the silence
    /// counted rather than hidden.
    #[test]
    fn an_essay_instead_of_the_lines_costs_the_cleanup_not_the_text() {
        let lines = vec![
            line(0, 0, "Так, начали."),
            line(19, 1, "Слышно."),
            line(28, 0, "Ну ладно. Окей."),
        ];
        let texts: Vec<String> = lines.iter().map(|l| l.text.clone()).collect();
        // What `parse_numbered` returns for «Вот **структурированный анализ**… ### Логистика…».
        let nothing = std::collections::HashMap::new();

        let (edits, omitted, rejected) = assemble_readable(&lines, &texts, &nothing);

        assert!(edits.is_empty(), "the model's prose reached the document: {edits:?}");
        assert_eq!(omitted, 3, "the silence must be counted, not hidden");
        assert_eq!(rejected, 0);
    }

    /// The model supplies the WORDS of a line and nothing else. It cannot put a heading, a label
    /// or a timecode into the readable text: those are not stored here at all — the labels are
    /// joined in at render time from the transcript and `meta.json`, which the model never sees.
    #[test]
    fn the_model_contributes_wordings_and_nothing_else() {
        let lines = vec![line(4, 0, "так начали"), line(19, 1, "Слышно.")];
        let texts: Vec<String> = lines.iter().map(|l| l.text.clone()).collect();
        let cleaned = std::collections::HashMap::from([(1, "Так, начали.".to_string())]);

        let (edits, _, _) = assemble_readable(&lines, &texts, &cleaned);

        // Keyed by OUR line index (0-based), holding the wording only.
        assert_eq!(edits.get(&0).map(String::as_str), Some("Так, начали."));
        assert!(
            !edits.values().any(|t| t.contains('[') || t.contains("00:")),
            "a label or a timecode got into the stored wording: {edits:?}"
        );
        // The untouched line is absent — its absence IS the record that nothing happened to it.
        assert!(!edits.contains_key(&1), "an untouched line was written down: {edits:?}");
    }

    /// The model answered for some lines and stayed silent about the rest — the normal case on a
    /// long recording, and the one that used to be indistinguishable from data loss. Every line
    /// stands; the silent ones keep the transcript's own wording.
    #[test]
    fn lines_the_model_never_answered_for_keep_their_own_wording() {
        let lines = vec![
            line(0, 0, "ну короче я это самое"),
            line(9, 0, "давай в субботу"),
            line(18, 0, "эээ ну да"),
        ];
        let texts: Vec<String> = lines.iter().map(|l| l.text.clone()).collect();
        let cleaned = std::collections::HashMap::from([(2, "Давай в субботу.".to_string())]);

        let (edits, omitted, rejected) = assemble_readable(&lines, &texts, &cleaned);

        assert_eq!(omitted, 2);
        assert_eq!(rejected, 0);
        // The model's numbering is 1-based, ours is the line index. Off by one here would hand
        // one line's cleaning to its neighbour.
        assert_eq!(edits.len(), 1, "{edits:?}");
        assert_eq!(edits.get(&1).map(String::as_str), Some("Давай в субботу."));
    }

    /// The fabrication that reached the owner's archive, now guarded on the readable text too:
    /// the model ANSWERED the line instead of cleaning it. Rolled back to the original — and the
    /// line still stands, because a rollback is not a deletion.
    #[test]
    fn a_fabricated_line_is_rolled_back_in_the_readable_text_too() {
        let was = "Ну, я тебя вижу, но не слышу. Вижу, что ты микрофон включил. У тебя, может, \
                   кнопка нажата? ИНе вижу.";
        let lines = vec![line(120, 0, was)];
        let texts: Vec<String> = lines.iter().map(|l| l.text.clone()).collect();
        let cleaned =
            std::collections::HashMap::from([(1, "Какая нахуй кнопка, блядь?".to_string())]);

        let (edits, omitted, rejected) = assemble_readable(&lines, &texts, &cleaned);

        assert_eq!(rejected, 1, "the fabrication got into the readable text");
        assert_eq!(omitted, 0);
        // Rolled back to the transcript's own words — which means there is no edit to record.
        // The line still stands: it is rendered from the transcript, as every unedited line is.
        assert!(edits.is_empty(), "the fabrication survived: {edits:?}");
    }

    /// THE MODEL ANSWERED THE LINE INSTEAD OF REPAIRING IT.
    ///
    /// All four cases are REAL — the only four lines the refine touched on the owner's recording
    /// 20260716_164443 (149 lines). Three are repairs and must live. The fourth is the invention
    /// that reached the archive: a plausible reply written over the question that prompted it.
    /// Nothing stopped it, because `grounding::check` guards inventions that carry a name or a
    /// number, and this one carried neither.
    #[test]
    fn a_line_the_model_replaced_instead_of_repairing_is_rolled_back() {
        // The invention. The one word it kept — «кнопка» — it took from the line it destroyed.
        assert!(
            !keeps_what_was_said(
                "Ну, я тебя вижу, но не слышу. Вижу, что ты микрофон включил. У тебя, может, \
                 кнопка нажата? ИНе вижу.",
                "Какая нахуй кнопка, блядь?"
            ),
            "the fabrication that reached the archive still passes"
        );

        // The repairs. Rolling any of these back is a loss, not a save.
        assert!(keeps_what_was_said(
            "И потом во сколько ты уыезжаешь в субботу?",
            "И потом во сколько ты уезжаешь в субботу?"
        ));
        assert!(keeps_what_was_said(
            "Да ты упариваешься, что ли? Ну, ну типа, это ты будешь",
            "Да ты упариваешься, что ли? Ну, ну типа, это ты будешь."
        ));
        assert!(keeps_what_was_said(
            "Э. Тогда там ближе к телу, тогда будет понятно точнее, чего и куда.  И в общем, \
             ориентируйсям сво.",
            "Э. Тогда там ближе к телу, тогда будет понятно точнее, чего и куда. И в общем, \
             ориентируйсям сво."
        ));
    }

    /// A short mangled line is repaired wholesale, and that is legitimate — «ориентируйсям сво» has
    /// no word worth keeping. A ratio over three words measures nothing, so it is not applied;
    /// inventions of that size still have to get past `grounding::check`.
    #[test]
    fn a_short_mangled_line_is_still_allowed_to_be_repaired() {
        assert!(keeps_what_was_said("ИНе вижу", "И не вижу"));
        assert!(keeps_what_was_said("ориентируйсям сво", "ориентируйся сам"));
    }

    /// The guard measures the SPEECH, not the punctuation: a line that only gained commas and a
    /// full stop has lost nothing.
    #[test]
    fn punctuation_alone_never_looks_like_a_replacement() {
        assert!(keeps_what_was_said(
            "ну короче условно там с тридцать можно подъехать куда-то к андрону этому",
            "Ну, короче, условно, там с тридцать можно подъехать куда-то к Андрону этому."
        ));
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
