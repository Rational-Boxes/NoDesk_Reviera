// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Reviera contributors

//! A toolkit for writing runners: implement [`Kind`] for each job kind and call
//! [`main_with`]; the toolkit serves `reviera.compute.v1` (negotiation, blobs, jobs, numbered
//! events, heartbeats, cancellation, resumable results, shutdown). A runner keeps its jobs in
//! memory only; the compute host that starts it holds the durable registry.
//!
//! A runner is started by a host as `<runner> --serve <address>` with the token in the
//! `REVIERA_COMPUTE_TOKEN` environment variable, and prints its kinds as JSON for
//! `<runner> --describe`.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{Notify, mpsc, watch};
use tokio_stream::Stream;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Code, Request, Response, Status, Streaming};

use crate::blob::{self, Assembler, Blob};
use crate::negotiate;
use crate::pb::job_event::Event;
use crate::pb::*;
use crate::transport::{self, Address};
use crate::{CONTRACT_VERSION, now_ms};

/// What a kind produces.
#[derive(Debug, Clone)]
pub struct Output {
    pub bytes: Vec<u8>,
    pub media_type: String,
    pub schema: String,
}

/// Why a run failed: a reason code and a message.
#[derive(Debug, Clone)]
pub struct JobFailure {
    pub reason: String,
    pub message: String,
}

impl JobFailure {
    pub fn new(reason: &str, message: impl Into<String>) -> JobFailure {
        JobFailure {
            reason: reason.into(),
            message: message.into(),
        }
    }
}

/// One job kind a runner performs.
pub trait Kind: Send + Sync + 'static {
    fn info(&self) -> KindInfo;
    /// Runs a job on a worker thread. Report progress through `ctx`, and return early with
    /// reason `cancelled` when [`JobContext::cancelled`] turns true.
    fn run(
        &self,
        ctx: &JobContext,
        options: Option<&prost_types::Any>,
        inputs: &[Blob],
    ) -> Result<Output, JobFailure>;
}

struct Job {
    record: Mutex<JobRecord>,
    events: Mutex<Vec<JobEvent>>,
    notify: watch::Sender<u64>,
    cancel: AtomicBool,
    output: Mutex<Option<(ResultManifest, Vec<u8>)>>,
}

impl Job {
    fn push(&self, event: Event) {
        let seq = {
            let mut ev = self.events.lock().unwrap();
            let seq = ev.len() as u64 + 1;
            ev.push(JobEvent {
                seq,
                time_ms: now_ms(),
                event: Some(event.clone()),
            });
            seq
        };
        {
            let mut r = self.record.lock().unwrap();
            r.last_seq = seq;
            match &event {
                Event::Progress(p) => {
                    r.progress = p.fraction;
                    r.phase = p.phase.clone();
                }
                Event::State(s) => {
                    r.state = s.state;
                    r.failure = s.failure.clone();
                }
                _ => {}
            }
        }
        self.notify.send_replace(seq);
    }

    fn terminal(&self) -> bool {
        is_terminal(self.record.lock().unwrap().state)
    }
}

/// Whether a job state is final.
pub fn is_terminal(state: i32) -> bool {
    matches!(
        JobState::try_from(state),
        Ok(JobState::Succeeded | JobState::Failed | JobState::Cancelled)
    )
}

/// What a running job reports through.
pub struct JobContext {
    job: Arc<Job>,
}

impl std::fmt::Debug for JobContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobContext").finish_non_exhaustive()
    }
}

impl JobContext {
    pub fn progress(&self, fraction: f32, phase: &str) {
        self.job.push(Event::Progress(Progress {
            fraction: fraction.clamp(0.0, 1.0),
            phase: phase.into(),
        }));
    }
    pub fn log(&self, line: impl Into<String>) {
        self.job.push(Event::Log(LogLine {
            level: "info".into(),
            line: line.into(),
        }));
    }
    pub fn warning(&self, line: impl Into<String>) {
        self.job.push(Event::Warning(line.into()));
    }
    pub fn checkpoint(&self, id: impl Into<String>) {
        self.job
            .push(Event::Checkpoint(Checkpoint { id: id.into() }));
    }
    pub fn cancelled(&self) -> bool {
        self.job.cancel.load(Ordering::Relaxed)
    }
}

/// A runner serving its kinds.
pub struct Runner {
    name: String,
    version: String,
    kinds: HashMap<String, Arc<dyn Kind>>,
    blobs: Mutex<HashMap<String, Blob>>,
    jobs: Mutex<HashMap<String, Arc<Job>>>,
    by_key: Mutex<HashMap<String, String>>,
    shutdown: Arc<Notify>,
    last_call: Mutex<std::time::Instant>,
}

impl std::fmt::Debug for Runner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runner").field("name", &self.name).finish()
    }
}

impl Runner {
    pub fn new(name: &str, version: &str, kinds: Vec<Arc<dyn Kind>>) -> Runner {
        Runner {
            name: name.into(),
            version: version.into(),
            kinds: kinds
                .into_iter()
                .map(|k| (k.info().kind.clone(), k))
                .collect(),
            blobs: Mutex::default(),
            jobs: Mutex::default(),
            by_key: Mutex::default(),
            shutdown: Arc::new(Notify::new()),
            last_call: Mutex::new(std::time::Instant::now()),
        }
    }

    fn touch(&self) {
        *self.last_call.lock().unwrap() = std::time::Instant::now();
    }

    fn job(&self, id: &str) -> Result<Arc<Job>, Status> {
        self.jobs
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| transport::status(Code::NotFound, "not_found", format!("no job {id}")))
    }

    /// The kinds' descriptions.
    pub fn describe(&self) -> DescribeResponse {
        let mut kinds: Vec<KindInfo> = self.kinds.values().map(|k| k.info()).collect();
        kinds.sort_by(|a, b| a.kind.cmp(&b.kind));
        DescribeResponse {
            contract_versions: vec![CONTRACT_VERSION.into()],
            kinds,
            resources: Some(Resources {
                cores: std::thread::available_parallelism()
                    .map(|n| n.get() as u32)
                    .unwrap_or(1),
                memory_bytes: 0,
                gpus: 0,
            }),
            max_job_bytes: 0,
            server: format!("{} {}", self.name, self.version),
        }
    }
}

type BoxStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send>>;

#[tonic::async_trait]
impl compute_server::Compute for Arc<Runner> {
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
                format!("this runner speaks {CONTRACT_VERSION}"),
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
        Ok(Response::new(Runner::describe(self)))
    }

    async fn health(&self, _: Request<HealthRequest>) -> Result<Response<HealthResponse>, Status> {
        self.touch();
        let jobs = self.jobs.lock().unwrap();
        let running = jobs.values().filter(|j| !j.terminal()).count() as u32;
        Ok(Response::new(HealthResponse {
            ready: true,
            jobs_running: running,
            jobs_queued: 0,
            memory_bytes: 0,
        }))
    }

    async fn has_blobs(
        &self,
        req: Request<HasBlobsRequest>,
    ) -> Result<Response<HasBlobsResponse>, Status> {
        self.touch();
        let blobs = self.blobs.lock().unwrap();
        Ok(Response::new(HasBlobsResponse {
            present: req
                .into_inner()
                .sha256
                .into_iter()
                .filter(|h| blobs.contains_key(h))
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
            if let Some(b) = asm
                .push(chunk)
                .map_err(|e| transport::status(Code::InvalidArgument, "invalid_input", e))?
            {
                stored.push(b.reference.sha256.clone());
                self.blobs
                    .lock()
                    .unwrap()
                    .insert(b.reference.sha256.clone(), b);
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
        if let Some(id) = self.by_key.lock().unwrap().get(&r.client_job_key).cloned()
            && !r.client_job_key.is_empty()
        {
            return Ok(Response::new(SubmitJobResponse {
                job_id: id,
                existing: true,
            }));
        }
        let kind = self.kinds.get(&r.kind).cloned().ok_or_else(|| {
            transport::status(
                Code::InvalidArgument,
                "unsupported_kind",
                format!("this runner has no kind {}", r.kind),
            )
        })?;
        let inputs: Vec<Blob> = {
            let blobs = self.blobs.lock().unwrap();
            r.inputs
                .iter()
                .map(|i| {
                    blobs.get(&i.sha256).cloned().ok_or_else(|| {
                        transport::status(
                            Code::FailedPrecondition,
                            "invalid_input",
                            format!("blob {} was not uploaded", i.sha256),
                        )
                    })
                })
                .collect::<Result<_, _>>()?
        };
        let job_id = format!(
            "{}-{}",
            now_ms(),
            &blob::sha256_hex(r.client_job_key.as_bytes())[..8]
        );
        let info = kind.info();
        let job = Arc::new(Job {
            record: Mutex::new(JobRecord {
                job_id: job_id.clone(),
                client_job_key: r.client_job_key.clone(),
                kind: r.kind.clone(),
                kind_version: r.kind_version,
                state: JobState::Running as i32,
                submitted_ms: now_ms(),
                started_ms: now_ms(),
                host: self.name.clone(),
                project: r.project.clone(),
                ..Default::default()
            }),
            events: Mutex::default(),
            notify: watch::channel(0).0,
            cancel: AtomicBool::new(false),
            output: Mutex::default(),
        });
        self.jobs
            .lock()
            .unwrap()
            .insert(job_id.clone(), job.clone());
        self.by_key
            .lock()
            .unwrap()
            .insert(r.client_job_key.clone(), job_id.clone());
        job.push(Event::State(StateChange {
            state: JobState::Running as i32,
            failure: None,
        }));
        // Heartbeats while the job runs.
        let hb = job.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                if hb.terminal() {
                    break;
                }
                hb.push(Event::Heartbeat(Heartbeat {}));
            }
        });
        let name = self.name.clone();
        let version = self.version.clone();
        let started = now_ms();
        tokio::task::spawn_blocking(move || {
            let ctx = JobContext { job: job.clone() };
            let result = kind.run(&ctx, r.options.as_ref(), &inputs);
            let (state, failure) = match result {
                Ok(out) => {
                    let codec = negotiate::ZSTD_V1;
                    match blob::encode(codec, &out.bytes) {
                        Ok(encoded) => {
                            let manifest = ResultManifest {
                                kind: info.kind.clone(),
                                kind_version: info.version,
                                runner: name.clone(),
                                runner_version: version.clone(),
                                distillation: r.distillation.clone(),
                                snapshot_hash: r.snapshot_hash.clone(),
                                inputs: r.inputs.iter().map(|i| i.sha256.clone()).collect(),
                                host: name,
                                started_ms: started,
                                finished_ms: now_ms(),
                                media_type: out.media_type,
                                schema: out.schema,
                                codec: codec.into(),
                                sha256: blob::sha256_hex(&out.bytes),
                                size: out.bytes.len() as u64,
                            };
                            {
                                let mut rec = job.record.lock().unwrap();
                                rec.result_sha256 = manifest.sha256.clone();
                                rec.result_size = manifest.size;
                                rec.finished_ms = now_ms();
                            }
                            *job.output.lock().unwrap() = Some((manifest, encoded));
                            (JobState::Succeeded, None)
                        }
                        Err(e) => (
                            JobState::Failed,
                            Some(Failure {
                                reason: "runner_failed".into(),
                                message: e,
                                runner_output: String::new(),
                            }),
                        ),
                    }
                }
                Err(f) if f.reason == "cancelled" => (JobState::Cancelled, None),
                Err(f) => (
                    JobState::Failed,
                    Some(Failure {
                        reason: f.reason,
                        message: f.message,
                        runner_output: String::new(),
                    }),
                ),
            };
            job.record.lock().unwrap().finished_ms = now_ms();
            job.push(Event::State(StateChange {
                state: state as i32,
                failure,
            }));
        });
        Ok(Response::new(SubmitJobResponse {
            job_id,
            existing: false,
        }))
    }

    async fn list_jobs(
        &self,
        _: Request<ListJobsRequest>,
    ) -> Result<Response<ListJobsResponse>, Status> {
        self.touch();
        let mut jobs: Vec<JobRecord> = self
            .jobs
            .lock()
            .unwrap()
            .values()
            .map(|j| j.record.lock().unwrap().clone())
            .collect();
        jobs.sort_by_key(|j| j.submitted_ms);
        Ok(Response::new(ListJobsResponse {
            jobs,
            next_page_token: String::new(),
        }))
    }

    async fn get_job(&self, req: Request<GetJobRequest>) -> Result<Response<JobRecord>, Status> {
        self.touch();
        let job = self.job(&req.into_inner().job_id)?;
        let r = job.record.lock().unwrap().clone();
        Ok(Response::new(r))
    }

    type WatchJobStream = BoxStream<JobEvent>;

    async fn watch_job(
        &self,
        req: Request<WatchJobRequest>,
    ) -> Result<Response<Self::WatchJobStream>, Status> {
        self.touch();
        let r = req.into_inner();
        let job = self.job(&r.job_id)?;
        Ok(Response::new(watch_stream(job, r.after_seq)))
    }

    type GetResultsStream = BoxStream<ResultChunk>;

    async fn get_results(
        &self,
        req: Request<GetResultsRequest>,
    ) -> Result<Response<Self::GetResultsStream>, Status> {
        self.touch();
        let r = req.into_inner();
        let job = self.job(&r.job_id)?;
        let Some((manifest, encoded)) = job.output.lock().unwrap().clone() else {
            return Err(transport::status(
                Code::FailedPrecondition,
                "not_found",
                "the job has no results (yet, or any more)",
            ));
        };
        let chunks = result_chunks(manifest, &encoded, r.offset);
        Ok(Response::new(Box::pin(tokio_stream::iter(
            chunks.into_iter().map(Ok),
        ))))
    }

    async fn ack_results(
        &self,
        req: Request<AckResultsRequest>,
    ) -> Result<Response<AckResultsResponse>, Status> {
        self.touch();
        let job = self.job(&req.into_inner().job_id)?;
        *job.output.lock().unwrap() = None;
        job.record.lock().unwrap().acknowledged = true;
        Ok(Response::new(AckResultsResponse {}))
    }

    async fn cancel_job(
        &self,
        req: Request<CancelJobRequest>,
    ) -> Result<Response<CancelJobResponse>, Status> {
        self.touch();
        let job = self.job(&req.into_inner().job_id)?;
        job.cancel.store(true, Ordering::Relaxed);
        let state = job.record.lock().unwrap().state;
        Ok(Response::new(CancelJobResponse { state }))
    }

    async fn shutdown(
        &self,
        _: Request<ShutdownRequest>,
    ) -> Result<Response<ShutdownResponse>, Status> {
        self.shutdown.notify_one();
        Ok(Response::new(ShutdownResponse {}))
    }
}

/// The chunks of an encoded output from `offset`, 4 MiB each; the first carries the manifest.
pub fn result_chunks(manifest: ResultManifest, encoded: &[u8], offset: u64) -> Vec<ResultChunk> {
    let total = encoded.len() as u64;
    let mut out = Vec::new();
    let mut at = offset.min(total) as usize;
    let mut first = true;
    loop {
        let end = (at + blob::CHUNK_BYTES).min(encoded.len());
        out.push(ResultChunk {
            manifest: first.then(|| manifest.clone()),
            offset: at as u64,
            total,
            data: encoded[at..end].to_vec(),
            last: end == encoded.len(),
        });
        first = false;
        if end == encoded.len() {
            break;
        }
        at = end;
    }
    out
}

fn watch_stream(job: Arc<Job>, after: u64) -> BoxStream<JobEvent> {
    let (tx, rx) = mpsc::channel(64);
    tokio::spawn(async move {
        let mut cursor = after;
        let mut w = job.notify.subscribe();
        loop {
            let batch: Vec<JobEvent> = job
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e.seq > cursor)
                .cloned()
                .collect();
            for e in batch {
                cursor = e.seq;
                if tx.send(Ok(e)).await.is_err() {
                    return;
                }
            }
            let last = job.events.lock().unwrap().len() as u64;
            if job.terminal() && cursor >= last {
                return;
            }
            if w.changed().await.is_err() {
                return;
            }
        }
    });
    Box::pin(ReceiverStream::new(rx))
}

/// The kinds as JSON, for `--describe`.
fn describe_json(r: &Runner) -> String {
    let d = r.describe();
    let kinds: Vec<String> = d
        .kinds
        .iter()
        .map(|k| {
            format!(
                "{{\"kind\":{:?},\"version\":{},\"licence\":{:?},\"supports_checkpoints\":{}}}",
                k.kind, k.version, k.licence, k.supports_checkpoints
            )
        })
        .collect();
    format!(
        "{{\"runner\":{:?},\"version\":{:?},\"contract\":{:?},\"kinds\":[{}]}}",
        r.name,
        r.version,
        CONTRACT_VERSION,
        kinds.join(",")
    )
}

/// A runner's `main`: `--describe` prints its kinds; `--serve <address>` serves them until
/// asked to shut down, or until no call has arrived for `REVIERA_RUNNER_IDLE_S` seconds
/// (default 600), so a runner whose host has gone does not stay resident.
pub fn main_with(runner: Runner) -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--describe") {
        println!("{}", describe_json(&runner));
        return std::process::ExitCode::SUCCESS;
    }
    let Some(addr) = args
        .iter()
        .position(|a| a == "--serve")
        .and_then(|i| args.get(i + 1))
        .and_then(|a| a.parse::<Address>().ok())
    else {
        eprintln!(
            "usage: {} --describe | --serve <unix:path|tcp:host:port>",
            args[0]
        );
        return std::process::ExitCode::from(2);
    };
    let token = std::env::var("REVIERA_COMPUTE_TOKEN").unwrap_or_default();
    let idle = std::env::var("REVIERA_RUNNER_IDLE_S")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(600u64);
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(async move {
        let runner = Arc::new(runner);
        let (listener, _) = match transport::bind(&addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("cannot listen on {addr}: {e}");
                return std::process::ExitCode::FAILURE;
            }
        };
        let stop = runner.shutdown.clone();
        let watcher = runner.clone();
        let shutdown = async move {
            tokio::select! {
                _ = stop.notified() => {}
                _ = async {
                    loop {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        let quiet = watcher.last_call.lock().unwrap().elapsed();
                        let busy = watcher.jobs.lock().unwrap().values().any(|j| !j.terminal());
                        if !busy && quiet > Duration::from_secs(idle) {
                            break;
                        }
                    }
                } => {}
            }
        };
        let r = transport::serve(listener, token, runner, shutdown).await;
        #[cfg(unix)]
        if let Address::Unix(p) = &addr {
            let _ = std::fs::remove_file(p);
        }
        match r {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("serve: {e}");
                std::process::ExitCode::FAILURE
            }
        }
    })
}
