// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The local compute host (DM-RPC-004, DM-RPC-009): a durable job registry, runners started on
//! demand under operating-system limits and stopped when idle, and the host itself exiting when
//! it has nothing to do. Jobs outlive the client that submitted them.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use prost::Message;
use reviera_compute_proto::blob::{self, Assembler};
use reviera_compute_proto::negotiate;
use reviera_compute_proto::pb::job_event::Event;
use reviera_compute_proto::pb::*;
use reviera_compute_proto::runner::{is_terminal, result_chunks};
use reviera_compute_proto::transport::{self, Address, Client};
use reviera_compute_proto::{CONTRACT_VERSION, now_ms};
use rusqlite::{Connection, OptionalExtension, params};
use tokio::sync::{Notify, mpsc, watch};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Code, Request, Response, Status, Streaming};

use crate::paths::{self, HostInfo};

/// How the host runs.
#[derive(Debug, Clone)]
pub struct HostConfig {
    pub state_dir: PathBuf,
    /// Runner programs; each lists its kinds with `--describe`.
    pub runners: Vec<PathBuf>,
    /// Exit after this long with no jobs, no runners and no calls.
    pub idle: Duration,
    /// Stop a runner after this long without jobs.
    pub runner_idle: Duration,
    /// Memory ceiling for each runner process.
    pub memory_limit: Option<u64>,
    /// Fail a job that sends no event for this long.
    pub heartbeat_timeout: Duration,
    /// How long results are kept.
    pub retention: Duration,
}

impl HostConfig {
    /// From the environment: `REVIERA_COMPUTE_STATE`, `REVIERA_COMPUTE_RUNNERS` (a path list),
    /// `REVIERA_COMPUTE_HOST_IDLE_S`, `REVIERA_COMPUTE_RUNNER_IDLE_S`,
    /// `REVIERA_COMPUTE_MEMORY_MB`, `REVIERA_COMPUTE_HEARTBEAT_S`.
    pub fn from_env() -> HostConfig {
        let secs = |name: &str, default: u64| {
            Duration::from_secs(
                std::env::var(name)
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(default),
            )
        };
        let runners = match std::env::var_os("REVIERA_COMPUTE_RUNNERS") {
            Some(v) => std::env::split_paths(&v).collect(),
            None => paths::bundled_runners(),
        };
        let memory_limit = std::env::var("REVIERA_COMPUTE_MEMORY_MB")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map(|mb| mb << 20)
            .or_else(|| paths::physical_memory().map(|m| m / 2));
        HostConfig {
            state_dir: paths::state_dir(),
            runners,
            idle: secs("REVIERA_COMPUTE_HOST_IDLE_S", 120),
            runner_idle: secs("REVIERA_COMPUTE_RUNNER_IDLE_S", 120),
            memory_limit,
            heartbeat_timeout: secs("REVIERA_COMPUTE_HEARTBEAT_S", 60),
            retention: Duration::from_secs(24 * 3600),
        }
    }
}

// ---- The registry ----

struct Db(Mutex<Connection>);

impl Db {
    fn open(path: &Path) -> rusqlite::Result<Db> {
        let c = Connection::open(path)?;
        c.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS jobs (
                 job_id TEXT PRIMARY KEY, client_key TEXT, record BLOB NOT NULL,
                 request BLOB NOT NULL, manifest BLOB);
             CREATE UNIQUE INDEX IF NOT EXISTS jobs_key ON jobs(client_key) WHERE client_key <> '';
             CREATE TABLE IF NOT EXISTS events (
                 job_id TEXT NOT NULL, seq INTEGER NOT NULL, event BLOB NOT NULL,
                 PRIMARY KEY (job_id, seq));",
        )?;
        Ok(Db(Mutex::new(c)))
    }

    fn insert(&self, rec: &JobRecord, req: &SubmitJobRequest) -> rusqlite::Result<()> {
        self.0.lock().unwrap().execute(
            "INSERT INTO jobs (job_id, client_key, record, request) VALUES (?1, ?2, ?3, ?4)",
            params![
                rec.job_id,
                rec.client_job_key,
                rec.encode_to_vec(),
                req.encode_to_vec()
            ],
        )?;
        Ok(())
    }

    fn put(&self, rec: &JobRecord) {
        let _ = self.0.lock().unwrap().execute(
            "UPDATE jobs SET record = ?2 WHERE job_id = ?1",
            params![rec.job_id, rec.encode_to_vec()],
        );
    }

    fn record(&self, id: &str) -> Option<JobRecord> {
        self.0
            .lock()
            .unwrap()
            .query_row("SELECT record FROM jobs WHERE job_id = ?1", [id], |r| {
                r.get::<_, Vec<u8>>(0)
            })
            .optional()
            .ok()
            .flatten()
            .and_then(|b| JobRecord::decode(b.as_slice()).ok())
    }

    fn request(&self, id: &str) -> Option<SubmitJobRequest> {
        self.0
            .lock()
            .unwrap()
            .query_row("SELECT request FROM jobs WHERE job_id = ?1", [id], |r| {
                r.get::<_, Vec<u8>>(0)
            })
            .optional()
            .ok()
            .flatten()
            .and_then(|b| SubmitJobRequest::decode(b.as_slice()).ok())
    }

    fn by_key(&self, key: &str) -> Option<String> {
        if key.is_empty() {
            return None;
        }
        self.0
            .lock()
            .unwrap()
            .query_row(
                "SELECT job_id FROM jobs WHERE client_key = ?1",
                [key],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten()
    }

    fn all(&self) -> Vec<JobRecord> {
        let c = self.0.lock().unwrap();
        let mut st = c.prepare("SELECT record FROM jobs").unwrap();
        st.query_map([], |r| r.get::<_, Vec<u8>>(0))
            .unwrap()
            .filter_map(|b| b.ok().and_then(|b| JobRecord::decode(b.as_slice()).ok()))
            .collect()
    }

    fn add_event(&self, id: &str, e: &JobEvent) {
        let _ = self.0.lock().unwrap().execute(
            "INSERT OR REPLACE INTO events (job_id, seq, event) VALUES (?1, ?2, ?3)",
            params![id, e.seq as i64, e.encode_to_vec()],
        );
    }

    fn events_after(&self, id: &str, seq: u64) -> Vec<JobEvent> {
        let c = self.0.lock().unwrap();
        let mut st = c
            .prepare("SELECT event FROM events WHERE job_id = ?1 AND seq > ?2 ORDER BY seq")
            .unwrap();
        st.query_map(params![id, seq as i64], |r| r.get::<_, Vec<u8>>(0))
            .unwrap()
            .filter_map(|b| b.ok().and_then(|b| JobEvent::decode(b.as_slice()).ok()))
            .collect()
    }

    fn set_manifest(&self, id: &str, m: &ResultManifest) {
        let _ = self.0.lock().unwrap().execute(
            "UPDATE jobs SET manifest = ?2 WHERE job_id = ?1",
            params![id, m.encode_to_vec()],
        );
    }

    fn manifest(&self, id: &str) -> Option<ResultManifest> {
        self.0
            .lock()
            .unwrap()
            .query_row("SELECT manifest FROM jobs WHERE job_id = ?1", [id], |r| {
                r.get::<_, Option<Vec<u8>>>(0)
            })
            .optional()
            .ok()
            .flatten()
            .flatten()
            .and_then(|b| ResultManifest::decode(b.as_slice()).ok())
    }
}

// ---- Runners ----

#[derive(Debug, Clone)]
struct Spec {
    exe: PathBuf,
    info: KindInfo,
}

struct RunnerProc {
    child: tokio::process::Child,
    client: Client,
    // Its socket, removed when it stops (Unix sockets; Windows' pipes go by themselves).
    #[cfg_attr(not(unix), allow(dead_code))]
    address: Address,
    log: PathBuf,
    running: u32,
    last_used: Instant,
}

/// The host.
pub struct Host {
    cfg: HostConfig,
    db: Db,
    kinds: HashMap<String, Spec>,
    runners: tokio::sync::Mutex<HashMap<PathBuf, RunnerProc>>,
    watchers: Mutex<HashMap<String, watch::Sender<u64>>>,
    active: Mutex<HashMap<String, Option<(Client, String)>>>,
    queue: mpsc::UnboundedSender<String>,
    last_call: Mutex<Instant>,
    stop: Notify,
    spawned: std::sync::atomic::AtomicU64,
}

impl std::fmt::Debug for Host {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Host")
            .field("cfg", &self.cfg)
            .finish_non_exhaustive()
    }
}

fn describe_runner(exe: &Path) -> Vec<KindInfo> {
    let Ok(out) = std::process::Command::new(exe).arg("--describe").output() else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else {
        return Vec::new();
    };
    let runner = v["runner"].as_str().unwrap_or_default().to_owned();
    let version = v["version"].as_str().unwrap_or_default().to_owned();
    v["kinds"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|k| KindInfo {
            kind: k["kind"].as_str().unwrap_or_default().into(),
            version: k["version"].as_u64().unwrap_or(1) as u32,
            runner: runner.clone(),
            runner_version: version.clone(),
            licence: k["licence"].as_str().unwrap_or_default().into(),
            supports_checkpoints: k["supports_checkpoints"].as_bool().unwrap_or(false),
            ..Default::default()
        })
        .collect()
}

fn failure(reason: &str, message: impl Into<String>, output: String) -> Failure {
    Failure {
        reason: reason.into(),
        message: message.into(),
        runner_output: output,
    }
}

fn tail(path: &Path, bytes: usize) -> String {
    let s = std::fs::read(path).unwrap_or_default();
    let from = s.len().saturating_sub(bytes);
    String::from_utf8_lossy(&s[from..]).into_owned()
}

impl Host {
    fn touch(&self) {
        *self.last_call.lock().unwrap() = Instant::now();
    }

    fn blob_path(&self, sha: &str) -> PathBuf {
        self.cfg.state_dir.join("blobs").join(sha)
    }

    fn result_path(&self, id: &str) -> PathBuf {
        self.cfg.state_dir.join("results").join(format!("{id}.bin"))
    }

    /// Records an event, updates the job's record from it, and wakes watchers.
    fn push(&self, id: &str, event: Event) {
        let Some(mut rec) = self.db.record(id) else {
            return;
        };
        let seq = rec.last_seq + 1;
        let e = JobEvent {
            seq,
            time_ms: now_ms(),
            event: Some(event.clone()),
        };
        self.db.add_event(id, &e);
        rec.last_seq = seq;
        match &event {
            Event::Progress(p) => {
                rec.progress = p.fraction;
                rec.phase = p.phase.clone();
            }
            Event::State(s) => {
                rec.state = s.state;
                rec.failure = s.failure.clone();
                match JobState::try_from(s.state) {
                    Ok(JobState::Running) => rec.started_ms = now_ms(),
                    _ if is_terminal(s.state) => rec.finished_ms = now_ms(),
                    _ => {}
                }
            }
            _ => {}
        }
        self.db.put(&rec);
        let w = self
            .watchers
            .lock()
            .unwrap()
            .entry(id.to_owned())
            .or_insert_with(|| watch::channel(0).0)
            .clone();
        w.send_replace(seq);
    }

    fn set_state(&self, id: &str, state: JobState, f: Option<Failure>) {
        self.push(
            id,
            Event::State(StateChange {
                state: state as i32,
                failure: f,
            }),
        );
    }

    /// Starts `exe` as a runner if it is not running, and returns its client.
    async fn runner(&self, exe: &Path) -> Result<Client, Failure> {
        let mut runners = self.runners.lock().await;
        if let Some(r) = runners.get_mut(exe) {
            if r.child.try_wait().ok().flatten().is_none() {
                r.running += 1;
                r.last_used = Instant::now();
                return Ok(r.client.clone());
            }
            runners.remove(exe);
        }
        let n = self
            .spawned
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let stem = exe
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "runner".into());
        let run_dir = self.cfg.state_dir.join("run");
        let _ = std::fs::create_dir_all(&run_dir);
        #[cfg(unix)]
        let address =
            Address::Unix(paths::socket_dir(&self.cfg.state_dir).join(format!("{stem}-{n}.sock")));
        #[cfg(not(unix))]
        let address = {
            let l = std::net::TcpListener::bind("127.0.0.1:0")
                .map_err(|e| failure("runner_failed", e.to_string(), String::new()))?;
            Address::Tcp(l.local_addr().unwrap())
        };
        let token = transport::new_token();
        let log = run_dir.join(format!("{stem}-{n}.log"));
        let stderr = std::fs::File::create(&log)
            .map_err(|e| failure("runner_failed", e.to_string(), String::new()))?;
        let mut cmd = tokio::process::Command::new(exe);
        cmd.arg("--serve")
            .arg(address.to_string())
            .env("REVIERA_COMPUTE_TOKEN", &token)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(stderr);
        #[cfg(unix)]
        {
            let limit = self.cfg.memory_limit;
            // SAFETY: only async-signal-safe calls between fork and exec.
            unsafe {
                cmd.pre_exec(move || {
                    libc::setpriority(libc::PRIO_PROCESS, 0, 10);
                    #[cfg(target_os = "linux")]
                    libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                    // No core dumps: a runner's memory holds model data.
                    let none = libc::rlimit {
                        rlim_cur: 0,
                        rlim_max: 0,
                    };
                    libc::setrlimit(libc::RLIMIT_CORE, &none);
                    if let Some(l) = limit {
                        let r = libc::rlimit {
                            rlim_cur: l as libc::rlim_t,
                            rlim_max: l as libc::rlim_t,
                        };
                        libc::setrlimit(libc::RLIMIT_DATA, &r);
                    }
                    Ok(())
                });
            }
        }
        let mut child = cmd.spawn().map_err(|e| {
            failure(
                "runner_failed",
                format!("cannot start {}: {e}", exe.display()),
                String::new(),
            )
        })?;
        let deadline = Instant::now() + Duration::from_secs(30);
        let client = loop {
            if let Ok(Some(status)) = child.try_wait() {
                return Err(failure(
                    "runner_failed",
                    format!("the runner exited while starting ({status})"),
                    tail(&log, 4096),
                ));
            }
            if let Ok(mut c) = transport::connect(&address, &token).await
                && c.health(HealthRequest {}).await.is_ok()
            {
                break c;
            }
            if Instant::now() > deadline {
                let _ = child.start_kill();
                return Err(failure(
                    "runner_failed",
                    "the runner did not answer within 30 s",
                    tail(&log, 4096),
                ));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        runners.insert(
            exe.to_path_buf(),
            RunnerProc {
                child,
                client: client.clone(),
                address,
                log,
                running: 1,
                last_used: Instant::now(),
            },
        );
        Ok(client)
    }

    async fn release(&self, exe: &Path) {
        if let Some(r) = self.runners.lock().await.get_mut(exe) {
            r.running = r.running.saturating_sub(1);
            r.last_used = Instant::now();
        }
    }

    /// Why a runner's connection broke: its exit and the end of its error output.
    async fn runner_failure(&self, exe: &Path, context: &str) -> Failure {
        let mut runners = self.runners.lock().await;
        let Some(mut r) = runners.remove(exe) else {
            return failure("runner_failed", context, String::new());
        };
        let mut status = None;
        for _ in 0..40 {
            if let Ok(Some(s)) = r.child.try_wait() {
                status = Some(s);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        if status.is_none() {
            let _ = r.child.start_kill();
        }
        #[cfg(unix)]
        if let Address::Unix(p) = &r.address {
            let _ = std::fs::remove_file(p);
        }
        let out = tail(&r.log, 4096);
        let killed = {
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                status.and_then(|s| s.signal()) == Some(libc::SIGKILL)
            }
            #[cfg(not(unix))]
            {
                false
            }
        };
        let reason =
            if out.contains("memory allocation of") || out.contains("out of memory") || killed {
                "memory_limit"
            } else {
                "runner_failed"
            };
        let what = match status {
            Some(s) => format!("{context}: the runner exited ({s})"),
            None => format!("{context}: the runner stopped answering"),
        };
        failure(reason, what, out)
    }

    async fn run_job(self: &Arc<Self>, id: String) {
        self.run_job_inner(&id).await;
        // Whatever happened, the job no longer keeps the host busy.
        self.active.lock().unwrap().remove(&id);
    }

    async fn run_job_inner(self: &Arc<Self>, id: &str) {
        let id = id.to_owned();
        let Some(req) = self.db.request(&id) else {
            return;
        };
        // Cancelled while it was queued.
        if self.db.record(&id).is_none_or(|r| is_terminal(r.state)) {
            return;
        }
        let Some(spec) = self.kinds.get(&req.kind).cloned() else {
            self.set_state(
                &id,
                JobState::Failed,
                Some(failure("unsupported_kind", req.kind.clone(), String::new())),
            );
            return;
        };
        self.set_state(&id, JobState::Starting, None);
        let mut client = match self.runner(&spec.exe).await {
            Ok(c) => c,
            Err(f) => {
                self.set_state(&id, JobState::Failed, Some(f));
                return;
            }
        };
        let outcome = self.drive(&id, &req, &spec, &mut client).await;
        match outcome {
            Ok(()) => {}
            Err(f) => self.set_state(&id, JobState::Failed, Some(f)),
        }
        self.release(&spec.exe).await;
    }

    async fn drive(
        self: &Arc<Self>,
        id: &str,
        req: &SubmitJobRequest,
        spec: &Spec,
        client: &mut Client,
    ) -> Result<(), Failure> {
        let broke = |e: Status| e.to_string();
        let nego = client
            .negotiate(NegotiateRequest {
                contract_versions: vec![CONTRACT_VERSION.into()],
                offers: negotiate::supported(),
            })
            .await
            .map_err(|e| failure("runner_failed", broke(e), String::new()))?
            .into_inner();
        let codec = negotiate::chosen(&nego.choices, negotiate::COMPRESSION)
            .unwrap_or(negotiate::IDENTITY_V1)
            .to_owned();
        // Inputs the runner does not hold yet.
        let present = client
            .has_blobs(HasBlobsRequest {
                sha256: req.inputs.iter().map(|b| b.sha256.clone()).collect(),
            })
            .await
            .map_err(|e| failure("runner_failed", broke(e), String::new()))?
            .into_inner()
            .present;
        let mut chunks = Vec::new();
        for r in req.inputs.iter().filter(|r| !present.contains(&r.sha256)) {
            let raw = std::fs::read(self.blob_path(&r.sha256)).map_err(|e| {
                failure(
                    "invalid_input",
                    format!("blob {}: {e}", r.sha256),
                    String::new(),
                )
            })?;
            let b = blob::Blob {
                reference: r.clone(),
                raw,
            };
            chunks.extend(
                blob::chunks(&b, &codec).map_err(|e| failure("runner_failed", e, String::new()))?,
            );
        }
        if !chunks.is_empty()
            && let Err(e) = client.put_blobs(tokio_stream::iter(chunks)).await
        {
            return Err(self.runner_failure(&spec.exe, &broke(e)).await);
        }
        let mut forwarded = req.clone();
        forwarded.client_job_key = id.to_owned();
        let runner_job = match client.submit_job(forwarded).await {
            Ok(r) => r.into_inner().job_id,
            Err(e) if transport::reason(&e).is_some() => {
                return Err(failure(
                    &transport::reason(&e).unwrap(),
                    e.message().to_owned(),
                    String::new(),
                ));
            }
            Err(e) => return Err(self.runner_failure(&spec.exe, &broke(e)).await),
        };
        self.active
            .lock()
            .unwrap()
            .insert(id.to_owned(), Some((client.clone(), runner_job.clone())));
        self.set_state(id, JobState::Running, None);
        let mut stream = match client
            .watch_job(WatchJobRequest {
                job_id: runner_job.clone(),
                after_seq: 0,
            })
            .await
        {
            Ok(s) => s.into_inner(),
            Err(e) => return Err(self.runner_failure(&spec.exe, &broke(e)).await),
        };
        loop {
            let next = tokio::time::timeout(self.cfg.heartbeat_timeout, stream.message()).await;
            let ev = match next {
                Err(_) => {
                    let f = self
                        .runner_failure(
                            &spec.exe,
                            &format!(
                                "no progress or heartbeat for {} s",
                                self.cfg.heartbeat_timeout.as_secs()
                            ),
                        )
                        .await;
                    return Err(f);
                }
                Ok(Ok(Some(ev))) => ev,
                Ok(Ok(None)) => {
                    return Err(self
                        .runner_failure(&spec.exe, "the event stream ended")
                        .await);
                }
                Ok(Err(e)) => return Err(self.runner_failure(&spec.exe, &broke(e)).await),
            };
            let Some(event) = ev.event else { continue };
            match event {
                Event::State(s) if is_terminal(s.state) => {
                    match JobState::try_from(s.state) {
                        Ok(JobState::Succeeded) => {
                            self.fetch_results(id, client, &runner_job, &spec.exe)
                                .await?;
                            self.set_state(id, JobState::Succeeded, None);
                        }
                        Ok(JobState::Cancelled) => self.set_state(id, JobState::Cancelled, None),
                        _ => self.set_state(id, JobState::Failed, s.failure),
                    }
                    let _ = client
                        .ack_results(AckResultsRequest { job_id: runner_job })
                        .await;
                    return Ok(());
                }
                Event::State(_) => {}
                other => self.push(id, other),
            }
        }
    }

    async fn fetch_results(
        &self,
        id: &str,
        client: &mut Client,
        runner_job: &str,
        exe: &Path,
    ) -> Result<(), Failure> {
        let mut stream = match client
            .get_results(GetResultsRequest {
                job_id: runner_job.into(),
                offset: 0,
            })
            .await
        {
            Ok(s) => s.into_inner(),
            Err(e) => return Err(self.runner_failure(exe, &e.to_string()).await),
        };
        let mut manifest = None;
        let mut data = Vec::new();
        loop {
            match stream.message().await {
                Ok(Some(c)) => {
                    if c.manifest.is_some() {
                        manifest = c.manifest;
                    }
                    data.extend_from_slice(&c.data);
                    if c.last {
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => return Err(self.runner_failure(exe, &e.to_string()).await),
            }
        }
        let mut manifest = manifest
            .ok_or_else(|| failure("runner_failed", "results without a manifest", String::new()))?;
        manifest.host = format!("reviera-compute-host {}", env!("CARGO_PKG_VERSION"));
        let path = self.result_path(id);
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, &data)
            .and_then(|_| std::fs::rename(&tmp, &path))
            .map_err(|e| {
                failure(
                    "runner_failed",
                    format!("storing results: {e}"),
                    String::new(),
                )
            })?;
        self.db.set_manifest(id, &manifest);
        if let Some(mut rec) = self.db.record(id) {
            rec.result_sha256 = manifest.sha256.clone();
            rec.result_size = manifest.size;
            rec.expires_ms = now_ms() + self.cfg.retention.as_millis() as i64;
            self.db.put(&rec);
        }
        Ok(())
    }

    /// Stops idle runners, notices dead ones, and stops the host when idle.
    async fn reap(self: &Arc<Self>) {
        loop {
            tokio::time::sleep(Duration::from_millis(250)).await;
            let mut stopping = Vec::new();
            {
                let mut runners = self.runners.lock().await;
                let mut idle: Vec<PathBuf> = Vec::new();
                for (k, r) in runners.iter_mut() {
                    if r.child.try_wait().ok().flatten().is_some()
                        || (r.running == 0 && r.last_used.elapsed() > self.cfg.runner_idle)
                    {
                        idle.push(k.clone());
                    }
                }
                for k in idle {
                    if let Some(r) = runners.remove(&k) {
                        stopping.push(r);
                    }
                }
            }
            for mut r in stopping {
                let _ = r.client.shutdown(ShutdownRequest {}).await;
                tokio::spawn(async move {
                    if tokio::time::timeout(Duration::from_secs(10), r.child.wait())
                        .await
                        .is_err()
                    {
                        let _ = r.child.start_kill();
                    }
                    #[cfg(unix)]
                    if let Address::Unix(p) = &r.address {
                        let _ = std::fs::remove_file(p);
                    }
                });
            }
            let busy = !self.active.lock().unwrap().is_empty();
            let runners = !self.runners.lock().await.is_empty();
            let quiet = self.last_call.lock().unwrap().elapsed() > self.cfg.idle;
            if !busy && !runners && quiet {
                self.stop.notify_one();
                return;
            }
        }
    }
}

type BoxStream<T> = std::pin::Pin<Box<dyn tokio_stream::Stream<Item = Result<T, Status>> + Send>>;

/// The host as a gRPC service.
#[derive(Debug, Clone)]
pub struct Service(Arc<Host>);

impl std::ops::Deref for Service {
    type Target = Arc<Host>;
    fn deref(&self) -> &Arc<Host> {
        &self.0
    }
}

#[tonic::async_trait]
impl compute_server::Compute for Service {
    async fn negotiate(
        &self,
        req: Request<NegotiateRequest>,
    ) -> Result<Response<NegotiateResponse>, Status> {
        self.touch();
        let r = req.into_inner();
        if !r.contract_versions.is_empty()
            && !r.contract_versions.iter().any(|v| v == CONTRACT_VERSION)
        {
            return Err(transport::status(
                Code::FailedPrecondition,
                "version_mismatch",
                format!("this host speaks {CONTRACT_VERSION}"),
            ));
        }
        let choices = negotiate::choose(&r.offers, &negotiate::supported())
            .map_err(|e| transport::status(Code::FailedPrecondition, "negotiation_failed", e))?;
        Ok(Response::new(NegotiateResponse {
            contract_version: CONTRACT_VERSION.into(),
            choices,
        }))
    }

    async fn describe(
        &self,
        _: Request<DescribeRequest>,
    ) -> Result<Response<DescribeResponse>, Status> {
        self.touch();
        let mut kinds: Vec<KindInfo> = self.kinds.values().map(|s| s.info.clone()).collect();
        kinds.sort_by(|a, b| a.kind.cmp(&b.kind));
        Ok(Response::new(DescribeResponse {
            contract_versions: vec![CONTRACT_VERSION.into()],
            kinds,
            resources: Some(Resources {
                cores: std::thread::available_parallelism()
                    .map(|n| n.get() as u32)
                    .unwrap_or(1),
                memory_bytes: paths::physical_memory().unwrap_or(0),
                gpus: 0,
            }),
            max_job_bytes: 0,
            server: format!("reviera-compute-host {}", env!("CARGO_PKG_VERSION")),
        }))
    }

    async fn health(&self, _: Request<HealthRequest>) -> Result<Response<HealthResponse>, Status> {
        self.touch();
        let active = self.active.lock().unwrap().len() as u32;
        Ok(Response::new(HealthResponse {
            ready: true,
            jobs_running: active,
            jobs_queued: 0,
            memory_bytes: 0,
        }))
    }

    async fn has_blobs(
        &self,
        req: Request<HasBlobsRequest>,
    ) -> Result<Response<HasBlobsResponse>, Status> {
        self.touch();
        Ok(Response::new(HasBlobsResponse {
            present: req
                .into_inner()
                .sha256
                .into_iter()
                .filter(|h| h.len() == 64 && self.blob_path(h).exists())
                .collect(),
        }))
    }

    async fn put_blobs(
        &self,
        req: Request<Streaming<BlobChunk>>,
    ) -> Result<Response<PutBlobsResponse>, Status> {
        self.touch();
        let mut stream = req.into_inner();
        let mut asm = Assembler::default();
        let mut stored = Vec::new();
        while let Some(chunk) = stream.message().await? {
            self.touch();
            if let Some(b) = asm
                .push(chunk)
                .map_err(|e| transport::status(Code::InvalidArgument, "invalid_input", e))?
            {
                let path = self.blob_path(&b.reference.sha256);
                let tmp = path.with_extension("tmp");
                std::fs::write(&tmp, &b.raw)
                    .and_then(|_| std::fs::rename(&tmp, &path))
                    .map_err(|e| Status::internal(e.to_string()))?;
                stored.push(b.reference.sha256);
            }
        }
        Ok(Response::new(PutBlobsResponse { stored }))
    }

    async fn submit_job(
        &self,
        req: Request<SubmitJobRequest>,
    ) -> Result<Response<SubmitJobResponse>, Status> {
        self.touch();
        let r = req.into_inner();
        if let Some(id) = self.db.by_key(&r.client_job_key) {
            return Ok(Response::new(SubmitJobResponse {
                job_id: id,
                existing: true,
            }));
        }
        let Some(spec) = self.kinds.get(&r.kind) else {
            return Err(transport::status(
                Code::InvalidArgument,
                "unsupported_kind",
                format!("no runner here offers {}", r.kind),
            ));
        };
        if r.kind_version != 0 && r.kind_version != spec.info.version {
            return Err(transport::status(
                Code::InvalidArgument,
                "version_mismatch",
                format!("{} is at version {}", r.kind, spec.info.version),
            ));
        }
        if let Some(missing) = r
            .inputs
            .iter()
            .find(|b| b.sha256.len() != 64 || !self.blob_path(&b.sha256).exists())
        {
            return Err(transport::status(
                Code::FailedPrecondition,
                "invalid_input",
                format!("blob {} was not uploaded", missing.sha256),
            ));
        }
        let id = uuid::Uuid::now_v7().to_string();
        let rec = JobRecord {
            job_id: id.clone(),
            client_job_key: r.client_job_key.clone(),
            kind: r.kind.clone(),
            kind_version: spec.info.version,
            state: JobState::Queued as i32,
            submitted_ms: now_ms(),
            host: format!("reviera-compute-host {}", env!("CARGO_PKG_VERSION")),
            project: r.project.clone(),
            ..Default::default()
        };
        self.db
            .insert(&rec, &r)
            .map_err(|e| Status::internal(e.to_string()))?;
        self.active.lock().unwrap().insert(id.clone(), None);
        self.set_state(&id, JobState::Queued, None);
        let _ = self.queue.send(id.clone());
        Ok(Response::new(SubmitJobResponse {
            job_id: id,
            existing: false,
        }))
    }

    async fn list_jobs(
        &self,
        req: Request<ListJobsRequest>,
    ) -> Result<Response<ListJobsResponse>, Status> {
        self.touch();
        let r = req.into_inner();
        let mut jobs: Vec<JobRecord> = self
            .db
            .all()
            .into_iter()
            .filter(|j| r.states.is_empty() || r.states.contains(&j.state))
            .filter(|j| r.kind.is_empty() || j.kind == r.kind)
            .filter(|j| r.project.is_empty() || j.project == r.project)
            .collect();
        jobs.sort_by_key(|j| j.submitted_ms);
        Ok(Response::new(ListJobsResponse {
            jobs,
            next_page_token: String::new(),
        }))
    }

    async fn get_job(&self, req: Request<GetJobRequest>) -> Result<Response<JobRecord>, Status> {
        self.touch();
        let id = req.into_inner().job_id;
        self.db
            .record(&id)
            .map(Response::new)
            .ok_or_else(|| transport::status(Code::NotFound, "not_found", format!("no job {id}")))
    }

    type WatchJobStream = BoxStream<JobEvent>;

    async fn watch_job(
        &self,
        req: Request<WatchJobRequest>,
    ) -> Result<Response<Self::WatchJobStream>, Status> {
        self.touch();
        let r = req.into_inner();
        if self.db.record(&r.job_id).is_none() {
            return Err(transport::status(
                Code::NotFound,
                "not_found",
                format!("no job {}", r.job_id),
            ));
        }
        let host = self.clone();
        let mut w = self
            .watchers
            .lock()
            .unwrap()
            .entry(r.job_id.clone())
            .or_insert_with(|| watch::channel(0).0)
            .subscribe();
        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            let mut cursor = r.after_seq;
            loop {
                for e in host.db.events_after(&r.job_id, cursor) {
                    cursor = e.seq;
                    if tx.send(Ok(e)).await.is_err() {
                        return;
                    }
                }
                let rec = host.db.record(&r.job_id);
                if rec
                    .as_ref()
                    .is_none_or(|j| is_terminal(j.state) && cursor >= j.last_seq)
                {
                    return;
                }
                host.touch();
                if tokio::time::timeout(Duration::from_secs(5), w.changed())
                    .await
                    .is_ok_and(|r| r.is_err())
                {
                    return;
                }
            }
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }

    type GetResultsStream = BoxStream<ResultChunk>;

    async fn get_results(
        &self,
        req: Request<GetResultsRequest>,
    ) -> Result<Response<Self::GetResultsStream>, Status> {
        self.touch();
        let r = req.into_inner();
        let manifest = self.db.manifest(&r.job_id).ok_or_else(|| {
            transport::status(
                Code::FailedPrecondition,
                "not_found",
                "the job has no results",
            )
        })?;
        let data = std::fs::read(self.result_path(&r.job_id)).map_err(|_| {
            transport::status(
                Code::FailedPrecondition,
                "not_found",
                "the results were acknowledged or have expired",
            )
        })?;
        let chunks = result_chunks(manifest, &data, r.offset);
        Ok(Response::new(Box::pin(tokio_stream::iter(
            chunks.into_iter().map(Ok),
        ))))
    }

    async fn ack_results(
        &self,
        req: Request<AckResultsRequest>,
    ) -> Result<Response<AckResultsResponse>, Status> {
        self.touch();
        let id = req.into_inner().job_id;
        if let Some(mut rec) = self.db.record(&id) {
            rec.acknowledged = true;
            self.db.put(&rec);
            let _ = std::fs::remove_file(self.result_path(&id));
        }
        Ok(Response::new(AckResultsResponse {}))
    }

    async fn cancel_job(
        &self,
        req: Request<CancelJobRequest>,
    ) -> Result<Response<CancelJobResponse>, Status> {
        self.touch();
        let id = req.into_inner().job_id;
        let rec = self.db.record(&id).ok_or_else(|| {
            transport::status(Code::NotFound, "not_found", format!("no job {id}"))
        })?;
        if is_terminal(rec.state) {
            return Ok(Response::new(CancelJobResponse { state: rec.state }));
        }
        let running = self.active.lock().unwrap().get(&id).cloned().flatten();
        match running {
            Some((mut c, rj)) => {
                let _ = c.cancel_job(CancelJobRequest { job_id: rj }).await;
            }
            None => {
                // Queued: cancelled before it starts.
                self.active.lock().unwrap().remove(&id);
                self.set_state(&id, JobState::Cancelled, None);
            }
        }
        let state = self.db.record(&id).map(|r| r.state).unwrap_or(rec.state);
        Ok(Response::new(CancelJobResponse { state }))
    }

    async fn shutdown(
        &self,
        _: Request<ShutdownRequest>,
    ) -> Result<Response<ShutdownResponse>, Status> {
        self.stop.notify_one();
        Ok(Response::new(ShutdownResponse {}))
    }
}

/// Runs the host until it is idle, asked to shut down, or signalled.
pub fn main() -> std::process::ExitCode {
    let cfg = HostConfig::from_env();
    for d in ["blobs", "results", "run"] {
        if let Err(e) = std::fs::create_dir_all(cfg.state_dir.join(d)) {
            eprintln!("cannot create {}: {e}", cfg.state_dir.display());
            return std::process::ExitCode::FAILURE;
        }
    }
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(async move {
        let address = paths::host_address(&cfg.state_dir);
        let (listener, address) = match transport::bind(&address).await {
            Ok(l) => l,
            Err(e) => {
                // Another host already serves this state directory.
                eprintln!("cannot listen on {address}: {e}");
                return std::process::ExitCode::from(3);
            }
        };
        let db = match Db::open(&cfg.state_dir.join("registry.db")) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("cannot open the job registry: {e}");
                return std::process::ExitCode::FAILURE;
            }
        };
        let mut kinds = HashMap::new();
        for exe in &cfg.runners {
            for info in describe_runner(exe) {
                kinds.entry(info.kind.clone()).or_insert(Spec {
                    exe: exe.clone(),
                    info,
                });
            }
        }
        let token = transport::new_token();
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        let host = Arc::new(Host {
            cfg: cfg.clone(),
            db,
            kinds,
            runners: tokio::sync::Mutex::default(),
            watchers: Mutex::default(),
            active: Mutex::default(),
            queue: tx,
            last_call: Mutex::new(Instant::now()),
            stop: Notify::new(),
            spawned: std::sync::atomic::AtomicU64::new(0),
        });
        // Jobs that were running when the host last stopped cannot resume (no checkpoints yet);
        // queued ones run now.
        for rec in host.db.all() {
            match JobState::try_from(rec.state) {
                Ok(JobState::Queued) => {
                    host.active.lock().unwrap().insert(rec.job_id.clone(), None);
                    let _ = host.queue.send(rec.job_id.clone());
                }
                Ok(JobState::Starting | JobState::Running) => host.set_state(
                    &rec.job_id,
                    JobState::Failed,
                    Some(failure(
                        "host_restarted",
                        "the compute host stopped while the job was running; submit it again",
                        String::new(),
                    )),
                ),
                _ => {}
            }
            // Expired results.
            if rec.expires_ms > 0 && rec.expires_ms < now_ms() {
                let _ = std::fs::remove_file(host.result_path(&rec.job_id));
            }
        }
        let info = HostInfo {
            address: address.to_string(),
            token: token.clone(),
            pid: std::process::id(),
        };
        if let Err(e) = paths::write_host_info(&cfg.state_dir, &info) {
            eprintln!("cannot write the host file: {e}");
            return std::process::ExitCode::FAILURE;
        }
        let dispatcher = host.clone();
        tokio::spawn(async move {
            while let Some(id) = rx.recv().await {
                let h = dispatcher.clone();
                tokio::spawn(async move { h.run_job(id).await });
            }
        });
        let reaper = host.clone();
        tokio::spawn(async move { reaper.reap().await });
        let stopper = host.clone();
        let shutdown = async move {
            tokio::select! {
                _ = stopper.stop.notified() => {}
                _ = tokio::signal::ctrl_c() => {}
            }
        };
        let served = transport::serve(listener, token, Service(host.clone()), shutdown).await;
        // Stop runners and leave nothing behind.
        let mut runners = host.runners.lock().await;
        for (_, mut r) in runners.drain() {
            let _ = r.client.shutdown(ShutdownRequest {}).await;
            if tokio::time::timeout(Duration::from_secs(5), r.child.wait())
                .await
                .is_err()
            {
                let _ = r.child.start_kill();
            }
        }
        paths::remove_host_info(&cfg.state_dir, &info);
        #[cfg(unix)]
        if let Address::Unix(p) = &address {
            let _ = std::fs::remove_file(p);
        }
        match served {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("serve: {e}");
                std::process::ExitCode::FAILURE
            }
        }
    })
}
