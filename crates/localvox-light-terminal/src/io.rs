//! Separate bounded workers for control, documents, model queries and actions.
use crossbeam_channel::{bounded, Receiver, Sender};
use localvox_light_client::{types::*, Client, Scope};
use std::collections::{BTreeMap, HashSet};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Resource {
    Sessions,
    Record,
    Jobs,
    Queue,
    Summary,
    Transcript,
    Readable,
    Versions,
    Speakers,
    Sources,
    Languages,
    Progress,
    Peaks,
    Journal,
    Notes,
    Asks,
    Ask,
    Settings,
    Search,
    Chat,
    Action,
    Control,
    Shutdown,
    Route,
}
pub fn action_resource(task: &Task) -> Resource {
    match task {
        Task::Start(_) | Task::Stop => Resource::Control,
        Task::Shutdown => Resource::Shutdown,
        Task::Route(_) => Resource::Route,
        _ => Resource::Action,
    }
}
#[derive(Clone)]
pub enum Task {
    Read(Resource, String),
    Peaks(String, Option<String>),
    Journal(String, Option<String>, Option<String>),
    Search(String, String),
    Chat(String),
    Start(String),
    Stop,
    Shutdown,
    Ingest(String),
    Recook(String, Scope),
    Retry(String),
    Cancel(String),
    DeleteSession(String),
    Best(String, u32),
    Lang(String, String),
    Speaker(String, String, String),
    Source(String, u8, String),
    Confirm(String, String),
    Note(String, String),
    Route(String),
    DeleteNote(String, StoredNote),
    CreateAsk(AskRequest),
    CreateAskFile {
        path: String,
        provider: String,
        prompt: String,
        input_name: String,
    },
    DeleteAsk(String),
    SaveSettings(BTreeMap<String, Option<String>>),
    RenameParticipant(String, Participant, String),
    Copy(String),
    Export {
        path: String,
        text: String,
    },
}
#[derive(Clone, Debug)]
pub enum Data {
    Shutdown,
    Sessions(Vec<Session>),
    Record(RecordState),
    Jobs(Jobs),
    Queue(Vec<QueueItem>),
    Summary(Summary),
    Transcript(Vec<TranscriptLine>),
    TranscriptSnapshot(TranscriptDoc),
    Readable(ReadableDoc),
    Versions(Vec<Version>),
    Speakers(Vec<Participant>),
    Sources(Vec<SourceName>),
    Languages(Vec<String>),
    Progress(Progress),
    Peaks(Peaks),
    Journal(JournalPage, bool),
    Notes(Vec<SlotNotes>),
    Asks(Vec<AskRecord>),
    Ask(AskRecord),
    Settings(SettingsDoc),
    Search(Vec<Hit>),
    Chat(Answer),
    Reply(Reply),
    Route(Vec<RouteHint>),
}
#[derive(Clone)]
struct Work {
    id: u64,
    epoch: u64,
    resource: Resource,
    task: Task,
}
pub struct Response {
    pub id: u64,
    pub epoch: u64,
    pub resource: Resource,
    pub result: Result<Data, String>,
}
pub struct Io {
    tx: Sender<Work>,
    control: Sender<Work>,
    model: Sender<Work>,
    actions: Sender<Work>,
    epoch: Arc<AtomicU64>,
    pub rx: Receiver<Response>,
    pending: HashSet<(Resource, u64)>,
    next_id: u64,
}
impl Io {
    pub fn new(client: Client) -> anyhow::Result<Self> {
        use anyhow::Context;
        let (tx, requests) = bounded::<Work>(16);
        let (control, control_requests) = bounded::<Work>(4);
        let (model, model_requests) = bounded::<Work>(8);
        let (actions, action_requests) = bounded::<Work>(4);
        let epoch = Arc::new(AtomicU64::new(0));
        let (responses, rx) = bounded(32);
        for index in 0..5 {
            let requests = match index {
                0 => control_requests.clone(),
                1 => requests.clone(),
                2 => action_requests.clone(),
                _ => model_requests.clone(),
            };
            let responses = responses.clone();
            let client = client.clone();
            let epoch = epoch.clone();
            std::thread::Builder::new()
                .name(format!("terminal-http-{index}"))
                .spawn(move || {
                    while let Ok(w) = requests.recv() {
                        let result = if w.epoch != 0 && w.epoch != epoch.load(Ordering::Relaxed) {
                            Err("Контекст запроса изменился".into())
                        } else {
                            perform(&client, w.task).map_err(|e| format!("{e:#}"))
                        };
                        if responses
                            .send(Response {
                                id: w.id,
                                epoch: w.epoch,
                                resource: w.resource,
                                result,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .context("Не удалось создать I/O-поток терминального клиента")?;
        }
        Ok(Self {
            tx,
            control,
            model,
            actions,
            epoch,
            rx,
            pending: HashSet::new(),
            next_id: 0,
        })
    }
    pub fn submit(&mut self, epoch: u64, resource: Resource, task: Task) -> bool {
        if epoch != 0 {
            if epoch < self.epoch.load(Ordering::Relaxed) {
                return false;
            }
            self.set_epoch(epoch);
        }
        if self.pending.contains(&(resource, epoch))
            || (!matches!(
                resource,
                Resource::Record | Resource::Control | Resource::Shutdown
            ) && self.pending.len() >= 18)
        {
            return false;
        }
        let id = self.next_id.wrapping_add(1);
        let sender = if matches!(
            resource,
            Resource::Record | Resource::Control | Resource::Shutdown
        ) {
            &self.control
        } else if matches!(
            task,
            Task::Chat(_)
                | Task::Search(..)
                | Task::Route(_)
                | Task::Read(Resource::Peaks, _)
                | Task::Peaks(..)
        ) {
            &self.model
        } else if matches!(task, Task::Read(..) | Task::Journal(..)) {
            &self.tx
        } else {
            &self.actions
        };
        if sender
            .try_send(Work {
                id,
                epoch,
                resource,
                task,
            })
            .is_err()
        {
            return false;
        }
        self.next_id = id;
        self.pending.insert((resource, epoch));
        true
    }
    pub fn finished(&mut self, resource: Resource, epoch: u64) {
        self.pending.remove(&(resource, epoch));
    }
    pub fn set_epoch(&mut self, epoch: u64) {
        if epoch > self.epoch.load(Ordering::Relaxed) {
            self.epoch.store(epoch, Ordering::Relaxed);
            self.pending
                .retain(|(_, generation)| *generation == 0 || *generation == epoch);
        }
    }
}
pub fn perform(c: &Client, task: Task) -> anyhow::Result<Data> {
    Ok(match task {
        Task::Peaks(s, snapshot) => Data::Peaks(if snapshot.is_some() {
            c.peaks_snapshot(&s, 160, snapshot.as_deref())?
        } else {
            c.peaks(&s, 160)?
        }),
        Task::Journal(s, after, before) => Data::Journal(
            c.journal(&s, after.as_deref(), before.as_deref())?,
            before.is_some(),
        ),
        Task::Read(r, s) => match r {
            Resource::Sessions => Data::Sessions(c.sessions()?),
            Resource::Record => Data::Record(c.record()?),
            Resource::Jobs => Data::Jobs(c.jobs()?),
            Resource::Queue => Data::Queue(c.queue()?),
            Resource::Summary => Data::Summary(c.summary(&s)?),
            Resource::Transcript => Data::TranscriptSnapshot(c.transcript_document(&s)?),
            Resource::Readable => Data::Readable(c.readable(&s)?),
            Resource::Versions => Data::Versions(c.versions(&s)?),
            Resource::Speakers => Data::Speakers(c.speakers(&s)?),
            Resource::Sources => Data::Sources(c.sources(&s)?),
            Resource::Languages => Data::Languages(c.langs()?),
            Resource::Progress => Data::Progress(c.progress(&s)?),
            Resource::Peaks => Data::Peaks(c.peaks(&s, 160)?),
            Resource::Notes => Data::Notes(c.slot_notes()?),
            Resource::Asks => Data::Asks(c.asks()?),
            Resource::Ask => Data::Ask(c.get_ask(&s)?),
            Resource::Settings => Data::Settings(c.settings()?),
            _ => anyhow::bail!("Неизвестный ресурс"),
        },
        Task::Search(q, mode) => Data::Search(c.search(&q, &mode)?),
        Task::Chat(q) => Data::Chat(c.ask(&q)?),
        Task::Start(title) => Data::Reply(c.start_recording(&title)?),
        Task::Stop => Data::Reply(c.stop_recording()?),
        Task::Shutdown => {
            c.shutdown_application()?;
            Data::Shutdown
        }
        Task::Ingest(url) => Data::Reply(c.ingest(&url)?),
        Task::Recook(s, scope) => Data::Reply(c.recook(&s, scope)?),
        Task::Retry(s) => Data::Reply(c.retry(&s)?),
        Task::Cancel(s) => Data::Reply(c.cancel(&s)?),
        Task::DeleteSession(s) => Data::Reply(c.delete_session(&s)?),
        Task::Best(s, id) => {
            c.set_best(&s, id)?;
            Data::Reply(Reply {
                msg: Some("Рабочая версия выбрана".into()),
                ..Default::default()
            })
        }
        Task::Lang(s, lang) => Data::Reply(c.set_lang(&s, &lang)?),
        Task::Speaker(s, voice, name) => Data::Reply(c.name_speaker(&s, &voice, &name)?),
        Task::Source(s, id, name) => Data::Reply(c.name_source(&s, id, &name)?),
        Task::Confirm(s, artifact) => Data::Reply(c.confirm(&s, &artifact)?),
        Task::Note(text, slot) => Data::Reply(c.note(&text, &slot)?),
        Task::Route(text) => Data::Route(c.route(&text)?),
        Task::DeleteNote(slot, note) => Data::Reply(c.delete_note(&slot, &note)?),
        Task::CreateAsk(request) => Data::Ask(c.create_ask(&request)?),
        Task::CreateAskFile {
            path,
            provider,
            prompt,
            input_name,
        } => {
            use std::io::Read;
            let file = std::fs::File::open(&path)
                .map_err(|_| anyhow::anyhow!("Не удалось открыть выбранный текстовый файл"))?;
            let mut bytes = Vec::new();
            file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
            anyhow::ensure!(bytes.len() <= 1024 * 1024, "Файл превышает 1 MiB");
            let text = String::from_utf8(bytes).map_err(|_| {
                anyhow::anyhow!("Нужен UTF-8 текстовый файл; PDF/DOCX не извлекаются")
            })?;
            let text = text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned();
            anyhow::ensure!(
                !text.trim().is_empty() && text.chars().count() <= 400_000,
                "Нужно от 1 до 400000 символов"
            );
            let input_name = if input_name.is_empty() {
                std::path::Path::new(&path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            } else {
                input_name
            };
            Data::Ask(c.create_ask(&AskRequest {
                text,
                provider,
                prompt,
                input_name: Some(input_name),
            })?)
        }
        Task::DeleteAsk(id) => Data::Reply(c.delete_ask(&id)?),
        Task::SaveSettings(set) => Data::Reply(c.save_settings(set)?),
        Task::RenameParticipant(session, participant, name) => {
            // A participant may have several underlying labels. Do not silently rename just one.
            // On failure there is no retry: refresh shows any already applied labels.
            for voice in &participant.voices {
                c.name_speaker(&session, voice, &name)?;
            }
            for source in &participant.sources {
                c.name_source(&session, *source, &name)?;
            }
            Data::Reply(Reply {
                msg: Some("Имя участника сохранено в этой записи".into()),
                ..Default::default()
            })
        }
        Task::Copy(text) => {
            thread_local! {static CLIPBOARD:std::cell::RefCell<Option<arboard::Clipboard>>=const{std::cell::RefCell::new(None)};}
            CLIPBOARD.with(|slot| -> anyhow::Result<()> {
                let mut clipboard = slot.borrow_mut();
                if clipboard.is_none() {
                    *clipboard = Some(arboard::Clipboard::new().map_err(|_| {
                        anyhow::anyhow!("Буфер обмена недоступен; документ сохранён на экране")
                    })?);
                }
                clipboard.as_mut().unwrap().set_text(text).map_err(|_| {
                    anyhow::anyhow!("Не удалось скопировать; документ сохранён на экране")
                })?;
                Ok(())
            })?;
            Data::Reply(Reply {
                msg: Some("Скопировано в буфер обмена".into()),
                ..Default::default()
            })
        }
        Task::Export { path, text } => {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(|_| {
                    anyhow::anyhow!(
                        "Не удалось создать файл; выберите новый путь без перезаписи существующего"
                    )
                })?;
            file.write_all(text.as_bytes())?;
            file.flush()?;
            Data::Reply(Reply {
                msg: Some("Документ сохранён".into()),
                ..Default::default()
            })
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_queue_rejects_work_without_blocking_and_reserves_control() {
        let (tx, requests) = bounded(1);
        let (control, control_requests) = bounded(1);
        let (model, _model_requests) = bounded(1);
        let (actions, _action_requests) = bounded(1);
        let (_, rx) = bounded(1);
        let mut io = Io {
            tx,
            control,
            model,
            actions,
            epoch: Arc::new(AtomicU64::new(0)),
            rx,
            pending: HashSet::new(),
            next_id: 0,
        };
        assert!(io.submit(
            1,
            Resource::Summary,
            Task::Read(Resource::Summary, "a".into())
        ));
        assert!(!io.submit(
            2,
            Resource::Transcript,
            Task::Read(Resource::Transcript, "b".into())
        ));
        assert!(!io.submit(
            1,
            Resource::Summary,
            Task::Read(Resource::Summary, "a".into())
        ));
        assert!(io.submit(0, Resource::Control, Task::Stop));
        assert_eq!(requests.len(), 1);
        assert_eq!(control_requests.len(), 1);
        requests.recv().unwrap();
        io.finished(Resource::Summary, 1);
        assert!(io.submit(
            2,
            Resource::Summary,
            Task::Read(Resource::Summary, "b".into())
        ));
        io.finished(Resource::Summary, 1);
        assert!(!io.submit(
            2,
            Resource::Summary,
            Task::Read(Resource::Summary, "b".into())
        ));
    }
    #[test]
    fn export_refuses_to_overwrite_and_reports_a_real_io_error() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("document.txt");
        let client = Client::new("http://127.0.0.1:1", String::new()).unwrap();
        perform(
            &client,
            Task::Export {
                path: path.to_string_lossy().into(),
                text: "проверенный документ".into(),
            },
        )
        .unwrap();
        assert!(perform(
            &client,
            Task::Export {
                path: path.to_string_lossy().into(),
                text: "overwrite".into()
            }
        )
        .is_err());
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "проверенный документ"
        );
    }
}
