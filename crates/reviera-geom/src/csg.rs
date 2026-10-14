// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Applying boolean relations (ARC-MOD-028, `reviera_core::csg`) to bodies with the solid
//! kernel. Results are cached by the operation and the two bodies' geometry, so
//! a target is re-made only when it or its tool changes.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

use reviera_core::bodies::{Body, Mesh, Part};
use reviera_core::csg::CsgOp;
use reviera_core::store::Store;

/// Boolean results by input.
#[derive(Debug, Default)]
pub struct Cache(Mutex<HashMap<u64, Result<Vec<Mesh>, String>>>);

fn key(op: CsgOp, a: &[Mesh], b: &[Mesh]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (op as u8).hash(&mut h);
    for m in a.iter().chain([&(Vec::new(), vec![u32::MAX])]).chain(b) {
        for p in &m.0 {
            for c in p {
                c.to_bits().hash(&mut h);
            }
        }
        m.1.hash(&mut h);
    }
    h.finish()
}

impl Cache {
    fn boolean(&self, op: CsgOp, a: &[Mesh], b: &[Mesh]) -> Result<Vec<Mesh>, String> {
        let k = key(op, a, b);
        if let Some(r) = self.0.lock().unwrap_or_else(|e| e.into_inner()).get(&k) {
            return r.clone();
        }
        let geom = match op {
            CsgOp::Subtract => crate::Op::Subtract,
            CsgOp::Intersect => crate::Op::Intersect,
            CsgOp::Union => crate::Op::Union,
        };
        let r = crate::boolean(geom, a, b, 1e-6);
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        // Keep the cache bounded.
        if map.len() > 512 {
            map.clear();
        }
        map.insert(k, r.clone());
        r
    }
}

/// The elements a boolean involves (targets and tools).
pub fn involved(store: &Store) -> std::collections::HashSet<reviera_core::ElementId> {
    reviera_core::csg::resolved(store)
        .into_iter()
        .flat_map(|(t, u, _, _)| [t, u])
        .collect()
}

/// Applies the model's boolean relations to `bodies`, in order: each target's parts become the
/// result (one class, its first part's); tools that hide are dropped. A boolean that fails
/// leaves the target as it was (and is reported by `geometry.booleans`).
pub fn apply(cache: &Cache, store: &Store, bodies: &mut Vec<Body>) {
    let mut hidden = Vec::new();
    for (target, tool, op, hide) in reviera_core::csg::resolved(store) {
        let (Some(ti), Some(ui)) = (
            bodies.iter().position(|b| b.element == target),
            bodies.iter().position(|b| b.element == tool),
        ) else {
            continue;
        };
        let a: Vec<Mesh> = bodies[ti]
            .parts
            .iter()
            .filter(|p| !p.plan_hidden)
            .map(|p| p.mesh.clone())
            .collect();
        let b: Vec<Mesh> = bodies[ui]
            .parts
            .iter()
            .filter(|p| !p.plan_hidden)
            .map(|p| p.mesh.clone())
            .collect();
        if let Ok(result) = cache.boolean(op, &a, &b) {
            let class = bodies[ti]
                .parts
                .first()
                .map_or_else(String::new, |p| p.class.clone());
            // Door panels and other 3D-only parts stay as they were.
            let mut parts: Vec<Part> = result
                .into_iter()
                .map(|mesh| Part {
                    mesh,
                    class: class.clone(),
                    plan_hidden: false,
                })
                .collect();
            parts.extend(bodies[ti].parts.iter().filter(|p| p.plan_hidden).cloned());
            bodies[ti].parts = parts;
        }
        if hide {
            hidden.push(tool);
        }
    }
    bodies.retain(|b| !hidden.contains(&b.element));
}
