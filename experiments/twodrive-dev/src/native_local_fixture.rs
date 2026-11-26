//! Opt-in generated-provider host using the actual engine cache bridge and OS
//! callbacks. No account/profile, network or existing sync root is accepted.
use crate::{
    local_files::{LocalFileBridge, digest_file},
    model::{CloudBackend, DeltaResult, MetadataEntry},
    provider::{Capabilities, Progress, Provider},
};
use anyhow::{Context, ensure};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use twodrive_core::{Database, FileState};
use twodrive_local_files::fixtures::{Dataset, DownloadCounters};
use twodrive_local_files::native::{self, HydrationSource, NativeObservation, Session};

const SIZE: usize = 65536;
#[cfg(test)]
fn objects() -> Vec<MetadataEntry> {
    objects_for(Dataset::StandardV1)
}
fn objects_for(dataset: Dataset) -> Vec<MetadataEntry> {
    dataset
        .files()
        .into_iter()
        .map(|entry| {
            MetadataEntry::new_file(
                entry.id,
                format!("/{}", entry.relative_path),
                SIZE as u64,
                1,
                dataset.version(),
            )
        })
        .collect()
}
#[cfg(test)]
fn generated_bytes(index: usize) -> Vec<u8> {
    Dataset::StandardV1.bytes(index)
}
fn object_for(dataset: Dataset, id: &str) -> anyhow::Result<(usize, MetadataEntry)> {
    objects_for(dataset)
        .into_iter()
        .enumerate()
        .find(|(_, m)| m.remote_id == id)
        .context("unknown generated object")
}
fn metadata_for(dataset: Dataset) -> Vec<MetadataEntry> {
    dataset
        .directories()
        .into_iter()
        .map(|e| {
            MetadataEntry::new_dir(e.id, format!("/{}", e.relative_path), 1, dataset.version())
        })
        .chain(objects_for(dataset))
        .collect()
}
pub(crate) struct GeneratedProvider {
    pub(crate) dataset: Dataset,
    pub(crate) counters: Arc<DownloadCounters>,
}
impl CloudBackend for GeneratedProvider {
    fn is_conflict_error(&self, _: &anyhow::Error) -> bool {
        false
    }
    fn upload_file_with_version(
        &self,
        _: &str,
        _: &Path,
        _: Option<&str>,
        _: Option<&str>,
        _: &mut dyn FnMut(u64, u64) -> anyhow::Result<()>,
    ) -> anyhow::Result<MetadataEntry> {
        anyhow::bail!("generated provider is read-only")
    }
    fn list_all(&self) -> anyhow::Result<Vec<MetadataEntry>> {
        Ok(metadata_for(self.dataset))
    }
    fn list_delta(&self, _: Option<&str>) -> anyhow::Result<DeltaResult> {
        Ok(DeltaResult {
            entries: metadata_for(self.dataset),
            deleted_remote_ids: vec![],
            delta_link: None,
        })
    }
    fn get_metadata(&self, id: &str) -> anyhow::Result<Option<MetadataEntry>> {
        Ok(metadata_for(self.dataset)
            .into_iter()
            .find(|m| m.remote_id == id))
    }
    fn get_metadata_by_path(&self, path: &str) -> anyhow::Result<Option<MetadataEntry>> {
        Ok(metadata_for(self.dataset)
            .into_iter()
            .find(|m| m.path == path))
    }
    fn download(&self, id: &str) -> anyhow::Result<Vec<u8>> {
        Ok(self.dataset.bytes(object_for(self.dataset, id)?.0))
    }
    fn download_to(
        &self,
        id: &str,
        writer: &mut dyn Write,
        progress: &mut dyn FnMut(u64) -> anyhow::Result<()>,
    ) -> anyhow::Result<u64> {
        let bytes = self.download(id)?;
        let file_index = object_for(self.dataset, id)?.0;
        for (index, chunk) in bytes.chunks(4096).enumerate() {
            progress((index * 4096) as u64)?;
            writer.write_all(chunk)?;
            self.counters.provider_written(file_index, chunk.len());
        }
        progress(SIZE as u64)?;
        Ok(SIZE as u64)
    }
    fn upload(&self, _: &str, _: Vec<u8>) -> anyhow::Result<MetadataEntry> {
        anyhow::bail!("generated provider is read-only")
    }
    fn upload_with_etag(
        &self,
        _: &str,
        _: Vec<u8>,
        _: Option<&str>,
    ) -> anyhow::Result<MetadataEntry> {
        anyhow::bail!("generated provider is read-only")
    }
    fn create_folder(&self, _: &str) -> anyhow::Result<MetadataEntry> {
        anyhow::bail!("mkdir unsupported")
    }
    fn rename(&self, _: &str, _: &str) -> anyhow::Result<MetadataEntry> {
        anyhow::bail!("rename unsupported")
    }
    fn delete(&self, _: &str) -> anyhow::Result<()> {
        anyhow::bail!("delete unsupported")
    }
}
impl Provider for GeneratedProvider {
    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }
    fn list_directory(&self, path: &str) -> anyhow::Result<Vec<MetadataEntry>> {
        Ok(metadata_for(self.dataset)
            .into_iter()
            .filter(|m| m.parent_path == path)
            .collect())
    }
    fn upload_controlled(
        &self,
        _: &str,
        _: &Path,
        _: Option<&str>,
        _: Option<&str>,
        _: Progress,
    ) -> anyhow::Result<MetadataEntry> {
        anyhow::bail!("generated provider is read-only")
    }
}

struct EngineSource {
    bridge: LocalFileBridge,
    gate: Mutex<()>,
    offline: AtomicBool,
    failed: Mutex<HashSet<String>>,
    reading: Mutex<HashSet<String>>,
    dataset: Dataset,
    counters: Arc<DownloadCounters>,
}
impl HydrationSource for EngineSource {
    fn size(&self, identity: &[u8]) -> Result<usize, String> {
        object_for(
            self.dataset,
            std::str::from_utf8(identity).map_err(|_| "invalid identity")?,
        )
        .map(|(_, m)| m.size as usize)
        .map_err(|e| e.to_string())
    }
    fn read_range(
        &self,
        identity: &[u8],
        offset: usize,
        length: usize,
        cancelled: &AtomicBool,
    ) -> Result<Vec<u8>, String> {
        let id = std::str::from_utf8(identity).map_err(|_| "invalid identity")?;
        object_for(self.dataset, id).map_err(|e| e.to_string())?;
        self.reading
            .lock()
            .map_err(|_| "read state poisoned")?
            .insert(id.into());
        let result = self
            .read(identity, offset, length, cancelled)
            .map_err(|e| e.to_string());
        self.reading
            .lock()
            .map_err(|_| "read state poisoned")?
            .remove(id);
        let mut failed = self.failed.lock().map_err(|_| "error state poisoned")?;
        if result.is_err() {
            failed.insert(id.into());
        } else {
            failed.remove(id);
        }
        result
    }
}
impl EngineSource {
    fn read(
        &self,
        identity: &[u8],
        offset: usize,
        length: usize,
        cancelled: &AtomicBool,
    ) -> anyhow::Result<Vec<u8>> {
        let _gate = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("source gate poisoned"))?;
        let id = std::str::from_utf8(identity)?;
        let file_index = object_for(self.dataset, id)?.0;
        ensure!(
            length <= 65536 && offset.checked_add(length).is_some_and(|end| end <= SIZE),
            "bounded range required"
        );
        ensure!(!cancelled.load(Ordering::SeqCst), "read cancelled");
        ensure!(
            !self.bridge.journal().native_dirty(id)?,
            "native edit requires recovery"
        );
        let record = self
            .bridge
            .journal()
            .database()
            .get_by_cloud_remote_id(id)?;
        let local_id = if let Some(record) = record.filter(|r| r.cache_path.is_some()) {
            record.metadata.remote_id
        } else {
            ensure!(
                !self.offline.load(Ordering::SeqCst),
                "generated provider offline"
            );
            self.bridge.hydrate_cancellable(id, cancelled)?
        };
        let state = self.bridge.status(&local_id)?;
        ensure!(
            state.state == FileState::Cached
                && !state.dirty_intent
                && !state.recovery_required
                && !state.uncertain_upload
                && !state.pending_operations,
            "cache is not authoritative"
        );
        let mut file = self.bridge.journal().open_read(&local_id)?;
        let expected = file.acknowledged_digest().to_owned();
        if crate::local_files::digest_reader(&mut file)? != expected {
            drop(file);
            self.bridge
                .journal()
                .record_external_edit(&local_id, state.generation)?;
            anyhow::bail!("private cache bytes changed; preserved for recovery");
        }
        file.seek(SeekFrom::Start(offset as u64))?;
        let mut bytes = vec![0; length];
        file.read_exact(&mut bytes)?;
        self.counters.cache_read(file_index, bytes.len());
        ensure!(!cancelled.load(Ordering::SeqCst), "read cancelled");
        Ok(bytes)
    }
}

struct PhaseLog {
    file: fs::File,
    sequence: u64,
    bytes: u64,
    last: Vec<u8>,
    last_at: Instant,
}
impl PhaseLog {
    fn create(path: &Path) -> anyhow::Result<Self> {
        use std::os::windows::fs::OpenOptionsExt;
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(windows_share_read())
            .open(path)?;
        Ok(Self {
            file,
            sequence: 0,
            bytes: 0,
            last: Vec::new(),
            last_at: Instant::now(),
        })
    }
    fn record(
        &mut self,
        event: &str,
        snapshot: &Value,
        success: bool,
        force: bool,
    ) -> anyhow::Result<()> {
        ensure!(
            !event.is_empty()
                && event.len() <= 40
                && event.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
            "invalid observation event"
        );
        let fingerprint = serde_json::to_vec(snapshot)?;
        if !force && self.last == fingerprint && self.last_at.elapsed() < Duration::from_secs(5) {
            return Ok(());
        }
        ensure!(
            self.sequence < 10240 && self.bytes < 32 * 1024 * 1024,
            "observation log limit reached"
        );
        let utc_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis() as u64;
        let mut line = serde_json::to_vec(
            &json!({"schema_version":1,"sequence":self.sequence+1,"utc_ms":utc_ms,"event":event,"success":success,"snapshot":snapshot}),
        )?;
        line.push(b'\n');
        ensure!(
            self.bytes + line.len() as u64 <= 32 * 1024 * 1024,
            "observation log byte limit reached"
        );
        self.file.write_all(&line)?;
        self.file.sync_data()?;
        self.sequence += 1;
        self.bytes += line.len() as u64;
        self.last = fingerprint;
        self.last_at = Instant::now();
        Ok(())
    }
}
const fn windows_share_read() -> u32 {
    1
}

/// This service owns Session on its creating thread; raw OS handles are never
/// unsafely marked Send. A small channel is the engine/desktop boundary.
pub struct FixtureService {
    stopped: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    requests: Option<mpsc::SyncSender<Command>>,
    worker: Option<thread::JoinHandle<()>>,
}
trait FixtureOwner {
    fn command(&mut self, op: &str, id: &str, value: bool) -> anyhow::Result<Value>;
    fn tick(&mut self);
    fn serving(&self) -> bool {
        true
    }
    fn finished(&self) -> bool {
        false
    }
}
struct Command {
    op: String,
    id: String,
    value: bool,
    reply: mpsc::SyncSender<anyhow::Result<Value>>,
}
impl FixtureService {
    /// Explicit user-authorized new root only. State must be a NEW sibling of
    /// that root, not the desktop profile/cache or another client's data.
    pub fn start(root: PathBuf) -> anyhow::Result<Self> {
        ensure!(
            root.is_absolute() && !root.exists(),
            "new absolute generated root required"
        );
        Self::start_owner(move || Host::start(&root))
    }
    pub fn start_visual(root: PathBuf) -> anyhow::Result<Self> {
        ensure!(
            root.is_absolute() && !root.exists(),
            "new absolute generated root required"
        );
        Self::start_owner(move || Host::start_dataset(&root, Dataset::VisualV1))
    }
    fn start_owner<H: FixtureOwner + 'static>(
        create: impl FnOnce() -> anyhow::Result<H> + Send + 'static,
    ) -> anyhow::Result<Self> {
        let (requests, incoming) = mpsc::sync_channel::<Command>(4);
        let (ready, started) = mpsc::sync_channel(1);
        let running = Arc::new(AtomicBool::new(true));
        let stopped = Arc::new(AtomicBool::new(false));
        let owner_stopped = stopped.clone();
        let owner_running = running.clone();
        let worker = thread::spawn(move || {
            struct Running(Arc<AtomicBool>);
            impl Drop for Running {
                fn drop(&mut self) {
                    self.0.store(false, Ordering::SeqCst);
                }
            }
            let _running = Running(owner_running.clone());
            let host = create();
            match host {
                Err(error) => {
                    let _ = ready.send(Err(error));
                }
                Ok(mut host) => {
                    if ready.send(Ok(())).is_err() {
                        return;
                    }
                    let mut tick_due = Instant::now() + Duration::from_millis(500);
                    loop {
                        match incoming
                            .recv_timeout(tick_due.saturating_duration_since(Instant::now()))
                        {
                            Ok(command) => {
                                let stop = command.op == "stop";
                                let result = host.command(&command.op, &command.id, command.value);
                                let finished = stop && result.is_ok();
                                owner_running.store(host.serving(), Ordering::SeqCst);
                                let _ = command.reply.send(result);
                                if finished {
                                    owner_stopped.store(true, Ordering::SeqCst);
                                    break;
                                }
                            }
                            Err(mpsc::RecvTimeoutError::Timeout) => {}
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        }
                        if Instant::now() >= tick_due {
                            host.tick();
                            owner_running.store(host.serving(), Ordering::SeqCst);
                            tick_due = Instant::now() + Duration::from_millis(500);
                            if host.finished() {
                                owner_stopped.store(true, Ordering::SeqCst);
                                break;
                            }
                        }
                    }
                }
            }
        });
        match started.recv().context("fixture owner exited")? {
            Ok(()) => Ok(Self {
                stopped,
                running,
                requests: Some(requests),
                worker: Some(worker),
            }),
            Err(error) => {
                let _ = worker.join();
                Err(error)
            }
        }
    }
    pub fn request(&self, op: &str, id: &str, value: bool) -> anyhow::Result<Value> {
        if self.stopped.load(Ordering::SeqCst) {
            ensure!(op == "stop", "fixture stopped");
            return Ok(json!({"unregistered":true,"files_retained":true}));
        }
        let (reply, result) = mpsc::sync_channel(1);
        self.requests
            .as_ref()
            .context("fixture owner unavailable")?
            .send(Command {
                op: op.into(),
                id: id.into(),
                value,
                reply,
            })
            .context("fixture owner disconnected")?;
        let result = result.recv().context("fixture owner exited")?;
        if op == "stop" && result.is_ok() {
            self.stopped.store(true, Ordering::SeqCst);
        }
        result
    }
    pub fn active(&self) -> bool {
        !self.stopped.load(Ordering::SeqCst)
            && self.running.load(Ordering::SeqCst)
            && self
                .worker
                .as_ref()
                .is_some_and(|worker| !worker.is_finished())
    }
    pub fn unavailable(&self) -> bool {
        !self.stopped.load(Ordering::SeqCst) && !self.active()
    }
}
impl Drop for FixtureService {
    fn drop(&mut self) {
        // Finish returns explicit evidence. Drop also keeps callbacks alive until
        // disconnect; no private/generated bytes are deleted on shutdown.
        if self.worker.is_some() {
            if let Err(error) = self.request("stop", "", false) {
                eprintln!("native fixture cleanup unconfirmed: {error}");
            }
            // A refused stop keeps the owner alive for recovery. Drop closes
            // the channel before joining, so that recovery refusal cannot hang.
            self.requests.take();
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }
}
struct Host {
    session: Session,
    source: Arc<EngineSource>,
    paths: Vec<PathBuf>,
    stopped: bool,
    intents: Vec<IntentTracker>,
    shell_error: bool,
    dataset: Dataset,
    phases: PhaseLog,
    observation_error: bool,
    deadline: Option<Instant>,
    deadline_utc_ms: Option<u64>,
    expired: bool,
    next_expiry_stop: Instant,
}
impl Host {
    fn start(root: &Path) -> anyhow::Result<Self> {
        Self::start_dataset(root, Dataset::StandardV1)
    }
    fn start_dataset(root: &Path, dataset: Dataset) -> anyhow::Result<Self> {
        let started = Instant::now();
        let deadline = if dataset == Dataset::VisualV1 {
            Some(started + Duration::from_secs(45 * 60))
        } else {
            None
        };
        let deadline_utc_ms = if deadline.is_some() {
            Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_millis() as u64
                    + 45 * 60 * 1000,
            )
        } else {
            None
        };
        // Create sibling state only beneath original locked ordinary ancestors.
        let state = root.with_extension("engine-state");
        let state_guard = native::GeneratedDirectory::create(&state).map_err(anyhow::Error::msg)?;
        let state = state_guard.path();
        let cache_guard =
            native::GeneratedDirectory::create(&state.join("cache")).map_err(anyhow::Error::msg)?;
        let cache = cache_guard.path();
        let counters = Arc::new(DownloadCounters::default());
        let bridge = LocalFileBridge::generated_fixture_dataset(
            Database::new(state.join("fixture.sqlite3")),
            cache,
            dataset,
            counters.clone(),
        )?;
        let source = Arc::new(EngineSource {
            bridge,
            gate: Mutex::new(()),
            offline: AtomicBool::new(false),
            failed: Mutex::default(),
            reading: Mutex::default(),
            dataset,
            counters,
        });
        let session = Session::create_with_source_dataset(root, source.clone(), dataset)
            .map_err(anyhow::Error::msg)?;
        let mut host = Self {
            session,
            source,
            paths: vec![],
            stopped: false,
            intents: vec![IntentTracker::default(); objects_for(dataset).len()],
            shell_error: false,
            dataset,
            phases: PhaseLog::create(&state.join("native-phases.jsonl"))?,
            observation_error: false,
            deadline,
            deadline_utc_ms,
            expired: false,
            next_expiry_stop: started,
        };
        let initialized = (|| -> anyhow::Result<()> {
            host.session.attach_shell().map_err(anyhow::Error::msg)?;
            for folder in dataset.directories() {
                host.session
                    .directory_for(Path::new(&folder.relative_path), folder.id.as_bytes())
                    .map_err(anyhow::Error::msg)?;
            }
            for entry in objects_for(dataset) {
                host.paths.push(
                    host.session
                        .placeholder_for(
                            Path::new(entry.path.trim_start_matches('/')),
                            entry.remote_id.as_bytes(),
                            SIZE,
                        )
                        .map_err(anyhow::Error::msg)?,
                );
            }
            host.session
                .mark_directories_initially_in_sync()
                .map_err(anyhow::Error::msg)?;
            host.session
                .seal_manifest(&state.join("native-retention.json"))
                .map_err(anyhow::Error::msg)?;
            host.command("items", "", false)?;
            Ok(())
        })();
        if let Err(original) = initialized {
            if let Err(cleanup) = host.command("stop", "", false) {
                eprintln!("generated initialization cleanup unconfirmed: {cleanup:#}");
            }
            return Err(original);
        }
        Ok(host)
    }
    fn observe(&self, index: usize, entry: &MetadataEntry) -> anyhow::Result<NativeObservation> {
        match native::inspect_item(&self.paths[index], entry.remote_id.as_bytes(), SIZE) {
            Ok(os) => {
                if os.modified_bytes != 0 || !os.os_in_sync || os.logical_size != SIZE as u64 {
                    self.source
                        .bridge
                        .journal()
                        .record_native_edit(&entry.remote_id)?;
                }
                Ok(os)
            }
            Err(error) => {
                self.source
                    .bridge
                    .journal()
                    .record_native_edit(&entry.remote_id)?;
                Err(anyhow::Error::msg(error))
            }
        }
    }
    fn items(&mut self) -> anyhow::Result<Value> {
        let result = self.collect_items();
        if result.is_err() {
            self.shell_error = true;
            let root = self.session.root().to_owned();
            let _ = self.session.publish_shell(&root, shell_text("unknown"));
        }
        result
    }
    fn collect_items(&mut self) -> anyhow::Result<Value> {
        self.shell_error = false;
        let mut items = vec![];
        for (index, entry) in objects_for(self.dataset).into_iter().enumerate() {
            let os = self.observe(index, &entry)?;
            let journal = self.source.bridge.journal();
            let record = journal
                .database()
                .get_by_cloud_remote_id(&entry.remote_id)?;
            let state = record
                .as_ref()
                .filter(|r| r.cache_path.is_some())
                .map(|r| journal.snapshot(&r.metadata.remote_id))
                .transpose()?;
            let dirty = journal.native_dirty(&entry.remote_id)?;
            let failed = self
                .source
                .failed
                .lock()
                .map_err(|_| anyhow::anyhow!("error state poisoned"))?
                .contains(&entry.remote_id);
            let reading = self
                .source
                .reading
                .lock()
                .map_err(|_| anyhow::anyhow!("read state poisoned"))?
                .contains(&entry.remote_id);
            let sync = if dirty {
                "pending_sync"
            } else if failed {
                "error"
            } else if reading {
                "hydrating"
            } else if state.as_ref().is_some_and(|s| s.sync_confirmed()) {
                "in_sync"
            } else {
                "remote_version_known"
            };
            let local_intent = self.intents[index].status();
            let shell_state = if local_intent["pending"] == true {
                format!(
                    "{}; local {} intent pending ({})",
                    shell_text(sync),
                    local_intent["kind"].as_str().unwrap_or("unknown"),
                    local_intent["failure"].as_str().unwrap_or("queued")
                )
            } else {
                shell_text(sync).to_owned()
            };
            if let Err(error) = self.session.publish_shell(&self.paths[index], &shell_state) {
                eprintln!(
                    "generated Shell property failed for {}: {error}",
                    entry.remote_id
                );
                self.shell_error = true;
            }
            items.push(json!({"id":entry.remote_id,"name":entry.name,"relative_path":entry.path.trim_start_matches('/'),"size":entry.size,
                "residency":if os.pinned && !os.resident {"pin_pending"} else if os.resident {"locally_available"} else if os.partial {"partially_available"} else {"online_only"},
                "pinned":os.pinned,"pin_state_known":os.pinned||os.pin_known_unpinned,"os_in_sync":os.os_in_sync,"modified_bytes":os.modified_bytes,
                "downloaded":os.resident,"cache_resident":state.as_ref().is_some_and(|s|s.resident),
                "sync":sync,"local_intent":local_intent,
                "native_dirty":dirty,"generation":state.as_ref().map(|s|s.generation),"acknowledged_generation":state.as_ref().map(|s|s.acknowledged_generation),
                "pending_operations":state.as_ref().map(|s|s.pending_operations),"readers":state.as_ref().map(|s|s.readers),"writers":state.as_ref().map(|s|s.writers),
                "recovery_required":state.as_ref().map(|s|s.recovery_required)}));
        }
        let summary = aggregate(&items);
        let mut directories = Vec::new();
        for folder in self.dataset.directories() {
            let observed = native::inspect_directory(
                &self.session.root().join(&folder.relative_path),
                folder.id.as_bytes(),
            )
            .map_err(anyhow::Error::msg)?;
            let children = items
                .iter()
                .filter(|item| {
                    item["relative_path"]
                        .as_str()
                        .is_some_and(|p| p.starts_with(&(folder.relative_path.clone() + "/")))
                })
                .cloned()
                .collect::<Vec<_>>();
            let aggregate = aggregate(&children);
            let folder_path = self.session.root().join(&folder.relative_path);
            if self
                .session
                .publish_shell(
                    &folder_path,
                    shell_text(aggregate["sync"].as_str().unwrap_or("unknown")),
                )
                .is_err()
            {
                self.shell_error = true;
            }
            directories.push(json!({"id":folder.id,"relative_path":folder.relative_path,"population_complete":observed.resident,"pinned":observed.pinned,"summary":aggregate}));
        }
        let root = self.session.root().to_owned();
        let root_text = format!(
            "{}; {} local intent(s) pending",
            shell_text(summary["sync"].as_str().unwrap_or("unknown")),
            summary["local_intents_pending"]
        );
        if self.session.publish_shell(&root, &root_text).is_err() {
            self.shell_error = true;
        }
        Ok(
            json!({"schema_version":1,"scope":"generated_fixture_only","owner_pid":std::process::id(),"dataset":self.dataset,"items":items,"directories":directories,"summary":summary,"binding":self.session.binding_summary(),"download_counters":self.source.counters.snapshot(),"native_transfer_counters":self.session.transfer_counters(),"observation_log_error":self.observation_error,"session_expired":self.expired,"deadline_utc_ms":self.deadline_utc_ms,"max_session_seconds":if self.deadline.is_some(){Some(2700)}else{None},"root_path":self.session.root(),"shell_registered":true,"shell_state_error":self.shell_error,"fetch_callbacks":self.session.calls(),"fetch_failures":self.session.failures(),"offline":self.source.offline.load(Ordering::SeqCst)}),
        )
    }
    fn tick(&mut self) {
        if !self.session.serving() {
            return;
        }
        let now = Instant::now();
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            self.expired = true;
            if now >= self.next_expiry_stop {
                self.next_expiry_stop = now + Duration::from_secs(5);
                if let Err(error) = self.command("stop", "", false) {
                    eprintln!("visual deadline cleanup retained: {error:#}");
                }
            }
            return;
        }
        // Bounded metadata-only observation of our generated leaves. React only
        // to a new Shell pin/free-space intent, never to arbitrary directory data.
        for (index, entry) in objects_for(self.dataset).into_iter().enumerate() {
            let Ok(os) = self.observe(index, &entry) else {
                continue;
            };
            if !os.pinned && !os.pin_known_unpinned {
                // An unknown/inherited pin state is not a confirmed cancel.
                if self.intents[index].pending.is_some()
                    && self.intents[index]
                        .retry_at
                        .is_none_or(|at| Instant::now() >= at)
                {
                    self.intents[index].failed(Instant::now(), IntentFailure::GuardedOperation);
                }
                continue;
            }
            self.intents[index].observe((os.pinned, os.release_requested), os.resident);
            let mut intent = self.intents[index].clone();
            intent.drive(
                (os.pinned, os.release_requested),
                os.resident,
                Instant::now(),
                |kind| {
                    self.command(
                        if kind == IntentKind::Pin {
                            "read"
                        } else {
                            "dehydrate"
                        },
                        &entry.remote_id,
                        false,
                    )
                    .map(|_| ())
                    .map_err(|_| self.intent_failure(&entry.remote_id, kind))
                },
            );
            self.intents[index] = intent;
        }
        match self.items() {
            Ok(mut snapshot) => {
                if let Some(value) = snapshot.as_object_mut() {
                    value.remove("root_path");
                }
                if self
                    .phases
                    .record("observe", &snapshot, true, false)
                    .is_err()
                {
                    self.observation_error = true;
                }
            }
            Err(_) => self.shell_error = true,
        }
    }
    fn intent_failure(&self, id: &str, kind: IntentKind) -> IntentFailure {
        if self
            .source
            .bridge
            .journal()
            .native_dirty(id)
            .unwrap_or(true)
        {
            IntentFailure::DirtyRecovery
        } else if kind == IntentKind::Pin && self.source.offline.load(Ordering::SeqCst) {
            IntentFailure::Offline
        } else {
            IntentFailure::GuardedOperation
        }
    }
    fn command(&mut self, op: &str, id: &str, value: bool) -> anyhow::Result<Value> {
        let result = self.command_inner(op, id, value);
        let mut snapshot = match &result {
            Ok(value) => value.clone(),
            Err(_) => self
                .items()
                .unwrap_or_else(|_| json!({"state":"unavailable"})),
        };
        if let Some(object) = snapshot.as_object_mut() {
            object.remove("root_path");
        }
        if self
            .phases
            .record(op, &snapshot, result.is_ok(), op != "items")
            .is_err()
        {
            self.observation_error = true;
        }
        result
    }
    fn command_inner(&mut self, op: &str, id: &str, value: bool) -> anyhow::Result<Value> {
        if op == "stop" {
            if !self.session.teardown_started() {
                self.retain_generated_files()?;
            }
            self.session.finish().map_err(anyhow::Error::msg)?;
            self.stopped = true;
            return Ok(json!({"unregistered":true,"files_retained":true}));
        }
        ensure!(!self.stopped, "fixture stopped");
        ensure!(
            !self.expired || op == "items",
            "visual session expired; only status/retention stop remains available"
        );
        if !self.session.serving() {
            ensure!(
                op == "items",
                "root retained after incomplete teardown; native operations disabled"
            );
            return Ok(
                json!({"schema_version":1,"scope":"generated_fixture_only","items":[],"owner_unavailable":true,"native_operations_disabled":true,"cleanup_confirmed":false,"registration":"unconfirmed"}),
            );
        }
        match op {
            "items" => self.items(),
            "offline" => {
                self.source.offline.store(value, Ordering::SeqCst);
                self.items()
            }
            "reconnect" => {
                self.session.disconnect().map_err(anyhow::Error::msg)?;
                self.session.verify_shell().map_err(anyhow::Error::msg)?;
                self.session.connect().map_err(anyhow::Error::msg)?;
                self.items()
            }
            "pin" | "unpin" => {
                let (index, entry) = object_for(self.dataset, id)?;
                let os = self.observe(index, &entry)?;
                native::pin(&self.paths[index], op == "pin").map_err(anyhow::Error::msg)?;
                self.intents[index].observe((op == "pin", false), os.resident);
                if op == "pin" {
                    let result = self.command("read", id, false);
                    if result.is_ok() {
                        self.intents[index].complete();
                        return self.items();
                    } else {
                        let failure = self.intent_failure(id, IntentKind::Pin);
                        self.intents[index].failed(Instant::now(), failure);
                    }
                    return result;
                }
                self.items()
            }
            "read" => {
                let (index, entry) = object_for(self.dataset, id)?;
                self.observe(index, &entry)?;
                // Real normal OS read: only CldApi invokes EngineSource.
                let bytes = fs::read(&self.paths[index])?;
                ensure!(
                    bytes == self.dataset.bytes(index),
                    "generated file content differs; preserved"
                );
                self.items()
            }
            "dehydrate" => {
                let (index, entry) = object_for(self.dataset, id)?;
                self.observe(index, &entry)?;
                let bridge = &self.source.bridge;
                ensure!(!bridge.journal().native_dirty(id)?, "native dirty veto");
                let record = bridge
                    .journal()
                    .database()
                    .get_by_cloud_remote_id(id)?
                    .context("item not downloaded")?;
                let state = bridge.status(&record.metadata.remote_id)?;
                native::with_exclusive_placeholder(
                    &self.paths[index],
                    id.as_bytes(),
                    SIZE,
                    |locked| {
                        let os = locked.inspect()?;
                        if !os.clean() || os.logical_size != SIZE as u64 {
                            return Err("native state veto".into());
                        }
                        let result = bridge
                            .journal()
                            .with_clean_cache(
                                &record.metadata.remote_id,
                                state.generation,
                                |file, expected| {
                                    ensure!(
                                        digest_file(file)? == expected,
                                        "private cache content changed"
                                    );
                                    locked.dehydrate().map_err(anyhow::Error::msg)
                                },
                            )
                            .map_err(|e| e.to_string())?;
                        result.ok_or_else(|| "journal veto".to_owned())
                    },
                )
                .map_err(anyhow::Error::msg)?;
                // Only the OS copy was released. Retained private cache is
                // separately exposed; full cache eviction stays unsupported.
                self.items()
            }
            _ => anyhow::bail!("unsupported native fixture operation"),
        }
    }
    fn retain_generated_files(&self) -> anyhow::Result<()> {
        // This Provider is internal deterministic data, never a real account.
        // Materialize only clean missing ranges before destructive unregister.
        let previous = self.source.offline.swap(false, Ordering::SeqCst);
        let result = (|| {
            for (index, entry) in objects_for(self.dataset)
                .into_iter()
                .enumerate()
                .take(self.paths.len())
            {
                let os = self.observe(index, &entry)?;
                if !os.resident {
                    ensure!(
                        !self
                            .source
                            .bridge
                            .journal()
                            .native_dirty(&entry.remote_id)?,
                        "unretained dirty file requires recovery; root retained"
                    );
                    let bytes = fs::read(&self.paths[index])?;
                    ensure!(
                        bytes == self.dataset.bytes(index),
                        "generated retention content differs"
                    );
                }
            }
            Ok(())
        })();
        self.source.offline.store(previous, Ordering::SeqCst);
        result
    }
}
impl FixtureOwner for Host {
    fn command(&mut self, op: &str, id: &str, value: bool) -> anyhow::Result<Value> {
        Host::command(self, op, id, value)
    }
    fn tick(&mut self) {
        Host::tick(self);
    }
    fn serving(&self) -> bool {
        self.session.serving() && !self.expired
    }
    fn finished(&self) -> bool {
        self.stopped
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum IntentKind {
    Pin,
    Release,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum IntentFailure {
    Offline,
    GuardedOperation,
    DirtyRecovery,
}
#[derive(Clone, Default)]
struct IntentTracker {
    observed: (bool, bool),
    pending: Option<IntentKind>,
    attempts: u32,
    retry_at: Option<Instant>,
    failure: Option<IntentFailure>,
}
impl IntentTracker {
    fn observe(&mut self, pin: (bool, bool), resident: bool) {
        if self.observed != pin {
            self.observed = pin;
            self.pending = if pin.0 && !resident {
                Some(IntentKind::Pin)
            } else if pin.1 && resident {
                Some(IntentKind::Release)
            } else {
                None
            };
            self.attempts = 0;
            self.retry_at = None;
            self.failure = None;
        }
    }
    fn complete(&mut self) {
        self.pending = None;
        self.retry_at = None;
        self.failure = None;
        self.attempts = 0;
    }
    fn failed(&mut self, now: Instant, failure: IntentFailure) {
        self.failure = Some(failure);
        self.attempts = self.attempts.saturating_add(1);
        let seconds = if failure == IntentFailure::DirtyRecovery {
            30
        } else {
            (1u64 << self.attempts.saturating_sub(1).min(5)).min(30)
        };
        self.retry_at = Some(now + Duration::from_secs(seconds));
    }
    fn status(&self) -> Value {
        json!({"pending":self.pending.is_some(),"kind":self.pending.map(|k|if k==IntentKind::Pin {"pin"} else {"release"}),"attempts":self.attempts,"failure":self.failure.map(|f|match f {IntentFailure::Offline=>"offline",IntentFailure::GuardedOperation=>"guarded_operation",IntentFailure::DirtyRecovery=>"dirty_recovery_required"})})
    }
    fn drive(
        &mut self,
        pin: (bool, bool),
        resident: bool,
        now: Instant,
        action: impl FnOnce(IntentKind) -> Result<(), IntentFailure>,
    ) {
        self.observe(pin, resident);
        let Some(kind) = self.pending else {
            return;
        };
        if (kind == IntentKind::Pin && resident) || (kind == IntentKind::Release && !resident) {
            self.complete();
            return;
        }
        if self.retry_at.is_some_and(|at| now < at) {
            return;
        }
        match action(kind) {
            Ok(()) => self.complete(),
            Err(failure) => self.failed(now, failure),
        }
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        if !self.stopped {
            let retained = if self.session.teardown_started() {
                Ok(())
            } else {
                self.retain_generated_files()
            };
            let result = retained.and_then(|()| self.session.finish().map_err(anyhow::Error::msg));
            if let Err(error) = result {
                eprintln!("generated root retained; cleanup unconfirmed: {error}");
            }
        }
    }
}

fn shell_text(sync: &str) -> &'static str {
    match sync {
        "in_sync" => "Acknowledged generation (generated provider)",
        "pending_sync" => "Pending recovery - local edits preserved",
        "hydrating" => "Reading generated provider",
        "error" => "Read failed - retry required",
        "conflict" => "Conflict - local bytes preserved",
        "remote_version_known" => "Remote version known - no local acknowledgement",
        _ => "Unknown - synchronization unverified",
    }
}
fn aggregate(items: &[Value]) -> Value {
    let sync = [
        "conflict",
        "error",
        "pending_sync",
        "hydrating",
        "remote_version_known",
    ]
    .into_iter()
    .find(|label| items.iter().any(|i| i["sync"] == *label))
    .unwrap_or(
        if !items.is_empty() && items.iter().all(|i| i["sync"] == "in_sync") {
            "in_sync"
        } else {
            "unknown"
        },
    );
    json!({"sync":sync,"files":items.len(),"native_resident":items.iter().filter(|i| i["downloaded"] == true).count(),"private_cache_resident":items.iter().filter(|i|i["cache_resident"] == true).count(),"pinned":items.iter().filter(|i| i["pinned"] == true).count(),"local_intents_pending":items.iter().filter(|i|i["local_intent"]["pending"]==true).count()})
}

#[cfg(test)]
mod tests {
    #[test]
    fn owner_finished_from_tick_is_a_confirmed_idempotent_stop() {
        struct TimerOwner(bool);
        impl super::FixtureOwner for TimerOwner {
            fn command(&mut self, _: &str, _: &str, _: bool) -> anyhow::Result<serde_json::Value> {
                Ok(serde_json::json!({"alive":!self.0}))
            }
            fn tick(&mut self) {
                self.0 = true;
            }
            fn serving(&self) -> bool {
                !self.0
            }
            fn finished(&self) -> bool {
                self.0
            }
        }
        let service = super::FixtureService::start_owner(|| Ok(TimerOwner(false))).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while service.active() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!service.active());
        assert!(!service.unavailable());
        assert_eq!(
            service.request("stop", "", false).unwrap()["unregistered"],
            true
        );
    }
    #[test]
    fn visual_metadata_and_thumbnail_like_ranges_keep_separate_cumulative_counters() {
        use super::*;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let cache = root.join("cache");
        fs::create_dir(&cache).unwrap();
        let dataset = Dataset::VisualV1;
        let counters = Arc::new(DownloadCounters::default());
        let bridge = LocalFileBridge::generated_fixture_dataset(
            Database::new(root.join("state.db")),
            &cache,
            dataset,
            counters.clone(),
        )
        .unwrap();
        let source = EngineSource {
            bridge,
            gate: Mutex::new(()),
            offline: AtomicBool::new(false),
            failed: Mutex::default(),
            reading: Mutex::default(),
            dataset,
            counters,
        };
        let provider = GeneratedProvider {
            dataset,
            counters: source.counters.clone(),
        };
        assert_eq!(provider.list_all().unwrap().len(), 4);
        assert_eq!(provider.list_directory("/").unwrap().len(), 2);
        assert_eq!(
            provider
                .list_directory(&format!(
                    "/{}",
                    twodrive_local_files::fixtures::VISUAL_DIRECTORY
                ))
                .unwrap()
                .len(),
            2
        );
        assert_eq!(source.counters.snapshot()["provider_download_bytes"], 0);
        let cancel = AtomicBool::new(false);
        let id = b"visual-object-1";
        assert_eq!(
            source.read_range(id, 0, 64, &cancel).unwrap(),
            dataset.bytes(1)[..64]
        );
        assert_eq!(source.counters.snapshot()["provider_download_bytes"], 65536);
        assert_eq!(source.counters.snapshot()["cache_served_bytes"], 64);
        source.offline.store(true, Ordering::SeqCst);
        assert_eq!(
            source.read_range(id, 64, 32, &cancel).unwrap(),
            dataset.bytes(1)[64..96]
        );
        assert!(
            source
                .read_range(b"visual-object-2", 0, 64, &cancel)
                .is_err()
        );
        assert_eq!(source.counters.snapshot()["provider_download_bytes"], 65536);
        assert_eq!(source.counters.snapshot()["cache_served_bytes"], 96);
        assert!(source.failed.lock().unwrap().contains("visual-object-2"));
    }
    #[test]
    fn observation_log_has_durable_sequences_dedup_and_rejects_private_event_text() {
        use super::*;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("phases.jsonl");
        let mut log = PhaseLog::create(&path).unwrap();
        let metadata = json!({"provider_download_bytes":0,"native_transferred_bytes":0});
        log.record("metadata", &metadata, true, false).unwrap();
        log.record("metadata", &metadata, true, false).unwrap();
        log.record(
            "thumbnail_read",
            &json!({"provider_download_bytes":65536,"cache_served_bytes":64}),
            true,
            true,
        )
        .unwrap();
        assert!(
            log.record("C:/private/path", &metadata, true, true)
                .is_err()
        );
        let lines = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["sequence"], 1);
        assert_eq!(lines[1]["sequence"], 2);
        assert!(lines[1]["utc_ms"].as_u64().unwrap() > 0);
        log.bytes = 32 * 1024 * 1024;
        assert!(log.record("observe", &metadata, true, true).is_err());
    }
    use super::*;
    #[cfg(feature = "native-test-hooks")]
    fn wait_for_status(
        service: &FixtureService,
        ready: impl Fn(&Value) -> bool,
    ) -> anyhow::Result<Value> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let value = service.request("items", "", false)?;
            if ready(&value) {
                return Ok(value);
            }
            ensure!(Instant::now() < deadline, "native intent deadline exceeded");
            thread::sleep(Duration::from_millis(50));
        }
    }
    #[test]
    fn free_space_intent_retries_after_real_handle_closes() {
        use std::os::windows::fs::OpenOptionsExt;
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("generated-handle-veto.bin");
        fs::write(&file, b"retained").unwrap();
        let handle = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&file)
            .unwrap();
        let mut intent = IntentTracker::default();
        let now = Instant::now();
        let attempt = || {
            fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&file)
                .and_then(|mut f| f.write_all(b"completed"))
                .map_err(|_| IntentFailure::GuardedOperation)
        };
        intent.drive((false, true), true, now, |_| attempt());
        assert_eq!(fs::read(&file).unwrap(), b"retained");
        drop(handle);
        intent.drive((false, true), true, now + Duration::from_secs(2), |_| {
            attempt()
        });
        assert_eq!(
            fs::read(file).unwrap(),
            b"completed",
            "unchanged Shell intent was lost after the handle veto"
        );
    }
    #[test]
    fn pin_intent_retries_actual_engine_after_offline_recovery() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let cache = root.join("cache");
        fs::create_dir(&cache).unwrap();
        let source = EngineSource {
            bridge: LocalFileBridge::generated_fixture(
                Database::new(root.join("state.db")),
                &cache,
            )
            .unwrap(),
            gate: Mutex::default(),
            offline: AtomicBool::new(true),
            failed: Mutex::default(),
            reading: Mutex::default(),
            dataset: Dataset::StandardV1,
            counters: Arc::default(),
        };
        let mut intent = IntentTracker::default();
        let now = Instant::now();
        let completed = AtomicBool::new(false);
        let attempt = || {
            source
                .read_range(b"generated-object-2", 0, 4096, &AtomicBool::new(false))
                .map(|_| {
                    completed.store(true, Ordering::SeqCst);
                })
                .map_err(|_| IntentFailure::Offline)
        };
        intent.drive((true, false), false, now, |_| attempt());
        assert!(!completed.load(Ordering::SeqCst));
        source.offline.store(false, Ordering::SeqCst);
        intent.drive((true, false), false, now + Duration::from_secs(2), |_| {
            attempt()
        });
        assert!(
            completed.load(Ordering::SeqCst),
            "offline pin intent did not retry after recovery"
        );
    }
    struct StopOwner {
        refused: bool,
        file: PathBuf,
    }
    struct TickOwner(Arc<std::sync::atomic::AtomicUsize>);
    impl FixtureOwner for TickOwner {
        fn command(&mut self, _: &str, _: &str, _: bool) -> anyhow::Result<Value> {
            Ok(json!({}))
        }
        fn tick(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    #[test]
    fn status_traffic_does_not_starve_pending_intent_tick() {
        let ticks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = ticks.clone();
        let service = FixtureService::start_owner(move || Ok(TickOwner(ticks))).unwrap();
        let deadline = Instant::now() + Duration::from_millis(850);
        while Instant::now() < deadline {
            service.request("items", "", false).unwrap();
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            observed.load(Ordering::SeqCst) > 0,
            "status traffic starved pending-intent tick"
        );
    }
    impl FixtureOwner for StopOwner {
        fn command(&mut self, op: &str, _: &str, _: bool) -> anyhow::Result<Value> {
            match op {
                "stop" if !self.refused => {
                    self.refused = true;
                    anyhow::bail!("retention gate refused");
                }
                "stop" => Ok(json!({"unregistered":true})),
                "items" => Ok(json!({"running":true})),
                "read" => Ok(json!({"bytes":fs::read(&self.file)?})),
                "reconnect" => Ok(json!({"reconnected":true})),
                _ => anyhow::bail!("unknown test command"),
            }
        }
        fn tick(&mut self) {}
    }
    #[test]
    fn rejected_stop_keeps_owner_for_status_read_and_reconnect() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("generated.bin");
        fs::write(&file, b"retained").unwrap();
        let service = FixtureService::start_owner(move || {
            Ok(StopOwner {
                refused: false,
                file,
            })
        })
        .unwrap();
        assert!(service.request("stop", "", false).is_err());
        assert!(service.active());
        assert_eq!(
            service.request("items", "", false).unwrap()["running"],
            true
        );
        assert_eq!(
            service.request("read", "", false).unwrap()["bytes"],
            json!(b"retained".to_vec())
        );
        assert_eq!(
            service.request("reconnect", "", false).unwrap()["reconnected"],
            true
        );
        service.request("stop", "", false).unwrap();
        assert!(!service.active());
    }
    struct TerminalOwner(bool);
    impl FixtureOwner for TerminalOwner {
        fn command(&mut self, op: &str, _: &str, _: bool) -> anyhow::Result<Value> {
            if op == "stop" {
                self.0 = false;
                anyhow::bail!("partial teardown; retained for recovery");
            }
            if op == "items" {
                return Ok(json!({"cleanup_confirmed":false}));
            }
            anyhow::bail!("native operations disabled")
        }
        fn tick(&mut self) {}
        fn serving(&self) -> bool {
            self.0
        }
    }
    #[test]
    fn terminal_teardown_disables_capabilities_and_drop_closes_owner_channel() {
        let service = FixtureService::start_owner(|| Ok(TerminalOwner(true))).unwrap();
        assert!(service.request("stop", "", false).is_err());
        assert!(!service.active());
        assert!(service.unavailable());
        assert_eq!(
            service.request("items", "", false).unwrap()["cleanup_confirmed"],
            false
        );
        assert!(service.request("read", "", false).is_err());
        drop(service);
    }
    #[test]
    #[ignore = "requires its own explicitly approved NEW generated Shell root"]
    fn untouched_placeholders_survive_normal_exit() {
        let root = PathBuf::from(
            std::env::var_os("TWODRIVE_APPROVED_RETENTION_FIXTURE_ROOT")
                .expect("explicit approved retention root required"),
        );
        let mut host = Host::start(&root).unwrap();
        assert_eq!(host.session.calls(), 0);
        assert!(
            host.session
                .finish()
                .unwrap_err()
                .contains("not fully retained")
        );
        host.session.verify_shell().unwrap();
        assert_eq!(host.session.calls(), 0);
        host.command("stop", "", false).unwrap();
        drop(host);
        native::verify_unregistered(&root).unwrap();
        for (index, entry) in objects().into_iter().enumerate() {
            assert_eq!(
                fs::read(root.join(entry.name)).unwrap(),
                generated_bytes(index)
            );
        }
        println!("generated untouched placeholders fully retained; Shell/CFAPI unregistered");
    }
    #[test]
    fn folder_summary_keeps_errors_pending_and_cache_separate() {
        let clean =
            json!({"sync":"in_sync","downloaded":false,"cache_resident":true,"pinned":false});
        let pending =
            json!({"sync":"pending_sync","downloaded":true,"cache_resident":true,"pinned":true});
        assert_eq!(aggregate(&[])["sync"], "unknown");
        assert_eq!(aggregate(std::slice::from_ref(&clean))["sync"], "in_sync");
        let summary = aggregate(&[clean.clone(), pending.clone()]);
        assert_eq!(summary["sync"], "pending_sync");
        assert_eq!(summary["native_resident"], 1);
        assert_eq!(summary["private_cache_resident"], 2);
        for (state, expected) in [
            ("error", "error"),
            ("conflict", "conflict"),
            ("remote_version_known", "remote_version_known"),
        ] {
            assert_eq!(
                aggregate(&[clean.clone(), json!({"sync":state})])["sync"],
                expected
            );
        }
        assert!(shell_text("unknown").contains("unverified"));
    }
    #[test]
    fn intent_backoff_is_bounded_and_cancel_does_not_bypass_dirty_gate() {
        let mut intent = IntentTracker::default();
        let now = Instant::now();
        let attempts = std::cell::Cell::new(0);
        let fail = |_| {
            attempts.set(attempts.get() + 1);
            Err(IntentFailure::DirtyRecovery)
        };
        intent.drive((false, true), true, now, fail);
        for _ in 0..100 {
            intent.drive((false, true), true, now + Duration::from_secs(2), fail);
        }
        assert_eq!(attempts.get(), 1);
        assert_eq!(intent.status()["pending"], true);
        assert_eq!(intent.status()["failure"], "dirty_recovery_required");
        intent.drive((false, true), true, now + Duration::from_secs(31), fail);
        assert_eq!(attempts.get(), 2);
        assert_eq!(intent.status()["pending"], true);
        intent.drive((false, false), true, now + Duration::from_secs(32), fail);
        assert_eq!(intent.status()["pending"], false);
        assert_eq!(attempts.get(), 2);
    }
    #[test]
    #[ignore = "requires an explicitly approved NEW CFAPI fixture root; never run automatically"]
    fn native_engine_end_to_end_requires_explicit_registration() {
        let root = PathBuf::from(
            std::env::var_os("TWODRIVE_APPROVED_NATIVE_FIXTURE_ROOT")
                .expect("explicit approved fixture root required"),
        );
        #[cfg(not(feature = "native-test-hooks"))]
        let service = FixtureService::start(root.clone()).unwrap();
        #[cfg(feature = "native-test-hooks")]
        let (service, concurrent_checked, concurrency_enabled) = {
            use native::TeardownStage;
            let checked = Arc::new(AtomicBool::new(false));
            let done = checked.clone();
            let enabled = Arc::new(AtomicBool::new(false));
            let run = enabled.clone();
            let worker_root = root.clone();
            let first = root.join(&objects()[0].name);
            let (began_tx, began_rx) = mpsc::sync_channel(1);
            let (release_tx, release_rx) = mpsc::sync_channel(1);
            let (done_tx, done_rx) = mpsc::sync_channel(1);
            let mut sender = Some((began_tx, release_rx, done_tx));
            let mut writer = None;
            let mut phase = 0;
            let service = FixtureService::start_owner(move || {
                let mut host = Host::start(&worker_root)?;
                host.session.set_teardown_hook(move |stage| {
                    if !run.load(Ordering::SeqCst) {
                        return Ok(());
                    }
                    if stage == TeardownStage::AllProtected && phase == 0 {
                        let (began, release, finished) =
                            sender.take().ok_or("writer already started")?;
                        let path = first.clone();
                        writer = Some(thread::spawn(move || {
                            let _ = began.send(());
                            let result = (|| -> std::io::Result<()> {
                                let mut file = match fs::OpenOptions::new()
                                    .write(true)
                                    .truncate(true)
                                    .open(&path)
                                {
                                    Ok(file) => file,
                                    Err(_) => {
                                        let _ = release.recv();
                                        fs::OpenOptions::new()
                                            .write(true)
                                            .truncate(true)
                                            .open(&path)?
                                    }
                                };
                                let mut bytes = generated_bytes(0);
                                let prefix = b"generated concurrent teardown edit";
                                bytes[..prefix.len()].copy_from_slice(prefix);
                                file.write_all(&bytes)?;
                                file.sync_all()
                            })();
                            let _ = finished.send(result.map_err(|e| e.to_string()));
                        }));
                        began_rx
                            .recv_timeout(Duration::from_secs(2))
                            .map_err(|e| e.to_string())?;
                        if done_rx.recv_timeout(Duration::from_millis(50)).is_ok() {
                            return Err(
                                "writer completed while every protected reference was held".into(),
                            );
                        }
                        phase = 1;
                    } else if stage == TeardownStage::AllReverted && phase == 1 {
                        let _ = release_tx.send(());
                        done_rx
                            .recv_timeout(Duration::from_secs(5))
                            .map_err(|e| e.to_string())??;
                        if let Some(writer) = writer.take() {
                            writer.join().map_err(|_| "writer panicked")?;
                        }
                        let mut expected = generated_bytes(0);
                        let prefix = b"generated concurrent teardown edit";
                        expected[..prefix.len()].copy_from_slice(prefix);
                        if fs::read(&first).map_err(|e| e.to_string())? != expected {
                            return Err("concurrent ordinary-file bytes differ".into());
                        }
                        done.store(true, Ordering::SeqCst);
                        phase = 2;
                    }
                    Ok(())
                });
                Ok(host)
            })
            .unwrap();
            (service, checked, enabled)
        };
        let result = (|| -> anyhow::Result<Value> {
            let initial = service.request("items", "", false)?;
            ensure!(
                initial["shell_registered"] == true && initial["shell_state_error"] == false,
                "Shell registration/property publication failed"
            );
            ensure!(
                initial["fetch_callbacks"] == 0,
                "metadata unexpectedly hydrated"
            );
            ensure!(
                initial["items"]
                    .as_array()
                    .context("items")?
                    .iter()
                    .all(|item| item["residency"] == "online_only"
                        && item["cache_resident"] == false),
                "initial state"
            );
            for id in ["generated-object-0", "generated-object-1"] {
                service.request("read", id, false)?;
            }
            let first = root.join(&objects()[0].name);
            let open = fs::File::open(&first)?;
            ensure!(
                service
                    .request("dehydrate", "generated-object-0", false)
                    .is_err(),
                "open file did not veto"
            );
            #[cfg(feature = "native-test-hooks")]
            {
                native::test_release_intent(&first).map_err(anyhow::Error::msg)?;
                let pending = wait_for_status(&service, |s| {
                    s["items"][0]["local_intent"]["pending"] == true
                })?;
                ensure!(
                    pending["items"][0]["downloaded"] == true,
                    "open handle protection was bypassed"
                );
            }
            drop(open);
            #[cfg(not(feature = "native-test-hooks"))]
            let released = service.request("dehydrate", "generated-object-0", false)?;
            #[cfg(feature = "native-test-hooks")]
            let released =
                wait_for_status(&service, |s| s["items"][0]["residency"] == "online_only")?;
            ensure!(
                released["items"][0]["residency"] == "online_only"
                    && released["items"][0]["cache_resident"] == true,
                "OS/cache separation"
            );
            service.request("offline", "", true)?;
            service.request("read", "generated-object-0", false)?;
            ensure!(
                service
                    .request("read", "generated-object-2", false)
                    .is_err(),
                "unavailable source succeeded"
            );
            #[cfg(feature = "native-test-hooks")]
            {
                native::pin(&root.join(&objects()[2].name), true).map_err(anyhow::Error::msg)?;
                wait_for_status(&service, |s| {
                    s["items"][2]["local_intent"]["pending"] == true
                })?;
            }
            service.request("offline", "", false)?;
            #[cfg(not(feature = "native-test-hooks"))]
            service.request("read", "generated-object-2", false)?;
            #[cfg(feature = "native-test-hooks")]
            wait_for_status(&service, |s| s["items"][2]["downloaded"] == true)?;
            let reconnected = service.request("reconnect", "", false)?;
            ensure!(
                reconnected["items"][2]["downloaded"] == true,
                "reconnect changed residency"
            );
            let second = root.join(&objects()[1].name);
            let pinned = service.request("pin", "generated-object-1", false)?;
            ensure!(
                pinned["items"][1]["pinned"] == true && pinned["items"][1]["downloaded"] == true,
                "pin not backed by native residency"
            );
            ensure!(
                service
                    .request("dehydrate", "generated-object-1", false)
                    .is_err(),
                "pin did not veto"
            );
            service.request("unpin", "generated-object-1", false)?;
            ensure!(
                fs::read(second)? == generated_bytes(1),
                "unpin discarded bytes"
            );
            let mut dirty = generated_bytes(0);
            let prefix = b"generated unuploaded edit";
            dirty[..prefix.len()].copy_from_slice(prefix);
            let mut file = fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&first)?;
            file.write_all(&dirty)?;
            file.sync_all()?;
            drop(file);
            let mut final_status = service.request("items", "", false)?;
            final_status.as_object_mut().unwrap().remove("root_path");
            ensure!(
                final_status["items"][0]["native_dirty"] == true
                    && final_status["items"][0]["sync"] == "pending_sync",
                "dirty state not durable"
            );
            ensure!(
                service
                    .request("dehydrate", "generated-object-0", false)
                    .is_err(),
                "dirty release succeeded"
            );
            ensure!(fs::read(&first)? == dirty, "whole dirty file changed");
            Ok(
                json!({"metadata_no_hydration":true,"normal_os_reads_through_engine":true,"chinese_long_leaf":true,"open_handle_veto":true,"native_copy_dehydration":true,"private_cache_retained":true,"offline_cached_read":true,"offline_missing_cache_failure_and_retry":true,"pin_veto":true,"native_dirty_durable_veto":true,"whole_dirty_file_preserved":true,"final_status":final_status}),
            )
        })();
        if let Err(error) = &result {
            eprintln!("native validation failed before cleanup: {error:#}");
        }
        #[cfg(feature = "native-test-hooks")]
        concurrency_enabled.store(result.is_ok(), Ordering::SeqCst);
        let cleanup = service.request("stop", "", false);
        if let Err(error) = &cleanup {
            eprintln!("native cleanup failed: {error:#}");
        }
        drop(service);
        native::verify_unregistered(&root).expect("new fixture root cleanup unconfirmed");
        assert_eq!(cleanup.unwrap()["unregistered"], true);
        #[cfg(feature = "native-test-hooks")]
        {
            assert!(concurrent_checked.load(Ordering::SeqCst));
            let mut expected = generated_bytes(0);
            let prefix = b"generated concurrent teardown edit";
            expected[..prefix.len()].copy_from_slice(prefix);
            assert_eq!(
                fs::read(root.join(&objects()[0].name)).unwrap(),
                expected,
                "concurrent bytes were lost during unregister"
            );
        }
        let report = result.unwrap();
        println!(
            "{}",
            json!({"result":report,"unregistered":true,"files_retained":true,"explorer_icons":"unverified","real_cloud":"not_tested"})
        );
    }
    #[test]
    fn actual_engine_source_hydrates_opaque_object_and_reuses_private_cache_offline() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let cache = root.join("cache");
        fs::create_dir(&cache).unwrap();
        let source = EngineSource {
            bridge: LocalFileBridge::generated_fixture(
                Database::new(root.join("state.db")),
                &cache,
            )
            .unwrap(),
            gate: Mutex::new(()),
            offline: AtomicBool::new(false),
            failed: Mutex::default(),
            reading: Mutex::default(),
            dataset: Dataset::StandardV1,
            counters: Arc::default(),
        };
        let cancel = AtomicBool::new(false);
        let id = b"generated-object-0";
        assert_eq!(
            source.read_range(id, 0, 4096, &cancel).unwrap(),
            generated_bytes(0)[..4096]
        );
        source.offline.store(true, Ordering::SeqCst);
        assert_eq!(
            source.read_range(id, 4096, 4096, &cancel).unwrap(),
            generated_bytes(0)[4096..8192]
        );
        assert!(
            source
                .read_range(b"generated-object-1", 0, 4096, &cancel)
                .is_err()
        );
        assert!(source.failed.lock().unwrap().contains("generated-object-1"));
        assert!(source.reading.lock().unwrap().is_empty());
        source.offline.store(false, Ordering::SeqCst);
        assert_eq!(
            source
                .read_range(b"generated-object-1", 0, 4096, &cancel)
                .unwrap(),
            generated_bytes(1)[..4096]
        );
        assert!(!source.failed.lock().unwrap().contains("generated-object-1"));
        cancel.store(true, Ordering::SeqCst);
        assert!(source.read_range(id, 0, 4096, &cancel).is_err());
        assert!(
            source
                .read_range(b"/generated-object-0", 0, 4096, &AtomicBool::new(false))
                .is_err()
        );
    }
}
