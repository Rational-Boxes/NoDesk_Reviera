// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Growing layouts (spec Ch 3 §3.6a.5, ARC-GLZ-005): a glazed enclosure's floor filled with
//! growing systems and aisles, and its growing area reported.
//!
//! A layout is an `IfcElementAssembly` (ObjectType `GrowingLayout`) on an enclosure, whose
//! recipe gives the system and its sizes; it owns its pieces, each its own element with a
//! `recipe.growing_part` (so schedules count them):
//! - **benches** and **rolling benches**: `IfcFurniture` (ObjectType `Bench`, `RollingBench`), a
//!   40 mm top on four legs;
//! - **ebb-and-flow tables**: `IfcFurniture` (`EbbFlowTable`), a 100 mm tray on legs;
//! - **NFT tables**: `IfcBuildingElementProxy` (`GrowingGutter`), channels 100 mm wide at 200 mm
//!   centres falling `slope` along their run, on legs, no run longer than 12 m.
//!
//! The pieces stand clear of the walls by `clearance` (never less than the outer posts need) and
//! are split where an inner gutter line's posts stand (a valley's; a Venlo's truss lines), so
//! none meets a post:
//! - **across** (the default): pieces run across the span from the side walls to a main aisle
//!   along the middle of the length (in line with the end walls' doors), repeated along the
//!   length at `width` + `aisle`;
//! - **along**: pieces run along the length, repeated across each span, split by a main aisle
//!   across the middle.
//!
//! Rolling benches stand in groups with a 50 mm gap and one `aisle` per group, the aisle opened
//! by rolling them. A layout is laid out again when its enclosure changes.
//!
//! Not yet: layouts in rooms, high-wire gutters, drainage of NFT and ebb-and-flow systems to
//! plumbing, benches from Component Library types.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::bodies::Mesh;
use crate::element::{Element, NewElement};
use crate::enclosures::{self, EnclosureRecipe};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::RECIPE_KEY;

pub const RECIPE: &str = "recipe.growing_layout";
pub const PART_RECIPE: &str = "recipe.growing_part";
pub const OBJECT_TYPE: &str = "GrowingLayout";
/// The parameter by which a piece names its layout (GlobalId).
pub const PART_KEY: &str = "rxd:growing_layout";
/// The longest NFT run before it needs a second feed.
pub const NFT_RUN: f64 = 12.0;
const LEG: f64 = 0.05;
const ROLLING_GAP: f64 = 0.05;
const ROLLING_GROUP: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum System {
    Benches,
    RollingBenches,
    EbbFlow,
    Nft,
}

impl System {
    fn object_type(self) -> &'static str {
        match self {
            System::Benches => "Bench",
            System::RollingBenches => "RollingBench",
            System::EbbFlow => "EbbFlowTable",
            System::Nft => "GrowingGutter",
        }
    }

    fn class(self) -> &'static str {
        if self == System::Nft { "IfcBuildingElementProxy" } else { "IfcFurniture" }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    #[default]
    Across,
    Along,
}

/// The stored recipe of a growing layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LayoutRecipe {
    pub kind: String,
    /// The enclosure (GlobalId).
    pub enclosure: String,
    pub system: System,
    #[serde(default)]
    pub direction: Direction,
    /// A piece's width; the aisle between pieces, and the main aisle.
    pub width: f64,
    pub aisle: f64,
    pub main_aisle: f64,
    /// Clear of the walls and posts.
    pub clearance: f64,
    /// The growing surface's height above the floor.
    pub height: f64,
    /// NFT channels' fall along their run (rise over run).
    pub slope: f64,
    #[serde(default)]
    pub parts: Vec<String>,
}

/// One piece's stored recipe: its outline in plan, its top's height at each end of its run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PartRecipe {
    pub kind: String,
    pub system: System,
    pub level: String,
    /// The run's start and end on the piece's centre line, and its width across.
    pub start: [f64; 2],
    pub end: [f64; 2],
    pub width: f64,
    /// The growing surface's height at the start and the end.
    pub top: [f64; 2],
}

pub fn recipe(e: &Element) -> Option<LayoutRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

pub fn part_recipe(e: &Element) -> Option<PartRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == PART_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// A box between two plan points, `width` across, its top at `z1` (start, end) and its bottom
/// `z0` at the start, falling with the top.
fn slab(a: [f64; 2], b: [f64; 2], width: f64, z0: f64, z1: [f64; 2]) -> Mesh {
    let l = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-9);
    let n = [-(b[1] - a[1]) / l * width / 2.0, (b[0] - a[0]) / l * width / 2.0];
    let ring = [[a[0] - n[0], a[1] - n[1]], [b[0] - n[0], b[1] - n[1]], [b[0] + n[0], b[1] + n[1]], [a[0] + n[0], a[1] + n[1]]];
    let tops = [z1[0], z1[1], z1[1], z1[0]];
    let fall = z1[1] - z1[0];
    let bottoms = [z0, z0 + fall, z0 + fall, z0];
    let mut pts: Vec<[f64; 3]> = (0..4).map(|k| [ring[k][0], ring[k][1], bottoms[k]]).collect();
    pts.extend((0..4).map(|k| [ring[k][0], ring[k][1], tops[k]]));
    let quads: [[u32; 4]; 6] = [[3, 2, 1, 0], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]];
    (pts, quads.iter().flat_map(|q| [q[0], q[1], q[2], q[0], q[2], q[3]]).collect())
}

/// A piece's meshes: its top (a tray, or channels) and its legs.
pub fn meshes(p: &PartRecipe, level_z: f64) -> Vec<Mesh> {
    let (a, b, w) = (p.start, p.end, p.width);
    let [t0, t1] = [level_z + p.top[0], level_z + p.top[1]];
    let l = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-9);
    let (u, n) = ([(b[0] - a[0]) / l, (b[1] - a[1]) / l], [-(b[1] - a[1]) / l, (b[0] - a[0]) / l]);
    let at = |s: f64, c: f64| [a[0] + u[0] * s + n[0] * c, a[1] + u[1] * s + n[1] * c];
    let depth = match p.system {
        System::Benches | System::RollingBenches => 0.04,
        System::EbbFlow | System::Nft => 0.1,
    };
    let mut out = Vec::new();
    if p.system == System::Nft {
        // Channels 100 mm wide at 200 mm centres across the table.
        let count = ((w / 0.2).floor() as usize).max(1);
        let first = -(count as f64 - 1.0) * 0.2 / 2.0;
        for k in 0..count {
            let c = first + 0.2 * k as f64;
            out.push(slab(at(0.0, c), at(l, c), 0.1, t0 - depth, [t0, t1]));
        }
    } else {
        out.push(slab(a, b, w, t0 - depth, [t0, t1]));
    }
    // Legs at the corners and at most every 3 m along, under the top.
    let under = depth + 0.001;
    let steps = (l / 3.0).ceil().max(1.0) as usize;
    let inset = LEG / 2.0 + 0.02;
    for k in 0..=steps {
        let s = (l * k as f64 / steps as f64).clamp(inset, l - inset);
        let z = t0 + (t1 - t0) * s / l - under;
        for c in [-w / 2.0 + inset, w / 2.0 - inset] {
            let p = at(s, c);
            out.push(slab([p[0] - LEG / 2.0, p[1]], [p[0] + LEG / 2.0, p[1]], LEG, level_z, [z, z]));
        }
    }
    out
}

/// Every piece with its meshes.
pub fn all(store: &Store) -> Vec<(ElementId, &'static str, Vec<Mesh>)> {
    store
        .iter()
        .filter_map(|e| {
            let p = part_recipe(e)?;
            let z = crate::levels::elevation_of(store, &p.level).unwrap_or(0.0);
            Some((e.id(), p.system.class(), meshes(&p, z)))
        })
        .collect()
}

/// The growing area of the pieces (their tops seen from above).
pub fn growing_area(parts: &[PartRecipe]) -> f64 {
    parts.iter().map(|p| (p.end[0] - p.start[0]).hypot(p.end[1] - p.start[1]) * p.width).sum()
}

/// Intervals [a, b] between cut lines `cuts` (sorted), each kept clear of the cuts by `clear`.
fn between(lo: f64, hi: f64, cuts: &[f64], clear: f64) -> Vec<(f64, f64)> {
    let mut edges = vec![lo];
    edges.extend(cuts.iter().copied().filter(|c| *c > lo && *c < hi));
    edges.push(hi);
    edges
        .windows(2)
        .map(|w| (w[0] + if w[0] == lo { 0.0 } else { clear }, w[1] - if w[1] == hi { 0.0 } else { clear }))
        .filter(|(a, b)| b - a > 0.3)
        .collect()
}

/// Pieces across `(a, b)` of width `w`, `aisle` apart (rolling benches: groups with gaps), as
/// their centre lines' offsets.
fn rows(a: f64, b: f64, w: f64, aisle: f64, rolling: bool) -> Vec<f64> {
    let mut out = Vec::new();
    let mut x = a;
    let mut in_group = 0;
    while x + w <= b + 1e-9 {
        out.push(x + w / 2.0);
        in_group += 1;
        let gap = if rolling && in_group < ROLLING_GROUP { ROLLING_GAP } else { in_group = 0; aisle };
        x += w + gap;
    }
    // Centred in the room left over.
    if let (Some(first), Some(last)) = (out.first().copied(), out.last().copied()) {
        let shift = ((b - (last + w / 2.0)) - (first - w / 2.0 - a)) / 2.0;
        for o in &mut out {
            *o += shift;
        }
    }
    out
}

/// The pieces of a layout over an enclosure, in plan (the enclosure's frame turned to the
/// project's), each with its run, width and top heights.
pub fn plan(r: &LayoutRecipe, e: &EnclosureRecipe, level: &str) -> Vec<PartRecipe> {
    let (l, w) = (e.length, e.width());
    // Posts stand at the bays on the gutter lines: the outer ones inside the walls (kept clear by
    // the wall clearance), the inner ones on the valleys (the pieces split round them).
    let framed = e.frame && !e.form.tunnel();
    let post = if framed { 0.06 + r.clearance / 2.0 } else { 0.0 };
    let wall = if framed { r.clearance.max(0.2) } else { r.clearance };
    let every = if e.form == enclosures::Form::Venlo { e.peaks.unwrap_or(2).max(1) } else { 1 };
    let gutter_lines: Vec<f64> = if post > 0.0 { (1..e.spans).filter(|i| i % every == 0).map(|i| e.span * f64::from(i)).collect() } else { Vec::new() };
    let (s, c) = e.rotation.sin_cos();
    let at = |x: f64, y: f64| [e.origin[0] + c * x - s * y, e.origin[1] + s * x + c * y];
    let rolling = r.system == System::RollingBenches;
    let mut out = Vec::new();
    let mut push = |a: [f64; 2], b: [f64; 2], len: f64| {
        // NFT runs no longer than 12 m, each falling from its feed end.
        let pieces = if r.system == System::Nft { (len / NFT_RUN).ceil().max(1.0) as usize } else { 1 };
        for k in 0..pieces {
            let f0 = k as f64 / pieces as f64;
            let f1 = (k + 1) as f64 / pieces as f64;
            let p0 = [a[0] + (b[0] - a[0]) * f0, a[1] + (b[1] - a[1]) * f0];
            let p1 = [a[0] + (b[0] - a[0]) * f1, a[1] + (b[1] - a[1]) * f1];
            let run = len / pieces as f64;
            let fall = if r.system == System::Nft { r.slope * run } else { 0.0 };
            out.push(PartRecipe { kind: PART_RECIPE.into(), system: r.system, level: level.to_owned(), start: p0, end: p1, width: r.width, top: [r.height, r.height - fall] });
        }
    };
    match r.direction {
        // Across the span, from the side walls to the main aisle, bay by bay.
        Direction::Across => {
            let mid = w / 2.0;
            let halves = [(wall, mid - r.main_aisle / 2.0), (mid + r.main_aisle / 2.0, w - wall)];
            for x in rows(wall, l - wall, r.width, r.aisle, rolling) {
                for (y0, y1) in halves {
                    // Split where a gutter line's posts stand.
                    for (a, b) in between(y0, y1, &gutter_lines, post) {
                        push(at(x, a), at(x, b), b - a);
                    }
                }
            }
        }
        // Along the length within each span, split by a main aisle across the middle.
        Direction::Along => {
            let mid = l / 2.0;
            let halves = [(wall, mid - r.main_aisle / 2.0), (mid + r.main_aisle / 2.0, l - wall)];
            for (y0, y1) in between(wall, w - wall, &gutter_lines, post) {
                for y in rows(y0, y1, r.width, r.aisle, rolling) {
                    for (a, b) in halves {
                        push(at(a, y), at(b, y), b - a);
                    }
                }
            }
        }
    }
    out
}

/// Makes a layout's pieces from its recipe.
fn build(txn: &mut Transaction<'_>, guid: &str, r: &LayoutRecipe) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let owner = GlobalId::parse(&r.enclosure)
        .and_then(|g| store.find_by_guid(&g))
        .and_then(|id| store.get(id))
        .ok_or_else(|| Error::new("growing.no_enclosure", "the layout's enclosure no longer exists"))?;
    let e = enclosures::recipe(owner).ok_or_else(|| Error::new("growing.no_enclosure", "not a glazed enclosure"))?;
    let mut ids = Vec::new();
    for p in plan(r, &e, &e.level) {
        ids.push(txn.create(
            NewElement::new(r.system.class())
                .name(match r.system {
                    System::Benches => "Bench",
                    System::RollingBenches => "Rolling Bench",
                    System::EbbFlow => "Ebb and Flow Table",
                    System::Nft => "NFT Table",
                })
                .param("@PredefinedType", "USERDEFINED")
                .param("@ObjectType", r.system.object_type())
                .param(PART_KEY, json!(guid))
                .param(RECIPE_KEY, serde_json::to_value(&p).expect("serialises")),
        )?);
    }
    Ok(ids)
}

fn write(txn: &mut Transaction<'_>, id: ElementId, r: &LayoutRecipe) -> Result<()> {
    let value = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })
}

/// The layout's pieces made again.
fn rebuild(txn: &mut Transaction<'_>, id: ElementId, mut r: LayoutRecipe) -> Result<()> {
    let store = txn.store().clone();
    for g in &r.parts {
        if let Some(part) = GlobalId::parse(g).and_then(|g| store.find_by_guid(&g))
            && txn.store().contains(part)
        {
            txn.delete(part)?;
        }
    }
    let guid = store.get(id).expect("exists").guid().to_string();
    let parts = build(txn, &guid, &r)?;
    r.parts = parts.iter().map(|p| txn.store().get(*p).expect("made").guid().to_string()).collect();
    write(txn, id, &r)
}

/// Lays out again every layout on the enclosure `guid` (after the enclosure changed).
pub(crate) fn follow(txn: &mut Transaction<'_>, guid: &str) -> Result<()> {
    let layouts: Vec<(ElementId, LayoutRecipe)> = txn.store().iter().filter_map(|e| recipe(e).filter(|r| r.enclosure == guid).map(|r| (e.id(), r))).collect();
    for (id, r) in layouts {
        rebuild(txn, id, r)?;
    }
    Ok(())
}

// ---- Commands ----

/// Sizes of `growing.layout` and `growing.set_layout`, in metres.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Sizes {
    pub direction: Option<Direction>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub width: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub aisle: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub main_aisle: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub clearance: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub height: Option<f64>,
    /// NFT: the channels' fall, rise over run (default 0.02).
    pub slope: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LayoutParams {
    /// The glazed enclosure to fill.
    pub enclosure_id: ElementRef,
    pub system: System,
    #[serde(flatten)]
    pub sizes: Sizes,
}

/// A system's defaults (provisional, EXPERT_REVIEW E-48).
fn defaults(system: System, enclosure: String) -> LayoutRecipe {
    let (width, aisle, height) = match system {
        System::Benches => (1.8, 0.6, 0.8),
        System::RollingBenches => (1.8, 0.6, 0.8),
        System::EbbFlow => (1.8, 0.6, 0.75),
        System::Nft => (1.2, 0.6, 0.9),
    };
    LayoutRecipe { kind: RECIPE.into(), enclosure, system, direction: Direction::Across, width, aisle, main_aisle: 1.2, clearance: 0.2, height, slope: 0.02, parts: Vec::new() }
}

fn apply(r: &mut LayoutRecipe, s: &Sizes) -> Result<()> {
    if let Some(d) = s.direction {
        r.direction = d;
    }
    for (v, slot) in [(s.width, &mut r.width), (s.aisle, &mut r.aisle), (s.main_aisle, &mut r.main_aisle), (s.clearance, &mut r.clearance), (s.height, &mut r.height)] {
        if let Some(v) = v {
            *slot = v;
        }
    }
    if let Some(v) = s.slope {
        r.slope = v;
    }
    let bad = |m: &str| Err(Error::new("growing.bad_value", m.to_owned()));
    if !(0.3..=5.0).contains(&r.width) {
        return bad("a piece is 0.3 to 5 m wide");
    }
    if !(0.3..=5.0).contains(&r.aisle) || !(0.0..=10.0).contains(&r.main_aisle) || !(0.0..=3.0).contains(&r.clearance) {
        return bad("aisles are 0.3 to 5 m, the main aisle 0 to 10 m and the clearance 0 to 3 m");
    }
    if !(0.1..=2.5).contains(&r.height) {
        return bad("the growing surface is 0.1 to 2.5 m high");
    }
    if r.system == System::Nft && !(0.005..=0.05).contains(&r.slope) {
        return bad("NFT channels fall 0.5 to 5 %");
    }
    Ok(())
}

/// `growing.layout`.
pub(crate) fn layout(txn: &mut Transaction<'_>, p: LayoutParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let owner = store
        .resolve(&p.enclosure_id)
        .filter(|id| store.get(*id).and_then(enclosures::recipe).is_some())
        .ok_or_else(|| Error::new("growing.no_enclosure", "growing layouts go in a glazed enclosure"))?;
    let mut r = defaults(p.system, store.get(owner).expect("resolved").guid().to_string());
    apply(&mut r, &p.sizes)?;
    let guid = GlobalId::new();
    let id = txn.create(
        NewElement::new("IfcElementAssembly")
            .guid(guid.clone())
            .name("Growing Layout")
            .param("@PredefinedType", "USERDEFINED")
            .param("@ObjectType", OBJECT_TYPE)
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )?;
    let parts = build(txn, guid.as_str(), &r)?;
    if parts.is_empty() {
        return Err(Error::new("growing.nothing_fits", "nothing of that size fits in the enclosure"));
    }
    r.parts = parts.iter().map(|p| txn.store().get(*p).expect("made").guid().to_string()).collect();
    write(txn, id, &r)?;
    Ok(id)
}

fn resolve(store: &Store, r: &ElementRef) -> Result<(ElementId, LayoutRecipe)> {
    let id = store.resolve(r).ok_or_else(|| Error::new("element.not_found", "no such layout"))?;
    let rec = recipe(store.get(id).expect("resolved")).ok_or_else(|| Error::new("growing.not_a_layout", format!("element {id} is not a growing layout")))?;
    Ok((id, rec))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub layout_id: ElementRef,
    pub system: Option<System>,
    #[serde(flatten)]
    pub sizes: Sizes,
}

/// `growing.set_layout`.
pub(crate) fn set_layout(txn: &mut Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = resolve(&store, &p.layout_id)?;
    if let Some(s) = p.system.filter(|s| *s != r.system) {
        let d = defaults(s, r.enclosure.clone());
        r = LayoutRecipe { parts: r.parts, direction: r.direction, ..d };
    }
    apply(&mut r, &p.sizes)?;
    rebuild(txn, id, r)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SummaryParams {
    pub layout_id: ElementRef,
}

/// What `growing.summary` returns.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Summary {
    pub system: System,
    pub pieces: usize,
    /// The growing surface's area, the floor's, and the share (percent).
    pub growing_area: f64,
    pub floor_area: f64,
    pub share: f64,
    /// NFT: the longest run and its fall.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longest_run: Option<f64>,
}

/// `growing.summary`.
pub(crate) fn summary(store: &Store, p: SummaryParams) -> Result<Summary> {
    let (_, r) = resolve(store, &p.layout_id)?;
    let parts: Vec<PartRecipe> = r.parts.iter().filter_map(|g| store.get(store.find_by_guid(&GlobalId::parse(g)?)?)).filter_map(part_recipe).collect();
    let floor = GlobalId::parse(&r.enclosure).and_then(|g| store.get(store.find_by_guid(&g)?)).and_then(enclosures::recipe).map_or(0.0, |e| e.width() * e.length);
    let area = growing_area(&parts);
    let longest = (r.system == System::Nft).then(|| parts.iter().map(|p| (p.end[0] - p.start[0]).hypot(p.end[1] - p.start[1])).fold(0.0, f64::max));
    Ok(Summary { system: r.system, pieces: parts.len(), growing_area: area, floor_area: floor, share: 100.0 * area / floor.max(1e-9), longest_run: longest })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_fill_and_centre() {
        // 9 m takes four 1.8 m benches with 0.6 m aisles exactly (4 × 1.8 + 3 × 0.6).
        let r = rows(0.0, 9.0, 1.8, 0.6, false);
        assert_eq!(r.len(), 4, "{r:?}");
        let r = rows(0.0, 8.0, 1.8, 0.6, false);
        assert_eq!(r.len(), 3);
        assert!(((r[0] - 0.9) - (8.0 - (r[2] + 0.9))).abs() < 1e-9, "centred: {r:?}");
        // Rolling benches pack five to a group.
        assert!(rows(0.0, 9.6, 1.8, 0.6, true).len() >= 5);
        assert_eq!(between(0.0, 10.0, &[4.0], 0.1), vec![(0.0, 3.9), (4.1, 10.0)]);
    }
}
