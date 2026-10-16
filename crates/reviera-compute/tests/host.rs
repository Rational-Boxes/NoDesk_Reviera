// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The remote computation plumbing end to end, with real processes: a client, the local
//! compute host it starts, and the built-in runner the host starts (DM-RPC-001 … 009).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use prost::Message;
use reviera_compute::client::ComputeClient;
use reviera_compute::distill::{self, Distilled};
use reviera_compute::kinds::{CLASH, FAULT};
use reviera_compute::paths;
use reviera_compute_proto::analysis::v1 as an;
use reviera_compute_proto::negotiate;
use reviera_compute_proto::testing::v1::FaultOptions;
use reviera_compute_proto::to_any;
use reviera_core::ElementId;
use reviera_core::bodies::{Body, Part, extrude};
use reviera_core::overlap;

struct Env {
    dir: tempfile::TempDir,
    extra: Vec<(String, String)>,
}

impl Env {
    fn new(extra: &[(&str, &str)]) -> Env {
        // Short paths: Unix socket paths are limited to about 100 bytes.
        let base = if std::env::temp_dir().as_os_str().len() > 40 {
            PathBuf::from("/tmp")
        } else {
            std::env::temp_dir()
        };
        Env {
            dir: tempfile::Builder::new()
                .prefix("rvc")
                .tempdir_in(base)
                .unwrap(),
            extra: extra
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    fn client(&self) -> ComputeClient {
        let mut env = vec![
            (
                "REVIERA_COMPUTE_RUNNERS".to_owned(),
                env!("CARGO_BIN_EXE_reviera-analysis-builtin").to_owned(),
            ),
            ("REVIERA_COMPUTE_HOST_IDLE_S".to_owned(), "20".to_owned()),
            ("REVIERA_COMPUTE_RUNNER_IDLE_S".to_owned(), "10".to_owned()),
        ];
        env.extend(self.extra.iter().cloned());
        ComputeClient::new(
            self.dir.path().to_owned(),
            Some(env!("CARGO_BIN_EXE_reviera-compute-host").into()),
        )
        .with_host_env(env)
    }

    fn host_pid(&self) -> Option<u32> {
        paths::read_host_info(self.dir.path()).map(|i| i.pid)
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        // Leave no host behind.
        if let Some(pid) = self.host_pid() {
            kill(pid, 15);
        }
    }
}

fn kill(pid: u32, signal: i32) {
    let _ = std::process::Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(pid.to_string())
        .status();
}

fn alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
        && !std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .unwrap_or_default()
            .contains(") Z ")
}

fn nothing() -> Distilled {
    Distilled {
        distiller: "none".into(),
        version: 1,
        detail: "coarse".into(),
        anonymised: true,
        blobs: Vec::new(),
        key_map: BTreeMap::new(),
        elements: 0,
        triangles: 0,
        snapshot_hash: String::new(),
    }
}

fn fault(c: &ComputeClient, o: FaultOptions, key: Option<&str>) -> String {
    c.submit(
        FAULT,
        Some(to_any(&o)),
        &nothing(),
        "test",
        key.map(str::to_owned),
    )
    .unwrap()
}

const WAIT: Duration = Duration::from_secs(60);

#[test]
fn a_job_runs_end_to_end_with_numbered_events_and_checked_results() {
    let env = Env::new(&[]);
    let c = env.client();
    let kinds = c.kinds().unwrap();
    assert!(kinds.iter().any(|k| k.kind == CLASH));
    assert!(
        kinds
            .iter()
            .any(|k| k.kind == FAULT && k.licence == "GPL-3.0-or-later")
    );

    let id = fault(
        &c,
        FaultOptions {
            steps: 5,
            step_ms: 20,
            checkpoint_every: 2,
            output_bytes: 3_000_000,
            ..Default::default()
        },
        Some("job-1"),
    );
    // Idempotent: the same key is the same job.
    assert_eq!(fault(&c, FaultOptions::default(), Some("job-1")), id);
    let j = c.wait(&id, WAIT).unwrap();
    assert_eq!(j.state, "succeeded", "{j:?}");
    assert_eq!(j.progress, 1.0);

    let (m, out) = c.results(&id).unwrap();
    assert_eq!(out.len(), 3_000_000);
    assert!(out.iter().enumerate().all(|(i, b)| *b == (i % 251) as u8));
    assert_eq!(m.kind, FAULT);
    assert_eq!(m.runner, "reviera-analysis-builtin");
    assert_eq!(m.codec, negotiate::ZSTD_V1);
    assert!(m.host.starts_with("reviera-compute-host"));
    c.ack(&id).unwrap();
    let gone = c.results(&id).unwrap_err();
    assert_eq!(gone.reason, "not_found");

    // The host's registry: events numbered 1…n, resumable from any point.
    let host = c.host_jobs().unwrap();
    let rec = host.iter().find(|r| r.job_id == id).unwrap();
    assert!(rec.acknowledged);
    let events = watch_all(&env, &id, 0);
    let seqs: Vec<u64> = events.iter().map(|e| e.seq).collect();
    assert_eq!(seqs, (1..=rec.last_seq).collect::<Vec<_>>());
    let resumed = watch_all(&env, &id, 4);
    assert_eq!(resumed.first().unwrap().seq, 5);
    assert_eq!(resumed.len() as u64, rec.last_seq - 4);
}

/// Every event after `after`, straight from the host.
fn watch_all(env: &Env, id: &str, after: u64) -> Vec<reviera_compute_proto::pb::JobEvent> {
    let info = paths::read_host_info(env.dir.path()).unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut c =
            reviera_compute_proto::transport::connect(&info.address.parse().unwrap(), &info.token)
                .await
                .unwrap();
        let mut s = c
            .watch_job(reviera_compute_proto::pb::WatchJobRequest {
                job_id: id.into(),
                after_seq: after,
            })
            .await
            .unwrap()
            .into_inner();
        let mut out = Vec::new();
        while let Some(e) = s.message().await.unwrap() {
            out.push(e);
        }
        out
    })
}

#[test]
fn failures_crashes_and_cancellation_are_reported_and_contained() {
    let env = Env::new(&[]);
    let c = env.client();
    let failing = fault(
        &c,
        FaultOptions {
            steps: 5,
            fail_at_step: 2,
            ..Default::default()
        },
        None,
    );
    let j = c.wait(&failing, WAIT).unwrap();
    assert_eq!(j.state, "failed");
    assert_eq!(j.failure.as_ref().unwrap().reason, "runner_failed");

    let crashing = fault(
        &c,
        FaultOptions {
            steps: 5,
            crash_at_step: 2,
            ..Default::default()
        },
        None,
    );
    let j = c.wait(&crashing, WAIT).unwrap();
    assert_eq!(j.state, "failed", "{j:?}");
    let f = j.failure.unwrap();
    assert_eq!(f.reason, "runner_failed");
    assert!(f.runner_output.contains("crashing at step 2"), "{f:?}");

    // The next job starts the runner again.
    let next = fault(
        &c,
        FaultOptions {
            steps: 2,
            ..Default::default()
        },
        None,
    );
    assert_eq!(c.wait(&next, WAIT).unwrap().state, "succeeded");

    let long = fault(
        &c,
        FaultOptions {
            steps: 200,
            step_ms: 50,
            ..Default::default()
        },
        None,
    );
    std::thread::sleep(Duration::from_millis(400));
    c.cancel(&long).unwrap();
    assert_eq!(c.wait(&long, WAIT).unwrap().state, "cancelled");
}

#[cfg(target_os = "linux")]
#[test]
fn a_runaway_is_stopped_at_its_memory_limit() {
    let env = Env::new(&[("REVIERA_COMPUTE_MEMORY_MB", "256")]);
    let c = env.client();
    let id = fault(
        &c,
        FaultOptions {
            steps: 40,
            step_ms: 10,
            alloc_mb_per_step: 32,
            ..Default::default()
        },
        None,
    );
    let j = c.wait(&id, WAIT).unwrap();
    assert_eq!(j.state, "failed", "{j:?}");
    assert_eq!(j.failure.unwrap().reason, "memory_limit");
    // The host is unharmed and runs the next job.
    let next = fault(
        &c,
        FaultOptions {
            steps: 2,
            ..Default::default()
        },
        None,
    );
    assert_eq!(c.wait(&next, WAIT).unwrap().state, "succeeded");
}

#[cfg(target_os = "linux")]
#[test]
fn a_runner_that_cannot_start_fails_its_job_and_the_host_still_exits_when_idle() {
    use std::os::unix::fs::PermissionsExt;
    let env = Env::new(&[]);
    // A runner that describes a kind but cannot serve it.
    let fake = env.dir.path().join("broken-runner");
    std::fs::write(
        &fake,
        "#!/bin/sh\nif [ \"$1\" = --describe ]; then echo '{\"runner\":\"broken\",\"version\":\"0\",\"kinds\":[{\"kind\":\"test.fault\",\"version\":1,\"licence\":\"MIT\"}]}'; exit 0; fi\necho 'cannot listen' >&2\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let c = env.client().with_host_env(vec![
        (
            "REVIERA_COMPUTE_RUNNERS".into(),
            fake.to_string_lossy().into_owned(),
        ),
        ("REVIERA_COMPUTE_HOST_IDLE_S".into(), "2".into()),
    ]);
    let id = fault(
        &c,
        FaultOptions {
            steps: 2,
            ..Default::default()
        },
        None,
    );
    let j = c.wait(&id, WAIT).unwrap();
    assert_eq!(j.state, "failed");
    let f = j.failure.unwrap();
    assert_eq!(f.reason, "runner_failed");
    assert!(f.runner_output.contains("cannot listen"), "{f:?}");
    let pid = env.host_pid().unwrap();
    let end = Instant::now() + Duration::from_secs(20);
    while alive(pid) && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        !alive(pid),
        "the host is still running after a failed start"
    );
}

#[cfg(unix)]
#[test]
fn a_long_state_path_still_gets_working_sockets() {
    // As under Flatpak: ~/.var/app/<id>/.local/state/reviera/compute is over 100 bytes.
    let env = Env::new(&[]);
    let long = env
        .dir
        .path()
        .join("a-very-long-directory-name-standing-for-a-flatpak-app-data-folder")
        .join("and-another-long-one-for-good-measure");
    let c = ComputeClient::new(
        long.clone(),
        Some(env!("CARGO_BIN_EXE_reviera-compute-host").into()),
    )
    .with_host_env(vec![(
        "REVIERA_COMPUTE_RUNNERS".into(),
        env!("CARGO_BIN_EXE_reviera-analysis-builtin").into(),
    )]);
    let id = fault(
        &c,
        FaultOptions {
            steps: 2,
            ..Default::default()
        },
        None,
    );
    assert_eq!(c.wait(&id, WAIT).unwrap().state, "succeeded");
    c.shutdown_host().unwrap();
}

#[test]
fn negotiation_without_a_common_technique_fails() {
    let env = Env::new(&[]);
    let c = env.client().with_offers(vec![negotiate::offer(
        negotiate::COMPRESSION,
        &["future-v9"],
    )]);
    let e = c.kinds().unwrap_err();
    assert_eq!(e.reason, "negotiation_failed", "{e}");
}

#[cfg(target_os = "linux")]
#[test]
fn runner_and_host_exit_when_idle() {
    let env = Env::new(&[]);
    let c = env.client().with_host_env(vec![
        (
            "REVIERA_COMPUTE_RUNNERS".into(),
            env!("CARGO_BIN_EXE_reviera-analysis-builtin").into(),
        ),
        ("REVIERA_COMPUTE_HOST_IDLE_S".into(), "2".into()),
        ("REVIERA_COMPUTE_RUNNER_IDLE_S".into(), "1".into()),
    ]);
    let id = fault(
        &c,
        FaultOptions {
            steps: 2,
            ..Default::default()
        },
        None,
    );
    assert_eq!(c.wait(&id, WAIT).unwrap().state, "succeeded");
    let pid = env.host_pid().unwrap();
    let run = env.dir.path().join("run");
    let end = Instant::now() + Duration::from_secs(20);
    while alive(pid) && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(!alive(pid), "the host is still running");
    assert!(paths::read_host_info(env.dir.path()).is_none());
    let sockets = std::fs::read_dir(run)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|x| x == "sock")
        })
        .count();
    assert_eq!(sockets, 0, "a runner socket was left behind");
    // A new call starts the host again, and the finished job is still in its registry.
    assert!(c.host_jobs().unwrap().iter().any(|r| r.job_id == id));
}

#[test]
fn a_job_outlives_its_client_and_is_collected_by_another() {
    let env = Env::new(&[]);
    let id = {
        let a = env.client();
        fault(
            &a,
            FaultOptions {
                steps: 20,
                step_ms: 50,
                output_bytes: 1000,
                ..Default::default()
            },
            Some("handover"),
        )
        // `a` is dropped here, with the job still running.
    };
    let b = env.client();
    let reattached = b.reattach(None);
    assert!(reattached.iter().any(|j| j.job_id == id), "{reattached:?}");
    let j = b.wait(&id, WAIT).unwrap();
    assert_eq!(j.state, "succeeded");
    assert_eq!(b.results(&id).unwrap().1.len(), 1000);
}

#[cfg(target_os = "linux")]
#[test]
fn a_job_interrupted_by_a_host_restart_is_reported() {
    let env = Env::new(&[]);
    let c = env.client();
    let id = fault(
        &c,
        FaultOptions {
            steps: 400,
            step_ms: 50,
            ..Default::default()
        },
        None,
    );
    let end = Instant::now() + WAIT;
    while c.status(&id).unwrap().state != "running" && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    kill(env.host_pid().unwrap(), 9);
    let j = c.wait(&id, WAIT).unwrap();
    assert_eq!(j.state, "failed", "{j:?}");
    assert_eq!(j.failure.unwrap().reason, "host_restarted");
}

fn cube(id: u64, lo: [f64; 3], hi: [f64; 3]) -> Body {
    let ring = [
        [lo[0], lo[1]],
        [hi[0], lo[1]],
        [hi[0], hi[1]],
        [lo[0], hi[1]],
    ];
    Body {
        element: ElementId(id),
        parts: vec![Part {
            mesh: extrude(&ring, &[], lo[2], hi[2]),
            class: "IfcWall".into(),
            plan_hidden: false,
        }],
    }
}

#[test]
fn a_clash_run_out_of_process_matches_the_in_process_engine() {
    let env = Env::new(&[]);
    let c = env.client();
    // A grid of boxes, each overlapping its neighbours, and one pair excluded by design.
    let mut bodies = Vec::new();
    for i in 0..12u64 {
        for j in 0..12u64 {
            let (x, y) = (i as f64 * 0.9, j as f64 * 0.9);
            bodies.push(cube(i * 12 + j + 1, [x, y, 0.0], [x + 1.0, y + 1.0, 3.0]));
        }
    }
    let guid = |id: ElementId| Some(format!("GUID-{:04}", id.0));
    let excluded = vec![(ElementId(1), ElementId(2))];
    let d = distill::clash_bodies(&bodies, &guid, &excluded, "");
    assert!(d.blobs.len() > 2, "distilled into {} blobs", d.blobs.len());
    assert!(d.key_map.values().all(|g| g.starts_with("GUID-")));
    // Keys do not reveal GlobalIds.
    for b in &d.blobs {
        assert!(!String::from_utf8_lossy(&b.raw).contains("GUID-"));
    }
    let opts = an::ClashOptions::default();
    let id = c
        .submit(CLASH, Some(to_any(&opts)), &d, "test", None)
        .unwrap();
    let first_upload = c.uploaded_bytes();
    assert_eq!(c.wait(&id, WAIT).unwrap().state, "succeeded");
    let (m, raw) = c.results(&id).unwrap();
    assert_eq!(m.schema, "reviera.analysis.v1.ClashResult");
    let result = an::ClashResult::decode(raw.as_slice()).unwrap();

    // The same engine in-process, with the same exclusion.
    let skip = |a: ElementId, b: ElementId| (a.0.min(b.0), a.0.max(b.0)) == (1, 2);
    let local = overlap::find(&bodies, None, &skip, &overlap::Options::default());
    let key_of: BTreeMap<String, u64> = d
        .key_map
        .iter()
        .map(|(k, g)| (k.clone(), g[5..].parse().unwrap()))
        .collect();
    let mut remote: Vec<(u64, u64, u64)> = result
        .clashes
        .iter()
        .map(|c| {
            let (a, b) = (key_of[&c.a], key_of[&c.b]);
            (a.min(b), a.max(b), c.volume.to_bits())
        })
        .collect();
    let mut expected: Vec<(u64, u64, u64)> = local
        .iter()
        .map(|o| (o.a.0.min(o.b.0), o.a.0.max(o.b.0), o.volume.to_bits()))
        .collect();
    remote.sort();
    expected.sort();
    assert!(!expected.is_empty());
    assert_eq!(remote, expected);

    // Running it again uploads nothing new: the host holds every blob.
    let again = c
        .submit(CLASH, Some(to_any(&opts)), &d, "test", None)
        .unwrap();
    assert_eq!(c.uploaded_bytes(), first_upload);
    assert_eq!(c.wait(&again, WAIT).unwrap().state, "succeeded");

    // Moving one box re-sends only the chunk it is in.
    bodies[40] = cube(41, [3.0, 3.0, 0.5], [4.0, 4.0, 3.5]);
    let d2 = distill::clash_bodies(&bodies, &guid, &excluded, "");
    let changed = d2
        .blobs
        .iter()
        .filter(|b| {
            !d.blobs
                .iter()
                .any(|o| o.reference.sha256 == b.reference.sha256)
        })
        .count();
    assert!(
        changed <= 2,
        "{changed} of {} blobs changed",
        d2.blobs.len()
    );
}
