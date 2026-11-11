//! Feature operations behind the stable Archive API facade.
use super::*;

impl Archive {
    /// "This is bad — re-cook it." Throws away EVERYTHING derived (the summary, the
    /// cleaned-up text, the drafts) and puts the session up for a repeat cook with
    /// `--force`: a new transcript version, a new best, rebuilt indexes.
    ///
    /// The audio and `meta.json` are not touched — they are primary and inviolable,
    /// and everything else is recreated from them. That is why throwing away the
    /// derivatives is not a loss: it is the only way to let a person say "the model
    /// lied" and get the result anew instead of arguing with the archive.
    ///
    /// Old transcript versions stay in the manifest (version history — P2): `--force`
    /// adds a new one and makes it `best` instead of erasing the previous ones.
    /// Has the current run's progress not moved for longer than any normal quiet stretch (a summary
    /// ~4 min, a model load ~2.5 min)? With per-chunk/per-batch heartbeats a live cook updates every
    /// few seconds, so a long silence means a dead process. Shared by the progress view (to show
    /// «оборвалось») and by recook (to let a human override a stalled job).
    pub(super) fn progress_stale(&self, dir: &Path) -> bool {
        let stages = localvox_light_core::progress::fold(&localvox_light_core::progress::read(dir));
        let ts = |s: &str| {
            chrono::DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|t| t.timestamp())
        };
        let now = chrono::Local::now().timestamp();
        stages
            .iter()
            .filter_map(|s| s.updated_at.as_deref())
            .filter_map(ts)
            .max()
            .is_some_and(|t| now - t > COOK_STALL_SEC)
    }

    /// Resume an interrupted request; unlike recook, preserve all committed artifacts.
    pub fn retry(&self, session: &str) -> Result<String> {
        use localvox_light_core::jobs::{JobQueue, JobState};
        let dir = self.session_dir(session)?;
        self.checked_mutation_target(session, &dir)?;
        let _lease = localvox_light_core::jobs::control::MutationLease::try_processing(&dir)?
            .context("обработчик ещё занят; повторите после его остановки")?;
        JobQueue::mutate(&self.work_dir, |queue| -> Result<String> {
            if let Some(error) = queue.error() {
                return Err(QueueUnavailable(error.to_owned()).into());
            }
            let job = queue
                .jobs()
                .iter()
                .find(|job| job.session == session)
                .context("нет остановленного задания для повтора")?;
            anyhow::ensure!(
                matches!(job.state, JobState::Failed | JobState::Cancelled),
                "повтор этапа доступен только для остановленного задания"
            );
            let phase = job.phase;
            if job.kind != localvox_light_core::jobs::JobKind::Incremental {
                self.ensure_not_recording(session)?;
            }
            if !queue.retry_failed(session) {
                return Err(QueueUnavailable("не удалось сохранить повтор этапа".into()).into());
            }
            localvox_light_core::progress::new_run(&dir);
            Ok(format!(
                "повтор этапа {} поставлен в очередь; готовые результаты сохранены",
                phase.as_str()
            ))
        })
    }

    pub fn recook(&self, session: &str, scope: RecookScope) -> Result<String> {
        let dir = self.session_dir(session)?;
        let _lease = self.mutation_lease(session, &dir)?;
        self.recook_owned(session, scope)
    }

    /// The caller owns the session lease through queue commit and invalidation.
    pub(super) fn recook_owned(&self, session: &str, scope: RecookScope) -> Result<String> {
        use localvox_light_core::processing as proc;
        let dir = self.session_dir(session)?;
        // Preconditions first, destruction after.
        self.ensure_not_recording(session)?;

        let readable = localvox_light_core::readable::FILE;
        // Stale is a filesystem read on `&self` — do it BEFORE the lock so the critical section stays
        // short. Everything that TOUCHES the queue then happens in ONE locked, fresh-loaded section,
        // so an unlocked load+save here can neither clobber nor be clobbered by the autocook thread.
        let stale = self.progress_stale(&dir);

        // What each scope THROWS AWAY, what records it FORGETS (no file → no «done» record, or
        // discovery would decide there is nothing to remake), and how it re-queues. «Реплики» and
        // «Текст» are one artifact — processed.json — so both mean «cleanup + summary»; «Сводка» is
        // the leaf; «Всё» redoes the transcript too. Summary and Text re-queue WITHOUT force (the
        // transcript stays, only the missing derivatives are remade); All forces from the audio up.
        //
        // We queue the work FIRST and destroy afterwards: a queue refusal («already cooking») must
        // not leave the session stripped of its derivatives with no job to bring them back.
        let dropped = localvox_light_core::jobs::JobQueue::mutate(
            &self.work_dir,
            |queue| -> Result<Vec<&str>> {
                self.ensure_recookable(session, &dir, scope)?;
                // A human's «Переварить» must WIN over a stalled cook — a job stuck `Running` because
                // the daemon (or the cook) died mid-work. The enqueue methods refuse a Running job to
                // protect a LIVE cook; so if nothing has moved for a long time, we reclaim it first.
                // A genuinely-live cook is not stale and is still refused below — its own work
                // finishes, and «оборвалось, нажмите Переварить» does what it promises.
                if stale
                    && queue.jobs().iter().any(|j| {
                        j.session == session
                            && matches!(j.state, localvox_light_core::jobs::JobState::Running)
                    })
                {
                    queue.reclaim_session(session);
                }
                let (files, forget, queued): (Vec<&str>, Vec<&str>, bool) = match scope {
                    RecookScope::Summary => (
                        vec!["summary.md", "summary.unverified.md"],
                        vec![proc::SUMMARY],
                        queue.requeue_derivatives(session, true, false, false),
                    ),
                    RecookScope::Text => (
                        vec![
                            "summary.md",
                            readable,
                            "summary.unverified.md",
                            "processed.unverified.md",
                        ],
                        vec![proc::PROCESSED, proc::SUMMARY],
                        queue.requeue_derivatives(session, true, true, false),
                    ),
                    RecookScope::All => {
                        // Full redo uses the daemon's post-processing flags, from the SHARED place —
                        // our own reading of the same env once diverged, so «redo» erased the summary
                        // and queued a job that never remade it.
                        let (summary, cleanup, refine) =
                            localvox_light_core::jobs::post_processing_from_env();
                        (
                            vec![
                                "summary.md",
                                readable,
                                "summary.unverified.md",
                                "processed.unverified.md",
                            ],
                            vec![proc::SUMMARY, proc::PROCESSED, proc::TRANSCRIPT],
                            queue.enqueue_recook(session, summary, cleanup, refine),
                        )
                    }
                };
                if !queued {
                    anyhow::bail!("сессия уже обрабатывается или очередь не удалось сохранить");
                }
                // Keep the queue lock through invalidation: workers must not reuse the old files
                // between seeing the new Pending job and this request removing its derivatives.
                localvox_light_core::progress::new_run(&dir);
                let mut dropped = Vec::new();
                for f in files {
                    if fs::remove_file(dir.join(f)).is_ok() {
                        dropped.push(f);
                    }
                }
                proc::forget(&dir, &forget);
                Ok(dropped)
            },
        )?;

        tracing::info!(
            "re-cook on demand: {session} ({}, dropped: {dropped:?})",
            scope.label()
        );
        Ok(format!(
            "поставлено на переварку ({}); выброшено производных файлов: {}",
            scope.label(),
            dropped.len()
        ))
    }

    /// Whether this session may be re-cooked. Checked BEFORE any destruction —
    /// including before a language change: changing the language and then failing to
    /// re-cook means leaving the person with a summary in the language they have just
    /// rejected.
    pub(super) fn ensure_not_recording(&self, session: &str) -> Result<()> {
        // A live session MUST NOT be re-cooked: the cook would commit a PARTIAL
        // transcript with the current recipe, and discovery would consider it cooked
        // forever — the rest of the recording would never be transcribed. We check
        // against the marker, not against file freshness: while paused the files are
        // not updated, but the engine is alive and will finish writing the session.
        if localvox_light_core::jobs::recording_session(&self.work_dir).as_deref() == Some(session)
        {
            bail!("сессия записывается прямо сейчас — остановите запись, потом переваривайте");
        }
        Ok(())
    }

    pub(super) fn mutation_lease(
        &self,
        session: &str,
        dir: &Path,
    ) -> Result<localvox_light_core::jobs::control::MutationLease> {
        self.ensure_not_recording(session)?;
        self.checked_mutation_target(session, dir)?;
        localvox_light_core::jobs::control::MutationLease::try_acquire(dir)?
            .context("сессия занята: обработка ещё выполняется; изменения не применены")
    }

    pub(super) fn ensure_recookable(
        &self,
        session: &str,
        dir: &Path,
        scope: RecookScope,
    ) -> Result<()> {
        self.ensure_not_recording(session)?;
        match scope {
            RecookScope::All => {
                let files = localvox_light_core::artifacts::audio(dir)
                    .context("нет полного пригодного аудио для повторного распознавания")?;
                anyhow::ensure!(
                    !files.is_empty(),
                    "в сессии нет аудио для повторного распознавания"
                );
            }
            RecookScope::Text | RecookScope::Summary => {
                localvox_light_core::artifacts::source_transcript(dir)
                    .context("нет пригодной исходной расшифровки для повторной обработки текста")?;
            }
        }
        Ok(())
    }

    /// Delete a session ENTIRELY — audio and everything derived from it.
    ///
    /// This is the ONLY irreversible action in the whole system. Everywhere else the audio is the
    /// source of truth and the derivatives are disposable — a bad summary is re-cooked, a wrong
    /// language is redone, all from the audio. Here the audio itself goes, and nothing brings it
    /// back. So the guards are real, not decorative:
    ///
    ///  * a session being RECORDED right now is not deletable — stop it first, or the engine keeps
    ///    writing into a directory we just removed;
    ///  * the caller must pass the session's own name back as `confirm`. A generic "yes" is too
    ///    easy to click through; echoing the name means the deletion was aimed, not fat-fingered.
    ///
    /// The queue entry goes too — a job pointing at a directory that no longer exists would fail
    /// forever. The search index rebuilds itself from the session files, so the deleted session
    /// drops out of it on the next search with no extra work here.
    pub fn delete_session(&self, session: &str, confirm: &str) -> Result<String> {
        let dir = self.session_dir(session)?;
        if !dir.exists() {
            bail!("сессия не найдена — возможно, уже удалена");
        }
        if localvox_light_core::jobs::recording_session(&self.work_dir).as_deref() == Some(session)
        {
            bail!("сессия записывается прямо сейчас — сначала остановите запись");
        }
        if confirm != session {
            // Not a user-facing message: the UI names the session itself. This only fires if a
            // script calls the API by hand, and then it must say exactly what it wants.
            bail!("удаление не подтверждено: передайте имя сессии в поле confirm");
        }

        let _capture = localvox_light_core::chunks::capture::try_mutation_lock(&dir)?
            .context("запись ещё сохраняет аудио или план обработки; файлы не удалены")?;
        let _lease = self.stopped_lease(session, &dir, std::time::Duration::from_secs(5))?;
        std::fs::remove_dir_all(&dir).with_context(|| format!("не удалось удалить {session}"))?;
        // The cancellation tombstone was saved before touching files. A cleanup failure
        // cannot resurrect the removed source and is not a failed physical deletion.
        if let Err(error) = localvox_light_core::jobs::JobQueue::mutate(&self.work_dir, |queue| {
            queue.try_forget_session(session)
        }) {
            tracing::warn!("session deleted, queue cleanup pending: {error:#}");
        }
        tracing::info!("session deleted: {session}");
        Ok(session.to_string())
    }

    /// Change the language of the recording. `None` — go back to auto-detection.
    ///
    /// There is no need for separate code to reset the derivatives: the language is
    /// part of the recipes, so the transcript and the summary have already become
    /// "made the wrong way" and will be made anew. We only erase the derived files
    /// right away, without waiting for the queue — so that the person does not read a
    /// summary in a language they rejected.
    pub fn set_lang(&self, session: &str, lang: Option<&str>) -> Result<String> {
        let dir = self.session_dir(session)?;
        let _lease = self.mutation_lease(session, &dir)?;
        self.ensure_recookable(session, &dir, RecookScope::All)?;

        // The model for the language MUST exist BEFORE we destroy anything. Otherwise
        // one click in the UI looked like this: the language is written down, the
        // summary and the cleaned-up text are deleted, the answer is "ok, queued for
        // re-cook" — and the cook fails on "no model", and the artifacts do not come
        // back. The person was told "done", yet they lost the summary and will not
        // guess why.
        if let Some(code) = lang {
            let code = localvox_light_core::lang::normalize(code)?;
            let default_dir = localvox_light_core::lang::default_model_dir()
                .context("не найден каталог ASR-модели — укажите LOCALVOX_ASR_MODEL_DIR")?;
            if !localvox_light_core::lang::has_model(&code, &default_dir) {
                // The same text as the cook's: one cause — one explanation.
                localvox_light_core::lang::asr_model(&code, &default_dir)?;
            }
        }

        let previous_meta = fs::read(dir.join("meta.json"))?;
        if !localvox_light_core::lang::set(&dir, lang)? {
            return Ok("язык не изменился".into());
        }
        // A language change invalidates EVERYTHING (the transcript picks the model by language), so
        // it is always a full redo.
        let msg = match self.recook_owned(session, RecookScope::All) {
            Ok(msg) => msg,
            Err(e) => {
                localvox_light_core::artifacts::atomic_write(
                    &dir.join("meta.json"),
                    &previous_meta,
                )
                .with_context(|| {
                    format!("повтор не поставлен ({e:#}); не удалось восстановить прежний язык")
                })?;
                return Err(e);
            }
        };
        Ok(match lang {
            Some(code) => format!("язык: {code}; {msg}"),
            None => format!("язык: авто; {msg}"),
        })
    }
}
