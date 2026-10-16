// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The built-in runner's job kinds: `analysis.clash` (a project-wide clash test, ANA-CLASH-001,
//! run out of process on the same overlap engine as the live checks, ARC-MOD-030) and
//! `test.fault` (behaviours for testing hosts and clients).

use std::collections::HashSet;

use prost::Message;
use reviera_compute_proto::analysis::v1 as an;
use reviera_compute_proto::blob::Blob;
use reviera_compute_proto::pb::KindInfo;
use reviera_compute_proto::runner::{JobContext, JobFailure, Kind, Output};
use reviera_compute_proto::testing::v1 as tv;
use reviera_core::ElementId;
use reviera_core::bodies::{Body, Part};
use reviera_core::overlap;

pub const CLASH: &str = "analysis.clash";
pub const FAULT: &str = "test.fault";
pub const MESH_SET: &str = "reviera.analysis.v1.MeshSet";
pub const EXCLUSIONS: &str = "reviera.analysis.v1.ClashExclusions";
pub const CLASH_RESULT: &str = "reviera.analysis.v1.ClashResult";
pub const PROTOBUF: &str = "application/x-protobuf";
pub const LICENCE: &str = "GPL-3.0-or-later";

fn version() -> String {
    env!("CARGO_PKG_VERSION").into()
}

/// A body as a distillation element.
pub fn element_mesh(key: &str, model: &str, body: &Body) -> an::ElementMesh {
    an::ElementMesh {
        key: key.into(),
        ifc_class: body
            .parts
            .first()
            .map(|p| p.class.clone())
            .unwrap_or_default(),
        parts: body
            .parts
            .iter()
            .map(|p| an::MeshPart {
                positions: p.mesh.0.iter().flatten().copied().collect(),
                indices: p.mesh.1.clone(),
            })
            .collect(),
        model: model.into(),
    }
}

/// A distillation element as a body, numbered `id`.
pub fn body_of(id: u64, e: &an::ElementMesh) -> Body {
    Body {
        element: ElementId(id),
        parts: e
            .parts
            .iter()
            .map(|p| Part {
                mesh: (p.positions.as_chunks::<3>().0.to_vec(), p.indices.clone()),
                class: e.ifc_class.clone(),
                plan_hidden: false,
            })
            .collect(),
    }
}

/// `analysis.clash`: every overlap among the distilled bodies, except excluded pairs.
#[derive(Debug, Default)]
pub struct ClashKind;

impl Kind for ClashKind {
    fn info(&self) -> KindInfo {
        KindInfo {
            kind: CLASH.into(),
            version: 1,
            runner: "reviera-analysis-builtin".into(),
            runner_version: version(),
            licence: LICENCE.into(),
            capabilities: vec!["batch".into(), "multi-model".into()],
            accepted_formats: vec![format!("{PROTOBUF};schema={MESH_SET}")],
            supports_checkpoints: false,
            supports_sessions: false,
        }
    }

    fn run(
        &self,
        ctx: &JobContext,
        options: Option<&prost_types::Any>,
        inputs: &[Blob],
    ) -> Result<Output, JobFailure> {
        let o: an::ClashOptions = match options {
            Some(a) => reviera_compute_proto::from_any(a)
                .map_err(|e| JobFailure::new("invalid_input", e))?,
            None => an::ClashOptions::default(),
        };
        ctx.progress(0.0, "reading");
        let mut elements: Vec<an::ElementMesh> = Vec::new();
        let mut excluded: HashSet<(String, String)> = HashSet::new();
        for b in inputs {
            match b.reference.schema.as_str() {
                MESH_SET => elements.extend(
                    an::MeshSet::decode(b.raw.as_slice())
                        .map_err(|e| JobFailure::new("invalid_input", e.to_string()))?
                        .elements,
                ),
                EXCLUSIONS => excluded.extend(
                    an::ClashExclusions::decode(b.raw.as_slice())
                        .map_err(|e| JobFailure::new("invalid_input", e.to_string()))?
                        .pairs
                        .into_iter()
                        .map(|p| ordered(p.a, p.b)),
                ),
                other => {
                    return Err(JobFailure::new(
                        "invalid_input",
                        format!("unexpected input schema {other}"),
                    ));
                }
            }
        }
        if ctx.cancelled() {
            return Err(JobFailure::new("cancelled", "cancelled"));
        }
        // Numbered in key order, so results do not depend on the chunking.
        elements.sort_by(|a, b| a.key.cmp(&b.key));
        let bodies: Vec<Body> = elements
            .iter()
            .enumerate()
            .map(|(i, e)| body_of(i as u64, e))
            .collect();
        let key = |id: ElementId| elements[id.0 as usize].key.as_str();
        let model = |id: ElementId| elements[id.0 as usize].model.as_str();
        ctx.progress(0.1, "measuring");
        let skip = |a: ElementId, b: ElementId| {
            if !o.against_models.is_empty() {
                // Multi-model scope: only pairs across the listed models and the rest.
                let (ma, mb) = (model(a), model(b));
                let la = o.against_models.iter().any(|m| m == ma);
                let lb = o.against_models.iter().any(|m| m == mb);
                if la == lb {
                    return true;
                }
            }
            excluded.contains(&ordered(key(a).into(), key(b).into()))
        };
        let opts = overlap::Options {
            tolerance: if o.tolerance > 0.0 {
                o.tolerance
            } else {
                0.001
            },
            min_volume: if o.min_volume > 0.0 {
                o.min_volume
            } else {
                1e-6
            },
            resolution: if o.resolution > 0 {
                (o.resolution as usize).clamp(4, 1024)
            } else {
                48
            },
        };
        let mut found = overlap::find(&bodies, None, &skip, &opts);
        if o.limit > 0 {
            found.truncate(o.limit as usize);
        }
        ctx.progress(0.95, "writing");
        let result = an::ClashResult {
            clashes: found
                .iter()
                .map(|c| an::Clash {
                    a: key(c.a).into(),
                    b: key(c.b).into(),
                    volume: c.volume,
                    depth: c.depth,
                    min: c.min.to_vec(),
                    max: c.max.to_vec(),
                    centroid: c.centroid.to_vec(),
                })
                .collect(),
            bodies: bodies.len() as u32,
            pairs_measured: 0,
        };
        ctx.log(format!(
            "{} clashes among {} bodies",
            result.clashes.len(),
            result.bodies
        ));
        ctx.progress(1.0, "done");
        Ok(Output {
            bytes: result.encode_to_vec(),
            media_type: PROTOBUF.into(),
            schema: CLASH_RESULT.into(),
        })
    }
}

fn ordered(a: String, b: String) -> (String, String) {
    if a <= b { (a, b) } else { (b, a) }
}

/// `test.fault`: steps with delays, and on request a failure, a crash, a memory runaway or
/// checkpoints.
#[derive(Debug, Default)]
pub struct FaultKind;

impl Kind for FaultKind {
    fn info(&self) -> KindInfo {
        KindInfo {
            kind: FAULT.into(),
            version: 1,
            runner: "reviera-analysis-builtin".into(),
            runner_version: version(),
            licence: LICENCE.into(),
            capabilities: vec!["test".into()],
            accepted_formats: vec![],
            supports_checkpoints: true,
            supports_sessions: false,
        }
    }

    fn run(
        &self,
        ctx: &JobContext,
        options: Option<&prost_types::Any>,
        _inputs: &[Blob],
    ) -> Result<Output, JobFailure> {
        let o: tv::FaultOptions = match options {
            Some(a) => reviera_compute_proto::from_any(a)
                .map_err(|e| JobFailure::new("invalid_input", e))?,
            None => tv::FaultOptions::default(),
        };
        let steps = if o.steps == 0 { 10 } else { o.steps };
        let mut kept: Vec<Vec<u8>> = Vec::new();
        for s in 1..=steps {
            if ctx.cancelled() {
                return Err(JobFailure::new("cancelled", "cancelled"));
            }
            std::thread::sleep(std::time::Duration::from_millis(o.step_ms as u64));
            if o.alloc_mb_per_step > 0 {
                // Touched, so it is really resident.
                kept.push(vec![1u8; (o.alloc_mb_per_step as usize) << 20]);
            }
            if s == o.fail_at_step {
                return Err(JobFailure::new(
                    "runner_failed",
                    format!("failed at step {s} as asked"),
                ));
            }
            if s == o.crash_at_step {
                eprintln!("test.fault: crashing at step {s} as asked");
                std::process::abort();
            }
            if o.checkpoint_every > 0 && s % o.checkpoint_every == 0 {
                ctx.checkpoint(format!("step-{s}"));
            }
            ctx.progress(s as f32 / steps as f32, &format!("step {s}"));
        }
        let bytes: Vec<u8> = (0..o.output_bytes).map(|i| (i % 251) as u8).collect();
        drop(kept);
        Ok(Output {
            bytes,
            media_type: "application/octet-stream".into(),
            schema: "reviera.testing.v1.FaultOutput".into(),
        })
    }
}
