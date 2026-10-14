// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Walls attached to a roof (spec Ch 3 §3.3.13, ARC-WALL-008): the wall's top follows the roof's
//! underside over the wall's footprint, rising to a ridge or falling to an eave.
//!
//! The footprint (a convex quadrilateral, as joined) is split by the roof's faces. Each piece's
//! top lies on its face's plane, and the pieces' tops, the bottom and vertical sides around the
//! footprint make one closed solid. The roof must cover the whole footprint.
//!
//! Built so far: tops attached to Reviera roofs. Bases, floors, ceilings and reference planes as
//! targets, and partial cover, come later.

use crate::error::{Error, Result};
use crate::id::{ElementId, GlobalId};
use crate::roofs::{self, RoofGeometry};
use crate::store::Store;

use super::WallBody;

/// Triangle mesh: positions and index triples.
pub type Mesh = (Vec<[f64; 3]>, Vec<u32>);

/// Plan points closer than this are one vertex.
const TOL: f64 = 1e-6;

/// The body of a wall whose top is attached.
#[derive(Debug, Clone, PartialEq)]
pub struct Attached {
    /// The whole wall, from its base to the roof (closed).
    pub solid: Mesh,
    /// The part above `z_min` (closed), drawn over the wall cut by its openings up to `z_min`.
    pub cap: Mesh,
    /// The lowest and highest points of the top.
    pub z_min: f64,
    pub z_max: f64,
}

/// The roof a wall's top is attached to, if it still exists and regenerates.
pub fn top_target(store: &Store, wall: ElementId) -> Option<(ElementId, RoofGeometry)> {
    let e = store.get(wall)?;
    let guid = super::wall_recipe(e)?.attach_top?;
    let roof = store.find_by_guid(&GlobalId::parse(&guid)?)?;
    let g = roofs::regenerate(store, store.get(roof)?).ok()?;
    Some((roof, g))
}

/// The attached body of a wall, if its top is attached to a roof that exists.
pub fn attached_body(store: &Store, wall: ElementId, body: &WallBody) -> Option<Result<Attached>> {
    let (_, roof) = top_target(store, wall)?;
    Some(attach_to(&roof, body))
}

/// A plane given by a point and an upward normal: its height over a plan point.
#[derive(Debug, Clone, Copy)]
struct Plane {
    a: [f64; 3],
    n: [f64; 3],
}

impl Plane {
    fn z(&self, p: [f64; 2]) -> f64 {
        self.a[2] - (self.n[0] * (p[0] - self.a[0]) + self.n[1] * (p[1] - self.a[1])) / self.n[2]
    }
}

fn cross2(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn area(ring: &[[f64; 2]]) -> f64 {
    crate::sketch::signed_area(ring)
}

/// Clips `subject` to the convex counter-clockwise polygon `window` (Sutherland–Hodgman).
fn clip(subject: &[[f64; 2]], window: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut out = subject.to_vec();
    for i in 0..window.len() {
        let (a, b) = (window[i], window[(i + 1) % window.len()]);
        let d = [b[0] - a[0], b[1] - a[1]];
        let side = |p: [f64; 2]| cross2(d, [p[0] - a[0], p[1] - a[1]]);
        let input = std::mem::take(&mut out);
        for k in 0..input.len() {
            let (p, q) = (input[k], input[(k + 1) % input.len()]);
            let (sp, sq) = (side(p), side(q));
            if sp >= 0.0 {
                out.push(p);
            }
            if (sp >= 0.0) != (sq >= 0.0) {
                let t = sp / (sp - sq);
                out.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
            }
        }
        if out.is_empty() {
            break;
        }
    }
    // Drop repeated points.
    let mut ring: Vec<[f64; 2]> = Vec::new();
    for p in out {
        if ring
            .last()
            .is_none_or(|q: &[f64; 2]| (q[0] - p[0]).hypot(q[1] - p[1]) > TOL)
        {
            ring.push(p);
        }
    }
    while ring.len() > 1 {
        let (f, l) = (ring[0], ring[ring.len() - 1]);
        if (f[0] - l[0]).hypot(f[1] - l[1]) <= TOL {
            ring.pop();
        } else {
            break;
        }
    }
    ring
}

/// The wall's body attached to `roof`.
pub fn attach_to(roof: &RoofGeometry, body: &WallBody) -> Result<Attached> {
    let mut footprint = body.footprint.to_vec();
    if area(&footprint) < 0.0 {
        footprint.reverse();
    }
    // The roof's sloped and flat faces over the footprint.
    let mut pieces: Vec<(Vec<[f64; 2]>, Plane)> = Vec::new();
    for f in &roof.faces {
        if f.normal[2] < 1e-6 {
            continue;
        }
        let mut ring: Vec<[f64; 2]> = f.bottom.iter().map(|p| [p[0], p[1]]).collect();
        if area(&ring) < 0.0 {
            ring.reverse();
        }
        let piece = clip(&ring, &footprint);
        if piece.len() >= 3 && area(&piece) > 1e-10 {
            pieces.push((
                piece,
                Plane {
                    a: f.bottom[0],
                    n: f.normal,
                },
            ));
        }
    }
    let covered: f64 = pieces.iter().map(|(p, _)| area(p)).sum();
    let whole = area(&footprint);
    if (covered - whole).abs() > 1e-6 * whole.max(1e-6) + 1e-9 {
        return Err(Error::new(
            "wall.attach_not_covered",
            "the roof does not cover the whole wall",
        ));
    }
    let base = body.geometry.base_z;
    // Top vertices, welded by plan position.
    let mut top: Vec<[f64; 3]> = Vec::new();
    let vertex = |p: [f64; 2], z: f64, top: &mut Vec<[f64; 3]>| -> u32 {
        if let Some(i) = top
            .iter()
            .position(|q| (q[0] - p[0]).hypot(q[1] - p[1]) <= TOL)
        {
            return i as u32;
        }
        top.push([p[0], p[1], z]);
        (top.len() - 1) as u32
    };
    let mut top_tris: Vec<[u32; 3]> = Vec::new();
    for (ring, plane) in &pieces {
        let ids: Vec<u32> = ring
            .iter()
            .map(|p| vertex(*p, plane.z(*p), &mut top))
            .collect();
        let mut tris: Vec<u32> = Vec::new();
        earcut::Earcut::new().earcut(ring.iter().copied(), &[] as &[u32], &mut tris);
        for t in tris.chunks(3) {
            let [a, b, c] = [t[0] as usize, t[1] as usize, t[2] as usize];
            let ccw = cross2(
                [ring[b][0] - ring[a][0], ring[b][1] - ring[a][1]],
                [ring[c][0] - ring[a][0], ring[c][1] - ring[a][1]],
            ) > 0.0;
            if ccw {
                top_tris.push([ids[a], ids[b], ids[c]]);
            } else {
                top_tris.push([ids[a], ids[c], ids[b]]);
            }
        }
    }
    // The boundary: the footprint's corners and every top vertex on its edges, in order.
    let n = footprint.len();
    let mut boundary: Vec<u32> = Vec::new();
    for i in 0..n {
        let (a, b) = (footprint[i], footprint[(i + 1) % n]);
        let d = [b[0] - a[0], b[1] - a[1]];
        let len2 = d[0] * d[0] + d[1] * d[1];
        let mut on: Vec<(f64, u32)> = top
            .iter()
            .enumerate()
            .filter_map(|(k, q)| {
                let w = [q[0] - a[0], q[1] - a[1]];
                let t = (w[0] * d[0] + w[1] * d[1]) / len2;
                let off = cross2(d, w).abs() / len2.sqrt();
                (off <= 10.0 * TOL && t > -1e-9 && t < 1.0 - 1e-9).then_some((t, k as u32))
            })
            .collect();
        on.sort_by(|x, y| x.0.total_cmp(&y.0));
        boundary.extend(on.into_iter().map(|(_, k)| k));
    }
    boundary.dedup();
    let z_min = top.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
    let z_max = top.iter().map(|p| p[2]).fold(f64::NEG_INFINITY, f64::max);
    if z_min <= base + 0.01 {
        return Err(Error::new(
            "wall.attach_below_base",
            "the roof is at or below the wall's base",
        ));
    }
    let solid = closed(&top, &top_tris, &boundary, base);
    let cap = closed(&top, &top_tris, &boundary, z_min);
    Ok(Attached {
        solid,
        cap,
        z_min,
        z_max,
    })
}

/// A closed mesh: the top triangles, a flat bottom at `bottom`, and vertical sides around the
/// boundary.
fn closed(top: &[[f64; 3]], top_tris: &[[u32; 3]], boundary: &[u32], bottom: f64) -> Mesh {
    let n = top.len() as u32;
    let mut positions = top.to_vec();
    positions.extend(top.iter().map(|p| [p[0], p[1], bottom]));
    let mut indices: Vec<u32> = top_tris.iter().flatten().copied().collect();
    // Bottom: the boundary ring, facing down.
    let ring: Vec<[f64; 2]> = boundary
        .iter()
        .map(|k| [top[*k as usize][0], top[*k as usize][1]])
        .collect();
    let mut tris: Vec<u32> = Vec::new();
    earcut::Earcut::new().earcut(ring.iter().copied(), &[] as &[u32], &mut tris);
    for t in tris.chunks(3) {
        let [a, b, c] = [t[0] as usize, t[1] as usize, t[2] as usize];
        let ccw = cross2(
            [ring[b][0] - ring[a][0], ring[b][1] - ring[a][1]],
            [ring[c][0] - ring[a][0], ring[c][1] - ring[a][1]],
        ) > 0.0;
        let (a, b, c) = if ccw { (a, b, c) } else { (a, c, b) };
        indices.extend([n + boundary[a], n + boundary[c], n + boundary[b]]);
    }
    // Sides, facing out of the counter-clockwise boundary.
    for i in 0..boundary.len() {
        let (a, b) = (boundary[i], boundary[(i + 1) % boundary.len()]);
        indices.extend([n + a, n + b, b, n + a, b, a]);
    }
    (positions, indices)
}

// ---- Commands ----

/// Which end of a wall attaches.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum End {
    #[default]
    Top,
    Base,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AttachParams {
    pub wall_ids: Vec<crate::id::ElementRef>,
    /// The end that attaches (default top).
    #[serde(default)]
    pub end: End,
    /// The roof the walls attach to.
    pub target_id: crate::id::ElementRef,
}

fn set_target(
    txn: &mut crate::txn::Transaction<'_>,
    wall: ElementId,
    target: Option<String>,
) -> Result<()> {
    let e = txn.store().get(wall).expect("resolved");
    let mut recipe = super::wall_recipe(e).ok_or_else(|| {
        Error::new(
            "wall.not_reviera",
            format!("element {wall} is not a Reviera wall"),
        )
    })?;
    if recipe.attach_top == target {
        return Ok(());
    }
    recipe.attach_top = target;
    let value = serde_json::to_value(&recipe).expect("serialises");
    txn.modify(wall, |e| {
        e.params.insert(super::RECIPE_KEY.into(), value);
    })
}

/// Attaches walls' tops to a roof; a wall the roof does not cover is refused.
pub(crate) fn attach(txn: &mut crate::txn::Transaction<'_>, p: AttachParams) -> Result<()> {
    if p.end == End::Base {
        return Err(Error::new(
            "wall.attach_base_unsupported",
            "attaching wall bases comes later; walls can attach their tops to roofs",
        ));
    }
    let store = txn.store().clone();
    let target = store
        .resolve(&p.target_id)
        .ok_or_else(|| Error::new("element.not_found", "no such attach target"))?;
    if store.get(target).and_then(roofs::roof_recipe).is_none() {
        return Err(Error::new(
            "wall.attach_target",
            "walls can attach to Reviera roofs (floors, ceilings and planes come later)",
        ));
    }
    let guid = store.get(target).expect("resolved").guid().to_string();
    let mut ids = Vec::new();
    for r in &p.wall_ids {
        let id = store
            .resolve(r)
            .ok_or_else(|| Error::new("element.not_found", "no such wall"))?;
        crate::modify::check_unpinned(&store, id)?;
        set_target(txn, id, Some(guid.clone()))?;
        ids.push(id);
    }
    // Every wall must fit under the roof.
    let solved = super::solve(txn.store());
    for id in ids {
        // A curved wall: every piece along its curve.
        let store = txn.store();
        let e = store.get(id).expect("resolved");
        if let Some(path) = super::wall_recipe(e).and_then(|r| super::curved::path_of(&r)) {
            let g = super::regenerate(store, e)?;
            let (_, roof) = top_target(store, id).ok_or_else(|| {
                Error::new("wall.attach_bad_target", "the roof does not regenerate")
            })?;
            super::curved::attach_check(&path, g.width, g.base_z, &roof)
                .map_err(|e| Error::new(e.code, format!("wall {id}: {}", e.message)))?;
            continue;
        }
        let body = solved.bodies.get(&id).ok_or_else(|| {
            Error::new(
                "wall.cannot_regenerate",
                format!("wall {id} cannot regenerate"),
            )
        })?;
        if let Some(Err(e)) = attached_body(txn.store(), id, body) {
            return Err(Error::new(e.code, format!("wall {id}: {}", e.message)));
        }
    }
    Ok(())
}

/// The Reviera walls under `roof` that are not attached to it yet: perimeter walls (one side
/// outside every enclosed region of their level) that the roof covers whole and that start
/// below its lowest point over them (ARC-ROOF-001: the walls whose gables fill when a roof is
/// finished). Interior walls are left to be attached by hand.
/// Whether another wall stands on `body`: its base at or above `body`'s top, with the middle
/// of one wall's location line inside the other's footprint.
fn has_wall_above(solved: &super::Solved, id: ElementId, body: &super::WallBody) -> bool {
    let top = body.geometry.base_z + body.geometry.height;
    let inside = |p: [f64; 2], q: &[[f64; 2]; 4]| {
        (0..4).all(|i| {
            let (a, b) = (q[i], q[(i + 1) % 4]);
            (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]) >= -1e-9
        }) || (0..4).all(|i| {
            let (a, b) = (q[i], q[(i + 1) % 4]);
            (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]) <= 1e-9
        })
    };
    let mid =
        |g: &super::WallGeometry| [(g.start[0] + g.end[0]) / 2.0, (g.start[1] + g.end[1]) / 2.0];
    solved.bodies.iter().any(|(other, o)| {
        *other != id
            && (o.geometry.base_z - top).abs() < 0.05
            && (inside(mid(&o.geometry), &body.footprint)
                || inside(mid(&body.geometry), &o.footprint))
    })
}

pub fn walls_below(store: &Store, roof: ElementId) -> Vec<ElementId> {
    let Some(e) = store.get(roof) else {
        return Vec::new();
    };
    let Ok(g) = roofs::regenerate(store, e) else {
        return Vec::new();
    };
    let guid = e.guid().to_string();
    let solved = super::solve(store);
    let mut out: Vec<ElementId> = solved
        .bodies
        .iter()
        .filter(|(id, _)| {
            store
                .get(**id)
                .and_then(super::wall_recipe)
                .is_some_and(|r| r.attach_top.as_deref() != Some(guid.as_str()))
        })
        .filter(|(_, body)| attach_to(&g, body).is_ok_and(|a| a.z_min > body.geometry.base_z + 0.1))
        // A wall with another standing on it (a lower storey's) does not reach the roof.
        .filter(|(id, body)| !has_wall_above(&solved, **id, body))
        .map(|(id, _)| *id)
        .collect();
    // Enclosed regions by the height they are taken at (each wall's base, a little up).
    let mut regions: Vec<(i64, Vec<crate::arrangement::Region>)> = Vec::new();
    out.retain(|id| {
        let geometry = &solved.bodies[id].geometry;
        let z = geometry.base_z + 0.05;
        let key = (z * 1000.0).round() as i64;
        if !regions.iter().any(|(k, _)| *k == key) {
            let edges = crate::arrangement::wall_edges(store, z);
            regions.push((key, crate::arrangement::regions(&edges)));
        }
        let rs = &regions.iter().find(|(k, _)| *k == key).expect("added").1;
        is_perimeter(geometry, rs, &g)
    });
    // Curved walls (ARC-WALL-002): covered all along and starting below the roof, on the
    // perimeter judged at the curve's middle.
    for e in store.iter() {
        let Some(r) = super::wall_recipe(e) else {
            continue;
        };
        if r.attach_top.as_deref() == Some(guid.as_str()) {
            continue;
        }
        let (Some(path), Ok(wg)) = (super::curved::path_of(&r), super::regenerate(store, e)) else {
            continue;
        };
        let Ok(z_min) = super::curved::attach_check(&path, wg.width, wg.base_z, &g) else {
            continue;
        };
        if z_min <= wg.base_z + 0.1 {
            continue;
        }
        let (mid, d) = path.at(path.length() / 2.0);
        let at_mid = super::WallGeometry {
            start: [mid[0] - d[0] * 0.05, mid[1] - d[1] * 0.05],
            end: [mid[0] + d[0] * 0.05, mid[1] + d[1] * 0.05],
            ..wg
        };
        let z = wg.base_z + 0.05;
        let key = (z * 1000.0).round() as i64;
        if !regions.iter().any(|(k, _)| *k == key) {
            let edges = crate::arrangement::wall_edges(store, z);
            regions.push((key, crate::arrangement::regions(&edges)));
        }
        let rs = &regions.iter().find(|(k, _)| *k == key).expect("added").1;
        if is_perimeter(&at_mid, rs, &g) {
            out.push(e.id());
        }
    }
    out.sort();
    out
}

/// Whether a wall is on the perimeter: beside one of its faces, at its middle, lies no enclosed
/// region, or the region there reaches out from under the roof (a courtyard).
fn is_perimeter(
    g: &super::WallGeometry,
    regions: &[crate::arrangement::Region],
    roof: &RoofGeometry,
) -> bool {
    let m = [(g.start[0] + g.end[0]) / 2.0, (g.start[1] + g.end[1]) / 2.0];
    let n = g.normal();
    let d = g.width / 2.0 + 0.05;
    [1.0, -1.0].iter().any(|s| {
        let at = |k: f64| [m[0] + n[0] * s * (d + k), m[1] + n[1] * s * (d + k)];
        let p = at(0.0);
        let Some(r) = regions.iter().find(|r| r.contains(p)) else {
            return true;
        };
        // Walk away from the wall within the region: a spot the roof leaves open is outdoors.
        (1..=200)
            .map(|k| at(0.25 * f64::from(k)))
            .take_while(|q| r.contains(*q))
            .any(|q| roof.underside(q).is_none())
    })
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AttachBelowParams {
    pub roof_id: crate::id::ElementRef,
}

/// Attaches every wall under a roof to it, so gable walls rise to the roof and the rest meet
/// its underside (`roof.attach_walls`). Returns the walls attached.
pub(crate) fn attach_below(
    txn: &mut crate::txn::Transaction<'_>,
    p: AttachBelowParams,
) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let roof = store
        .resolve(&p.roof_id)
        .filter(|r| store.get(*r).and_then(roofs::roof_recipe).is_some())
        .ok_or_else(|| Error::new("element.not_found", "no such roof"))?;
    let walls = walls_below(&store, roof);
    if walls.is_empty() {
        return Ok(walls);
    }
    attach(
        txn,
        AttachParams {
            wall_ids: walls
                .iter()
                .map(|w| crate::id::ElementRef::Id(*w))
                .collect(),
            end: End::Top,
            target_id: crate::id::ElementRef::Id(roof),
        },
    )?;
    Ok(walls)
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DetachParams {
    pub wall_ids: Vec<crate::id::ElementRef>,
    /// The end to detach (default top).
    #[serde(default)]
    pub end: End,
    /// Detach only from these targets; omitted, from all (Detach All).
    pub target_ids: Option<Vec<crate::id::ElementRef>>,
}

/// Detaches walls' tops; they return to their own top constraint.
pub(crate) fn detach(txn: &mut crate::txn::Transaction<'_>, p: DetachParams) -> Result<()> {
    let store = txn.store().clone();
    let only: Option<Vec<String>> = p.target_ids.as_ref().map(|t| {
        t.iter()
            .filter_map(|r| Some(store.get(store.resolve(r)?)?.guid().to_string()))
            .collect()
    });
    if p.end == End::Base {
        return Ok(());
    }
    for r in &p.wall_ids {
        let id = store
            .resolve(r)
            .ok_or_else(|| Error::new("element.not_found", "no such wall"))?;
        crate::modify::check_unpinned(&store, id)?;
        let current = store
            .get(id)
            .and_then(super::wall_recipe)
            .and_then(|r| r.attach_top);
        if current.is_some_and(|c| only.as_ref().is_none_or(|o| o.contains(&c))) {
            set_target(txn, id, None)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roofs::{RoofEdge, generate};
    use crate::walls::WallGeometry;

    fn volume(m: &Mesh) -> f64 {
        m.1.chunks(3)
            .map(|t| {
                let [a, b, c] = [m.0[t[0] as usize], m.0[t[1] as usize], m.0[t[2] as usize]];
                (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                    + a[2] * (b[0] * c[1] - b[1] * c[0]))
                    / 6.0
            })
            .sum()
    }

    fn wall(start: [f64; 2], end: [f64; 2]) -> WallBody {
        let geometry = WallGeometry {
            start,
            end,
            base_z: 0.0,
            height: 3.0,
            width: 0.2,
        };
        WallBody {
            geometry,
            footprint: geometry.footprint(),
        }
    }

    #[test]
    fn a_gable_wall_rises_to_the_ridge() {
        // A 10 × 6 gable roof at 45° on a 3 m plate, gables at x = 0 and x = 10, 0.5 m overhang.
        let ring = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 6.0], [0.0, 6.0]];
        let edges: Vec<RoofEdge> = (0..4)
            .map(|i| RoofEdge {
                defines_slope: i % 2 == 0,
                slope: None,
                overhang: 0.5,
                eave: None,
            })
            .collect();
        let roof = generate(
            &ring,
            &[1, 2, 3, 4],
            &edges,
            std::f64::consts::FRAC_PI_4,
            3.0,
            None,
            0.3,
        )
        .unwrap();
        // The west gable wall, along y at x = 0.
        let a = attach_to(&roof, &wall([0.0, 6.0], [0.0, 0.0])).unwrap();
        assert!((a.z_max - 6.0).abs() < 1e-9, "{}", a.z_max);
        // At its ends, on the eave plate lines, the wall stops at the plate.
        assert!((a.z_min - 3.0).abs() < 1e-9, "{}", a.z_min);
        // Volume: a 6 m × 0.2 m wall up to the plate, plus the gable triangle (6 × 3 / 2) × 0.2.
        let v = volume(&a.solid);
        let want = 6.0 * 0.2 * 3.0 + 6.0 * 3.0 / 2.0 * 0.2;
        assert!((v - want).abs() < 1e-9, "{v} vs {want}");
        assert!(volume(&a.cap) > 0.0);
        // An eave wall stops under the eave: flat top at the plate (sloping across its width).
        let e = attach_to(&roof, &wall([0.0, 0.0], [10.0, 0.0])).unwrap();
        assert!(e.z_max < 3.11 && e.z_min > 2.89, "{} {}", e.z_min, e.z_max);
        // A wall outside the roof cannot attach.
        let out = attach_to(&roof, &wall([20.0, 0.0], [30.0, 0.0])).unwrap_err();
        assert_eq!(out.code, "wall.attach_not_covered");
    }
}
