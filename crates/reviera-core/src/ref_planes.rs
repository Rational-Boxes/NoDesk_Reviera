// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Reference planes (spec Ch 3 §3.2.3, ARC-REFP-001): unbounded construction planes for
//! alignment, dimensioning, sketch guides and work planes (ARC-WP-001).
//!
//! A reference plane is drawn as a line in a view: the plane passes through the line and is at
//! right angles to the view (it contains the view's direction, `view_normal`). Drawn in a plan it
//! is vertical. It is an `IfcAnnotation` (`ObjectType = "ReferencePlane"`) whose `Axis`
//! representation is the defining line; the line, the view direction and the subcategory are its
//! recipe (`recipe.ref_plane`). A named plane is offered as a work plane.
//!
//! Views show a plane where they look along it: a plan shows a vertical plane as its line, a
//! section or elevation at right angles to it shows it as the line where it cuts the view.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::RECIPE_KEY;
use crate::work_planes::{Plane, cross, dot, sub, unit};

pub const RECIPE: &str = "recipe.ref_plane";
pub const OBJECT_TYPE: &str = "ReferencePlane";
/// The shortest reference plane line (metres).
const MIN_LENGTH: f64 = 0.01;

/// The stored recipe of a reference plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RefPlaneRecipe {
    pub kind: String,
    /// The defining line's ends (world, metres).
    pub start: [f64; 3],
    pub end: [f64; 3],
    /// The direction of the view it was drawn in, which the plane contains (up for a plan).
    pub view_normal: [f64; 3],
    /// A user subcategory, for visibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subcategory: Option<String>,
}

pub fn recipe(e: &Element) -> Option<RefPlaneRecipe> {
    let r: RefPlaneRecipe = serde_json::from_value(e.param(RECIPE_KEY)?.clone()).ok()?;
    (r.kind == RECIPE).then_some(r)
}

/// The plane of a recipe: origin at the line's start, x along the line, y toward the view
/// direction, normal x × y.
pub fn plane_of(r: &RefPlaneRecipe) -> Result<Plane> {
    let x = sub(r.end, r.start);
    Plane::new(r.start, x, cross(x, r.view_normal))
}

/// A reference plane as listed.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RefPlaneInfo {
    pub id: ElementId,
    pub guid: String,
    pub name: Option<String>,
    pub start: [f64; 3],
    pub end: [f64; 3],
    pub view_normal: [f64; 3],
    /// The plane's normal.
    pub normal: [f64; 3],
    pub subcategory: Option<String>,
}

/// Every reference plane, in creation order.
pub fn list(store: &Store) -> Vec<RefPlaneInfo> {
    store
        .iter()
        .filter_map(|e| {
            let r = recipe(e)?;
            let p = plane_of(&r).ok()?;
            Some(RefPlaneInfo {
                id: e.id(),
                guid: e.guid().to_string(),
                name: e.name.clone(),
                start: r.start,
                end: r.end,
                view_normal: r.view_normal,
                normal: p.normal,
                subcategory: r.subcategory,
            })
        })
        .collect()
}

/// `refplane.list`.
pub(crate) fn list_command(
    store: &Store,
    _: crate::builtin::NoParams,
) -> Result<Vec<RefPlaneInfo>> {
    Ok(list(store))
}

/// A point given as `[x, y]` (at `z`) or `[x, y, z]`.
fn point(p: &[f64], z: f64) -> Result<[f64; 3]> {
    match *p {
        [x, y] => Ok([x, y, z]),
        [x, y, z] => Ok([x, y, z]),
        _ => Err(Error::new(
            "refplane.point",
            "a point is [x, y] or [x, y, z]",
        )),
    }
}

fn check(r: &RefPlaneRecipe) -> Result<()> {
    let x = sub(r.end, r.start);
    if dot(x, x).sqrt() < MIN_LENGTH {
        return Err(Error::new(
            "refplane.too_short",
            "a reference plane's line must be at least 10 mm long",
        ));
    }
    let n = unit(r.view_normal)
        .ok_or_else(|| Error::new("refplane.view_normal", "the view direction is zero"))?;
    if unit(cross(x, n)).is_none() {
        return Err(Error::new(
            "refplane.along_view",
            "the line runs along the view direction: it does not define a plane",
        ));
    }
    Ok(())
}

fn unique_name(store: &Store, name: &str, except: Option<ElementId>) -> Result<()> {
    let taken = store
        .iter()
        .any(|e| Some(e.id()) != except && recipe(e).is_some() && e.name.as_deref() == Some(name));
    if taken {
        return Err(Error::new(
            "refplane.name_taken",
            format!("a reference plane is already named {name:?}"),
        ));
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// The line's start: `[x, y]` (at the level's elevation, or `z`) or `[x, y, z]`.
    pub start: Vec<f64>,
    pub end: Vec<f64>,
    /// The height of 2D points (metres); omitted, the level's elevation, else 0.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub z: Option<f64>,
    /// A level whose elevation 2D points take.
    pub level: Option<ElementRef>,
    /// The direction of the view the line is drawn in; the plane contains it. Omitted: up (a plan).
    pub view_normal: Option<[f64; 3]>,
    pub name: Option<String>,
    pub subcategory: Option<String>,
}

/// `refplane.create`.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let z = match (p.z, &p.level) {
        (Some(z), _) => z,
        (None, Some(l)) => {
            let id = store
                .resolve(l)
                .filter(|id| {
                    store
                        .get(*id)
                        .is_some_and(|e| e.class == crate::levels::CLASS)
                })
                .ok_or_else(|| Error::new("level.not_found", format!("no level {l:?}")))?;
            crate::levels::elevation(store.get(id).expect("resolved"))
        }
        (None, None) => 0.0,
    };
    let r = RefPlaneRecipe {
        kind: RECIPE.into(),
        start: point(&p.start, z)?,
        end: point(&p.end, z)?,
        view_normal: p.view_normal.unwrap_or([0.0, 0.0, 1.0]),
        subcategory: p.subcategory.filter(|s| !s.trim().is_empty()),
    };
    check(&r)?;
    let name = p.name.filter(|n| !n.trim().is_empty());
    if let Some(n) = &name {
        unique_name(&store, n, None)?;
    }
    let mut n = NewElement::new("IfcAnnotation").param("@ObjectType", OBJECT_TYPE);
    n.name = name;
    let id = txn.create(n)?;
    write(txn, id, &r)?;
    Ok(id)
}

pub(crate) fn write(txn: &mut Transaction<'_>, id: ElementId, r: &RefPlaneRecipe) -> Result<()> {
    check(r)?;
    let value = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })
}

fn resolve_plane(store: &Store, r: &ElementRef) -> Result<(ElementId, RefPlaneRecipe)> {
    store
        .resolve(r)
        .and_then(|id| Some((id, recipe(store.get(id)?)?)))
        .ok_or_else(|| Error::new("refplane.not_found", format!("no reference plane {r:?}")))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenameParams {
    pub ref_plane_id: ElementRef,
    /// The new name; empty unnames it.
    pub name: String,
}

/// `refplane.rename`.
pub(crate) fn rename(txn: &mut Transaction<'_>, p: RenameParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, _) = resolve_plane(&store, &p.ref_plane_id)?;
    let name = p.name.trim().to_owned();
    if !name.is_empty() {
        unique_name(&store, &name, Some(id))?;
    }
    txn.modify(id, |e| e.name = (!name.is_empty()).then_some(name))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub ref_plane_id: ElementRef,
    /// New ends: `[x, y]` (keeping their heights) or `[x, y, z]`.
    pub start: Option<Vec<f64>>,
    pub end: Option<Vec<f64>>,
    /// Empty clears it.
    pub subcategory: Option<String>,
}

/// `refplane.set`: moves a reference plane's line ends or changes its subcategory.
pub(crate) fn set(txn: &mut Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = resolve_plane(&store, &p.ref_plane_id)?;
    if let Some(s) = &p.start {
        r.start = point(s, r.start[2])?;
    }
    if let Some(e) = &p.end {
        r.end = point(e, r.end[2])?;
    }
    if let Some(s) = p.subcategory {
        r.subcategory = (!s.trim().is_empty()).then_some(s);
    }
    write(txn, id, &r)
}

/// Moves, turns or mirrors a reference plane in plan (and raises it by `dz`).
pub(crate) fn transform(
    txn: &mut Transaction<'_>,
    id: ElementId,
    point: &dyn Fn([f64; 2]) -> [f64; 2],
    dz: f64,
) -> Result<()> {
    let mut r = recipe(txn.store().get(id).expect("exists")).expect("a reference plane");
    let move_point = |p: [f64; 3]| {
        let q = point([p[0], p[1]]);
        [q[0], q[1], p[2] + dz]
    };
    let o = point([0.0, 0.0]);
    let v = point([r.view_normal[0], r.view_normal[1]]);
    r.start = move_point(r.start);
    r.end = move_point(r.end);
    r.view_normal = [v[0] - o[0], v[1] - o[1], r.view_normal[2]];
    write(txn, id, &r)
}

/// The world line where a reference plane meets a horizontal plane at `z`, clipped to the
/// defining line's extent (`None` unless the plane is vertical).
pub fn plan_trace(r: &RefPlaneRecipe) -> Option<([f64; 2], [f64; 2])> {
    let p = plane_of(r).ok()?;
    p.is_vertical()
        .then_some(([r.start[0], r.start[1]], [r.end[0], r.end[1]]))
}
