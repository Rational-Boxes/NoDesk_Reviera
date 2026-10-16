// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Distillations (DM-RPC-003): what a job sends instead of the model. This module has the
//! `clash.bodies` distiller: the tessellated bodies of the chosen elements (booleans applied),
//! keyed by opaque keys, in deterministic chunks, with the pairs a clash test leaves out.
//! The map from keys back to GlobalIds stays in Reviera.

use std::collections::BTreeMap;

use prost::Message;
use reviera_compute_proto::analysis::v1 as an;
use reviera_compute_proto::blob::{Blob, sha256_hex};
use reviera_core::ElementId;
use reviera_core::bodies::Body;

use crate::kinds::{EXCLUSIONS, MESH_SET, PROTOBUF, element_mesh};

pub const CLASH_BODIES: &str = "clash.bodies";
pub const CLASH_BODIES_VERSION: u32 = 1;

/// Target size of one chunk before encoding.
const CHUNK_TARGET: usize = 2 << 20;

/// A distillation ready to send.
#[derive(Debug, Clone)]
pub struct Distilled {
    pub distiller: String,
    pub version: u32,
    pub detail: String,
    pub anonymised: bool,
    pub blobs: Vec<Blob>,
    /// Opaque key → GlobalId; kept locally, never sent.
    pub key_map: BTreeMap<String, String>,
    pub elements: usize,
    pub triangles: usize,
    /// A hash of the distillation's blobs, standing for the model state it was made from.
    pub snapshot_hash: String,
}

impl Distilled {
    pub fn size_bytes(&self) -> u64 {
        self.blobs.iter().map(|b| b.reference.size).sum()
    }
}

/// An opaque key for a GlobalId: stable for the same element, not reversible to it.
pub fn opaque_key(guid: &str) -> String {
    format!("k{}", &sha256_hex(guid.as_bytes())[..20])
}

/// The `clash.bodies` distillation of `bodies`: `guid` names each element (elements without
/// one are left out), `excluded` are pairs to leave out, `model` labels the source model.
pub fn clash_bodies(
    bodies: &[Body],
    guid: &dyn Fn(ElementId) -> Option<String>,
    excluded: &[(ElementId, ElementId)],
    model: &str,
) -> Distilled {
    let mut keyed: Vec<(String, String, &Body)> = bodies
        .iter()
        .filter(|b| !b.parts.is_empty())
        .filter_map(|b| guid(b.element).map(|g| (opaque_key(&g), g, b)))
        .collect();
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    keyed.dedup_by(|a, b| a.0 == b.0);
    let key_of: BTreeMap<ElementId, String> = keyed
        .iter()
        .map(|(k, _, b)| (b.element, k.clone()))
        .collect();

    // Chunks close at content-defined boundaries (a key whose hash ends in a set pattern) or
    // when they reach the target size, so an edit changes only the chunk its element is in.
    let mut blobs = Vec::new();
    let mut current = an::MeshSet::default();
    let mut size = 0usize;
    let mut triangles = 0usize;
    for (key, _, body) in &keyed {
        let e = element_mesh(key, model, body);
        triangles += body.parts.iter().map(|p| p.mesh.1.len() / 3).sum::<usize>();
        size += e.encoded_len();
        current.elements.push(e);
        let boundary = key.as_bytes().last().is_some_and(|c| *c == b'0');
        if boundary || size >= CHUNK_TARGET {
            blobs.push(Blob::new(current.encode_to_vec(), PROTOBUF, MESH_SET));
            current = an::MeshSet::default();
            size = 0;
        }
    }
    if !current.elements.is_empty() {
        blobs.push(Blob::new(current.encode_to_vec(), PROTOBUF, MESH_SET));
    }
    let mut pairs: Vec<an::KeyPair> = excluded
        .iter()
        .filter_map(|(a, b)| {
            let (ka, kb) = (key_of.get(a)?, key_of.get(b)?);
            let (a, b) = if ka <= kb { (ka, kb) } else { (kb, ka) };
            Some(an::KeyPair {
                a: a.clone(),
                b: b.clone(),
            })
        })
        .collect();
    pairs.sort_by(|x, y| (&x.a, &x.b).cmp(&(&y.a, &y.b)));
    pairs.dedup();
    blobs.push(Blob::new(
        an::ClashExclusions { pairs }.encode_to_vec(),
        PROTOBUF,
        EXCLUSIONS,
    ));
    let snapshot_hash = sha256_hex(
        blobs
            .iter()
            .map(|b| b.reference.sha256.as_str())
            .collect::<Vec<_>>()
            .join(",")
            .as_bytes(),
    );
    Distilled {
        distiller: CLASH_BODIES.into(),
        version: CLASH_BODIES_VERSION,
        detail: "fine".into(),
        anonymised: true,
        blobs,
        key_map: keyed.into_iter().map(|(k, g, _)| (k, g)).collect(),
        elements: key_of.len(),
        triangles,
        snapshot_hash,
    }
}

/// Pairs whose boxes overlap by more than `tolerance` (a sweep along x), for the caller to
/// decide which to exclude.
pub fn candidate_pairs(bodies: &[Body], tolerance: f64) -> Vec<(ElementId, ElementId)> {
    let mut boxes: Vec<(ElementId, [f64; 3], [f64; 3])> = bodies
        .iter()
        .filter_map(|b| b.bounds().map(|(lo, hi)| (b.element, lo, hi)))
        .collect();
    boxes.sort_by(|a, b| a.1[0].total_cmp(&b.1[0]));
    let mut out = Vec::new();
    for i in 0..boxes.len() {
        for j in i + 1..boxes.len() {
            if boxes[j].1[0] >= boxes[i].2[0] - tolerance {
                break;
            }
            if (0..3).all(|k| {
                boxes[i].2[k].min(boxes[j].2[k]) - boxes[i].1[k].max(boxes[j].1[k]) > tolerance
            }) && boxes[i].0 != boxes[j].0
            {
                out.push((boxes[i].0, boxes[j].0));
            }
        }
    }
    out
}
