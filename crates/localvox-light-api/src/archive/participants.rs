//! Feature operations behind the stable Archive API facade.
use super::*;

impl Archive {
    /// Who spoke in the recording: the roster of participants with their captions.
    ///
    /// Empty — either there was no diarization (no model) or no voices were found.
    /// Both are honest: we MUST NOT silently show «Участник 1» where we counted
    /// nobody.
    /// WHO IS IN THIS RECORDING — derived from the lines themselves, never from a register.
    ///
    /// It used to come from the diarization roster, and the roster lies by omission in both
    /// directions. Measured 19.07.2026 on the owner's archive:
    ///
    ///   20260718_230541_youtube  roster: «Участник 1» (1.4 мин) + «Арсен Маркарян» (32.7 мин)
    ///                            text:   «Арсен Маркарян» only. «Участник 1» says NOT ONE LINE.
    ///   20260716_164443          roster lists «Участник 3»; the text has no such line either,
    ///                            and has «Собеседники», which the roster knows nothing about.
    ///
    /// A phantom in this list is not a cosmetic flaw: the owner is offered a rename for somebody
    /// who never speaks in their document, and the count of participants is wrong. So the list is
    /// GROUPED FROM THE LINES. Someone who says nothing cannot appear, because there is nothing to
    /// group them from.
    ///
    /// It also answers the question the old screen could not: «чем «Собеседники» отличается от
    /// «Участник 1»». Both are attributions in the same text, produced by different mechanisms —
    /// a voice diarization separated, versus the input the sound arrived through — and each entry
    /// now says which of the two it is.
    pub fn speakers(&self, session: &str) -> Result<Vec<ParticipantOut>> {
        let dir = self.session_dir(session)?;
        let store = VersionStore::open(&dir)?;
        // The very version the document shows, so the list and the text can never disagree.
        let id = localvox_light_core::readable::load(&dir)
            .map(|r| r.version_id)
            .ok()
            .or_else(|| store.best().map(|v| v.id))
            .context("у сессии нет транскрипта (сначала localvox-process)")?;
        let path = store.resolve(id).context("файл версии не найден")?;
        let lines = read_transcript_lines(&path)?;
        let meta: localvox_light_core::chunks::SessionMeta = fs::read(dir.join("meta.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();

        // Keyed by what PRODUCED the attribution, not by the name it currently shows: a source
        // renamed to the same string as a voice is still a different thing, and merging them would
        // hand one rename to the other.
        let mut order: Vec<(Option<String>, u8)> = Vec::new();
        let mut acc: std::collections::HashMap<(Option<String>, u8), (f64, usize)> =
            std::collections::HashMap::new();
        for l in &lines {
            let key = (l.speaker.clone(), l.source_id);
            let e = acc.entry(key.clone()).or_insert_with(|| {
                order.push(key.clone());
                (0.0, 0)
            });
            e.0 += (l.end_sec - l.start_sec).max(0.0);
            e.1 += 1;
        }

        // ONE ROW PER NAME. The two mechanisms can land on the same name, and then they are one
        // person as far as anyone reading is concerned. Measured on the owner's video: 73 lines
        // carried the separated voice «Арсен Маркарян» and 3 more had no voice separated, so they
        // fell back to the source — which he had renamed to «Арсен Маркарян» too. Listed apart,
        // that reads as the same man twice, and honest data looks like a defect.
        //
        // Merged, the row still states the whole truth in `what`: which mechanisms attributed it.
        // And it keeps BOTH keys, because renaming «этого человека» has to reach every line the
        // screen shows under that name — a rename that fixed 73 lines and left 3 behind would be
        // the same defect wearing a different shape.
        let mut rows: Vec<ParticipantOut> = Vec::new();
        for key in order {
            let (speech_sec, lines) = acc[&key];
            let (voice, source_id) = key;
            let (name, what, named) = match &voice {
                // Diarization separated this voice. «Участник N» means exactly «a voice we could
                // not put a name to» — an honest answer, and the button asks for one.
                Some(label) => (
                    label.clone(),
                    "отдельный голос",
                    !label.starts_with("Участник"),
                ),
                // No voice was separated — the line is attributed to the INPUT it came through,
                // and that is all we honestly know about it.
                None => (
                    localvox_light_core::chunks::source_label(&meta, source_id),
                    if source_id == 0 {
                        if meta.source.is_some() {
                            "звук из источника"
                        } else {
                            "микрофон"
                        }
                    } else {
                        "системный звук"
                    },
                    meta.source_names.contains_key(&source_id.to_string()),
                ),
            };

            let row = match rows.iter().position(|r| r.name == name) {
                Some(i) => {
                    let r = &mut rows[i];
                    r.speech_sec += speech_sec;
                    r.lines += lines;
                    r.named |= named;
                    if !r.what.split(" · ").any(|p| p == what) {
                        r.what = format!("{} · {what}", r.what);
                    }
                    r
                }
                None => {
                    rows.push(ParticipantOut {
                        name,
                        what: what.to_string(),
                        named,
                        voices: Vec::new(),
                        sources: Vec::new(),
                        speech_sec,
                        lines,
                    });
                    rows.last_mut().unwrap()
                }
            };
            match voice {
                Some(label) => row.voices.push(label),
                None => row.sources.push(source_id),
            }
        }
        // Loudest first: the person who carried the recording belongs at the top.
        rows.sort_by(|a, b| b.speech_sec.total_cmp(&a.speech_sec));
        Ok(rows)
    }

    /// What each audio SOURCE is called in this session — with the name in force right now.
    ///
    /// Sources are not speakers: they are «which input the sound came through». Diarization names
    /// voices; this names the fallback used when it has not. The two live in different places
    /// because they are different facts.
    pub fn sources(&self, session: &str) -> Result<Vec<serde_json::Value>> {
        let dir = self.session_dir(session)?;
        let meta: localvox_light_core::chunks::SessionMeta =
            serde_json::from_slice(&fs::read(dir.join("meta.json")).context("нет meta.json")?)?;
        // Only the sources this recording actually HAS: a session that arrived by link has no
        // second track, and offering to rename one would invent a participant.
        let ids: Vec<u8> = if meta.source.is_some() {
            vec![0]
        } else {
            vec![0, 1]
        };
        Ok(ids
            .into_iter()
            .map(|id| {
                json!({
                    "source_id": id,
                    "name": localvox_light_core::chunks::source_label(&meta, id),
                    "custom": meta.source_names.contains_key(&id.to_string()),
                    "what": if id == 0 {
                        if meta.source.is_some() { "звук из источника" } else { "микрофон" }
                    } else {
                        "системный звук"
                    },
                })
            })
            .collect())
    }

    /// Rename a source: the narrator of a downloaded video is not «Я».
    ///
    /// ONLY THE SUMMARY IS REBUILT. It is prose the model wrote, with the name inside its
    /// sentences — «Я рассказываю про…» can only become «Арсен рассказывает про…» by writing the
    /// summary again. Leaving it would pass the correction off as taken into account.
    ///
    /// The readable text is NOT rebuilt and NOT deleted, and that is the whole point of storing it
    /// as a delta: it holds no name at all. The label is joined in from `meta.json` on every read,
    /// so it already says the new name — before this function returns. Re-cooking it would burn
    /// minutes of LLM time to arrive at exactly the file that is already on disk.
    pub fn name_source(&self, session: &str, source_id: u8, name: &str) -> Result<String> {
        let dir = self.session_dir(session)?;
        let _lease = self.mutation_lease(session, &dir)?;
        let meta_path = dir.join("meta.json");
        let mut meta: localvox_light_core::chunks::SessionMeta =
            serde_json::from_slice(&fs::read(&meta_path).context("нет meta.json")?)?;
        let name = name.trim();
        if name.is_empty() {
            // Back to the default — which depends on where the audio came from.
            meta.source_names.remove(&source_id.to_string());
        } else {
            meta.source_names
                .insert(source_id.to_string(), name.to_string());
        }
        localvox_light_core::chunks::save_meta_checked(&meta_path, &meta)?;
        let now = localvox_light_core::chunks::source_label(&meta, source_id);

        // Neither the transcript lines nor the readable text are touched: the label is not stored
        // in either of them, it is rendered from the source id on every read. Only the summary
        // QUOTES it, inside sentences the model composed, and only the summary is rebuilt.
        let queued = self.requeue_named_summary(session, &dir)?;
        Ok(format!(
            "теперь «{now}» — текст и реплики переподписаны сразу{}",
            if queued {
                ", сводка пересобирается"
            } else {
                "; сводка ожидает расшифровки"
            }
        ))
    }

    /// Name a voice: «Участник 2» is Ivan.
    ///
    /// Does three things, and all three are mandatory:
    ///
    /// 1. **remembers the manual name in this recording's roster**;
    /// 2. **re-labels the lines** in every version of the transcript — WITHOUT
    ///    re-recognition: the audio has not changed, only one caption has to change;
    /// 3. **throws away the summary** and queues it for a rebuild — it says «Участник 2»
    ///    inside sentences the model composed, and leaving that means lying that the name
    ///    was taken into account. The readable text needs nothing: it stores no name, and
    ///    step 2 is already the only place the caption lives.
    ///
    /// Naming never enrolls or consults a shared voice profile.
    pub fn name_speaker(&self, session: &str, label: &str, name: &str) -> Result<String> {
        let name = name.trim();
        anyhow::ensure!(!name.is_empty(), "имя не может быть пустым");
        let dir = self.session_dir(session)?;
        let _lease = self.mutation_lease(session, &dir)?;

        let mut roster = localvox_light_core::diarize::roster::load(&dir);
        let member = roster
            .by_label(label)
            .cloned()
            .with_context(|| format!("в этой записи нет говорящего «{label}»"))?;
        if member.label == name && member.manually_named {
            return Ok(format!("«{name}» — и так его имя"));
        }

        anyhow::ensure!(
            !roster
                .members
                .iter()
                .any(|m| m.id != member.id && m.label == name),
            "в этой записи уже есть участник с именем «{name}»"
        );

        roster.rename(label, name);

        let store = VersionStore::open(&dir)?;
        let lines = store.relabel_speaker(label, name)?;
        localvox_light_core::diarize::roster::save(&dir, &roster)?;

        // Only the SUMMARY is written about «Участник 2» — it is prose, and the name is inside
        // its sentences. The readable text quotes nothing: it renders the caption from the very
        // lines just relabelled, so it already says the new name. The audio is not touched
        // either: re-cooking half an hour of sound for the sake of a name is a mockery, and then
        // names simply would not be used at all.
        let queued = self.requeue_named_summary(session, &dir)?;

        Ok(format!(
            "«{label}» → «{name}»: переподписано строк: {lines}{}",
            if queued {
                "; сводка пересобирается"
            } else {
                "; сводка ожидает расшифровки"
            }
        ))
    }

    /// Called while owning the session. A rename invalidates only prose containing names.
    pub(super) fn requeue_named_summary(&self, session: &str, dir: &Path) -> Result<bool> {
        for file in ["summary.md", "summary.unverified.md"] {
            match fs::remove_file(dir.join(file)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(e).context("имя сохранено, но устаревшую сводку не удалось удалить")
                }
            }
        }
        localvox_light_core::processing::forget(dir, &[localvox_light_core::processing::SUMMARY]);
        if VersionStore::open(dir)?.best().is_none() {
            return Ok(false);
        }
        localvox_light_core::artifacts::source_transcript(dir)
            .context("имя сохранено, но исходная расшифровка не прошла проверку")?;
        let queued = localvox_light_core::jobs::JobQueue::mutate(&self.work_dir, |queue| {
            queue.requeue_derivatives(session, true, false, false)
        });
        anyhow::ensure!(
            queued,
            "имя сохранено; повтор сводки не поставлен: задание занято или очередь недоступна"
        );
        Ok(true)
    }

    /// Legacy profiles remain readable/deletable; they no longer identify voices.
    pub fn known_speakers(&self) -> Vec<SpeakerOut> {
        localvox_light_core::diarize::profiles::list(&self.work_dir)
            .into_iter()
            .map(|(label, speech_sec)| SpeakerOut {
                label,
                speech_sec,
                owner: false,
            })
            .collect()
    }

    /// Forget a voice. A person MUST be able to erase biometrics — with one button.
    /// The transcripts are not rewritten in the process: what was said was said, and
    /// the caption under it is a fact of the recording. What is erased is the PRINT,
    /// that is, the ability to recognize this voice from now on.
    pub fn forget_speaker(&self, name: &str) -> Result<String> {
        if localvox_light_core::diarize::profiles::forget(&self.work_dir, name)? {
            Ok(format!("голос «{name}» забыт"))
        } else {
            Ok(format!("голоса «{name}» мы и не знали"))
        }
    }
}
