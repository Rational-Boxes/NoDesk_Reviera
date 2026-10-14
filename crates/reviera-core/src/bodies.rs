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
    for (z, up) in [(z1, true), (z0, false)] {
        let base = positions.len() as u32;
        positions.extend(points.iter().map(|p| [p[0], p[1], z]));
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
            for (p, z) in [(a, z0), (b, z0), (b, z1), (a, z1)] {
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
    for (id, mut body) in crate::walls::solve(store).bodies {
        let class = class_of(id, "IfcWall");
        if let Some(Ok(a)) = crate::walls::attach::attached_body(store, id, &body) {
            body.geometry.height = a.z_min - body.geometry.base_z;
            push(id, &class, a.cap, false);
        }
        let mesh = match cuts.get(&id) {
            Some(c) => crate::walls::openings::cut_mesh(&body, c),
            None => body.mesh(),
        };
        push(id, &class, mesh, false);
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
        for p in &f.profiles {
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
    for (id, g) in crate::stairs::all(store) {
        for mesh in g.runs.into_iter().chain(g.landings) {
            push(id, "IfcStair", mesh, false);
        }
    }
    for p in crate::inserts::placed(store) {
        let class = p.kind.class();
        for (mesh, panel) in p.meshes() {
            push(p.id, class, mesh, panel);
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
    ha.contains(&gb) || hb.contains(&ga) || same_dormer || crate::csg::related(store, a, b)
}
