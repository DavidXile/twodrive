//! Opt-in, leased test-process control carried by an existing paired file share.
use crate::state::{Share, State, now, random_secret};
use anyhow::{Context, ensure};
use cap_std::{ambient_authority, fs::Dir};
use iroh::{PublicKey, SecretKey, Signature};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
const JOB_DOMAIN: &[u8] = b"twodrive/test-agent/job/1\0";
const REPORT_DOMAIN: &[u8] = b"twodrive/test-agent/report/1\0";
const MAX_JSON: usize = 256 * 1024;
const MAX_LOG: usize = 16 * 1024;
const MAX_EXE: u64 = 128 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signed<T> {
    pub payload: T,
    pub signature: Signature,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Route {
    Auto,
    Direct,
    System,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Start {
        client: PublicKey,
        candidate_sha256: Option<String>,
        route: Route,
        missing_interface: bool,
        no_relay: bool,
        relay_only: bool,
        relays: Vec<String>,
    },
    Snapshot {},
    Stop {},
    Finish {},
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub version: u8,
    pub session: String,
    pub seq: u64,
    pub expires: u64,
    pub command: Action,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub version: u8,
    pub session: String,
    pub owner: PublicKey,
    pub seq: u64,
    pub expires: u64,
    pub status: String,
    pub data: Value,
}
fn signed_bytes(domain: &[u8], payload: &impl Serialize) -> anyhow::Result<Vec<u8>> {
    let mut bytes = domain.to_vec();
    bytes.extend(serde_json::to_vec(payload)?);
    Ok(bytes)
}
fn sign<T: Serialize>(key: &SecretKey, domain: &[u8], payload: T) -> anyhow::Result<Signed<T>> {
    let signature = key.sign(&signed_bytes(domain, &payload)?);
    Ok(Signed { payload, signature })
}
fn verify<T: Serialize>(key: PublicKey, domain: &[u8], value: &Signed<T>) -> anyhow::Result<()> {
    key.verify(&signed_bytes(domain, &value.payload)?, &value.signature)?;
    Ok(())
}
pub fn sign_job(state: &State, input: &Path, output: &Path) -> anyhow::Result<()> {
    let job: Job = read_bounded(input)?;
    ensure!(
        job.version == 1 && job.seq > 0,
        "invalid job version or sequence"
    );
    crate::state::write_new_json(output, &sign(&state.identity()?, JOB_DOMAIN, job)?)
}
pub fn verify_report(input: &Path, device: PublicKey, output: &Path) -> anyhow::Result<()> {
    let report: Signed<Report> = read_bounded(input)?;
    verify(device, REPORT_DOMAIN, &report)?;
    ensure!(
        report.payload.version == 1 && report.payload.expires >= now(),
        "expired report"
    );
    crate::state::write_new_json(output, &report.payload)
}
fn read_bounded<T: DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take((MAX_JSON + 1) as u64)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= MAX_JSON, "control JSON too large");
    Ok(serde_json::from_slice(&bytes)?)
}
fn atomic_report<T: Serialize>(dir: &Dir, report: &Signed<T>) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec(report)?;
    ensure!(bytes.len() <= MAX_JSON, "report too large");
    let name = format!("report-{}.tmp", random_secret());
    let result = (|| {
        let mut file = dir.open_with(
            &name,
            cap_std::fs::OpenOptions::new().write(true).create_new(true),
        )?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        dir.rename(&name, dir, "response.json")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = dir.remove_file(name);
    }
    result
}
fn digest(path: &Path) -> anyhow::Result<String> {
    let mut file = File::open(path)?;
    let mut sha = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        sha.update(&buffer[..count]);
    }
    Ok(format!("{:x}", sha.finalize()))
}
fn trusted(state: &State, owner: PublicKey) -> anyhow::Result<()> {
    ensure!(
        state.control(|c| Ok(c.peers.contains(&owner.to_string())))?,
        "control owner is not an authorized paired device"
    );
    Ok(())
}
fn accept(owner: PublicKey, session: &str, last: u64, value: &Signed<Job>) -> anyhow::Result<()> {
    verify(owner, JOB_DOMAIN, value)?;
    let job = &value.payload;
    ensure!(
        job.version == 1 && job.session == session,
        "wrong control session"
    );
    ensure!(
        job.seq > last && job.seq <= last.saturating_add(1024),
        "replayed or invalid sequence"
    );
    ensure!(
        job.expires >= now() && job.expires <= now() + 300,
        "expired or excessive job lifetime"
    );
    Ok(())
}
#[derive(Default)]
struct Tail {
    bytes: Arc<Mutex<VecDeque<u8>>>,
    reader: Option<JoinHandle<()>>,
}
impl Tail {
    fn capture(mut source: impl Read + Send + 'static) -> Self {
        let bytes = Arc::new(Mutex::new(VecDeque::new()));
        let target = bytes.clone();
        let reader = thread::spawn(move || {
            let mut buffer = [0; 4096];
            while let Ok(count) = source.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                let mut tail = target.lock().unwrap();
                tail.extend(&buffer[..count]);
                while tail.len() > MAX_LOG {
                    tail.pop_front();
                }
            }
        });
        Self {
            bytes,
            reader: Some(reader),
        }
    }
    fn text(&self) -> String {
        let bytes: Vec<_> = self.bytes.lock().unwrap().iter().copied().collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }
    fn join(&mut self) {
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
#[cfg(windows)]
struct WindowsJob(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl WindowsJob {
    fn attach(child: &mut Child) -> anyhow::Result<Self> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::*;
        // Non-inheritable handle: closing/crashing the controller kills all
        // processes in this job, including a test candidate's descendants.
        let job = Self(unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) });
        ensure!(
            !job.0.is_null(),
            "create owned Windows process job: {}",
            std::io::Error::last_os_error()
        );
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        ensure!(
            unsafe {
                SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    std::mem::size_of_val(&limits) as u32,
                )
            } != 0,
            "configure owned Windows process job: {}",
            std::io::Error::last_os_error()
        );
        if unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle()) } == 0 {
            let error = std::io::Error::last_os_error();
            if child.try_wait()?.is_none() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error).context("attach child to owned Windows process job");
            }
        }
        Ok(job)
    }
    fn terminate(&self) -> anyhow::Result<()> {
        ensure!(
            unsafe { windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0, 1) } != 0,
            "stop owned Windows process tree: {}",
            std::io::Error::last_os_error()
        );
        Ok(())
    }
}
#[cfg(windows)]
impl Drop for WindowsJob {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(self.0);
            }
        }
    }
}

struct Process {
    #[cfg(windows)]
    job: WindowsJob,
    child: Child,
    stdout: Tail,
    stderr: Tail,
}
impl Process {
    fn spawn(command: &mut Command) -> anyhow::Result<Self> {
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        #[cfg(windows)]
        let job = WindowsJob::attach(&mut child)?;
        let stdout = Tail::capture(child.stdout.take().unwrap());
        let stderr = Tail::capture(child.stderr.take().unwrap());
        Ok(Self {
            #[cfg(windows)]
            job,
            child,
            stdout,
            stderr,
        })
    }
    fn stop(&mut self) -> anyhow::Result<()> {
        #[cfg(windows)]
        self.job.terminate()?;
        #[cfg(not(windows))]
        if self.child.try_wait()?.is_none() {
            self.child.kill()?;
        }
        self.child.wait()?;
        self.stdout.join();
        self.stderr.join();
        Ok(())
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
struct TestChild {
    process: Process,
    #[cfg(windows)]
    sampler: Option<Process>,
    state: State,
    runtime: tempfile::TempDir,
    started: Instant,
    executable_sha256: String,
}
impl TestChild {
    fn start(dir: &Dir, action: &Action) -> anyhow::Result<Self> {
        let Action::Start {
            client,
            candidate_sha256,
            route,
            missing_interface,
            no_relay,
            relay_only,
            relays,
        } = action
        else {
            anyhow::bail!("not a start command")
        };
        ensure!(
            relays.len() <= 8 && !(*no_relay && (*relay_only || !relays.is_empty())),
            "invalid relay options"
        );
        for relay in relays {
            let url = url::Url::parse(relay)?;
            ensure!(
                url.scheme() == "https"
                    && url.username().is_empty()
                    && url.password().is_none()
                    && relay.len() <= 2048,
                "invalid HTTPS relay"
            );
        }
        let runtime = tempfile::Builder::new()
            .prefix("twodrive-test-child-")
            .tempdir()?;
        let state = State::new(runtime.path().join("state"))?;
        state.control(|c| {
            c.peers.insert(client.to_string());
            Ok(())
        })?;
        let root = runtime.path().join("share");
        fs::create_dir(&root)?;
        let exe = if let Some(expected) = candidate_sha256 {
            ensure!(
                expected.len() == 64 && expected.bytes().all(|b| b.is_ascii_hexdigit()),
                "invalid candidate SHA256"
            );
            let target = runtime.path().join(if cfg!(windows) {
                "candidate.exe"
            } else {
                "candidate"
            });
            let source = dir.open("candidate.bin")?;
            ensure!(
                source.metadata()?.is_file() && source.metadata()?.len() <= MAX_EXE,
                "candidate too large or not a file"
            );
            let mut output = File::create(&target)?;
            std::io::copy(&mut source.take(MAX_EXE + 1), &mut output)?;
            output.sync_all()?;
            drop(output);
            ensure!(
                fs::metadata(&target)?.len() <= MAX_EXE,
                "candidate too large"
            );
            ensure!(
                digest(&target)?.eq_ignore_ascii_case(expected),
                "candidate SHA256 mismatch"
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&target, fs::Permissions::from_mode(0o700))?;
            }
            target
        } else {
            std::env::current_exe()?
        };
        let executable_sha256 = digest(&exe)?;
        let mut command = Command::new(exe);
        command
            .arg("--state")
            .arg(&state.dir)
            .arg("serve")
            .arg("--root")
            .arg(&root)
            .arg("--write")
            .arg("--max-upload")
            .arg("67108864")
            .arg("--network-route")
            .arg(match route {
                Route::Auto => "auto",
                Route::Direct => "direct",
                Route::System => "system",
            });
        if *missing_interface {
            command
                .arg("--network-interface")
                .arg("twodrive-test-missing-interface");
        }
        if *no_relay {
            command.arg("--no-relay");
        }
        if *relay_only {
            command.arg("--relay-only");
        }
        for relay in relays {
            command.arg("--relay").arg(relay);
        }
        let process = Process::spawn(&mut command)?;
        #[cfg(windows)]
        let sampler = {
            let script = runtime.path().join("sample.ps1");
            fs::write(&script, include_str!("../scripts/agent-sample.ps1"))?;
            Process::spawn(
                Command::new("powershell.exe")
                    .arg("-NoProfile")
                    .arg("-ExecutionPolicy")
                    .arg("Bypass")
                    .arg("-File")
                    .arg(script)
                    .arg("-ProcessId")
                    .arg(process.child.id().to_string())
                    .arg("-OutputDirectory")
                    .arg(runtime.path()),
            )
            .ok()
        };
        Ok(Self {
            process,
            #[cfg(windows)]
            sampler,
            state,
            runtime,
            started: Instant::now(),
            executable_sha256,
        })
    }
    fn snapshot(&mut self) -> anyhow::Result<Value> {
        let exited = self.process.child.try_wait()?;
        if exited.is_some() {
            self.process.stdout.join();
            self.process.stderr.join();
        }
        let ready = exited.is_none() && self.process.stdout.text().contains("Serving device=");
        let share = if ready {
            Some(self.state.read::<Share>("share.json")?.address)
        } else {
            None
        };
        let network = read_bounded::<Value>(&self.runtime.path().join("stats.json")).ok();
        #[cfg(windows)] let sampler = self.sampler.as_mut().map(|p| json!({"pid":p.child.id(), "exited":p.child.try_wait().ok().flatten().is_some(), "stderr":p.stderr.text()}));
        #[cfg(not(windows))]
        let sampler = Value::Null;
        Ok(
            json!({"pid": self.process.child.id(), "ready": ready, "exited": exited.is_some(), "exit_code": exited.and_then(|s| s.code()), "age_seconds": self.started.elapsed().as_secs(), "address": share, "executable_sha256": self.executable_sha256, "stdout": self.process.stdout.text(), "stderr": self.process.stderr.text(), "network": network, "sampler": sampler}),
        )
    }
    fn stop(&mut self) -> anyhow::Result<Value> {
        #[cfg(windows)]
        if let Some(sampler) = self.sampler.as_mut() {
            fs::write(self.runtime.path().join("stop"), b"stop")?;
            let deadline = Instant::now() + Duration::from_secs(10);
            while sampler.child.try_wait()?.is_none() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(100));
            }
            sampler.stop()?;
        }
        self.process.stop()?;
        self.snapshot()
    }
    fn finish(mut self) -> anyhow::Result<Value> {
        let snapshot = self.stop()?;
        drop(self.process);
        #[cfg(windows)]
        drop(self.sampler);
        self.runtime.close().context("remove owned child runtime")?;
        Ok(snapshot)
    }
}
/// The absolute lease cannot be extended by a job. Errors reap the owned child.
pub fn run(
    state: &State,
    queue: &Path,
    owner: PublicKey,
    lifetime: u64,
    cancelled: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    ensure!(
        (120..=7200).contains(&lifetime),
        "agent lifetime must be 120..7200 seconds"
    );
    trusted(state, owner)?;
    let queue = queue.canonicalize().context("canonicalize control queue")?;
    ensure!(
        queue.is_dir() && !state.dir.starts_with(&queue),
        "queue must be separate from private state"
    );
    let dir = Dir::open_ambient_dir(&queue, ambient_authority())
        .context("open control queue capability")?;
    let lock = dir
        .open_with(
            "agent.lock",
            cap_std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true),
        )?
        .into_std();
    fs2::FileExt::try_lock_exclusive(&lock).context("another controller holds this queue")?;
    let key = state.identity()?;
    let session = random_secret();
    let lease_expires = now() + lifetime;
    let expires = lease_expires + 300;
    let deadline = Instant::now() + Duration::from_secs(lifetime);
    let mut report = Report {
        version: 1,
        session: session.clone(),
        owner,
        seq: 0,
        expires,
        status: "ready".into(),
        data: json!({"device": key.public(), "agent_sha256": digest(&std::env::current_exe()?).context("hash controller executable")?, "agent_pid": std::process::id(), "lease_expires": lease_expires, "proxy_environment_present": (["HTTP_PROXY","HTTPS_PROXY","ALL_PROXY","http_proxy","https_proxy","all_proxy"].iter().any(|name| std::env::var_os(name).is_some()))}),
    };
    atomic_report(&dir, &sign(&key, REPORT_DOMAIN, &report)?)
        .context("write signed control response")?;
    println!("Test control ready; paired owner pinned; lease={lifetime}s");
    let mut child: Option<TestChild> = None;
    let mut last_job = Vec::new();
    let mut rejected = 0u64;
    loop {
        if cancelled.load(Ordering::Relaxed)
            || Instant::now() >= deadline
            || trusted(state, owner).is_err()
        {
            break;
        }
        if let Some(child) = child.as_mut()
            && child.started.elapsed() > Duration::from_secs(1200)
        {
            child.stop()?;
        }
        let bytes = match dir.open("command.json") {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take((MAX_JSON + 1) as u64).read_to_end(&mut bytes)?;
                bytes
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                thread::sleep(Duration::from_millis(200));
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        if bytes == last_job {
            thread::sleep(Duration::from_millis(200));
            continue;
        }
        last_job = bytes.clone();
        let value = (|| {
            ensure!(bytes.len() <= MAX_JSON, "control JSON too large");
            let value: Signed<Job> = serde_json::from_slice(&bytes)?;
            accept(owner, &session, report.seq, &value)?;
            Ok::<_, anyhow::Error>(value)
        })();
        let value = match value {
            Ok(value) => value,
            Err(_) => {
                rejected += 1;
                eprintln!("test control: rejected unsigned, stale or invalid command ({rejected})");
                continue;
            }
        };
        report.seq = value.payload.seq;
        let finish = matches!(value.payload.command, Action::Finish {});
        let result = match &value.payload.command {
            Action::Start { .. } => {
                if let Some(previous) = child.take() {
                    previous.finish()?;
                }
                match TestChild::start(&dir, &value.payload.command) {
                    Ok(mut started) => {
                        let data = started.snapshot();
                        child = Some(started);
                        data
                    }
                    Err(error) => Err(error),
                }
            }
            Action::Snapshot {} => child
                .as_mut()
                .map(TestChild::snapshot)
                .unwrap_or_else(|| Ok(json!({"running": false}))),
            Action::Stop {} | Action::Finish {} => {
                let data = if let Some(stopped) = child.take() {
                    stopped.finish()
                } else {
                    Ok(json!({"running": false}))
                };
                data.map(|snapshot| json!({"child": snapshot, "temporary_child_removed": true}))
            }
        };
        match result {
            Ok(data) => {
                report.status = if finish { "finished" } else { "ok" }.into();
                report.data = data;
            }
            Err(error) => {
                report.status = "error".into();
                report.data = json!({"error": format!("{error:#}")});
            }
        }
        report.data["request_signature"] = serde_json::to_value(value.signature)?;
        atomic_report(&dir, &sign(&key, REPORT_DOMAIN, &report)?)
            .context("write signed control response")?;
        if finish {
            return Ok(());
        }
    }
    let cleanup = if let Some(child) = child.take() {
        child.finish()?
    } else {
        json!({"running": false})
    };
    report.status = "expired_or_revoked".into();
    report.data = json!({"child": cleanup, "temporary_child_removed": true});
    atomic_report(&dir, &sign(&key, REPORT_DOMAIN, report)?)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn job(session: &str, seq: u64) -> Job {
        Job {
            version: 1,
            session: session.into(),
            seq,
            expires: now() + 60,
            command: Action::Stop {},
        }
    }
    #[test]
    fn owner_signature_session_expiry_and_sequence_are_enforced() {
        let key = SecretKey::generate();
        let good = sign(&key, JOB_DOMAIN, job("session", 1)).unwrap();
        assert!(accept(key.public(), "session", 0, &good).is_ok());
        assert!(accept(key.public(), "session", 1, &good).is_err());
        assert!(accept(key.public(), "other", 0, &good).is_err());
        let wrong = SecretKey::generate();
        assert!(accept(wrong.public(), "session", 0, &good).is_err());
        let mut tampered = good;
        tampered.payload.seq = 2;
        assert!(accept(key.public(), "session", 0, &tampered).is_err());
        let mut stale = job("session", 1);
        stale.expires = now() - 1;
        assert!(
            accept(
                key.public(),
                "session",
                0,
                &sign(&key, JOB_DOMAIN, stale).unwrap()
            )
            .is_err()
        );
        assert!(
            verify(
                key.public(),
                REPORT_DOMAIN,
                &sign(&key, JOB_DOMAIN, job("session", 1)).unwrap()
            )
            .is_err()
        );
    }
    #[test]
    fn unknown_actions_and_shell_fields_are_rejected() {
        assert!(
            serde_json::from_value::<Action>(json!({"action":"shell", "command":"whoami"}))
                .is_err()
        );
        assert!(serde_json::from_value::<Action>(json!({"action":"stop", "path":".."})).is_err());
    }
    #[test]
    fn redirected_failure_logs_are_bounded_and_kept_after_exit() {
        #[cfg(windows)]
        let mut command = {
            let mut c = Command::new("cmd.exe");
            c.args(["/C", "echo controlled-failure 1>&2 & exit /B 23"]);
            c
        };
        #[cfg(not(windows))]
        let mut command = {
            let mut c = Command::new("sh");
            c.args(["-c", "printf controlled-failure >&2; exit 23"]);
            c
        };
        let mut process = Process::spawn(&mut command).unwrap();
        assert_eq!(process.child.wait().unwrap().code(), Some(23));
        process.stop().unwrap();
        assert!(process.stderr.text().contains("controlled-failure"));
        let mut tail = Tail::capture(std::io::Cursor::new(vec![b'x'; MAX_LOG * 4]));
        tail.join();
        assert_eq!(tail.text().len(), MAX_LOG);
    }
}
