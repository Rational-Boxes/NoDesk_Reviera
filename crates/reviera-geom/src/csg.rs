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

/// The elements a boolean involves (targets and tools), and those steel cuts cut.
pub fn involved(store: &Store) -> std::collections::HashSet<reviera_core::ElementId> {
    reviera_core::csg::resolved(store)
        .into_iter()
        .flat_map(|(t, u, _, _)| [t, u])
        .chain(reviera_core::steel_cuts::all(store).into_iter().map(|(_, t, _)| t))
        .chain(store.iter().filter_map(reviera_core::steel_detail::pattern_recipe).flat_map(|r| {
            r.elements
                .iter()
                .filter_map(|g| reviera_core::id::GlobalId::parse(g).and_then(|g| store.find_by_guid(&g)))
                .collect::<Vec<_>>()
        }))
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
    apply_cuts(cache, store, bodies);
}

/// Steel cuts (STR-STL-008): each target loses its cutters' solids.
fn apply_cuts(cache: &Cache, store: &Store, bodies: &mut [Body]) {
    // Fastener holes (STR-STL-006), found in the bodies as they are before any cut.
    let merged = |id: reviera_core::ElementId, bodies: &[Body]| -> Option<Mesh> {
        let b = bodies.iter().find(|b| b.element == id)?;
        let mut m: Mesh = (Vec::new(), Vec::new());
        for p in b.parts.iter().filter(|p| !p.plan_hidden) {
            let base = m.0.len() as u32;
            m.0.extend(p.mesh.0.iter().copied());
            m.1.extend(p.mesh.1.iter().map(|i| i + base));
        }
        Some(m)
    };
    let mut holes: Vec<(reviera_core::ElementId, Mesh)> = Vec::new();
    for e in store.iter() {
        if reviera_core::steel_detail::pattern_recipe(e).is_none() {
            continue;
        }
        for (id, a, b, d) in reviera_core::steel_detail::holes_in(store, e, &|i| merged(i, bodies)) {
            // A little past each face, so the hole cuts clean through.
            let ax = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let l = (ax[0] * ax[0] + ax[1] * ax[1] + ax[2] * ax[2]).sqrt().max(1e-9);
            let ext = |p: [f64; 3], s: f64| [p[0] + ax[0] / l * s, p[1] + ax[1] / l * s, p[2] + ax[2] / l * s];
            holes.push((id, reviera_core::steel_fab::cylinder(ext(a, -0.001), ext(b, 0.001), d / 2.0, 12)));
        }
    }
    let cuts = reviera_core::steel_cuts::all(store);
    if cuts.is_empty() && holes.is_empty() {
        return;
    }
    let mut by_target: std::collections::BTreeMap<reviera_core::ElementId, Vec<Mesh>> = Default::default();
    for (id, m) in holes {
        by_target.entry(id).or_default().push(m);
    }
    for (target, tools) in by_target {
        subtract(cache, bodies, target, &tools);
    }
    for (_, target, r) in cuts {
        let Some(ti) = bodies.iter().position(|b| b.element == target) else {
            continue;
        };
        let body_of = |id: reviera_core::ElementId| -> Option<Vec<Mesh>> {
            bodies
                .iter()
                .find(|b| b.element == id)
                .map(|b| b.parts.iter().filter(|p| !p.plan_hidden).map(|p| p.mesh.clone()).collect())
        };
        let Some(tools) = reviera_core::steel_cuts::cutter(store, &r, &body_of) else {
            continue;
        };
        if tools.is_empty() {
            continue;
        }
        let _ = ti;
        subtract(cache, bodies, target, &tools);
    }
}

/// Replaces an element's body by its subtraction by `tools`.
fn subtract(cache: &Cache, bodies: &mut [Body], target: reviera_core::ElementId, tools: &[Mesh]) {
    let Some(ti) = bodies.iter().position(|b| b.element == target) else {
        return;
    };
    let a: Vec<Mesh> = bodies[ti]
        .parts
        .iter()
        .filter(|p| !p.plan_hidden)
        .map(|p| p.mesh.clone())
        .collect();
    if let Ok(result) = cache.boolean(CsgOp::Subtract, &a, tools) {
        let class = bodies[ti].parts.first().map_or_else(String::new, |p| p.class.clone());
        let mut parts: Vec<Part> = result
            .into_iter()
            .map(|mesh| Part { mesh, class: class.clone(), plan_hidden: false })
            .collect();
        parts.extend(bodies[ti].parts.iter().filter(|p| p.plan_hidden).cloned());
        bodies[ti].parts = parts;
    }
}
