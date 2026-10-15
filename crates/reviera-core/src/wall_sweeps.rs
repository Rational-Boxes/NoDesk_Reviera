// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Wall sweeps and reveals (spec Ch 3 §3.3.15, ARC-WALL-010): a profile run along walls'
//! faces, adding (a cornice, a skirting) or cutting (a groove).
//!
//! A sweep or reveal has a recipe (`recipe.wall_sweep`): its walls (GlobalIds, a run of joined
//! walls), the face side, horizontal at a height above a level or vertical at a distance along
//! its first wall, and its profile: a polygon with x out from the face and y up (horizontal) or
//! along the wall (vertical). A horizontal run follows the walls' faces, mitred where they meet.
//! A reveal is subtracted from its walls, and a sweep that *Cuts Wall* too, through the model's
//! boolean relations (ARC-MOD-028), so the 3D view, plans and IFC show the cut.
//!
//! IFC: a sweep is an `IfcCovering` (`MOLDING`) and a reveal an `IfcOpeningElement` (`RECESS`),
//! each with a faceted B-rep body.
//!
//! Not built yet: profile families (the profile is a rectangle or typed points), returns
//! (Straight only), type-level sweeps and reveals, Cut by Inserts, curved walls, slanted walls'
//! Orientation, end drag handles.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bodies::Mesh;
use crate::csg::{self, CsgOp};
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::{self, RECIPE_KEY, wall_recipe};

pub const RECIPE: &str = "recipe.wall_sweep";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    #[default]
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    #[default]
    Exterior,
    Interior,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WallSweepRecipe {
    pub kind: String,
    /// A reveal (cut into the walls) rather than a sweep.
    #[serde(default)]
    pub reveal: bool,
    /// The walls it runs along (GlobalIds).
    pub walls: Vec<String>,
    #[serde(default)]
    pub orientation: Orientation,
    #[serde(default)]
    pub side: Side,
    /// The level its height is measured from (GlobalId).
    pub level: String,
    /// Horizontal: the profile's origin above the level (metres).
    #[serde(default)]
    pub offset: f64,
    /// Vertical: the profile's origin along the first wall from its start (metres).
    #[serde(default)]
    pub distance: f64,
    /// Out from the face (metres).
    #[serde(default)]
    pub wall_offset: f64,
    /// The profile: x out from the face, y up (horizontal) or along the wall (vertical).
    pub profile: Vec<[f64; 2]>,
    /// A sweep that also cuts its walls where they overlap.
    #[serde(default)]
    pub cuts_wall: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
}

pub fn recipe(e: &Element) -> Option<WallSweepRecipe> {
    let v = e.param(RECIPE_KEY)?;
    if v.get("kind")?.as_str()? != RECIPE {
        return None;
    }
    serde_json::from_value(v.clone()).ok()
}

fn wall_by_guid<'a>(store: &'a Store, guid: &str) -> Option<&'a Element> {
    store.get(store.find_by_guid(&GlobalId::parse(guid)?)?)
}

/// The unit vector out of a wall's face on a side.
fn outward(e: &Element, g: &walls::WallGeometry, side: Side) -> [f64; 2] {
    let flip = wall_recipe(e).is_some_and(|r| r.flip);
    let ext = if flip { -1.0 } else { 1.0 };
    let s = match side {
        Side::Exterior => ext,
        Side::Interior => -ext,
    };
    let n = g.normal();
    [n[0] * s, n[1] * s]
}

/// The face segments of the walls on a side, each directed so that the face's outside is on
/// its right, after joins.
fn face_segments(store: &Store, r: &WallSweepRecipe) -> Vec<([f64; 2], [f64; 2])> {
    let solved = walls::solve(store);
    r.walls
        .iter()
        .filter_map(|guid| {
            let e = wall_by_guid(store, guid)?;
            let body = solved.bodies.get(&e.id())?;
            let g = &body.geometry;
            let o = outward(e, g, r.side);
            let f = body.footprint;
            let d = |p: [f64; 2]| (p[0] - g.start[0]) * o[0] + (p[1] - g.start[1]) * o[1];
            // Corners 0, 1 make one long side and 3, 2 the other, start to end.
            let (a, b) = if d(f[0]) > d(f[3]) {
                (f[0], f[1])
            } else {
                (f[3], f[2])
            };
            let right = [b[1] - a[1], -(b[0] - a[0])];
            Some(if right[0] * o[0] + right[1] * o[1] >= 0.0 {
                (a, b)
            } else {
                (b, a)
            })
        })
        .collect()
}

/// Joins segments end to start into runs: `(points, closed)`.
fn chains(mut segs: Vec<([f64; 2], [f64; 2])>) -> Vec<(Vec<[f64; 2]>, bool)> {
    let near = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).hypot(p[1] - q[1]) < 1e-4;
    let mut out = Vec::new();
    while !segs.is_empty() {
        // Start where no other segment ends, if there is such a segment.
        let start = (0..segs.len())
            .find(|&i| !segs.iter().any(|s| near(s.1, segs[i].0)))
            .unwrap_or(0);
        let (a, b) = segs.remove(start);
        let mut path = vec![a, b];
        while let Some(k) = segs
            .iter()
            .position(|s| near(s.0, *path.last().expect("has points")))
        {
            let (_, b) = segs.remove(k);
            path.push(b);
        }
        let closed = path.len() > 3 && near(path[0], *path.last().expect("has points"));
        if closed {
            path.pop();
        }
        out.push((path, closed));
    }
    out
}

/// A profile swept along a plan path at height `z`, mitred at its corners: the profile's x is
/// to the right of the path and its y up. Its ends are capped when the path is open.
pub fn sweep_mitred(path: &[[f64; 2]], closed: bool, z: f64, profile: &[[f64; 2]]) -> Mesh {
    let m = path.len();
    if m < 2 || profile.len() < 3 {
        return (Vec::new(), Vec::new());
    }
    let mut profile = profile.to_vec();
    if crate::polygons::area(&profile) < 0.0 {
        profile.reverse();
    }
    let n = profile.len();
    let seg_out = |k: usize| {
        let (a, b) = (path[k % m], path[(k + 1) % m]);
        let l = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-12);
        [(b[1] - a[1]) / l, -(b[0] - a[0]) / l]
    };
    let segments = if closed { m } else { m - 1 };
    let miter = |k: usize| -> [f64; 2] {
        let (prev, next) = match (closed, k) {
            (false, 0) => (seg_out(0), seg_out(0)),
            (false, k) if k == m - 1 => (seg_out(m - 2), seg_out(m - 2)),
            (_, k) => (seg_out((k + m - 1) % m), seg_out(k % m)),
        };
        let s = [prev[0] + next[0], prev[1] + next[1]];
        let sl = s[0].hypot(s[1]);
        if sl < 1e-9 {
            return next;
        }
        let u = [s[0] / sl, s[1] / sl];
        let cos = (u[0] * next[0] + u[1] * next[1]).max(0.2);
        [u[0] / cos, u[1] / cos]
    };
    let mut positions = Vec::with_capacity(m * n);
    for (k, corner) in path.iter().enumerate() {
        let mv = miter(k);
        for p in &profile {
            positions.push([corner[0] + mv[0] * p[0], corner[1] + mv[1] * p[0], z + p[1]]);
        }
    }
    let at = |k: usize, j: usize| ((k % m) * n + j) as u32;
    let mut indices: Vec<u32> = Vec::new();
    for k in 0..segments {
        for j in 0..n {
            let j1 = (j + 1) % n;
            let (a, b, c, d) = (at(k, j), at(k, j1), at(k + 1, j1), at(k + 1, j));
            indices.extend([a, c, b, a, d, c]);
        }
    }
    if !closed {
        let mut tris: Vec<u32> = Vec::new();
        earcut::Earcut::new().earcut(profile.iter().copied(), &[], &mut tris);
        let l = m - 1;
        for t in tris.chunks(3) {
            let ccw = {
                let [a, b, c] = [
                    profile[t[0] as usize],
                    profile[t[1] as usize],
                    profile[t[2] as usize],
                ];
                (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]) > 0.0
            };
            let (i, j, k) = (t[0] as usize, t[1] as usize, t[2] as usize);
            let (j, k) = if ccw { (j, k) } else { (k, j) };
            indices.extend([at(0, i), at(0, j), at(0, k)]);
            indices.extend([at(l, i), at(l, k), at(l, j)]);
        }
    }
    (positions, indices)
}

/// A sweep's or reveal's body.
pub fn regenerate(store: &Store, e: &Element) -> Result<Vec<Mesh>> {
    let r = recipe(e).ok_or_else(|| Error::new("wallsweep.not", "not a wall sweep"))?;
    let level_z = crate::levels::elevation_of(store, &r.level)?;
    let profile: Vec<[f64; 2]> = r
        .profile
        .iter()
        .map(|p| [p[0] + r.wall_offset, p[1]])
        .collect();
    match r.orientation {
        Orientation::Horizontal => Ok(chains(face_segments(store, &r))
            .into_iter()
            .map(|(path, closed)| sweep_mitred(&path, closed, level_z + r.offset, &profile))
            .collect()),
        Orientation::Vertical => {
            let w = r
                .walls
                .first()
                .and_then(|g| wall_by_guid(store, g))
                .ok_or_else(|| Error::new("wallsweep.no_wall", "the sweep's wall is missing"))?;
            let g = walls::regenerate(store, w)?;
            let (u, o) = (g.direction(), outward(w, &g, r.side));
            let p = [
                g.start[0] + u[0] * r.distance + o[0] * g.width / 2.0,
                g.start[1] + u[1] * r.distance + o[1] * g.width / 2.0,
            ];
            let outline: Vec<[f64; 2]> = profile
                .iter()
                .map(|q| {
                    [
                        p[0] + o[0] * q[0] + u[0] * q[1],
                        p[1] + o[1] * q[0] + u[1] * q[1],
                    ]
                })
                .collect();
            Ok(vec![crate::bodies::extrude(
                &outline,
                &[],
                g.base_z,
                g.base_z + g.height,
            )])
        }
    }
}

/// Every sweep and reveal that regenerates.
pub fn all(store: &Store) -> Vec<(ElementId, bool, Vec<Mesh>)> {
    store
        .iter()
        .filter_map(|e| {
            let r = recipe(e)?;
            Some((e.id(), r.reveal, regenerate(store, e).ok()?))
        })
        .collect()
}

/// Walls joined end to end with `first` at the sweep's height, on the same level, through
/// corners where only two walls meet.
fn joined_run(store: &Store, first: ElementId, z: f64) -> Vec<ElementId> {
    let straight: Vec<(ElementId, walls::WallGeometry)> = store
        .iter()
        .filter(|e| wall_recipe(e).is_some_and(|r| r.through.is_empty()))
        .filter_map(|e| Some((e.id(), walls::regenerate(store, e).ok()?)))
        .filter(|(_, g)| z > g.base_z - 1e-9 && z < g.base_z + g.height + 1e-9)
        .collect();
    let near = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).hypot(p[1] - q[1]) < 1e-3;
    let at = |p: [f64; 2]| -> Vec<ElementId> {
        straight
            .iter()
            .filter(|(_, g)| near(g.start, p) || near(g.end, p))
            .map(|(id, _)| *id)
            .collect()
    };
    let mut run = vec![first];
    let mut frontier = vec![first];
    while let Some(id) = frontier.pop() {
        let Some((_, g)) = straight.iter().find(|(w, _)| *w == id) else {
            continue;
        };
        for p in [g.start, g.end] {
            let here = at(p);
            if here.len() != 2 {
                continue;
            }
            for w in here {
                if !run.contains(&w) {
                    run.push(w);
                    frontier.push(w);
                }
            }
        }
    }
    run
}

/// Keeps the boolean relations of a sweep in step with its walls: a reveal, or a sweep that
/// cuts its walls, is subtracted from each of them.
fn sync_relations(txn: &mut Transaction<'_>, id: ElementId) -> Result<()> {
    let store = txn.store().clone();
    let e = store.get(id).expect("exists");
    let r = recipe(e).expect("a sweep");
    let targets: Vec<String> = if r.reveal || r.cuts_wall {
        r.walls.clone()
    } else {
        Vec::new()
    };
    csg::set_tool_targets(txn, e.guid().as_str(), &targets, CsgOp::Subtract, r.reveal)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// The wall(s) to run along; a horizontal run continues around the walls joined end to end
    /// with the first unless `chain` is false.
    pub wall_ids: Vec<ElementRef>,
    #[serde(default)]
    pub orientation: Orientation,
    #[serde(default)]
    pub side: Side,
    /// Horizontal: the height above the first wall's base level (metres).
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset_from_level: Option<f64>,
    /// Vertical: the distance along the first wall from its start (metres).
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub distance_along: Option<f64>,
    /// A rectangular profile: out from the face (into it for a reveal) and up (metres).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub width: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub height: Option<f64>,
    /// Or the profile's points (x out from the face, y up or along), instead of the rectangle.
    pub profile: Option<Vec<[f64; 2]>>,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub wall_offset: f64,
    #[serde(default)]
    pub cuts_wall: bool,
    pub material: Option<String>,
    /// Continue around joined walls (horizontal; default true).
    pub chain: Option<bool>,
}

fn create(txn: &mut Transaction<'_>, p: CreateParams, reveal: bool) -> Result<ElementId> {
    let store = txn.store().clone();
    let mut ids = Vec::new();
    for r in &p.wall_ids {
        let id = store
            .resolve(r)
            .filter(|id| {
                store
                    .get(*id)
                    .and_then(wall_recipe)
                    .is_some_and(|w| w.through.is_empty())
            })
            .ok_or_else(|| {
                Error::new(
                    "wallsweep.not_wall",
                    format!("{r:?} is not a straight Reviera wall"),
                )
            })?;
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    let Some(&first) = ids.first() else {
        return Err(Error::new("wallsweep.no_wall", "pick at least one wall"));
    };
    let e = store.get(first).expect("resolved");
    let w = wall_recipe(e).expect("a wall");
    if reveal
        && matches!(
            w.cross_section,
            Some(walls::cross_section::CrossSection::Slanted { .. })
        )
    {
        return Err(Error::new(
            "wallreveal.slanted",
            "reveals are not allowed on slanted walls",
        ));
    }
    let g = walls::regenerate(&store, e)?;
    let level_z = g.base_z - w.base_offset;
    let offset = p
        .offset_from_level
        .unwrap_or(g.height / 2.0 + w.base_offset);
    if p.orientation == Orientation::Horizontal && p.chain.unwrap_or(true) {
        for id in joined_run(&store, first, level_z + offset) {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    let (wd, ht) = (
        p.width.unwrap_or(if reveal { 0.02 } else { 0.05 }),
        p.height.unwrap_or(if reveal { 0.02 } else { 0.1 }),
    );
    let profile = match p.profile {
        Some(pts) => pts,
        // A reveal reaches a millimetre outside the face so that the cut is clean.
        None if reveal => vec![[-wd, 0.0], [0.001, 0.0], [0.001, ht], [-wd, ht]],
        None => vec![[0.0, 0.0], [wd, 0.0], [wd, ht], [0.0, ht]],
    };
    if profile.len() < 3 || crate::polygons::area(&profile).abs() < 1e-9 {
        return Err(Error::new(
            "wallsweep.profile",
            "the profile needs an area (a width and a height)",
        ));
    }
    let recipe = WallSweepRecipe {
        kind: RECIPE.into(),
        reveal,
        walls: ids
            .iter()
            .map(|id| store.get(*id).expect("resolved").guid().to_string())
            .collect(),
        orientation: p.orientation,
        side: p.side,
        level: w.base_level.clone(),
        offset,
        distance: p.distance_along.unwrap_or(g.length() / 2.0),
        wall_offset: p.wall_offset,
        profile,
        cuts_wall: p.cuts_wall,
        material: p.material,
    };
    let (class, predefined) = if reveal {
        ("IfcOpeningElement", "RECESS")
    } else {
        ("IfcCovering", "MOLDING")
    };
    let id = txn.create(
        NewElement::new(class)
            .name(if reveal { "Reveal" } else { "Wall Sweep" })
            .param("@PredefinedType", predefined)
            .param(
                RECIPE_KEY,
                serde_json::to_value(&recipe).expect("serialises"),
            ),
    )?;
    regenerate(txn.store(), txn.store().get(id).expect("created"))?;
    sync_relations(txn, id)?;
    Ok(id)
}

/// `wallsweep.create`.
pub(crate) fn create_sweep(txn: &mut Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    create(txn, p, false)
}

/// `wallreveal.create`.
pub(crate) fn create_reveal(txn: &mut Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    create(txn, p, true)
}

fn sweep(store: &Store, r: &ElementRef) -> Result<(ElementId, WallSweepRecipe)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such sweep"))?;
    let rec = store
        .get(id)
        .and_then(recipe)
        .ok_or_else(|| Error::new("wallsweep.not", "not a wall sweep or reveal"))?;
    Ok((id, rec))
}

fn write(txn: &mut Transaction<'_>, id: ElementId, r: &WallSweepRecipe) -> Result<()> {
    let value = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })?;
    sync_relations(txn, id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WallsParams {
    pub sweep_id: ElementRef,
    pub wall_ids: Vec<ElementRef>,
}

fn guids(store: &Store, ids: &[ElementRef]) -> Result<Vec<String>> {
    ids.iter()
        .map(|r| {
            store
                .resolve(r)
                .filter(|id| store.get(*id).and_then(wall_recipe).is_some())
                .map(|id| store.get(id).expect("resolved").guid().to_string())
                .ok_or_else(|| Error::new("wallsweep.not_wall", format!("{r:?} is not a wall")))
        })
        .collect()
}

/// `wallsweep.add_walls`.
pub(crate) fn add_walls(txn: &mut Transaction<'_>, p: WallsParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = sweep(&store, &p.sweep_id)?;
    for g in guids(&store, &p.wall_ids)? {
        if !r.walls.contains(&g) {
            r.walls.push(g);
        }
    }
    write(txn, id, &r)
}

/// `wallsweep.remove_walls`; a sweep left with no walls is deleted.
pub(crate) fn remove_walls(txn: &mut Transaction<'_>, p: WallsParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = sweep(&store, &p.sweep_id)?;
    let gone = guids(&store, &p.wall_ids)?;
    r.walls.retain(|g| !gone.contains(g));
    if r.walls.is_empty() {
        csg::set_tool_targets(
            txn,
            store.get(id).expect("exists").guid().as_str(),
            &[],
            CsgOp::Subtract,
            false,
        )?;
        return txn.delete(id);
    }
    write(txn, id, &r)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub sweep_id: ElementRef,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset_from_level: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub distance_along: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub wall_offset: Option<f64>,
    pub side: Option<Side>,
    pub profile: Option<Vec<[f64; 2]>>,
    pub cuts_wall: Option<bool>,
    pub material: Option<String>,
}

/// `wallsweep.set`.
pub(crate) fn set(txn: &mut Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = sweep(&store, &p.sweep_id)?;
    if let Some(v) = p.offset_from_level {
        r.offset = v;
    }
    if let Some(v) = p.distance_along {
        r.distance = v;
    }
    if let Some(v) = p.wall_offset {
        r.wall_offset = v;
    }
    if let Some(v) = p.side {
        r.side = v;
    }
    if let Some(v) = p.profile {
        if v.len() < 3 || crate::polygons::area(&v).abs() < 1e-9 {
            return Err(Error::new("wallsweep.profile", "the profile needs an area"));
        }
        r.profile = v;
    }
    if let Some(v) = p.cuts_wall {
        r.cuts_wall = v;
    }
    if let Some(v) = p.material {
        r.material = (!v.trim().is_empty()).then(|| v.trim().to_owned());
    }
    write(txn, id, &r)
}

/// Whether a sweep runs along a wall (GlobalId).
pub fn uses(r: &WallSweepRecipe, guid: &str) -> bool {
    r.walls.iter().any(|g| g == guid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_square_run_closes_with_mitres() {
        let segs = vec![
            ([0.0, 0.0], [0.0, 4.0]),
            ([4.0, 4.0], [4.0, 0.0]),
            ([0.0, 4.0], [4.0, 4.0]),
            ([4.0, 0.0], [0.0, 0.0]),
        ];
        let c = chains(segs);
        assert_eq!(c.len(), 1);
        assert!(c[0].1, "closed");
        assert_eq!(c[0].0.len(), 4);
        let m = sweep_mitred(
            &c[0].0,
            true,
            0.0,
            &[[0.0, 0.0], [0.1, 0.0], [0.1, 0.1], [0.0, 0.1]],
        );
        assert_eq!(m.0.len(), 16);
    }
}
