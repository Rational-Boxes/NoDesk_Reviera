// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Modify tools (spec Ch 3 §3.12, ARC-MOD-001 … 013): moving, copying, rotating and mirroring
//! elements, offsetting, splitting, trimming and extending walls, aligning, and pinning.
//!
//! What each element does:
//!
//! - **Reviera walls** transform their location line; their doors and windows go with them
//!   (copied with a copied wall). Mirroring swaps the exterior side, and turns the inserts to
//!   face the mirrored side.
//! - **Grids** transform their line; copies get the next name.
//! - **Doors and windows** without their wall only slide along it (and up or down); a copy
//!   stays in the same wall.
//!
//! A **pinned** element (ARC-MOD-013) refuses user moves, rotations, mirrors, flips and deletes
//! ("Element is pinned") but still follows implicit changes, such as its level moving.

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::store::Store;
use crate::txn::{Transaction, Warning};
use crate::walls::{self, Line, RECIPE_KEY};
use crate::{annotations, grids, inserts};

/// Element data section marking a pinned element.
pub const PIN_KEY: &str = "rxd:pinned";

pub fn is_pinned(e: &Element) -> bool {
    e.param(PIN_KEY).and_then(Value::as_bool).unwrap_or(false)
}

/// Refuses an edit of a pinned element.
pub fn check_unpinned(store: &Store, id: ElementId) -> Result<()> {
    if store.get(id).is_some_and(is_pinned) {
        return Err(Error::new(
            "element.pinned",
            format!("Element is pinned (element {id}); unpin it first"),
        ));
    }
    Ok(())
}

/// A plan transformation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Xform {
    /// By a vector (z moves doors and windows up or down).
    Translate([f64; 3]),
    /// About a point, counter-clockwise, radians.
    Rotate { center: [f64; 2], angle: f64 },
    /// In the vertical plane through a line.
    Mirror { a: [f64; 2], b: [f64; 2] },
}

impl Xform {
    pub fn point(&self, p: [f64; 2]) -> [f64; 2] {
        match *self {
            Xform::Translate(v) => [p[0] + v[0], p[1] + v[1]],
            Xform::Rotate { center: c, angle } => {
                let (s, co) = angle.sin_cos();
                let (x, y) = (p[0] - c[0], p[1] - c[1]);
                [c[0] + x * co - y * s, c[1] + x * s + y * co]
            }
            Xform::Mirror { a, b } => {
                let d = [b[0] - a[0], b[1] - a[1]];
                let l2 = (d[0] * d[0] + d[1] * d[1]).max(f64::EPSILON);
                let t = ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2;
                let foot = [a[0] + d[0] * t, a[1] + d[1] * t];
                [2.0 * foot[0] - p[0], 2.0 * foot[1] - p[1]]
            }
        }
    }

    pub fn line(&self, l: Line) -> Line {
        Line {
            start: self.point(l.start),
            end: self.point(l.end),
        }
    }

    /// Whether the transformation reverses orientation (mirror).
    pub fn reflects(&self) -> bool {
        matches!(self, Xform::Mirror { .. })
    }
}

fn resolve_all(store: &Store, refs: &[ElementRef]) -> Result<Vec<ElementId>> {
    refs.iter()
        .map(|r| {
            store
                .resolve(r)
                .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))
        })
        .collect()
}

fn set_recipe(txn: &mut Transaction<'_>, id: ElementId, recipe: &impl Serialize) -> Result<()> {
    let value = serde_json::to_value(recipe).expect("recipes serialise");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })
}

fn set_grid_curve(txn: &mut Transaction<'_>, id: ElementId, curve: Line) -> Result<()> {
    let mut d = grids::grid_data(txn.store().get(id).expect("exists")).expect("a grid");
    d.curve = curve;
    let value = serde_json::to_value(&d).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(grids::DATUM_KEY.into(), value);
    })
}

/// The parameters of an element to copy (its identity data stays with the original).
fn copied(e: &Element) -> NewElement {
    let mut n = NewElement::new(e.class.clone());
    n.name = e.name.clone();
    n.params = e
        .params
        .iter()
        .filter(|(k, _)| k.as_str() != "rxd:identity" && k.as_str() != PIN_KEY)
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    n
}

/// Transforms elements, or copies of them. Returns the moved elements, or the copies.
pub fn transform(
    txn: &mut Transaction<'_>,
    refs: &[ElementRef],
    x: Xform,
    copy: bool,
) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let ids = resolve_all(&store, refs)?;
    if !copy {
        for id in &ids {
            check_unpinned(&store, *id)?;
        }
    }
    let selected: BTreeSet<ElementId> = ids.iter().copied().collect();
    let mut out = Vec::new();
    let host_moves = |guid: &str| {
        crate::id::GlobalId::parse(guid)
            .and_then(|g| store.find_by_guid(&g))
            .is_some_and(|h| selected.contains(&h))
    };
    for id in ids {
        let store = txn.store().clone();
        let e = store.get(id).expect("resolved");
        // Tags and text notes; a tag whose host moves too follows it.
        if let Some(moved) = crate::notes::transform(txn, e, &x, copy, &host_moves)? {
            out.extend(moved);
            continue;
        }
        if let Some(mut r) = walls::wall_recipe(e) {
            r.curve = x.line(r.curve);
            r.through = r.through.iter().map(|p| x.point(*p)).collect();
            r.flip ^= x.reflects();
            let hosted = inserts::hosted_by(&store, e.guid().as_str());
            if copy {
                let mut n = copied(e);
                n.params.insert(
                    RECIPE_KEY.into(),
                    serde_json::to_value(&r).expect("serialises"),
                );
                let new = txn.create(n)?;
                let guid = txn.store().get(new).expect("created").guid().to_string();
                for i in hosted {
                    let along = inserts::insert_recipe(store.get(i).expect("hosted"))
                        .expect("an insert")
                        .along;
                    inserts::copy_to(txn, i, &guid, along, x.reflects())?;
                }
                for o in crate::wall_openings::of_host(&store, e.guid().as_str()) {
                    crate::wall_openings::copy_to(txn, o, &guid, [0.0; 3])?;
                }
                out.push(new);
            } else {
                set_recipe(txn, id, &r)?;
                if x.reflects() {
                    inserts::toggle_facing(txn, &hosted)?;
                }
                out.push(id);
            }
        } else if let Some(g) = grids::grid_data(e) {
            let curve = x.line(g.curve);
            if copy {
                let mut n = copied(e);
                let guid = crate::id::GlobalId::new();
                n.class = format!("{}{}", grids::CLASS_PREFIX, guid.as_str());
                n.guid = Some(guid);
                n.name = Some(grids::auto_name(txn.store()));
                let mut d = g;
                d.curve = curve;
                n.params.insert(
                    grids::DATUM_KEY.into(),
                    serde_json::to_value(&d).expect("serialises"),
                );
                out.push(txn.create(n)?);
            } else {
                set_grid_curve(txn, id, curve)?;
                out.push(id);
            }
        } else if inserts::kind_of(e).is_some() {
            let r = inserts::insert_recipe(e).expect("an insert");
            // With its wall in the selection, the wall carries (or copies) it.
            let host = crate::id::GlobalId::parse(&r.host).and_then(|g| store.find_by_guid(&g));
            if host.is_some_and(|h| selected.contains(&h)) {
                continue;
            }
            let Xform::Translate(v) = x else {
                return Err(Error::new(
                    "insert.cannot_rotate",
                    "doors and windows only slide along their wall; select the wall too",
                ));
            };
            if copy {
                let p = inserts::resolve(&store, e)?;
                let u = p.wall.direction();
                let along = r.along + v[0] * u[0] + v[1] * u[1];
                let new = inserts::copy_to(txn, id, &r.host, along, false)?;
                if v[2] != 0.0 {
                    inserts::move_by(txn, new, [0.0, 0.0, v[2]])?;
                }
                let placed = inserts::resolve(txn.store(), txn.store().get(new).expect("created"))?;
                inserts::check_fit(txn.store(), &placed)?;
                out.push(new);
            } else {
                inserts::move_by(txn, id, v)?;
                out.push(id);
            }
        } else if let Some(r) = crate::wall_openings::wall_opening_recipe(e) {
            // With its wall in the selection, the wall carries (or copies) it.
            let host = crate::id::GlobalId::parse(&r.host).and_then(|g| store.find_by_guid(&g));
            if host.is_some_and(|h| selected.contains(&h)) {
                continue;
            }
            let Xform::Translate(v) = x else {
                return Err(Error::new(
                    "opening.cannot_rotate",
                    "wall openings only slide along their wall; select the wall too",
                ));
            };
            if copy {
                out.push(crate::wall_openings::copy_to(txn, id, &r.host, v)?);
            } else {
                crate::wall_openings::move_by(txn, id, v)?;
                out.push(id);
            }
        } else if crate::ref_planes::recipe(e).is_some() {
            let target = if copy { txn.create(copied(e))? } else { id };
            if copy {
                // Names are unique: a copy is unnamed.
                txn.modify(target, |n| n.name = None)?;
            }
            let dz = match x {
                Xform::Translate(v) => v[2],
                _ => 0.0,
            };
            crate::ref_planes::transform(txn, target, &|p| x.point(p), dz)?;
            out.push(target);
        } else if let Some(r) = crate::model_lines::recipe(e) {
            // On a reference plane or grid that moves too, it follows it.
            if !copy
                && crate::work_planes::host_of(&r.plane).is_some_and(|h| {
                    !matches!(r.plane, crate::work_planes::WorkPlaneRef::Level { .. })
                        && host_moves(h)
                })
            {
                continue;
            }
            let target = if copy { txn.create(copied(e))? } else { id };
            let dz = match x {
                Xform::Translate(v) => v[2],
                _ => 0.0,
            };
            crate::model_lines::transform(txn, target, &|p| x.point(p), dz)?;
            out.push(target);
        } else if let Some(s) = crate::skylights::skylight_recipe(e) {
            // With its roof in the selection, the roof carries it.
            let roof = crate::id::GlobalId::parse(&s.roof).and_then(|g| store.find_by_guid(&g));
            if roof.is_some_and(|r| selected.contains(&r)) {
                continue;
            }
            let target = if copy { txn.create(copied(e))? } else { id };
            let turn = match x {
                Xform::Rotate { angle, .. } => angle,
                _ => 0.0,
            };
            crate::skylights::transform(txn, target, &|p| x.point(p), turn)?;
            out.push(target);
        } else if crate::ramps::ramp_recipe(e).is_some() {
            let target = if copy { txn.create(copied(e))? } else { id };
            let dz = match x {
                Xform::Translate(v) => v[2],
                _ => 0.0,
            };
            crate::ramps::transform(txn, target, &|p| x.point(p), dz)?;
            out.push(target);
        } else if crate::curtain::curtain_recipe(e).is_some() {
            let target = if copy { txn.create(copied(e))? } else { id };
            let dz = match x {
                Xform::Translate(v) => v[2],
                _ => 0.0,
            };
            crate::curtain::transform(txn, target, &|p| x.point(p), x.reflects(), dz)?;
            out.push(target);
        } else if crate::path_arrays::path_array_recipe(e).is_some() {
            let target = if copy { txn.create(copied(e))? } else { id };
            let (turn, mirror, dz) = match x {
                Xform::Translate(v) => (0.0, None, v[2]),
                Xform::Rotate { angle, .. } => (angle, None, 0.0),
                Xform::Mirror { a, b } => (0.0, Some((b[1] - a[1]).atan2(b[0] - a[0])), 0.0),
            };
            crate::path_arrays::transform(txn, target, &|p| x.point(p), turn, mirror, dz)?;
            out.push(target);
        } else if crate::mesh_objects::mesh_object_recipe(e).is_some() {
            let target = if copy { txn.create(copied(e))? } else { id };
            let (turn, mirror, dz) = match x {
                Xform::Translate(v) => (0.0, None, v[2]),
                Xform::Rotate { angle, .. } => (angle, None, 0.0),
                Xform::Mirror { a, b } => (0.0, Some((b[1] - a[1]).atan2(b[0] - a[0])), 0.0),
            };
            crate::mesh_objects::transform(txn, target, &|p| x.point(p), turn, mirror, dz)?;
            out.push(target);
        } else if crate::columns::column_recipe(e).is_some() {
            let target = if copy { txn.create(copied(e))? } else { id };
            let (turn, mirror, dz) = match x {
                Xform::Translate(v) => (0.0, None, v[2]),
                Xform::Rotate { angle, .. } => (angle, None, 0.0),
                Xform::Mirror { a, b } => (0.0, Some((b[1] - a[1]).atan2(b[0] - a[0])), 0.0),
            };
            crate::columns::transform(txn, target, &|p| x.point(p), turn, mirror, dz)?;
            out.push(target);
        } else if crate::slab_openings::shaft_recipe(e).is_some() {
            let target = if copy { txn.create(copied(e))? } else { id };
            let dz = match x {
                Xform::Translate(v) => v[2],
                _ => 0.0,
            };
            crate::slab_openings::transform_shaft(txn, target, &|p| x.point(p), dz)?;
            out.push(target);
        } else if crate::rooms::room_recipe(e).is_some() {
            let target = if copy {
                let mut n = copied(e);
                n.name = Some(crate::rooms::next_number(txn.store()));
                txn.create(n)?
            } else {
                id
            };
            crate::rooms::translate(txn, target, &|p| x.point(p))?;
            out.push(target);
        } else if crate::railings::railing_recipe(e).is_some() {
            let target = if copy { txn.create(copied(e))? } else { id };
            crate::railings::transform(txn, target, &|p| x.point(p))?;
            out.push(target);
        } else if crate::stairs::stair_recipe(e).is_some() {
            let target = if copy { txn.create(copied(e))? } else { id };
            crate::stairs::transform(txn, target, &|p| x.point(p))?;
            out.push(target);
        } else if crate::floors::slab_recipe(e).is_some() || crate::roofs::roof_recipe(e).is_some()
        {
            let target = if copy { txn.create(copied(e))? } else { id };
            crate::floors::translate(txn, target, &|p| x.point(p), x.reflects())?;
            // Its vertical openings and skylights go with it (a copy is made whole).
            if !copy {
                crate::slab_openings::follow_host(txn, e.guid().as_str(), &|p| x.point(p))?;
                crate::skylights::follow_roof(txn, e.guid().as_str(), &|p| x.point(p))?;
            }
            out.push(target);
        } else {
            return Err(Error::new(
                "element.not_movable",
                format!("element {id} ({}) cannot be moved yet", e.class),
            ));
        }
    }
    Ok(out)
}

/// Deletes the doors and windows of `wall` that no longer fit it (after a trim or split), with a
/// warning naming them.
fn drop_misfits(txn: &mut Transaction<'_>, wall: ElementId) -> Result<()> {
    let store = txn.store().clone();
    let guid = store.get(wall).expect("exists").guid().to_string();
    let mut gone = Vec::new();
    for i in inserts::hosted_by(&store, &guid) {
        let Ok(p) = inserts::resolve(&store, store.get(i).expect("hosted")) else {
            continue;
        };
        if inserts::check_fit(&store, &p).is_err() {
            gone.extend(crate::builtin::delete_closure(&store, &[i]));
        }
    }
    if gone.is_empty() {
        return Ok(());
    }
    for id in &gone {
        if txn.store().contains(*id) {
            txn.delete(*id)?;
        }
    }
    txn.warn(Warning {
        id: "arc.insert.outside_host".into(),
        message: "Doors or windows outside the changed wall were deleted".into(),
        element_ids: gone,
    });
    Ok(())
}

fn wall_of(store: &Store, r: &ElementRef) -> Result<(ElementId, walls::WallRecipe)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such wall"))?;
    let recipe = walls::wall_recipe(store.get(id).expect("resolved")).ok_or_else(|| {
        Error::new(
            "element.not_trimmable",
            format!("element {id} is not a Reviera wall"),
        )
    })?;
    Ok((id, recipe))
}

/// The intersection of two infinite lines, if they are not parallel.
fn intersect(a: Line, b: Line) -> Option<[f64; 2]> {
    let d1 = [a.end[0] - a.start[0], a.end[1] - a.start[1]];
    let d2 = [b.end[0] - b.start[0], b.end[1] - b.start[1]];
    let den = d1[0] * d2[1] - d1[1] * d2[0];
    let scale = (d1[0].hypot(d1[1]) * d2[0].hypot(d2[1])).max(f64::EPSILON);
    if den.abs() / scale < 1e-9 {
        return None;
    }
    let w = [b.start[0] - a.start[0], b.start[1] - a.start[1]];
    let t = (w[0] * d2[1] - w[1] * d2[0]) / den;
    Some([a.start[0] + d1[0] * t, a.start[1] + d1[1] * t])
}

/// Position of `p` along `l` (0 at the start, 1 at the end).
fn param(l: Line, p: [f64; 2]) -> f64 {
    let d = [l.end[0] - l.start[0], l.end[1] - l.start[1]];
    let l2 = (d[0] * d[0] + d[1] * d[1]).max(f64::EPSILON);
    ((p[0] - l.start[0]) * d[0] + (p[1] - l.start[1]) * d[1]) / l2
}

/// Trims or extends `l` to `x`, keeping the part on `keep`'s side of `x`.
fn trimmed(l: Line, x: [f64; 2], keep: [f64; 2]) -> Line {
    if param(l, keep) < param(l, x) {
        Line {
            start: l.start,
            end: x,
        }
    } else {
        Line {
            start: x,
            end: l.end,
        }
    }
}

/// Changes a wall's location line (with its pin, length and fit checks).
fn reshape_wall(txn: &mut Transaction<'_>, id: ElementId, curve: Line) -> Result<()> {
    check_unpinned(txn.store(), id)?;
    if curve.length() < walls::TOL_SHORT {
        return Err(Error::new("wall.too_short", "Wall is too short"));
    }
    let e = txn.store().get(id).expect("exists");
    let guid = e.guid().to_string();
    let mut r = walls::wall_recipe(e).expect("a wall");
    let old = r.curve;
    r.curve = curve;
    set_recipe(txn, id, &r)?;
    inserts::keep_in_place(txn, &guid, old, curve)?;
    drop_misfits(txn, id)
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveParams {
    pub element_ids: Vec<ElementRef>,
    /// Metres (x, y, and z for doors and windows).
    pub vector: [f64; 3],
}

pub(crate) fn move_(txn: &mut Transaction<'_>, p: MoveParams) -> Result<Vec<ElementId>> {
    transform(txn, &p.element_ids, Xform::Translate(p.vector), false)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CopyParams {
    pub element_ids: Vec<ElementRef>,
    /// One vector per copy (metres), each from the originals.
    pub vectors: Vec<[f64; 3]>,
}

pub(crate) fn copy(txn: &mut Transaction<'_>, p: CopyParams) -> Result<Vec<Vec<ElementId>>> {
    if p.vectors.is_empty() {
        return Err(Error::new("copy.no_vector", "give at least one vector"));
    }
    p.vectors
        .iter()
        .map(|v| transform(txn, &p.element_ids, Xform::Translate(*v), true))
        .collect()
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RotateParams {
    pub element_ids: Vec<ElementRef>,
    /// Centre of rotation in plan (metres).
    pub center: [f64; 2],
    /// Counter-clockwise; accepts units ("30°").
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub angle: f64,
    #[serde(default)]
    pub copy: bool,
}

pub(crate) fn rotate(txn: &mut Transaction<'_>, p: RotateParams) -> Result<Vec<ElementId>> {
    let x = Xform::Rotate {
        center: p.center,
        angle: p.angle,
    };
    transform(txn, &p.element_ids, x, p.copy)
}

/// A mirror axis: two plan points, or an element's line (a wall's location line, a grid).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Axis {
    Line(Line),
    Element { element: ElementRef },
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MirrorParams {
    pub element_ids: Vec<ElementRef>,
    pub axis: Axis,
    /// Mirror copies, keeping the originals (default on).
    #[serde(default = "yes")]
    pub copy: bool,
}

fn yes() -> bool {
    true
}

/// The line of an element used as an axis or boundary: a wall's location line or a grid.
fn element_line(store: &Store, r: &ElementRef) -> Result<Line> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such element"))?;
    let e = store.get(id).expect("resolved");
    if let Some(w) = walls::wall_recipe(e) {
        return Ok(w.curve);
    }
    if let Some(g) = grids::grid_data(e) {
        return Ok(g.curve);
    }
    Err(Error::new(
        "element.no_line",
        format!("element {id} has no line to use (walls and grids do)"),
    ))
}

pub(crate) fn mirror(txn: &mut Transaction<'_>, p: MirrorParams) -> Result<Vec<ElementId>> {
    let axis = match &p.axis {
        Axis::Line(l) => *l,
        Axis::Element { element } => element_line(txn.store(), element)?,
    };
    if axis.length() < walls::TOL_SHORT {
        return Err(Error::new("mirror.bad_axis", "the axis is too short"));
    }
    let x = Xform::Mirror {
        a: axis.start,
        b: axis.end,
    };
    transform(txn, &p.element_ids, x, p.copy)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OffsetParams {
    pub element_ids: Vec<ElementRef>,
    /// Metres (accepts units).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub distance: f64,
    /// A plan point on the side to offset toward.
    pub side_point: [f64; 2],
    /// Offset copies, keeping the originals (default on).
    #[serde(default = "yes")]
    pub copy: bool,
}

/// Offsets walls and grids by `distance` toward `side_point`, each along its own normal.
pub(crate) fn offset(txn: &mut Transaction<'_>, p: OffsetParams) -> Result<Vec<ElementId>> {
    if p.distance <= 0.0 {
        return Err(Error::new(
            "offset.bad_distance",
            "the distance must be greater than 0",
        ));
    }
    let mut out = Vec::new();
    for r in &p.element_ids {
        let line = element_line(txn.store(), r)?;
        let d = [line.end[0] - line.start[0], line.end[1] - line.start[1]];
        let len = d[0].hypot(d[1]);
        let n = [-d[1] / len, d[0] / len];
        let side =
            (p.side_point[0] - line.start[0]) * n[0] + (p.side_point[1] - line.start[1]) * n[1];
        let s = if side >= 0.0 { 1.0 } else { -1.0 };
        let v = [n[0] * s * p.distance, n[1] * s * p.distance, 0.0];
        out.extend(transform(
            txn,
            std::slice::from_ref(r),
            Xform::Translate(v),
            p.copy,
        )?);
    }
    Ok(out)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SplitParams {
    pub element_id: ElementRef,
    /// A plan point on the wall (projected onto it).
    pub point: [f64; 2],
}

/// Splits a wall in two at a point: the original keeps the start, a new wall of the same type
/// and parameters takes the rest, with the doors and windows beyond the point.
pub(crate) fn split(txn: &mut Transaction<'_>, p: SplitParams) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let (id, recipe) = wall_of(&store, &p.element_id)?;
    check_unpinned(&store, id)?;
    let l = recipe.curve;
    let t = param(l, p.point).clamp(0.0, 1.0);
    let length = l.length();
    if t * length < walls::TOL_SHORT || (1.0 - t) * length < walls::TOL_SHORT {
        return Err(Error::new("element.too_short", "Element too short"));
    }
    let at = [
        l.start[0] + (l.end[0] - l.start[0]) * t,
        l.start[1] + (l.end[1] - l.start[1]) * t,
    ];
    let cut = t * length;
    let e = store.get(id).expect("resolved");
    let hosted = inserts::hosted_by(&store, e.guid().as_str());
    for i in &hosted {
        let p = inserts::resolve(&store, store.get(*i).expect("hosted"))?;
        let c = p.cut();
        if c.x[0] < cut && cut < c.x[1] {
            return Err(Error::new(
                "split.through_insert",
                format!("the split point is inside door or window {i}"),
            ));
        }
    }
    let mut first = recipe.clone();
    first.curve = Line {
        start: l.start,
        end: at,
    };
    let mut second = recipe;
    second.curve = Line {
        start: at,
        end: l.end,
    };
    set_recipe(txn, id, &first)?;
    let mut n = copied(e);
    n.params.insert(
        RECIPE_KEY.into(),
        serde_json::to_value(&second).expect("serialises"),
    );
    let new = txn.create(n)?;
    let new_guid = txn.store().get(new).expect("created").guid().to_string();
    // Inserts beyond the point move to the new wall, measured from its start.
    for i in hosted {
        let mut r = inserts::insert_recipe(txn.store().get(i).expect("hosted")).expect("an insert");
        if r.along > cut {
            r.host = new_guid.clone();
            r.along -= cut;
            set_recipe(txn, i, &r)?;
        }
    }
    Ok(vec![id, new])
}

/// An element and the point picked on the part to keep.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Pick {
    pub element_id: ElementRef,
    pub keep_point: [f64; 2],
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrimCornerParams {
    pub a: Pick,
    pub b: Pick,
}

/// Trims or extends two walls to their intersection, keeping the picked parts (an L corner).
pub(crate) fn trim_corner(txn: &mut Transaction<'_>, p: TrimCornerParams) -> Result<()> {
    let store = txn.store().clone();
    let (a, ra) = wall_of(&store, &p.a.element_id)?;
    let (b, rb) = wall_of(&store, &p.b.element_id)?;
    if a == b {
        return Err(Error::new("trim.same_element", "pick two different walls"));
    }
    let x = intersect(ra.curve, rb.curve)
        .ok_or_else(|| Error::new("trim.no_intersection", "Elements do not intersect"))?;
    reshape_wall(txn, a, trimmed(ra.curve, x, p.a.keep_point))?;
    reshape_wall(txn, b, trimmed(rb.curve, x, p.b.keep_point))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrimExtendParams {
    /// The boundary: a wall (its location line) or a grid.
    pub boundary: ElementRef,
    pub target: Pick,
}

/// Trims or extends a wall to a boundary line, keeping the picked part.
pub(crate) fn trim_extend(txn: &mut Transaction<'_>, p: TrimExtendParams) -> Result<()> {
    let store = txn.store().clone();
    let boundary = element_line(&store, &p.boundary)?;
    let (id, r) = wall_of(&store, &p.target.element_id)?;
    let x = intersect(r.curve, boundary).ok_or_else(|| {
        Error::new(
            "trim.no_intersection",
            "Element cannot be trimmed or extended to the boundary",
        )
    })?;
    reshape_wall(txn, id, trimmed(r.curve, x, p.target.keep_point))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AlignParams {
    /// The reference line: a dimension reference (`face:ext`, `face:int`, `axis` of a wall,
    /// `axis` of a grid, or a `line`).
    pub reference: annotations::DimRefArg,
    /// The lines to bring onto it; their elements move.
    pub targets: Vec<annotations::DimRefArg>,
}

/// Moves elements so that each target line lies on the reference line (they must be parallel).
pub(crate) fn align(txn: &mut Transaction<'_>, p: AlignParams) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let line_of = |a: &annotations::DimRefArg| -> Result<(Option<ElementId>, [f64; 2], [f64; 2])> {
        let r = annotations::ref_of(&store, a)?;
        let (point, dir) = annotations::resolve_ref(&store, &r)
            .ok_or_else(|| Error::new("align.bad_ref", "the reference no longer exists"))?;
        let id = match &a.element {
            Some(e) => store.resolve(e),
            None => None,
        };
        Ok((id, point, dir))
    };
    let (_, rp, rd) = line_of(&p.reference)?;
    let mut moved = Vec::new();
    for t in &p.targets {
        let (id, tp, td) = line_of(t)?;
        let id =
            id.ok_or_else(|| Error::new("align.no_element", "the target has no element to move"))?;
        if (rd[0] * td[1] - rd[1] * td[0]).abs() > 1e-3 {
            return Err(Error::new(
                "align.not_parallel",
                "the target is not parallel to the reference",
            ));
        }
        // The perpendicular from the target line to the reference line.
        let n = [-rd[1], rd[0]];
        let gap = (rp[0] - tp[0]) * n[0] + (rp[1] - tp[1]) * n[1];
        if gap.abs() < 1e-9 {
            continue;
        }
        let v = [n[0] * gap, n[1] * gap, 0.0];
        moved.extend(transform(
            txn,
            &[ElementRef::Id(id)],
            Xform::Translate(v),
            false,
        )?);
    }
    Ok(moved)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PinParams {
    pub element_ids: Vec<ElementRef>,
}

/// Pins or unpins elements; elements already in that state are ignored.
pub(crate) fn pin(txn: &mut Transaction<'_>, p: PinParams, pinned: bool) -> Result<()> {
    let ids = resolve_all(txn.store(), &p.element_ids)?;
    for id in ids {
        if txn.store().get(id).is_some_and(is_pinned) == pinned {
            continue;
        }
        txn.modify(id, |e| {
            if pinned {
                e.params.insert(PIN_KEY.into(), Value::Bool(true));
            } else {
                e.params.remove(PIN_KEY);
            }
        })?;
    }
    Ok(())
}

// ---- Arrays (ARC-MOD-026) ----

/// Where an array's second pick goes: the second member, or the last.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MoveTo {
    #[default]
    Second,
    Last,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArrayLinearParams {
    pub element_ids: Vec<ElementRef>,
    /// From the original to the second member (`second`) or the last (`last`), metres.
    pub vector: [f64; 3],
    /// Members, the original included (2 or more).
    #[schemars(range(min = 2, max = 1000))]
    pub count: u32,
    #[serde(default)]
    pub move_to: MoveTo,
    /// Members as group instances of an array set (not available yet).
    #[serde(default)]
    pub group_and_associate: bool,
}

fn check_array(count: u32, group: bool) -> Result<()> {
    if !(2..=1000).contains(&count) {
        return Err(Error::new(
            "array.bad_count",
            "an array has between 2 and 1000 members",
        ));
    }
    if group {
        return Err(Error::new(
            "array.group_not_supported",
            "Group and Associate needs groups, which are not available yet; the members are made \
             as independent copies without it",
        ));
    }
    Ok(())
}

/// Copies elements in a row (`element.array_linear`): one copy per member after the original,
/// each the step on from the one before. Returns the copies, member by member.
pub(crate) fn array_linear(
    txn: &mut Transaction<'_>,
    p: ArrayLinearParams,
) -> Result<Vec<Vec<ElementId>>> {
    check_array(p.count, p.group_and_associate)?;
    let n = f64::from(p.count - 1);
    let step = match p.move_to {
        MoveTo::Second => p.vector,
        MoveTo::Last => p.vector.map(|c| c / n),
    };
    (1..p.count)
        .map(|k| {
            let k = f64::from(k);
            let v = step.map(|c| c * k);
            transform(txn, &p.element_ids, Xform::Translate(v), true)
        })
        .collect()
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArrayRadialParams {
    pub element_ids: Vec<ElementRef>,
    /// The centre in plan (metres).
    pub center: [f64; 2],
    /// To the second member (`second`) or the last (`last`), counter-clockwise; accepts units
    /// ("45°"). A full turn with `last` spaces the members evenly round it.
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub angle: f64,
    #[schemars(range(min = 2, max = 1000))]
    pub count: u32,
    #[serde(default)]
    pub move_to: MoveTo,
    #[serde(default)]
    pub group_and_associate: bool,
}

/// Copies elements round a centre (`element.array_radial`). Returns the copies, member by
/// member.
pub(crate) fn array_radial(
    txn: &mut Transaction<'_>,
    p: ArrayRadialParams,
) -> Result<Vec<Vec<ElementId>>> {
    check_array(p.count, p.group_and_associate)?;
    let full = (p.angle.abs() - std::f64::consts::TAU).abs() < 1e-6;
    let step = match p.move_to {
        MoveTo::Second => p.angle,
        // A full turn: the last member would land on the original.
        MoveTo::Last if full => p.angle / f64::from(p.count),
        MoveTo::Last => p.angle / f64::from(p.count - 1),
    };
    (1..p.count)
        .map(|k| {
            let x = Xform::Rotate {
                center: p.center,
                angle: step * f64::from(k),
            };
            transform(txn, &p.element_ids, x, true)
        })
        .collect()
}
