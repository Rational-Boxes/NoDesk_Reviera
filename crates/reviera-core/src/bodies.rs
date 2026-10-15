// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The bodies of Reviera's own elements, as they are now: each element's closed meshes in
//! project coordinates. The 3D view draws them and the overlap engine ([`crate::overlap`])
//! measures them, so the two always agree.
//!
//! Each element is regenerated from its recipe:
//! - walls joined and cut by their doors and windows (an attached wall's part above the roof's
//!   lowest point is its own closed part);
//! - floors, ceilings and decks (boards and framing);
//! - roofs (faces, fascias, soffits);
//! - stairs, railings, gates, doors and windows.
//!
//! Elements known only from the IFC file are tessellated by the application and added there.

use crate::id::ElementId;
use crate::store::Store;

/// Triangle mesh: positions and index triples.
pub type Mesh = (Vec<[f64; 3]>, Vec<u32>);

/// One closed part of an element's body.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub mesh: Mesh,
    /// The IFC class it is drawn as (a deck's boards are coverings, its framing members).
    pub class: String,
    /// Drawn in 3D only (a door's closed panel; plans show its swing).
    pub plan_hidden: bool,
}

/// An element's body: its closed parts.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub element: ElementId,
    pub parts: Vec<Part>,
}

impl Body {
    /// The axis-aligned box around every part; `None` for an empty body.
    pub fn bounds(&self) -> Option<([f64; 3], [f64; 3])> {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in self.parts.iter().flat_map(|p| p.mesh.0.iter()) {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        (lo[0] <= hi[0]).then_some((lo, hi))
    }
}

/// A closed mesh of a plan profile with holes between `z0` and `z1`, facing out.
pub fn extrude(outer: &[[f64; 2]], holes: &[Vec<[f64; 2]>], z0: f64, z1: f64) -> Mesh {
    extrude_between(outer, holes, &|_| z0, &|_| z1)
}

/// A closed mesh of a plan profile with holes between a bottom and a top that may vary over it
/// (planes over each piece, for ceilings that follow a roof), facing out.
pub fn extrude_between(
    outer: &[[f64; 2]],
    holes: &[Vec<[f64; 2]>],
    bottom: &dyn Fn([f64; 2]) -> f64,
    top: &dyn Fn([f64; 2]) -> f64,
) -> Mesh {
    let mut positions: Vec<[f64; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut points: Vec<[f64; 2]> = outer.to_vec();
    let mut starts = Vec::new();
    for h in holes {
        starts.push(points.len() as u32);
        points.extend(h.iter().copied());
    }
    let mut tris: Vec<u32> = Vec::new();
    earcut::Earcut::new().earcut(points.iter().copied(), &starts, &mut tris);
    let area = |t: &[u32]| {
        let [a, b, c] = [
            points[t[0] as usize],
            points[t[1] as usize],
            points[t[2] as usize],
        ];
        (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])
    };
    for (z, up) in [(top, true), (bottom, false)] {
        let base = positions.len() as u32;
        positions.extend(points.iter().map(|p| [p[0], p[1], z(*p)]));
        for t in tris.chunks(3) {
            if (area(t) > 0.0) == up {
                indices.extend([base + t[0], base + t[1], base + t[2]]);
            } else {
                indices.extend([base + t[0], base + t[2], base + t[1]]);
            }
        }
    }
    let ccw = |r: &[[f64; 2]]| {
        (0..r.len())
            .map(|i| {
                let (a, b) = (r[i], r[(i + 1) % r.len()]);
                a[0] * b[1] - b[0] * a[1]
            })
            .sum::<f64>()
            > 0.0
    };
    let rings = std::iter::once((outer, true)).chain(holes.iter().map(|h| (h.as_slice(), false)));
    for (ring, is_outer) in rings {
        let forward = ccw(ring) == is_outer;
        for i in 0..ring.len() {
            let (mut a, mut b) = (ring[i], ring[(i + 1) % ring.len()]);
            if !forward {
                std::mem::swap(&mut a, &mut b);
            }
            let base = positions.len() as u32;
            for (p, z) in [(a, bottom(a)), (b, bottom(b)), (b, top(b)), (a, top(a))] {
                positions.push([p[0], p[1], z]);
            }
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    (positions, indices)
}

/// The bodies of every Reviera element that regenerates, in a stable order (walls, decks,
/// floors and ceilings, roofs, railings, gates, stairs, doors and windows).
pub fn all(store: &Store) -> Vec<Body> {
    let mut out: Vec<Body> = Vec::new();
    let mut push = |element: ElementId, class: &str, mesh: Mesh, plan_hidden: bool| {
        if mesh.1.is_empty() {
            return;
        }
        let part = Part {
            mesh,
            class: class.to_owned(),
            plan_hidden,
        };
        match out.last_mut() {
            Some(b) if b.element == element => b.parts.push(part),
            _ => out.push(Body {
                element,
                parts: vec![part],
            }),
        }
    };
    let class_of = |id: ElementId, default: &str| {
        store
            .get(id)
            .map_or(default.to_owned(), |e| e.class.clone())
    };
    // Walls: joined, cut by their openings; an attached wall's part above the roof's lowest point
    // over it follows the roof (ARC-WALL-008).
    let cuts = crate::inserts::cuts_by_host(store);
    let solved = crate::walls::solve(store);
    let curved_ends = |id| crate::walls::curved::ends_of(&solved, id);
    for (id, mut body) in solved.bodies.clone() {
        let class = class_of(id, "IfcWall");
        // A dormer's walls stop at the main roof's underside (ARC-ROOF-007).
        let base = store
            .get(id)
            .and_then(|e| crate::dormers::wall_base_plane(store, e));
        let trim = |m: Mesh| match base {
            Some((n, d)) => crate::roofs::eaves::clip_plane(&m, n, d),
            None => m,
        };
        if let Some(Ok(a)) = crate::walls::attach::attached_body(store, id, &body) {
            body.geometry.height = a.z_min - body.geometry.base_z;
            push(id, &class, trim(a.cap), false);
        }
        // A stacked wall is its sub-walls (ARC-STKW-001).
        if let Some(e) = store
            .get(id)
            .filter(|e| crate::walls::stacked::of_wall(store, e).is_some())
        {
            let c = cuts.get(&id).map_or(&[][..], Vec::as_slice);
            for (_, mesh) in crate::walls::stacked::meshes(store, e, &body, c) {
                push(id, &class, trim(mesh), false);
            }
            continue;
        }
        let c = cuts.get(&id).map_or(&[][..], Vec::as_slice);
        // An edited profile (ARC-WALL-009) or a slanted or tapered wall (ARC-WALL-005).
        let special = store
            .get(id)
            .and_then(|e| crate::walls::profile::special_mesh(e, &body, c));
        let mesh = match special {
            Some(Ok(m)) => m,
            _ if c.is_empty() => body.mesh(),
            _ => crate::walls::openings::cut_mesh(&body, c),
        };
        push(id, &class, trim(mesh), false);
    }
    // Curved walls (ARC-WALL-002): pieces along the curve, cut by their openings.
    for e in store.iter() {
        let Some(r) = crate::walls::wall_recipe(e) else {
            continue;
        };
        let (Some(path), Ok(g)) = (
            crate::walls::curved::path_of(&r),
            crate::walls::regenerate(store, e),
        ) else {
            continue;
        };
        let class = e.class.clone();
        let c = cuts.get(&e.id()).map_or(&[][..], Vec::as_slice);
        let roof = crate::walls::attach::top_target(store, e.id()).map(|(_, r)| r);
        for mesh in crate::walls::curved::body(
            &path,
            g.width,
            g.base_z,
            g.base_z + g.height,
            c,
            roof.as_ref(),
            curved_ends(e.id()),
        ) {
            push(e.id(), &class, mesh, false);
        }
    }
    // Decks: boards (coverings) and framing (members).
    for (id, g) in crate::decks::all(store) {
        let n_boards = g.boards.len();
        for (k, mesh) in g.parts.into_iter().enumerate() {
            let class = if k < n_boards {
                "IfcCovering"
            } else {
                "IfcMember"
            };
            push(id, class, mesh, false);
        }
    }
    // Floors and ceilings: each outer boundary with its holes, between bottom and top.
    for (id, f) in crate::floors::all(store) {
        if store
            .get(id)
            .and_then(crate::decks::deck_settings)
            .is_some()
        {
            continue;
        }
        let class = class_of(id, "IfcSlab");
        // A ceiling that follows its roof.
        if let Some(meshes) = store.get(id).and_then(|e| crate::vaults::meshes(store, e)) {
            for m in meshes {
                push(id, &class, m, false);
            }
            continue;
        }
        // A warped top (ARC-SHAPE-001).
        if let Some(shape) = store.get(id).and_then(crate::shape_edit::shape_of)
            && let Some(p) = f.profiles.first()
            && let Ok(mesh) = crate::shape_edit::slab_mesh(p, &shape, f.top, f.thickness)
        {
            push(id, &class, mesh, false);
            continue;
        }
        // Less its vertical openings (ARC-OPEN-002).
        let profiles = match store.get(id) {
            Some(e) => crate::slab_openings::cut_profiles(store, e, &f.profiles),
            None => f.profiles.clone(),
        };
        for p in &profiles {
            push(
                id,
                &class,
                extrude(&p.outer, &p.holes, f.top - f.thickness, f.top),
                false,
            );
        }
    }
    // Roofs: the faces' slabs (a curved roof's one shell), then fascias and soffits.
    for (id, g) in crate::roofs::all(store) {
        for f in g.faces {
            push(id, "IfcRoof", f.slab, false);
        }
        for t in g.trims {
            push(id, "IfcRoof", t.mesh, false);
        }
    }
    for (id, g) in crate::railings::all(store) {
        for mesh in g.parts {
            push(id, "IfcRailing", mesh, false);
        }
    }
    for (id, parts) in crate::railings::all_gates(store) {
        for mesh in parts {
            push(id, "IfcDoor", mesh, false);
        }
    }
    for (id, g) in crate::skylights::all(store) {
        for (mesh, glass) in g.parts {
            push(
                id,
                if glass {
                    "IfcWindow:glass"
                } else {
                    "IfcWindow:metal"
                },
                mesh,
                false,
            );
        }
    }
    for (id, g) in crate::ramps::all(store) {
        for p in &g.pieces {
            push(id, "IfcRamp", g.mesh(p), false);
        }
    }
    for (id, g) in crate::curtain::all(store) {
        for p in &g.parts {
            let class = if p.mullion {
                "IfcMember:metal"
            } else if g.ty.panel == crate::curtain::Panel::Glazed {
                "IfcPlate:glass"
            } else {
                "IfcPlate"
            };
            push(id, class, g.mesh(p), false);
        }
    }
    for (id, g) in crate::columns::all(store) {
        push(id, "IfcColumn", g.mesh(), false);
    }
    // Slab edges (ARC-SLED-001).
    for (id, g) in crate::slab_edges::all(store) {
        push(id, "IfcSlab", g.mesh, false);
    }
    // Path arrays (ARC-MOD-033): every member's mesh, per slot, in the array's body.
    for (id, members) in crate::path_arrays::all(store) {
        for (_, g) in members {
            let class = g.ty.category.class();
            for (slot, mesh) in g.slot_meshes() {
                let color =
                    g.ty.slots
                        .get(slot)
                        .map_or([0.78, 0.78, 0.78, 1.0], |s| s.color);
                push(
                    id,
                    &format!("{class}:{}", crate::mesh_objects::color_hex(color)),
                    mesh,
                    false,
                );
            }
        }
    }
    // Custom objects (DM-IMP-015): one part per material slot, in its colour.
    for (id, g) in crate::mesh_objects::all(store) {
        let class = g.ty.category.class();
        for (slot, mesh) in g.slot_meshes() {
            let color =
                g.ty.slots
                    .get(slot)
                    .map_or([0.78, 0.78, 0.78, 1.0], |s| s.color);
            push(
                id,
                &format!("{class}:{}", crate::mesh_objects::color_hex(color)),
                mesh,
                false,
            );
        }
    }
    for (id, g) in crate::stairs::all(store) {
        for mesh in g.runs.into_iter().chain(g.landings) {
            push(id, "IfcStair", mesh, false);
        }
    }
    for p in crate::inserts::placed(store) {
        let class = p.kind.class();
        // Glass and metal parts draw as such (`IfcDoor:glass`).
        for (mesh, panel, material) in p.parts() {
            push(p.id, &format!("{class}{}", material.suffix()), mesh, panel);
        }
    }
    out
}

/// Whether two elements belong together, so that an overlap between them is by design: a door
/// or window and its wall, a dormer's parts and their roof, a gate and its railing, a railing
/// and its host, an element and its opening.
pub fn related(store: &Store, a: ElementId, b: ElementId) -> bool {
    let guid = |id: ElementId| store.get(id).map(|e| e.guid().to_string());
    let host_of = |id: ElementId| -> Vec<String> {
        let Some(e) = store.get(id) else {
            return Vec::new();
        };
        let mut hosts = Vec::new();
        if let Some(r) = crate::inserts::insert_recipe(e) {
            hosts.push(r.host);
        }
        if let Some(i) = crate::inserts::opening_of(e) {
            hosts.push(i);
        }
        if let Some(d) = crate::dormers::dormer_of(e) {
            hosts.push(d.to_owned());
            // A dormer's parts sit on its main roof.
            if let Some(r) = crate::id::GlobalId::parse(d)
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|d| store.get(d))
                .and_then(crate::dormers::dormer_recipe)
            {
                hosts.push(r.roof);
            }
        }
        if let Some((r, _)) = crate::railings::gate_of(e) {
            hosts.push(r);
        }
        if let Some(s) = crate::skylights::skylight_recipe(e) {
            hosts.push(s.roof);
        }
        if let Some(r) = crate::railings::railing_recipe(e) {
            match r.path {
                crate::railings::RailPath::Host { host, .. } => hosts.push(host),
                crate::railings::RailPath::Stair { stair, .. } => hosts.push(stair),
                crate::railings::RailPath::Free { .. } => {}
            }
        }
        hosts
    };
    let (Some(ga), Some(gb)) = (guid(a), guid(b)) else {
        return false;
    };
    let (ha, hb) = (host_of(a), host_of(b));
    // Parts of one dormer belong together; two windows in one wall do not.
    let same_dormer = match (store.get(a), store.get(b)) {
        (Some(x), Some(y)) => {
            crate::dormers::dormer_of(x).is_some()
                && crate::dormers::dormer_of(x) == crate::dormers::dormer_of(y)
        }
        _ => false,
    };
    // A curtain wall and the wall it is drawn in (ARC-WALL-013).
    let embedded = || {
        let curtain = |x: ElementId| {
            store
                .get(x)
                .and_then(crate::curtain::curtain_recipe)
                .is_some()
        };
        let (c, w) = match (curtain(a), curtain(b)) {
            (true, false) => (a, b),
            (false, true) => (b, a),
            _ => return false,
        };
        crate::curtain::embedded_cuts(store)
            .iter()
            .any(|(host, cw, _)| *host == w && *cw == c)
    };
    ha.contains(&gb)
        || hb.contains(&ga)
        || same_dormer
        || crate::csg::related(store, a, b)
        || embedded()
}
