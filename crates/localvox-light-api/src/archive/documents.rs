//! Feature operations behind the stable Archive API facade.
use super::*;

impl Archive {
    /// The lines of the recording, anchored to the audio: the view for LISTENING and checking a
    /// fragment against the sound.
    ///
    /// It shows the CLEANED wording when there is one, and that is a deliberate reversal. It used
    /// to insist on the raw cook, because a wording combed by an LLM is a derivative and the
    /// recording had to stay evidence (acceptance 13.07.2026: the model turned «там кратно превы…»
    /// into «там кратно выручка превышает расходы»). What made that reversal safe is not a change
    /// of mind but a change of mechanism: the cleanup no longer composes a document. It rewords
    /// ONE line at a time, each judged against its OWN original — nothing may be added
    /// (`grounding::check`) and nothing replaced (`keeps_what_was_said`) — and a line that fails
    /// keeps the recognizer's words. The evidence is still there, in the version file, untouched:
    /// this is a rendering of it, and the play button beside every line is the check that matters.
    ///
    /// Both views draw from the same join, so «расшифровка» and «читаемый текст» can never
    /// disagree about what was said. They differ in FORM: lines against the audio here, continuous
    /// prose there.
    pub fn transcript(&self, session: &str) -> Result<TranscriptDoc> {
        let dir = self.session_dir(session)?;
        if let Some((partial, lines)) = self.partial_document(&dir)? {
            return Ok(TranscriptDoc {
                session: session.into(),
                label: "Готовая часть".into(),
                model: String::new(),
                lines,
                partial: Some(partial),
            });
        }
        let store = VersionStore::open(&dir)?;
        let generation = Self::generation(&dir);

        // The cleanup pins the version it read. Serving its wording over any OTHER version would
        // put one text's cleaning under another text's line numbers.
        if let Ok(r) = localvox_light_core::readable::load(&dir) {
            if let Ok(lines) = localvox_light_core::readable::lines(&dir) {
                let v = store
                    .load()
                    .versions
                    .into_iter()
                    .find(|v| v.id == r.version_id);
                return Ok(TranscriptDoc {
                    partial: None,
                    session: session.to_string(),
                    label: v.as_ref().map(|v| v.label.clone()).unwrap_or_default(),
                    model: v.map(|v| v.model).unwrap_or_default(),
                    lines: lines
                        .into_iter()
                        .map(|l| TranscriptLineOut {
                            id: Self::line_id(
                                generation.as_deref(),
                                l.source_id,
                                l.start_sec,
                                l.end_sec,
                            ),
                            who: l.who,
                            source_id: l.source_id,
                            start_sec: l.start_sec,
                            end_sec: l.end_sec,
                            text: l.text,
                        })
                        .collect(),
                });
            }
        }

        let (version_id, lines) = localvox_light_core::readable::transcript_lines(&dir)?;
        let version = store
            .load()
            .versions
            .into_iter()
            .find(|v| v.id == version_id);
        Ok(TranscriptDoc {
            partial: None,
            session: session.to_string(),
            label: version
                .as_ref()
                .map(|v| v.label.clone())
                .unwrap_or_default(),
            model: version.map(|v| v.model).unwrap_or_default(),
            lines: lines
                .into_iter()
                .map(|l| TranscriptLineOut {
                    id: Self::line_id(generation.as_deref(), l.source_id, l.start_sec, l.end_sec),
                    who: l.who,
                    source_id: l.source_id,
                    start_sec: l.start_sec,
                    end_sec: l.end_sec,
                    text: l.text,
                })
                .collect(),
        })
    }

    /// The summary, as blocks with their sources RESOLVED TO TIMECODES.
    ///
    /// The file stores line indices — that is the durable form, since a timecode belongs to the
    /// transcript and would go stale the moment a recording is cooked again. The client gets
    /// seconds, because what it does with a source is play it, and making every client re-open
    /// the transcript to turn 12 into 01:30 would be handing them our homework.
    ///
    /// A citation whose line no longer exists is dropped here rather than sent as a dead button.
    /// The claim stays: it was checked when it was written, and a re-cooked transcript does not
    /// make it false — only unlocatable.
    pub fn summary_blocks(&self, session: &str) -> Result<serde_json::Value> {
        let dir = self.session_dir(session)?;
        let text = match fs::read_to_string(dir.join("summary.md")) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(json!({"blocks":[],"provenance":null,"available":false}))
            }
            Err(error) => return Err(error).context("не удалось прочитать сводку"),
        };
        let (prov, body) = localvox_light_core::provenance::split(&text);
        let blocks = localvox_light_core::citations::parse(body);

        let lines = VersionStore::open(&dir)
            .ok()
            .and_then(|s| s.best().and_then(|b| s.resolve(b.id)))
            .and_then(|p| read_transcript_lines(&p).ok())
            .unwrap_or_default();

        let out: Vec<serde_json::Value> = blocks
            .into_iter()
            .map(|b| {
                let sources: Vec<serde_json::Value> = b
                    .lines
                    .iter()
                    .filter_map(|&i| lines.get(i).map(|l| (i, l)))
                    .map(
                        |(i, l)| json!({"line": i, "start_sec": l.start_sec, "end_sec": l.end_sec}),
                    )
                    .collect();
                json!({"kind": b.kind, "text": b.text, "sources": sources})
            })
            .collect();
        Ok(json!({"blocks": out, "provenance": prov,"available":true}))
    }

    pub fn artifact(&self, session: &str, file: &str) -> Result<String> {
        let dir = self.session_dir(session)?;
        if let Ok(text) = fs::read_to_string(dir.join(file)) {
            return Ok(text);
        }
        // We explain the REASON instead of retelling the file-system error: "file not
        // found" tells the user nothing, whereas "the result is in quarantine" or
        // "this flag creates it" does.
        if file.ends_with(".unverified.md") {
            bail!("черновика нет: последний результат подтверждён записью");
        }
        let draft = file.replace(".md", ".unverified.md");
        if dir.join(&draft).exists() {
            bail!(
                "результат не подтверждён записью и сохранён черновиком — \
                 откройте вкладку «⚠ Черновик»"
            );
        }
        // We suggest THE flag that actually creates this file: previously, for a
        // missing processed.md we advised `--summary` — a command that does not make
        // it.
        let flag = match file {
            "summary.md" => "--summary",
            f if f == localvox_light_core::readable::FILE => "--cleanup",
            _ => "--summary --cleanup",
        };
        bail!(
            "нет {file} у этой сессии. Сгенерировать: localvox-process <сессия> {flag} \
             (пусто и после этого — значит, в записи не нашлось содержательной речи)"
        )
    }

    /// What is happening with the session, stage by stage. "⚙ cooking" for ten minutes answers
    /// "is anything happening at all" but not "where is it now" — and, when it breaks, not "what
    /// exactly broke".
    /// «Всё верно» — человек прочитал документ и снял пометку сомнения.
    ///
    /// Антоним «переварить заново»: там человек говорит «машина ошиблась, переделай», здесь —
    /// «машина придирается, всё нормально». Наша проверка сверяет буквально и всегда будет
    /// ошибаться (измерено: «Сергей» и «муж» помечены выдумкой, хотя прозвучали вслух). Человек
    /// читал текст и может послушать запись — машина не может. Его слово весомее, и оно
    /// ЗАПОМИНАЕТСЯ: предупреждение, которое возвращается после ответа, перестают читать вообще.
    ///
    /// Кнопка была, а этого метода и его адреса не было вовсе — она стучалась в никуда и молча
    /// ничего не делала.
    pub fn confirm(&self, session: &str, artifact: &str) -> Result<String> {
        // Только то, что мы правда умеем подтверждать: чужое имя артефакта не должно молча
        // создавать запись в журнале.
        let artifact = match artifact {
            "summary" => localvox_light_core::processing::SUMMARY,
            "processed" => localvox_light_core::processing::PROCESSED,
            other => bail!("неизвестный документ: {other}"),
        };
        let dir = self.session_dir(session)?;
        let _lease = self.mutation_lease(session, &dir)?;
        if localvox_light_core::processing::confirm(&dir, artifact)? {
            tracing::info!("{session}/{artifact}: человек подтвердил — пометка снята");
            Ok("Отмечено: всё верно".into())
        } else {
            // Не ошибка: подтверждать было нечего (пометки нет или её уже сняли).
            Ok("Пометки и так не было".into())
        }
    }

    /// The transcript versions of a session. Not "×7" on the card — that number
    /// answers nothing; a person needs to know WHAT it was cooked with, when, and
    /// which one is the working one right now.
    pub fn versions(&self, session: &str) -> Result<Vec<VersionInfo>> {
        let dir = self.session_dir(session)?;
        let store = VersionStore::open(&dir)?;
        let m = store.load();
        let best = store.best().map(|v| v.id);
        Ok(m.versions
            .iter()
            .map(|v| VersionInfo {
                id: v.id,
                label: v.label.clone(),
                model: v.model.clone(),
                created_at: v.created_at.clone(),
                derived: !v.parents.is_empty(),
                best: Some(v.id) == best,
                lines: store
                    .resolve(v.id)
                    .and_then(|p| read_transcript_lines(&p).ok())
                    .map(|l| l.len())
                    .unwrap_or(0),
            })
            .collect())
    }

    /// Make a version the working one: the summary, the search and the export are all
    /// computed from it.
    pub fn set_best(&self, session: &str, id: u32) -> Result<()> {
        let dir = self.session_dir(session)?;
        let _lease = self.mutation_lease(session, &dir)?;
        let store = VersionStore::open(&dir)?;
        store
            .set_best(id)
            .with_context(|| format!("нет версии v{id:03}"))?;
        tracing::info!("{session}: v{id:03} set as the working version");
        Ok(())
    }
}
