// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Temporary dimensions (spec Ch 3 §3.1.6, ARC-DIM-001): the dimensions shown from a selected
//! element to the nearest parallel references on each side, which can be typed to move it, or
//! made permanent.
//!
//! - A straight wall, grid or vertical reference plane measures from its line (a wall's location
//!   line) to the nearest parallel wall location line, grid or reference plane on each side,
//!   within 30 m.
//! - A door or window measures from its centre along its host wall to the nearest walls crossing
//!   the host on each side.
//!
//! Typing a value moves the selected element (`dim.temporary_set`); the other reference stays.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::store::Store;
use crate::txn::Transaction;

/// How far references are searched for (metres).
const REACH: f64 = 30.0;

/// A temporary dimension.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TempDim {
    /// Where it starts: on the selected element (plan, metres).
    pub from: [f64; 2],
    /// Where it ends: on the other reference.
    pub to: [f64; 2],
    pub value: f64,
    /// The other reference's element, if any.
    pub other: Option<ElementId>,
    /// What the other reference is ("wall", "grid", "reference plane").
    pub other_kind: String,
    /// The direction the selected element moves to make the dimension longer (unit).
    pub grow: [f64; 2],
}

fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

/// A reference line: a point, a unit direction, its element and kind.
struct RefLine {
    point: [f64; 2],
    dir: [f64; 2],
    id: ElementId,
    kind: &'static str,
}

/// Every straight reference line of the model: wall location lines (of walls on `level`, a
/// GlobalId, when given), grids, vertical reference planes.
fn reference_lines(store: &Store, level: Option<&str>) -> Vec<RefLine> {
    let mut out = Vec::new();
    let unit = |a: [f64; 2], b: [f64; 2]| {
        let l = (b[0] - a[0]).hypot(b[1] - a[1]);
        (l > 1e-9).then(|| [(b[0] - a[0]) / l, (b[1] - a[1]) / l])
    };
    for e in store.iter() {
        if let Some(r) = crate::walls::wall_recipe(e) {
            if !r.through.is_empty() || level.is_some_and(|l| l != r.base_level) {
                continue;
            }
            if let Some(d) = unit(r.curve.start, r.curve.end) {
                out.push(RefLine {
                    point: r.curve.start,
                    dir: d,
                    id: e.id(),
                    kind: "wall",
                });
            }
        } else if let Some(g) = crate::grids::grid_data(e) {
            if let Some(d) = unit(g.curve.start, g.curve.end) {
                out.push(RefLine {
                    point: g.curve.start,
                    dir: d,
                    id: e.id(),
                    kind: "grid",
                });
            }
        } else if let Some(rp) = crate::ref_planes::recipe(e) {
            let (a, b) = ([rp.start[0], rp.start[1]], [rp.end[0], rp.end[1]]);
            if crate::ref_planes::plan_trace(&rp).is_some()
                && let Some(d) = unit(a, b)
            {
                out.push(RefLine {
                    point: a,
                    dir: d,
                    id: e.id(),
                    kind: "reference plane",
                });
            }
        }
    }
    out
}

/// The temporary dimensions of an element: to walls on its own level (a wall's), or on `level`
/// (a GlobalId: the plan's) for other elements.
pub fn of_element(store: &Store, id: ElementId, level: Option<&str>) -> Vec<TempDim> {
    let Some(e) = store.get(id) else {
        return Vec::new();
    };
    if crate::inserts::kind_of(e).is_some() {
        return of_insert(store, id);
    }
    let own = crate::walls::wall_recipe(e).map(|r| r.base_level);
    let mut lines = reference_lines(store, own.as_deref().or(level));
    // The element itself, whatever its level.
    if !lines.iter().any(|l| l.id == id) {
        lines.extend(
            reference_lines(store, None)
                .into_iter()
                .filter(|l| l.id == id),
        );
    }
    let Some(me) = lines.iter().find(|l| l.id == id) else {
        return Vec::new();
    };
    // The middle of its line, where the dimensions are drawn.
    let mid = match crate::datum_extents::model_line(e)
        .or_else(|| crate::walls::wall_recipe(e).map(|r| r.curve))
    {
        Some(l) => [(l.start[0] + l.end[0]) / 2.0, (l.start[1] + l.end[1]) / 2.0],
        None => me.point,
    };
    let n = [-me.dir[1], me.dir[0]];
    let mut best: [Option<(f64, &RefLine)>; 2] = [None, None];
    for l in &lines {
        if l.id == id || cross(l.dir, me.dir).abs() > 1e-3 {
            continue;
        }
        let s = dot([l.point[0] - mid[0], l.point[1] - mid[1]], n);
        if s.abs() < 1e-6 || s.abs() > REACH {
            continue;
        }
        let k = usize::from(s < 0.0);
        if best[k].is_none_or(|(b, _)| s.abs() < b.abs()) {
            best[k] = Some((s, l));
        }
    }
    best.into_iter()
        .flatten()
        .map(|(s, l)| TempDim {
            from: mid,
            to: [mid[0] + n[0] * s, mid[1] + n[1] * s],
            value: s.abs(),
            other: Some(l.id),
            other_kind: l.kind.to_owned(),
            // Away from the other reference.
            grow: [-n[0] * s.signum(), -n[1] * s.signum()],
        })
        .collect()
}

/// A door's or window's: along its host to the nearest crossing walls.
fn of_insert(store: &Store, id: ElementId) -> Vec<TempDim> {
    let Some(e) = store.get(id) else {
        return Vec::new();
    };
    let Ok(p) = crate::inserts::resolve(store, e) else {
        return Vec::new();
    };
    if p.path.is_some() {
        return Vec::new();
    }
    let level = crate::walls::wall_recipe(store.get(p.host).expect("host")).map(|r| r.base_level);
    let (o, u) = (p.wall.start, p.wall.direction());
    let along = p.recipe.along;
    let here = [o[0] + u[0] * along, o[1] + u[1] * along];
    let mut best: [Option<(f64, ElementId)>; 2] = [None, None];
    for l in reference_lines(store, level.as_deref()) {
        if l.id == p.host || l.kind != "wall" || cross(l.dir, u).abs() < 1e-3 {
            continue;
        }
        // Where the other wall's line crosses the host's: o + u t = q + d s.
        let w = [l.point[0] - o[0], l.point[1] - o[1]];
        let t = cross(w, l.dir) / cross(u, l.dir);
        let s = t - along;
        if s.abs() < 1e-6 || s.abs() > REACH {
            continue;
        }
        let k = usize::from(s < 0.0);
        if best[k].is_none_or(|(b, _)| s.abs() < b.abs()) {
            best[k] = Some((s, l.id));
        }
    }
    best.into_iter()
        .flatten()
        .map(|(s, other)| TempDim {
            from: here,
            to: [here[0] + u[0] * s, here[1] + u[1] * s],
            value: s.abs(),
            other: Some(other),
            other_kind: "wall".into(),
            grow: [-u[0] * s.signum(), -u[1] * s.signum()],
        })
        .collect()
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryParams {
    pub element_id: ElementRef,
    /// A plan whose level's walls a grid or reference plane measures to.
    pub view_id: Option<ElementRef>,
}

/// The level (GlobalId) of a plan view.
fn view_level(store: &Store, view: Option<&ElementRef>) -> Option<String> {
    let e = store.get(store.resolve(view?)?)?;
    crate::views::view_data(e)?.level
}

fn resolve(store: &Store, r: &ElementRef) -> Result<ElementId> {
    store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))
}

/// `dim.temporary_query`.
pub(crate) fn query(store: &Store, p: QueryParams) -> Result<Vec<TempDim>> {
    let level = view_level(store, p.view_id.as_ref());
    Ok(of_element(
        store,
        resolve(store, &p.element_id)?,
        level.as_deref(),
    ))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub element_id: ElementRef,
    /// The plan it is shown in (as for `dim.temporary_query`).
    pub view_id: Option<ElementRef>,
    /// Which temporary dimension (as `dim.temporary_query` lists them).
    pub index: usize,
    /// The new value (metres).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub value: f64,
}

/// `dim.temporary_set`: moves the element so a temporary dimension has a new value.
pub(crate) fn set(txn: &mut Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let id = resolve(&store, &p.element_id)?;
    let level = view_level(&store, p.view_id.as_ref());
    let dims = of_element(&store, id, level.as_deref());
    let d = dims.get(p.index).ok_or_else(|| {
        Error::new(
            "dim.no_temporary",
            format!("the element has {} temporary dimensions", dims.len()),
        )
    })?;
    if p.value <= 0.0 {
        return Err(Error::new("dim.value", "Value must be greater than 0"));
    }
    let delta = p.value - d.value;
    crate::modify::transform(
        txn,
        &[ElementRef::Id(id)],
        crate::modify::Xform::Translate([d.grow[0] * delta, d.grow[1] * delta, 0.0]),
        false,
    )?;
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PermanentParams {
    pub view_id: ElementRef,
    pub element_id: ElementRef,
    pub index: usize,
}

/// `dim.make_permanent`: a permanent aligned dimension in place of a temporary one.
pub(crate) fn make_permanent(txn: &mut Transaction<'_>, p: PermanentParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let id = resolve(&store, &p.element_id)?;
    let level = view_level(&store, Some(&p.view_id));
    let dims = of_element(&store, id, level.as_deref());
    let d = dims
        .get(p.index)
        .ok_or_else(|| Error::new("dim.no_temporary", "no such temporary dimension"))?;
    let dir = [d.to[0] - d.from[0], d.to[1] - d.from[1]];
    let len = dir[0].hypot(dir[1]).max(1e-12);
    // The references run square to the dimension.
    let along = [-dir[1] / len, dir[0] / len];
    let reference = |element: Option<ElementId>, point: [f64; 2]| {
        let e = element.and_then(|i| store.get(i));
        let referable = e.is_some_and(|e| {
            crate::walls::wall_recipe(e).is_some()
                || crate::grids::is_grid(e)
                || crate::ref_planes::recipe(e).is_some()
        });
        if referable {
            json!({ "element": element, "sub": "axis", "point": point })
        } else {
            json!({ "sub": "line", "point": point, "direction": along })
        }
    };
    let me = crate::inserts::kind_of(store.get(id).expect("resolved")).is_none();
    let args: crate::annotations::CreateAlignedParams = serde_json::from_value(json!({
        "view_id": p.view_id,
        "refs": [reference(me.then_some(id), d.from), reference(d.other, d.to)],
        "line_point": [d.from[0] + along[0] * 0.5, d.from[1] + along[1] * 0.5],
    }))
    .map_err(|e| Error::new("dim.args", e.to_string()))?;
    crate::annotations::create_aligned(txn, args)
}
