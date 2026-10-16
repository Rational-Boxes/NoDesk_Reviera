// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Reviera's side of the contract: starting the local compute host on demand, negotiating,
//! uploading only the blobs a host lacks, submitting jobs, following them in the background
//! (reconnecting and resuming from the last event), downloading results resumably, and the
//! client's record of its jobs so it can reattach after a restart (DM-RPC-009).

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use reviera_compute_proto::blob;
use reviera_compute_proto::negotiate;
use reviera_compute_proto::pb::job_event::Event;
use reviera_compute_proto::pb::*;
use reviera_compute_proto::runner::is_terminal;
use reviera_compute_proto::transport::{self, Address, Client};
use reviera_compute_proto::{CONTRACT_VERSION, now_ms};
use serde::{Deserialize, Serialize};

use crate::distill::Distilled;
use crate::paths;

/// A failed call: a reason code and a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComputeError {
    pub reason: String,
    pub message: String,
}

impl std::fmt::Display for ComputeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.reason, self.message)
    }
}

impl std::error::Error for ComputeError {}

fn err(reason: &str, message: impl Into<String>) -> ComputeError {
    ComputeError {
        reason: reason.into(),
        message: message.into(),
    }
}

impl From<tonic::Status> for ComputeError {
    fn from(s: tonic::Status) -> Self {
        ComputeError {
            reason: transport::reason(&s).unwrap_or_else(|| "unavailable".into()),
            message: s.message().to_owned(),
        }
    }
}

/// A job's failure, as recorded locally.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobFailure {
    pub reason: String,
    pub message: String,
    pub runner_output: String,
}

/// The client's record of a job it submitted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalJob {
    pub job_id: String,
    pub client_job_key: String,
    pub kind: String,
    pub state: String,
    pub progress: f32,
    pub phase: String,
    pub last_seq: u64,
    pub failure: Option<JobFailure>,
    pub submitted_ms: i64,
    pub snapshot_hash: String,
    pub distiller: String,
    pub project: String,
    /// Opaque keys → GlobalIds, to attach results to elements; never sent.
    pub key_map: BTreeMap<String, String>,
    pub results_fetched: bool,
}

impl LocalJob {
    pub fn terminal(&self) -> bool {
        matches!(self.state.as_str(), "succeeded" | "failed" | "cancelled")
    }
}

/// A job state's name.
pub fn state_name(state: i32) -> &'static str {
    match JobState::try_from(state) {
        Ok(JobState::Queued) => "queued",
        Ok(JobState::Starting) => "starting",
        Ok(JobState::Running) => "running",
        Ok(JobState::Succeeded) => "succeeded",
        Ok(JobState::Failed) => "failed",
        Ok(JobState::Cancelled) => "cancelled",
        _ => "unknown",
    }
}

struct Conn {
    client: Client,
    codec: String,
}

struct Inner {
    state_dir: PathBuf,
    host_program: Option<PathBuf>,
    conn: tokio::sync::Mutex<Option<Conn>>,
    jobs: Mutex<BTreeMap<String, LocalJob>>,
    watching: Mutex<HashSet<String>>,
    offers: Vec<TechniqueOffer>,
    host_env: Vec<(String, String)>,
    uploaded: std::sync::atomic::AtomicU64,
}

/// The client. Calls block; following jobs happens on its own runtime.
pub struct ComputeClient {
    rt: Option<tokio::runtime::Runtime>,
    inner: Arc<Inner>,
}

impl Drop for ComputeClient {
    fn drop(&mut self) {
        // Never blocks, so it is safe inside another runtime.
        if let Some(rt) = self.rt.take() {
            rt.shutdown_background();
        }
    }
}

impl std::fmt::Debug for ComputeClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComputeClient")
            .field("state_dir", &self.inner.state_dir)
            .finish_non_exhaustive()
    }
}

impl ComputeClient {
    /// A client for the host serving `state_dir`, started from `host_program` when needed.
    pub fn new(state_dir: PathBuf, host_program: Option<PathBuf>) -> ComputeClient {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("reviera-compute")
            .enable_all()
            .build()
            .expect("tokio runtime");
        let jobs = load_registry(&state_dir);
        ComputeClient {
            rt: Some(rt),
            inner: Arc::new(Inner {
                state_dir,
                host_program,
                conn: tokio::sync::Mutex::default(),
                jobs: Mutex::new(jobs),
                watching: Mutex::default(),
                offers: negotiate::supported(),
                host_env: Vec::new(),
                uploaded: std::sync::atomic::AtomicU64::new(0),
            }),
        }
    }

    /// The default client: the per-user state directory and the bundled host program.
    pub fn local() -> ComputeClient {
        ComputeClient::new(paths::state_dir(), paths::host_program())
    }

    /// Offers these techniques instead of the defaults (for testing negotiation).
    pub fn with_offers(mut self, offers: Vec<TechniqueOffer>) -> ComputeClient {
        Arc::get_mut(&mut self.inner).expect("unshared").offers = offers;
        self
    }

    /// Environment for the host this client starts (for example limits and runners).
    pub fn with_host_env(mut self, env: Vec<(String, String)>) -> ComputeClient {
        Arc::get_mut(&mut self.inner).expect("unshared").host_env = env;
        self
    }

    /// Encoded bytes this client has uploaded.
    pub fn uploaded_bytes(&self) -> u64 {
        self.inner
            .uploaded
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Runs `f` to completion on the client's runtime; from inside another runtime it does so
    /// on a scoped thread, so callers on async threads do not panic.
    fn block<T: Send, F: std::future::Future<Output = T> + Send>(&self, f: F) -> T {
        let rt = self.rt.as_ref().expect("runtime");
        if tokio::runtime::Handle::try_current().is_ok() {
            std::thread::scope(|s| s.spawn(|| rt.block_on(f)).join().expect("compute call"))
        } else {
            rt.block_on(f)
        }
    }

    pub fn state_dir(&self) -> &std::path::Path {
        &self.inner.state_dir
    }

    /// The job kinds the host offers.
    pub fn kinds(&self) -> Result<Vec<KindInfo>, ComputeError> {
        self.block(async {
            let mut c = self.inner.client().await?;
            Ok(c.describe(DescribeRequest {}).await?.into_inner().kinds)
        })
    }

    /// Uploads what the host lacks and submits a job; returns its ID. The same
    /// `client_job_key` returns the existing job.
    pub fn submit(
        &self,
        kind: &str,
        options: Option<prost_types::Any>,
        distilled: &Distilled,
        project: &str,
        client_job_key: Option<String>,
    ) -> Result<String, ComputeError> {
        let key = client_job_key.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let inner = self.inner.clone();
        let job_id = self.block(async {
            let (mut c, codec) = inner.client_and_codec().await?;
            let present = c
                .has_blobs(HasBlobsRequest {
                    sha256: distilled
                        .blobs
                        .iter()
                        .map(|b| b.reference.sha256.clone())
                        .collect(),
                })
                .await?
                .into_inner()
                .present;
            let mut chunks = Vec::new();
            for b in distilled
                .blobs
                .iter()
                .filter(|b| !present.contains(&b.reference.sha256))
            {
                chunks.extend(blob::chunks(b, &codec).map_err(|e| err("invalid_input", e))?);
            }
            if !chunks.is_empty() {
                let bytes: u64 = chunks.iter().map(|c| c.data.len() as u64).sum();
                c.put_blobs(tokio_stream::iter(chunks)).await?;
                inner
                    .uploaded
                    .fetch_add(bytes, std::sync::atomic::Ordering::Relaxed);
            }
            let r = c
                .submit_job(SubmitJobRequest {
                    client_job_key: key.clone(),
                    kind: kind.into(),
                    kind_version: 0,
                    options,
                    inputs: distilled
                        .blobs
                        .iter()
                        .map(|b| b.reference.clone())
                        .collect(),
                    distillation: Some(Distillation {
                        distiller: distilled.distiller.clone(),
                        distiller_version: distilled.version,
                        detail: distilled.detail.clone(),
                        anonymised: distilled.anonymised,
                    }),
                    snapshot_hash: distilled.snapshot_hash.clone(),
                    project: project.into(),
                })
                .await?
                .into_inner();
            Ok::<_, ComputeError>(r.job_id)
        })?;
        {
            let mut jobs = self.inner.jobs.lock().unwrap();
            jobs.entry(job_id.clone()).or_insert_with(|| LocalJob {
                job_id: job_id.clone(),
                client_job_key: key,
                kind: kind.into(),
                state: "queued".into(),
                progress: 0.0,
                phase: String::new(),
                last_seq: 0,
                failure: None,
                submitted_ms: now_ms(),
                snapshot_hash: distilled.snapshot_hash.clone(),
                distiller: distilled.distiller.clone(),
                project: project.into(),
                key_map: distilled.key_map.clone(),
                results_fetched: false,
            });
        }
        self.inner.save();
        self.follow(&job_id);
        Ok(job_id)
    }

    /// Follows a job in the background until it finishes.
    fn follow(&self, job_id: &str) {
        if !self
            .inner
            .watching
            .lock()
            .unwrap()
            .insert(job_id.to_owned())
        {
            return;
        }
        let inner = self.inner.clone();
        let id = job_id.to_owned();
        self.rt.as_ref().expect("runtime").spawn(async move {
            inner.follow(&id).await;
            inner.watching.lock().unwrap().remove(&id);
        });
    }

    /// The client's record of a job.
    pub fn status(&self, job_id: &str) -> Option<LocalJob> {
        self.inner.jobs.lock().unwrap().get(job_id).cloned()
    }

    /// Waits until a job finishes or `timeout` passes; returns its record.
    pub fn wait(&self, job_id: &str, timeout: Duration) -> Option<LocalJob> {
        let end = Instant::now() + timeout;
        loop {
            let j = self.status(job_id)?;
            if j.terminal() || Instant::now() > end {
                return Some(j);
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    /// The job's output, decoded and checked, with its provenance. Downloads resume after a
    /// broken connection. Does not acknowledge: call [`ComputeClient::ack`] once stored.
    pub fn results(&self, job_id: &str) -> Result<(ResultManifest, Vec<u8>), ComputeError> {
        let inner = self.inner.clone();
        let out = self.block(async move {
            let mut data: Vec<u8> = Vec::new();
            let mut manifest: Option<ResultManifest> = None;
            let mut attempts = 0;
            loop {
                let mut c = inner.client().await?;
                let result = async {
                    let mut s = c
                        .get_results(GetResultsRequest {
                            job_id: job_id.into(),
                            offset: data.len() as u64,
                        })
                        .await?
                        .into_inner();
                    while let Some(chunk) = s.message().await? {
                        if chunk.manifest.is_some() && manifest.is_none() {
                            manifest = chunk.manifest;
                        }
                        if chunk.offset != data.len() as u64 {
                            return Err(err("invalid_input", "results arrived out of order"));
                        }
                        data.extend_from_slice(&chunk.data);
                        if chunk.last {
                            return Ok(true);
                        }
                    }
                    Ok(false)
                }
                .await;
                match result {
                    Ok(true) => break,
                    Ok(false) | Err(_) if attempts < 5 => {
                        if let Err(e) = &result
                            && e.reason == "not_found"
                        {
                            return Err(e.clone());
                        }
                        attempts += 1;
                        inner.reset().await;
                        tokio::time::sleep(Duration::from_millis(200 * attempts)).await;
                    }
                    Ok(false) => return Err(err("unavailable", "the results stream ended early")),
                    Err(e) => return Err(e),
                }
            }
            let m = manifest.ok_or_else(|| err("invalid_input", "results without a manifest"))?;
            let raw = blob::decode(&m.codec, &data, m.size).map_err(|e| err("invalid_input", e))?;
            if blob::sha256_hex(&raw) != m.sha256 {
                return Err(err("invalid_input", "results failed their hash check"));
            }
            Ok((m, raw))
        })?;
        if let Some(j) = self.inner.jobs.lock().unwrap().get_mut(job_id) {
            j.results_fetched = true;
        }
        self.inner.save();
        Ok(out)
    }

    /// Tells the host the results are stored, so it may release them.
    pub fn ack(&self, job_id: &str) -> Result<(), ComputeError> {
        self.block(async {
            let mut c = self.inner.client().await?;
            c.ack_results(AckResultsRequest {
                job_id: job_id.into(),
            })
            .await?;
            Ok(())
        })
    }

    /// Cancels a job; returns its state.
    pub fn cancel(&self, job_id: &str) -> Result<String, ComputeError> {
        self.block(async {
            let mut c = self.inner.client().await?;
            let s = c
                .cancel_job(CancelJobRequest {
                    job_id: job_id.into(),
                })
                .await?
                .into_inner()
                .state;
            Ok(state_name(s).to_owned())
        })
    }

    /// The client's jobs.
    pub fn jobs(&self) -> Vec<LocalJob> {
        self.inner.jobs.lock().unwrap().values().cloned().collect()
    }

    /// The host's registry.
    pub fn host_jobs(&self) -> Result<Vec<JobRecord>, ComputeError> {
        self.block(async {
            let mut c = self.inner.client().await?;
            Ok(c.list_jobs(ListJobsRequest::default())
                .await?
                .into_inner()
                .jobs)
        })
    }

    /// Follows every job that has not finished, or whose results have not been fetched.
    pub fn reattach(&self, job_id: Option<&str>) -> Vec<LocalJob> {
        let ids: Vec<String> = self
            .jobs()
            .into_iter()
            .filter(|j| job_id.is_none_or(|id| id == j.job_id))
            .filter(|j| !j.terminal() || (j.state == "succeeded" && !j.results_fetched))
            .map(|j| j.job_id)
            .collect();
        for id in &ids {
            self.follow(id);
        }
        ids.iter().filter_map(|id| self.status(id)).collect()
    }

    /// Removes a job from the client's record.
    pub fn forget(&self, job_id: &str) -> bool {
        let removed = self.inner.jobs.lock().unwrap().remove(job_id).is_some();
        if removed {
            self.inner.save_replacing(job_id);
        }
        removed
    }

    /// Asks the host to exit (for tests and administration).
    pub fn shutdown_host(&self) -> Result<(), ComputeError> {
        self.block(async {
            let mut c = self.inner.client().await?;
            c.shutdown(ShutdownRequest {}).await?;
            self.inner.reset().await;
            Ok(())
        })
    }
}

impl Inner {
    async fn client(&self) -> Result<Client, ComputeError> {
        Ok(self.client_and_codec().await?.0)
    }

    async fn reset(&self) {
        *self.conn.lock().await = None;
    }

    /// A connection to the host, starting it if it is not running, and the negotiated codec.
    async fn client_and_codec(&self) -> Result<(Client, String), ComputeError> {
        let mut conn = self.conn.lock().await;
        if let Some(c) = conn.as_mut()
            && c.client.health(HealthRequest {}).await.is_ok()
        {
            return Ok((c.client.clone(), c.codec.clone()));
        }
        *conn = None;
        let mut client = match self.try_connect().await {
            Some(c) => c,
            None => {
                self.start_host()?;
                let end = Instant::now() + Duration::from_secs(20);
                loop {
                    if let Some(c) = self.try_connect().await {
                        break c;
                    }
                    if Instant::now() > end {
                        return Err(err("unavailable", "the compute host did not start"));
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
        };
        let nego = client
            .negotiate(NegotiateRequest {
                contract_versions: vec![CONTRACT_VERSION.into()],
                offers: self.offers.clone(),
            })
            .await?
            .into_inner();
        let codec = negotiate::chosen(&nego.choices, negotiate::COMPRESSION)
            .unwrap_or(negotiate::IDENTITY_V1)
            .to_owned();
        *conn = Some(Conn {
            client: client.clone(),
            codec: codec.clone(),
        });
        Ok((client, codec))
    }

    async fn try_connect(&self) -> Option<Client> {
        let info = paths::read_host_info(&self.state_dir)?;
        let addr: Address = info.address.parse().ok()?;
        let mut c = transport::connect(&addr, &info.token).await.ok()?;
        c.health(HealthRequest {}).await.ok()?;
        Some(c)
    }

    /// Starts the host in its own session, so it outlives Reviera.
    fn start_host(&self) -> Result<(), ComputeError> {
        let program = self
            .host_program
            .clone()
            .ok_or_else(|| err("unavailable", "the compute host program was not found"))?;
        let mut cmd = std::process::Command::new(&program);
        cmd.envs(self.host_env.iter().map(|(k, v)| (k, v)))
            .env("REVIERA_COMPUTE_STATE", &self.state_dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // SAFETY: setsid is async-signal-safe.
            unsafe {
                cmd.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const DETACHED_PROCESS: u32 = 0x0000_0008;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn().map_err(|e| {
            err(
                "unavailable",
                format!("cannot start {}: {e}", program.display()),
            )
        })?;
        // Reap it when it exits, so it never lingers as a zombie of this process.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }

    /// Follows a job's events until it finishes, reconnecting with backoff.
    async fn follow(&self, id: &str) {
        let mut backoff = Duration::from_millis(250);
        loop {
            let after = match self.jobs.lock().unwrap().get(id) {
                Some(j) if j.terminal() => return,
                Some(j) => j.last_seq,
                None => return,
            };
            let stream = async {
                let mut c = self.client().await?;
                let mut s = c
                    .watch_job(WatchJobRequest {
                        job_id: id.into(),
                        after_seq: after,
                    })
                    .await?
                    .into_inner();
                while let Some(e) = s.message().await? {
                    if self.apply(id, e) {
                        return Ok(true);
                    }
                }
                Ok::<_, ComputeError>(false)
            }
            .await;
            match stream {
                Ok(true) => {
                    self.save();
                    return;
                }
                Err(e) if e.reason == "not_found" => {
                    self.mark_lost(id);
                    return;
                }
                _ => {
                    self.reset().await;
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(60));
                }
            }
        }
    }

    /// Applies an event to the local record; returns whether the job finished.
    fn apply(&self, id: &str, e: JobEvent) -> bool {
        let mut changed_state = false;
        let terminal = {
            let mut jobs = self.jobs.lock().unwrap();
            let Some(j) = jobs.get_mut(id) else {
                return true;
            };
            if e.seq <= j.last_seq {
                return j.terminal();
            }
            j.last_seq = e.seq;
            match e.event {
                Some(Event::Progress(p)) => {
                    j.progress = p.fraction;
                    j.phase = p.phase;
                }
                Some(Event::State(s)) => {
                    j.state = state_name(s.state).into();
                    j.failure = s.failure.map(|f| JobFailure {
                        reason: f.reason,
                        message: f.message,
                        runner_output: f.runner_output,
                    });
                    changed_state = true;
                }
                _ => {}
            }
            j.terminal()
        };
        if changed_state {
            self.save();
        }
        terminal
    }

    fn mark_lost(&self, id: &str) {
        if let Some(j) = self.jobs.lock().unwrap().get_mut(id) {
            j.state = "failed".into();
            j.failure = Some(JobFailure {
                reason: "not_found".into(),
                message: "the host no longer knows this job".into(),
                runner_output: String::new(),
            });
        }
        self.save();
    }

    fn registry_path(&self) -> PathBuf {
        self.state_dir.join("client-jobs.json")
    }

    /// Writes the record, merged with what other clients wrote meanwhile.
    fn save(&self) {
        self.save_merging(None);
    }

    fn save_replacing(&self, forgotten: &str) {
        self.save_merging(Some(forgotten));
    }

    fn save_merging(&self, forgotten: Option<&str>) {
        let _ = std::fs::create_dir_all(&self.state_dir);
        let mut merged = load_registry(&self.state_dir);
        if let Some(f) = forgotten {
            merged.remove(f);
        }
        for (k, j) in self.jobs.lock().unwrap().iter() {
            let newer = merged.get(k).is_none_or(|m| m.last_seq <= j.last_seq);
            if newer {
                merged.insert(k.clone(), j.clone());
            }
        }
        let path = self.registry_path();
        let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
        if std::fs::write(&tmp, serde_json::to_vec_pretty(&merged).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

fn load_registry(state_dir: &std::path::Path) -> BTreeMap<String, LocalJob> {
    std::fs::read(state_dir.join("client-jobs.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Whether `state` is final (re-exported for callers holding host records).
pub fn terminal(state: i32) -> bool {
    is_terminal(state)
}
