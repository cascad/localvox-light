//! Bounded framed result journal. Every frame is synced before becoming reusable.
//! JSON has no literal NUL, so a damaged length/tail can resync at the next magic.
use super::*;
use std::io::{Seek, SeekFrom, Write};

const MAGIC: &[u8; 8] = b"\0LVXASR1";
const HEADER: usize = 108;
const MAX_RECORD: usize = 8 * 1024 * 1024;
const MAX_SEGMENT: u64 = 16 * 1024 * 1024;
pub(super) const MAX_GROUP_RECORDS: usize = 8;
#[derive(Clone, Copy, Default, Serialize)]
pub struct JournalWriteStats {
    pub frames: u64,
    pub bytes: u64,
    pub syncs: u64,
    pub write_sec: f64,
    pub sync_sec: f64,
    pub max_sync_sec: f64,
}
#[derive(Clone)]
struct Location {
    segment: u32,
    offset: u64,
}
pub(super) struct Store {
    root: PathBuf,
    index: BTreeMap<String, Location>,
    current: u32,
    writer: Option<File>,
    directory_synced: bool,
    durable_segments: std::collections::BTreeSet<u32>,
    stats: JournalWriteStats,
}
fn key_bytes(key: &str) -> Result<[u8; 32]> {
    ensure!(is_hash(key), "invalid result journal key");
    let mut bytes = [0; 32];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = u8::from_str_radix(&key[i * 2..i * 2 + 2], 16)?;
    }
    Ok(bytes)
}
fn frame(key: &str, bytes: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        bytes.len() <= MAX_RECORD,
        "result journal record exceeds limit"
    );
    ensure!(
        !bytes.contains(&0),
        "result journal payload must be JSON without literal NUL"
    );
    let mut frame = Vec::with_capacity(HEADER + bytes.len());
    frame.extend_from_slice(MAGIC);
    frame.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    frame.extend_from_slice(&key_bytes(key)?);
    frame.extend_from_slice(&Sha256::digest(bytes));
    let header_checksum = Sha256::digest(&frame);
    frame.extend_from_slice(&header_checksum);
    frame.extend_from_slice(bytes);
    Ok(frame)
}
impl Store {
    pub fn open(root: PathBuf) -> Result<Self> {
        let created = !root.exists();
        fs::create_dir_all(&root)?;
        #[cfg(unix)]
        if created {
            File::open(root.parent().context("result journal parent")?)?.sync_all()?;
        }
        #[cfg(not(unix))]
        let _ = created;
        let mut store = Self {
            root,
            index: BTreeMap::new(),
            current: 0,
            writer: None,
            directory_synced: false,
            durable_segments: Default::default(),
            stats: JournalWriteStats::default(),
        };
        store.refresh()?;
        Ok(store)
    }
    fn path(&self, number: u32) -> PathBuf {
        self.root.join(format!("{number:08x}.wal"))
    }
    pub fn refresh(&mut self) -> Result<()> {
        self.writer = None;
        self.directory_synced = false;
        self.index.clear();
        self.durable_segments.clear();
        let mut segments = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path();
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if path.extension().and_then(|s| s.to_str()) != Some("wal")
                || stem.len() != 8
                || !entry.file_type()?.is_file()
            {
                continue;
            }
            if let Ok(number) = u32::from_str_radix(stem, 16) {
                segments.push(number);
            }
            ensure!(segments.len() <= 4096, "too many result journal segments");
        }
        segments.sort_unstable();
        self.current = segments.last().copied().unwrap_or(0);
        for segment in segments {
            let bytes = bytes_file(&self.path(segment), MAX_SEGMENT)?;
            let mut starts = bytes
                .windows(MAGIC.len())
                .enumerate()
                .filter_map(|(at, b)| (b == MAGIC).then_some(at))
                .peekable();
            while let Some(at) = starts.next() {
                if at + HEADER > bytes.len() {
                    break;
                }
                if Sha256::digest(&bytes[at..at + 76])[..] != bytes[at + 76..at + 108] {
                    continue;
                }
                let len = u32::from_le_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
                let end = at.checked_add(HEADER).and_then(|n| n.checked_add(len));
                if len > MAX_RECORD || end.is_none_or(|end| end > bytes.len()) {
                    continue;
                }
                let end = end.unwrap();
                while starts.peek().is_some_and(|next| *next < at + HEADER) {
                    starts.next();
                }
                if starts.peek().is_some_and(|next| *next < end) {
                    continue;
                }
                let payload = &bytes[at + HEADER..end];
                if Sha256::digest(payload)[..] != bytes[at + 44..at + 76] {
                    continue;
                }
                let key = bytes[at + 12..at + 44]
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>();
                self.index.insert(
                    key,
                    Location {
                        segment,
                        offset: at as u64,
                    },
                );
                ensure!(
                    self.index.len() <= 262144,
                    "result journal index exceeds limit"
                );
            }
        }
        Ok(())
    }
    pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let Some(location) = self.index.get(key) else {
            return Ok(None);
        };
        let mut file = match File::open(self.path(location.segment)) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        file.seek(SeekFrom::Start(location.offset))?;
        let mut header = [0; HEADER];
        if let Err(e) = file.read_exact(&mut header) {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                return Ok(None);
            }
            return Err(e.into());
        }
        let len = u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize;
        if &header[..8] != MAGIC
            || Sha256::digest(&header[..76])[..] != header[76..108]
            || header[12..44] != key_bytes(key)?
            || len > MAX_RECORD
        {
            return Ok(None);
        }
        let mut bytes = vec![0; len];
        if let Err(e) = file.read_exact(&mut bytes) {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                return Ok(None);
            }
            return Err(e.into());
        }
        if Sha256::digest(&bytes)[..] != header[44..76] {
            return Ok(None);
        }
        Ok(Some(bytes))
    }
    #[cfg(test)]
    pub fn put(&mut self, key: &str, bytes: &[u8]) -> Result<()> {
        self.put_many(&[(key, bytes)])
    }
    pub fn put_many(&mut self, entries: &[(&str, &[u8])]) -> Result<()> {
        self.put_many_with_sync(entries, |file| file.sync_all())
    }
    fn put_many_with_sync(
        &mut self,
        entries: &[(&str, &[u8])],
        mut sync: impl FnMut(&File) -> std::io::Result<()>,
    ) -> Result<()> {
        ensure!(
            entries.len() <= MAX_GROUP_RECORDS,
            "result journal group exceeds limit"
        );
        // Validate the complete bounded batch before any bytes are written.
        let frames: Vec<_> = entries
            .iter()
            .map(|(key, bytes)| Ok(((*key).to_owned(), frame(key, bytes)?)))
            .collect::<Result<_>>()?;
        let new_keys: std::collections::BTreeSet<_> = entries
            .iter()
            .map(|(key, _)| *key)
            .filter(|key| !self.index.contains_key(*key))
            .collect();
        ensure!(
            self.index.len() + new_keys.len() <= 262144,
            "result journal index exceeds limit"
        );
        let mut next = 0;
        while next < frames.len() {
            let path = self.path(self.current);
            if self.writer.is_none() || !path.is_file() {
                self.writer = Some(
                    fs::OpenOptions::new()
                        .read(true)
                        .append(true)
                        .create(true)
                        .open(&path)?,
                );
                self.directory_synced = false;
            }
            let file = self.writer.as_ref().unwrap();
            file.try_lock()
                .context("result journal already has a writer")?;
            struct Unlock<'a>(&'a File);
            impl Drop for Unlock<'_> {
                fn drop(&mut self) {
                    let _ = self.0.unlock();
                }
            }
            let lock = Unlock(file);
            let mut offset = file.metadata()?.len();
            if offset + frames[next].1.len() as u64 > MAX_SEGMENT {
                drop(lock);
                self.writer = None;
                self.current = self
                    .current
                    .checked_add(1)
                    .context("result journal segment id exhausted")?;
                ensure!(self.current < 4096, "result journal segment limit exceeded");
                continue;
            }
            let mut written = Vec::new();
            while next < frames.len() && offset + frames[next].1.len() as u64 <= MAX_SEGMENT {
                let (key, bytes) = &frames[next];
                let started = std::time::Instant::now();
                let write = (&*file).write_all(bytes);
                self.stats.write_sec += started.elapsed().as_secs_f64();
                write?;
                self.stats.frames += 1;
                self.stats.bytes += bytes.len() as u64;
                written.push((
                    key.clone(),
                    Location {
                        segment: self.current,
                        offset,
                    },
                ));
                offset += bytes.len() as u64;
                next += 1;
            }
            let started = std::time::Instant::now();
            let flushed = sync(file);
            let elapsed = started.elapsed().as_secs_f64();
            self.stats.sync_sec += elapsed;
            self.stats.max_sync_sec = self.stats.max_sync_sec.max(elapsed);
            self.stats.syncs += 1;
            flushed?;
            if !self.directory_synced {
                #[cfg(unix)]
                {
                    File::open(&self.root)?.sync_all()?;
                }
                self.directory_synced = true;
            }
            // No new location is reusable before the entire segment group is durable.
            self.index.extend(written);
            self.durable_segments.insert(self.current);
            drop(lock);
        }
        Ok(())
    }
    /// Orphan frames may be complete after a refused flush. A fresh reader must
    /// confirm their segment before returning them as accepted work.
    pub fn ensure_durable(&mut self, key: &str) -> Result<()> {
        self.ensure_durable_with_sync(key, |file| file.sync_all())
    }
    fn ensure_durable_with_sync(
        &mut self,
        key: &str,
        mut sync: impl FnMut(&File) -> std::io::Result<()>,
    ) -> Result<()> {
        let segment = self
            .index
            .get(key)
            .context("result journal entry missing")?
            .segment;
        if self.durable_segments.contains(&segment) {
            return Ok(());
        }
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(self.path(segment))?;
        let started = std::time::Instant::now();
        let result = sync(&file);
        let elapsed = started.elapsed().as_secs_f64();
        self.stats.sync_sec += elapsed;
        self.stats.max_sync_sec = self.stats.max_sync_sec.max(elapsed);
        self.stats.syncs += 1;
        result?;
        if !self.directory_synced {
            #[cfg(unix)]
            {
                File::open(&self.root)?.sync_all()?;
            }
            self.directory_synced = true;
        }
        self.durable_segments.insert(segment);
        Ok(())
    }
    #[cfg(any(feature = "onnx", test))]
    pub fn stats(&self) -> JournalWriteStats {
        self.stats
    }
    #[cfg(test)]
    pub(super) fn put_refusing_sync(&mut self, entries: &[(&str, &[u8])]) -> Result<()> {
        self.put_many_with_sync(entries, |_| {
            Err(std::io::Error::other("injected sync refusal"))
        })
    }
    #[cfg(test)]
    pub fn location(&self, key: &str) -> PathBuf {
        self.path(self.index.get(key).unwrap().segment)
    }
    #[cfg(test)]
    pub fn replace(&mut self, key: &str, payload: &[u8]) -> Result<()> {
        let loc = self.index.get(key).unwrap().clone();
        let path = self.path(loc.segment);
        let mut bytes = fs::read(&path)?;
        let at = loc.offset as usize;
        let len = u32::from_le_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
        bytes.splice(at..at + HEADER + len, frame(key, payload)?);
        fs::write(path, bytes)?;
        self.refresh()
    }
}

#[cfg(test)]
mod tests;
