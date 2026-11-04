// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Grab bars (ARC-BATH-003): round bars on a wall face, standing off it on brackets. A bar is a
//! polyline in the face's plane (along the wall from its start, and up from its level), so it
//! follows its wall; it is an `IfcRailing` (HANDRAIL), as NBS maps grab bars. Bars may be drawn
//! by hand (`grab_bar.create`) or placed for fixtures by the bathroom rule packs' bar rules
//! (`bath.place_grab_bars`, [`crate::bath_rules`]), which then check their diameter, wall gap,
//! height and length.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bodies::Mesh;
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;

pub const CLASS: &str = "IfcRailing";
pub const RECIPE_KEY: &str = "rxd:grab_bar";
pub const KIND: &str = "riviera.grab_bar";
/// The kind in models saved before the project's name was spelt correctly.
pub const LEGACY_KIND: &str = "reviera.grab_bar";

/// A bracket's arm: this share of the bar's diameter.
const ARM: f64 = 0.6;
/// A wall flange's diameter and thickness.
const FLANGE: (f64, f64) = (0.075, 0.006);

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// A grab bar's stored recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GrabBarRecipe {
    pub kind: String,
    /// The wall it is fixed to (GlobalId), and which face: 1 the exterior (left of the wall's
    /// direction), −1 the interior.
    pub wall: String,
    pub side: f64,
    /// The level its heights are from (GlobalId).
    pub level: String,
    /// Its centreline in the face's plane: metres along the wall's location line from its start,
    /// and the height of the bar's centre above the level.
    pub points: Vec<[f64; 2]>,
    /// The bar's outside diameter, and the clear space between it and the wall.
    pub diameter: f64,
    pub gap: f64,
    /// The fixture it was placed for (GlobalId) and the rule, when placed by rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixture: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
}

pub fn recipe(e: &Element) -> Option<GrabBarRecipe> {
    let v = e.param(RECIPE_KEY)?;
    matches!(v.get("kind")?.as_str()?, KIND | LEGACY_KIND).then(|| serde_json::from_value(v.clone()).ok())?
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

/// A grab bar as it is now: its centreline in project coordinates, and where each point meets
/// the wall face.
#[derive(Debug, Clone)]
pub struct GrabBar {
    pub centreline: Vec<[f64; 3]>,
    pub on_wall: Vec<[f64; 3]>,
    pub diameter: f64,
    pub gap: f64,
    /// The height of the top of the bar above its level, at its highest.
    pub top: f64,
    /// Its length along the centreline.
    pub length: f64,
}

pub fn regenerate(store: &Store, e: &Element) -> Result<GrabBar> {
    let r = recipe(e).ok_or_else(|| bad("grab_bar.not_riviera", "not a Riviera grab bar"))?;
    let wall = find(store, &r.wall).and_then(|w| store.get(w)).ok_or_else(|| bad("grab_bar.no_wall", "the grab bar's wall no longer exists"))?;
    let g = crate::walls::regenerate(store, wall)?;
    let z0 = crate::levels::elevation_of(store, &r.level)?;
    let (d, n) = (g.direction(), g.normal());
    let s = r.side.signum();
    let face = g.width / 2.0;
    let at = |u: f64, out: f64, z: f64| [g.start[0] + d[0] * u + n[0] * s * out, g.start[1] + d[1] * u + n[1] * s * out, z0 + z];
    let centreline: Vec<[f64; 3]> = r.points.iter().map(|[u, z]| at(*u, face + r.gap + r.diameter / 2.0, *z)).collect();
    let on_wall = r.points.iter().map(|[u, z]| at(*u, face, *z)).collect();
    let length = centreline.windows(2).map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2) + (w[1][2] - w[0][2]).powi(2)).sqrt()).sum();
    let top = r.points.iter().map(|p| p[1]).fold(f64::MIN, f64::max) + r.diameter / 2.0;
    Ok(GrabBar { centreline, on_wall, diameter: r.diameter, gap: r.gap, top, length })
}

impl GrabBar {
    /// The bar, a bracket and flange at each end and corner.
    pub fn mesh(&self) -> Mesh {
        let r = self.diameter / 2.0;
        let mut parts = Vec::new();
        for w in self.centreline.windows(2) {
            parts.push(crate::steel_fab::cylinder(w[0], w[1], r, 12));
        }
        for (c, f) in self.centreline.iter().zip(&self.on_wall) {
            parts.push(crate::steel_fab::cylinder(*f, *c, r * ARM, 8));
            // The flange: a thin disc on the face.
            let dir = [c[0] - f[0], c[1] - f[1], c[2] - f[2]];
            let l = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt().max(1e-9);
            let tip = [f[0] + dir[0] / l * FLANGE.1, f[1] + dir[1] / l * FLANGE.1, f[2] + dir[2] / l * FLANGE.1];
            parts.push(crate::steel_fab::cylinder(*f, tip, FLANGE.0 / 2.0, 16));
        }
        crate::steel_fab::join(parts)
    }

    /// The swept disks written to IFC: the bar and its brackets.
    pub fn lines(&self) -> Vec<(f64, Vec<[f64; 3]>)> {
        let mut out = vec![(self.diameter / 2.0, self.centreline.clone())];
        for (c, f) in self.centreline.iter().zip(&self.on_wall) {
            out.push((self.diameter / 2.0 * ARM, vec![*f, *c]));
        }
        out
    }
}

/// Every grab bar that regenerates.
pub fn all(store: &Store) -> Vec<(ElementId, GrabBar)> {
    store.iter().filter(|e| recipe(e).is_some()).filter_map(|e| Some((e.id(), regenerate(store, e).ok()?))).collect()
}

/// The grab bars placed for a fixture.
pub fn of_fixture(store: &Store, fixture: ElementId) -> Vec<(ElementId, GrabBarRecipe)> {
    let Some(guid) = store.get(fixture).map(|f| f.guid().to_string()) else { return Vec::new() };
    store.iter().filter_map(|e| Some((e.id(), recipe(e)?))).filter(|(_, r)| r.fixture.as_deref() == Some(guid.as_str())).collect()
}

/// The wall face nearest a plan point: (wall, side, along, distance).
pub(crate) fn nearest_face(store: &Store, p: [f64; 2], z: f64) -> Option<(ElementId, f64, f64, f64)> {
    let mut best: Option<(ElementId, f64, f64, f64)> = None;
    for w in store.iter().filter(|e| crate::walls::wall_recipe(e).is_some()) {
        let Ok(g) = crate::walls::regenerate(store, w) else { continue };
        if z < g.base_z || z > g.base_z + g.height {
            continue;
        }
        let (d, n) = (g.direction(), g.normal());
        let rel = [p[0] - g.start[0], p[1] - g.start[1]];
        let u = rel[0] * d[0] + rel[1] * d[1];
        if u < 0.0 || u > g.length() {
            continue;
        }
        let v = rel[0] * n[0] + rel[1] * n[1];
        let side = if v >= 0.0 { 1.0 } else { -1.0 };
        let dist = (v.abs() - g.width / 2.0).abs();
        if best.is_none_or(|b| dist < b.3) {
            best = Some((w.id(), side, u, dist));
        }
    }
    best
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// The wall; omitted, the wall face nearest the first point.
    pub wall_id: Option<ElementRef>,
    /// Which face: the side the first point is on when omitted.
    pub side: Option<f64>,
    pub level: ElementRef,
    /// Points in plan (metres) on or near the face, each with the height of the top of the bar
    /// above the level: a straight bar from two, an L or a U from more. Points off the face are
    /// taken onto it.
    pub points: Vec<[f64; 3]>,
    /// Outside diameter (default 38 mm, 1½ in).
    #[schemars(extend("x-riviera-data-type" = "length"))]
    pub diameter: Option<f64>,
    /// Clear space from the wall (default 38 mm, 1½ in).
    #[schemars(extend("x-riviera-data-type" = "length"))]
    pub gap: Option<f64>,
}

/// Puts a grab bar on a wall face (`grab_bar.create`).
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let level = store.resolve(&p.level).and_then(|l| store.get(l)).filter(|l| l.class == crate::levels::CLASS).ok_or_else(|| bad("grab_bar.no_level", "no such level"))?;
    if p.points.len() < 2 {
        return Err(bad("grab_bar.points", "a grab bar needs at least two points"));
    }
    let z0 = crate::levels::elevation(level);
    let first = p.points[0];
    let (wall, side) = match &p.wall_id {
        Some(w) => {
            let id = store.resolve(w).filter(|id| store.get(*id).is_some_and(|e| crate::walls::wall_recipe(e).is_some())).ok_or_else(|| bad("grab_bar.no_wall", "no such Riviera wall"))?;
            let g = crate::walls::regenerate(&store, store.get(id).expect("resolved"))?;
            let n = g.normal();
            let v = (first[0] - g.start[0]) * n[0] + (first[1] - g.start[1]) * n[1];
            (id, p.side.unwrap_or(if v >= 0.0 { 1.0 } else { -1.0 }))
        }
        None => {
            let (id, side, _, _) = nearest_face(&store, [first[0], first[1]], z0 + first[2]).ok_or_else(|| bad("grab_bar.no_wall", "no wall face near the first point"))?;
            (id, p.side.unwrap_or(side))
        }
    };
    let w = store.get(wall).expect("resolved");
    let g = crate::walls::regenerate(&store, w)?;
    let d = g.direction();
    let diameter = p.diameter.unwrap_or(0.038);
    let gap = p.gap.unwrap_or(0.038);
    if !(0.01..=0.1).contains(&diameter) || !(0.0..=0.2).contains(&gap) {
        return Err(bad("grab_bar.size", "a grab bar's diameter is 10–100 mm and its gap from the wall at most 200 mm"));
    }
    let points: Vec<[f64; 2]> = p.points.iter().map(|q| [(q[0] - g.start[0]) * d[0] + (q[1] - g.start[1]) * d[1], q[2] - diameter / 2.0]).collect();
    let recipe = GrabBarRecipe { kind: KIND.into(), wall: w.guid().to_string(), side, level: level.guid().to_string(), points, diameter, gap, fixture: None, rule: None };
    insert(txn, recipe)
}

pub(crate) fn insert(txn: &mut Transaction<'_>, recipe: GrabBarRecipe) -> Result<ElementId> {
    txn.create(NewElement::new(CLASS).name("Grab Bar").param("@PredefinedType", "HANDRAIL").param(RECIPE_KEY, serde_json::to_value(&recipe).expect("serialises")))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub element_ids: Vec<ElementRef>,
    #[schemars(extend("x-riviera-data-type" = "length"))]
    pub diameter: Option<f64>,
    #[schemars(extend("x-riviera-data-type" = "length"))]
    pub gap: Option<f64>,
    /// Move the whole bar up or down so the top of the bar is at this height above its level.
    #[schemars(extend("x-riviera-data-type" = "length"))]
    pub top: Option<f64>,
}

/// Changes grab bars' diameter, gap or height (`grab_bar.set`).
pub(crate) fn set(txn: &mut Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    for r in &p.element_ids {
        let id = store.resolve(r).ok_or_else(|| bad("element.not_found", format!("no element {r:?}")))?;
        let mut rec = store.get(id).and_then(recipe).ok_or_else(|| bad("grab_bar.not_riviera", format!("element {} is not a grab bar", id.0)))?;
        if let Some(d) = p.diameter {
            for q in &mut rec.points {
                q[1] += rec.diameter / 2.0 - d / 2.0;
            }
            rec.diameter = d;
        }
        if let Some(g) = p.gap {
            rec.gap = g;
        }
        if let Some(t) = p.top {
            let now = rec.points.iter().map(|q| q[1]).fold(f64::MIN, f64::max) + rec.diameter / 2.0;
            for q in &mut rec.points {
                q[1] += t - now;
            }
        }
        if !(0.01..=0.1).contains(&rec.diameter) || !(0.0..=0.2).contains(&rec.gap) {
            return Err(bad("grab_bar.size", "a grab bar's diameter is 10–100 mm and its gap from the wall at most 200 mm"));
        }
        let v = serde_json::to_value(&rec).expect("serialises");
        txn.modify(id, |e| {
            e.params.insert(RECIPE_KEY.into(), v);
        })?;
    }
    Ok(())
}
