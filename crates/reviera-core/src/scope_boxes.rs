// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Scope boxes (spec Ch 3 §3.2.5, ARC-SCOPE-001): named, possibly rotated 3D boxes that limit
//! where datums show and how far they reach.
//!
//! A scope box is an `IfcAnnotation` (`ObjectType = "ScopeBox"`) with a `Box` representation; its
//! recipe (`recipe.scope_box`) holds its centre, width (along its own x), depth, rotation, bottom
//! and height. Datums assigned to it ([`crate::datum_extents`]) show only in views whose plane
//! crosses it, and their lines are clipped to its footprint; moving, turning or resizing it fits
//! them again. Views may be assigned to it too (`rxd:scope_box` on the view); their crop follows
//! it once views have crop regions. Plans draw the boxes they cut through.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::{Line, RECIPE_KEY};

pub const RECIPE: &str = "recipe.scope_box";
pub const OBJECT_TYPE: &str = "ScopeBox";
/// Parameter on a view holding its scope box (GlobalId).
pub const VIEW_KEY: &str = "rxd:scope_box";
const MIN_SIZE: f64 = 0.1;

/// The stored recipe of a scope box.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScopeBox {
    pub kind: String,
    pub center: [f64; 2],
    /// Along the box's own x (its rotation).
    pub width: f64,
    pub depth: f64,
    /// Radians, counter-clockwise.
    #[serde(default)]
    pub rotation: f64,
    pub bottom: f64,
    pub height: f64,
}

impl ScopeBox {
    /// The box's own axes in plan.
    pub fn axes(&self) -> ([f64; 2], [f64; 2]) {
        let (s, c) = self.rotation.sin_cos();
        ([c, s], [-s, c])
    }

    /// The footprint's corners, counter-clockwise.
    pub fn corners(&self) -> [[f64; 2]; 4] {
        let (u, v) = self.axes();
        let (hw, hd) = (self.width / 2.0, self.depth / 2.0);
        let at = |a: f64, b: f64| {
            [
                self.center[0] + u[0] * a + v[0] * b,
                self.center[1] + u[1] * a + v[1] * b,
            ]
        };
        [at(-hw, -hd), at(hw, -hd), at(hw, hd), at(-hw, hd)]
    }

    /// The part of the (infinite) line through `l` inside the footprint, in `l`'s direction.
    pub fn clip(&self, l: Line) -> Option<Line> {
        let len = l.length();
        if len < 1e-9 {
            return None;
        }
        let d = [(l.end[0] - l.start[0]) / len, (l.end[1] - l.start[1]) / len];
        let (u, v) = self.axes();
        // In the box's frame: p(t) = p0 + d t.
        let rel = [l.start[0] - self.center[0], l.start[1] - self.center[1]];
        let p0 = [rel[0] * u[0] + rel[1] * u[1], rel[0] * v[0] + rel[1] * v[1]];
        let dd = [d[0] * u[0] + d[1] * u[1], d[0] * v[0] + d[1] * v[1]];
        let half = [self.width / 2.0, self.depth / 2.0];
        let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
        for k in 0..2 {
            if dd[k].abs() < 1e-12 {
                if p0[k].abs() > half[k] {
                    return None;
                }
                continue;
            }
            let a = (-half[k] - p0[k]) / dd[k];
            let b = (half[k] - p0[k]) / dd[k];
            lo = lo.max(a.min(b));
            hi = hi.min(a.max(b));
        }
        (hi - lo > 1e-6).then(|| Line {
            start: [l.start[0] + d[0] * lo, l.start[1] + d[1] * lo],
            end: [l.start[0] + d[0] * hi, l.start[1] + d[1] * hi],
        })
    }

    /// Whether a plan cut at `z` passes through the box.
    pub fn cut_by(&self, z: f64) -> bool {
        z >= self.bottom - 1e-9 && z <= self.bottom + self.height + 1e-9
    }
}

pub fn recipe(e: &Element) -> Option<ScopeBox> {
    let r: ScopeBox = serde_json::from_value(e.param(RECIPE_KEY)?.clone()).ok()?;
    (r.kind == RECIPE).then_some(r)
}

/// A scope box as listed.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ScopeBoxInfo {
    pub id: ElementId,
    pub guid: String,
    pub name: String,
    #[serde(flatten)]
    pub scope_box: ScopeBox,
    /// The datums assigned to it.
    pub datums: Vec<ElementId>,
}

pub fn list(store: &Store) -> Vec<ScopeBoxInfo> {
    store
        .iter()
        .filter_map(|e| {
            Some(ScopeBoxInfo {
                id: e.id(),
                guid: e.guid().to_string(),
                name: e.name.clone().unwrap_or_default(),
                scope_box: recipe(e)?,
                datums: crate::datum_extents::assigned_to(store, e.guid().as_str()),
            })
        })
        .collect()
}

/// `scopebox.list`.
pub(crate) fn list_command(
    store: &Store,
    _: crate::builtin::NoParams,
) -> Result<Vec<ScopeBoxInfo>> {
    Ok(list(store))
}

fn check(b: &ScopeBox) -> Result<()> {
    if b.width < MIN_SIZE || b.depth < MIN_SIZE || b.height < MIN_SIZE {
        return Err(Error::new(
            "scopebox.too_small",
            "a scope box must be at least 100 mm in each direction",
        ));
    }
    Ok(())
}

/// A rectangle in plan: two opposite corners, square to `rotation` (radians).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct Rectangle {
    pub corner_a: [f64; 2],
    pub corner_b: [f64; 2],
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: f64,
}

fn from_corners(r: &Rectangle, bottom: f64, height: f64) -> ScopeBox {
    let (s, c) = r.rotation.sin_cos();
    let (u, v) = ([c, s], [-s, c]);
    let d = [r.corner_b[0] - r.corner_a[0], r.corner_b[1] - r.corner_a[1]];
    ScopeBox {
        kind: RECIPE.into(),
        center: [
            (r.corner_a[0] + r.corner_b[0]) / 2.0,
            (r.corner_a[1] + r.corner_b[1]) / 2.0,
        ],
        width: (d[0] * u[0] + d[1] * u[1]).abs(),
        depth: (d[0] * v[0] + d[1] * v[1]).abs(),
        rotation: r.rotation,
        bottom,
        height,
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub name: Option<String>,
    pub rectangle: Rectangle,
    /// Metres; omitted, from the lowest level to above the highest.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub height: Option<f64>,
    /// The level its bottom is on; omitted, below the lowest level.
    pub level_id: Option<ElementRef>,
}

fn next_name(store: &Store) -> String {
    let n = store.iter().filter(|e| recipe(e).is_some()).count() + 1;
    format!("Scope Box {n}")
}

/// `scopebox.create`.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let heights = crate::datum_extents::default_heights(&store);
    let bottom = match &p.level_id {
        Some(l) => {
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
        None => heights[0],
    };
    let height = p.height.unwrap_or(heights[1] - bottom);
    let b = from_corners(&p.rectangle, bottom, height);
    check(&b)?;
    let name = p
        .name
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| next_name(&store));
    let id = txn.create(
        NewElement::new("IfcAnnotation")
            .name(name)
            .param("@ObjectType", OBJECT_TYPE),
    )?;
    write(txn, id, &b)?;
    Ok(id)
}

pub(crate) fn write(txn: &mut Transaction<'_>, id: ElementId, b: &ScopeBox) -> Result<()> {
    check(b)?;
    let value = serde_json::to_value(b).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })?;
    refit(txn, id, b)
}

/// Fits the datums assigned to box `id` to it.
fn refit(txn: &mut Transaction<'_>, id: ElementId, b: &ScopeBox) -> Result<()> {
    let store = txn.store().clone();
    let guid = store.get(id).expect("exists").guid().to_string();
    for d in crate::datum_extents::assigned_to(&store, &guid) {
        if let Some(kind) = store.get(d).and_then(crate::datum_extents::kind_of)
            && kind != crate::datum_extents::Kind::Level
        {
            crate::datum_extents::fit_to_box(txn, d, kind, b)?;
        }
    }
    Ok(())
}

fn resolve(store: &Store, r: &ElementRef) -> Result<(ElementId, ScopeBox)> {
    store
        .resolve(r)
        .and_then(|id| Some((id, recipe(store.get(id)?)?)))
        .ok_or_else(|| Error::new("scopebox.not_found", format!("no scope box {r:?}")))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetExtentsParams {
    pub scope_box_id: ElementRef,
    /// Two opposite corners `[x, y, z]`, square to the rotation (the current one if omitted).
    pub min: Option<[f64; 3]>,
    pub max: Option<[f64; 3]>,
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: Option<f64>,
    pub name: Option<String>,
}

/// `scopebox.set_extents`.
pub(crate) fn set_extents(txn: &mut Transaction<'_>, p: SetExtentsParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut b) = resolve(&store, &p.scope_box_id)?;
    let rotation = p.rotation.unwrap_or(b.rotation);
    match (p.min, p.max) {
        (Some(lo), Some(hi)) => {
            let r = Rectangle {
                corner_a: [lo[0], lo[1]],
                corner_b: [hi[0], hi[1]],
                rotation,
            };
            b = from_corners(&r, lo[2].min(hi[2]), (hi[2] - lo[2]).abs());
        }
        (None, None) => b.rotation = rotation,
        _ => {
            return Err(Error::new(
                "scopebox.corners",
                "give both corners, `min` and `max`, or neither",
            ));
        }
    }
    if let Some(n) = p.name.filter(|n| !n.trim().is_empty()) {
        txn.modify(id, |e| e.name = Some(n))?;
    }
    write(txn, id, &b)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssignViewsParams {
    pub view_ids: Vec<ElementRef>,
    /// `null` clears it.
    pub scope_box_id: Option<ElementRef>,
}

/// `view.assign_scope_box`: records a view's scope box (its crop follows the box once views have
/// crop regions).
pub(crate) fn assign_views(txn: &mut Transaction<'_>, p: AssignViewsParams) -> Result<()> {
    let store = txn.store().clone();
    let guid = p
        .scope_box_id
        .as_ref()
        .map(|r| {
            resolve(&store, r).map(|(id, _)| store.get(id).expect("resolved").guid().to_string())
        })
        .transpose()?;
    for v in &p.view_ids {
        let id = store
            .resolve(v)
            .filter(|id| store.get(*id).is_some_and(crate::views::is_view))
            .ok_or_else(|| Error::new("view.not_found", format!("no view {v:?}")))?;
        let g = guid.clone();
        txn.modify(id, |e| match g {
            Some(g) => {
                e.params.insert(VIEW_KEY.into(), g.into());
            }
            None => {
                e.params.remove(VIEW_KEY);
            }
        })?;
    }
    Ok(())
}

/// Moves, turns or mirrors a scope box in plan (and raises it by `dz`); its datums follow.
pub(crate) fn transform(
    txn: &mut Transaction<'_>,
    id: ElementId,
    point: &dyn Fn([f64; 2]) -> [f64; 2],
    dz: f64,
) -> Result<()> {
    let mut b = recipe(txn.store().get(id).expect("exists")).expect("a scope box");
    let o = point([0.0, 0.0]);
    let (u, _) = b.axes();
    let x = point(u);
    b.center = point(b.center);
    b.rotation = (x[1] - o[1]).atan2(x[0] - o[0]);
    b.bottom += dz;
    write(txn, id, &b)
}

/// The scope boxes a plan cut at `z` shows, with their names.
pub fn in_plan(store: &Store, z: f64) -> Vec<(ElementId, String, ScopeBox)> {
    list(store)
        .into_iter()
        .filter(|b| b.scope_box.cut_by(z))
        .map(|b| (b.id, b.name, b.scope_box))
        .collect()
}
