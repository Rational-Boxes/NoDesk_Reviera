// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Growing layouts (spec Ch 3 §3.6a.5, ARC-GLZ-005): a glazed enclosure's or a room's floor
//! filled with growing systems and aisles, and its growing area reported.
//!
//! A layout is an `IfcElementAssembly` (ObjectType `GrowingLayout`) on its host, whose
//! recipe gives the system and its sizes; it owns its pieces, each its own element with a
//! `recipe.growing_part` (so schedules count them):
//! - **benches** and **rolling benches**: `IfcFurniture` (ObjectType `Bench`, `RollingBench`), a
//!   40 mm top on four legs;
//! - **ebb-and-flow tables**: `IfcFurniture` (`EbbFlowTable`), a 100 mm tray on legs;
//! - **NFT tables**: `IfcBuildingElementProxy` (`GrowingGutter`), channels 100 mm wide at 200 mm
//!   centres falling `slope` along their run, on legs, no run longer than 12 m;
//! - **high-wire gutters**: `IfcBuildingElementProxy` (`HighWireGutter`), a hanging substrate
//!   gutter falling `slope` to its drain end, on hangers from the crop wire `wire` above the
//!   floor (below the eaves or the room's top), one per row.
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
//! In a **room** the frame runs along the room's longest edge, and each row's run is the part
//! of it inside the room's boundary (courtyards and columns' cut-outs too), clear of it by
//! `clearance`, split by the main aisle across the middle; it is laid out again with
//! `growing.set_layout` when the room changes.
//!
//! Not yet: drainage of NFT, ebb-and-flow and high-wire systems to plumbing, benches from
//! Component Library types.

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
/// A high-wire gutter's crop wire and hangers.
const WIRE: f64 = 0.01;
const HANGER: f64 = 0.012;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum System {
    Benches,
    RollingBenches,
    EbbFlow,
    Nft,
    HighWire,
}

impl System {
    fn object_type(self) -> &'static str {
        match self {
            System::Benches => "Bench",
            System::RollingBenches => "RollingBench",
            System::EbbFlow => "EbbFlowTable",
            System::Nft => "GrowingGutter",
            System::HighWire => "HighWireGutter",
        }
    }

    fn class(self) -> &'static str {
        if matches!(self, System::Nft | System::HighWire) { "IfcBuildingElementProxy" } else { "IfcFurniture" }
    }

    /// Its runs fall along their length.
    fn falls(self) -> bool {
        matches!(self, System::Nft | System::HighWire)
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
    /// The host: a glazed enclosure or a room (GlobalId).
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
    /// NFT channels' and high-wire gutters' fall along their run (rise over run).
    pub slope: f64,
    /// High-wire: the crop wire's height above the floor.
    #[serde(default = "wire_height")]
    pub wire: f64,
    #[serde(default)]
    pub parts: Vec<String>,
}

fn wire_height() -> f64 {
    3.25
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
    /// High-wire: the crop wire's height (above the level).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wire: Option<f64>,
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
        System::HighWire => 0.12,
    };
    let mut out = Vec::new();
    if p.system == System::HighWire {
        // The gutter, the crop wire over it, and hangers at most 3 m apart between them.
        out.push(slab(a, b, w, t0 - depth, [t0, t1]));
        let wire = level_z + p.wire.unwrap_or(p.top[0] + 2.0);
        out.push(slab(a, b, WIRE, wire - WIRE, [wire, wire]));
        let steps = (l / 3.0).ceil().max(1.0) as usize;
        for k in 0..=steps {
            let s = (l * k as f64 / steps as f64).clamp(HANGER, l - HANGER);
            let z = t0 + (t1 - t0) * s / l;
            let q = at(s, 0.0);
            out.push(slab([q[0] - HANGER / 2.0, q[1]], [q[0] + HANGER / 2.0, q[1]], HANGER, z + 0.001, [wire - WIRE - 0.001, wire - WIRE - 0.001]));
        }
        return out;
    }
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
    let gutter_lines: Vec<f64> = if post > 0.0 { e.gutter_lines() } else { Vec::new() };
    let (s, c) = e.rotation.sin_cos();
    let at = |x: f64, y: f64| [e.origin[0] + c * x - s * y, e.origin[1] + s * x + c * y];
    let rolling = r.system == System::RollingBenches;
    let mut out = Vec::new();
    // The crop wire under the gutter beams.
    let wire = r.wire.min(if e.form.tunnel() { e.ridge() - 1.0 } else { e.eave - 0.4 });
    let mut push = |a: [f64; 2], b: [f64; 2], len: f64| pieces(r, level, 0.0, wire, a, b, len, &mut out);
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

/// A run from `a` to `b` (`len` long) as pieces: NFT runs no longer than 12 m, each falling
/// from its feed end; high-wire gutters falling to their drain end, under the crop wire at
/// `wire`. Heights are above the level, the floor `base` above it.
#[allow(clippy::too_many_arguments)]
fn pieces(r: &LayoutRecipe, level: &str, base: f64, wire: f64, a: [f64; 2], b: [f64; 2], len: f64, out: &mut Vec<PartRecipe>) {
    let n = if r.system == System::Nft { (len / NFT_RUN).ceil().max(1.0) as usize } else { 1 };
    for k in 0..n {
        let f0 = k as f64 / n as f64;
        let f1 = (k + 1) as f64 / n as f64;
        let p0 = [a[0] + (b[0] - a[0]) * f0, a[1] + (b[1] - a[1]) * f0];
        let p1 = [a[0] + (b[0] - a[0]) * f1, a[1] + (b[1] - a[1]) * f1];
        let run = len / n as f64;
        let fall = if r.system.falls() { r.slope * run } else { 0.0 };
        let top = base + r.height;
        out.push(PartRecipe {
            kind: PART_RECIPE.into(),
            system: r.system,
            level: level.to_owned(),
            start: p0,
            end: p1,
            width: r.width,
            top: [top, top - fall],
            wire: (r.system == System::HighWire).then_some(base + wire),
        });
    }
}

/// The intervals of the line `y` (in a frame) inside `rings` (even-odd), sorted.
fn scan(rings: &[Vec<[f64; 2]>], y: f64) -> Vec<(f64, f64)> {
    let mut xs = Vec::new();
    for ring in rings {
        for k in 0..ring.len() {
            let (p, q) = (ring[k], ring[(k + 1) % ring.len()]);
            if (p[1] > y) != (q[1] > y) {
                xs.push(p[0] + (q[0] - p[0]) * (y - p[1]) / (q[1] - p[1]));
            }
        }
    }
    xs.sort_by(f64::total_cmp);
    xs.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0], c[1])).collect()
}

/// The overlap of two interval lists.
fn overlap(a: &[(f64, f64)], b: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for (a0, a1) in a {
        for (b0, b1) in b {
            let (lo, hi) = (a0.max(*b0), a1.min(*b1));
            if hi > lo {
                out.push((lo, hi));
            }
        }
    }
    out
}

/// The pieces of a layout over a room's floor: `rings` its boundary (outer first, then holes)
/// in plan, the floor `base` above the level, the crop wire under its top `height`.
pub fn room_plan(r: &LayoutRecipe, rings: &[Vec<[f64; 2]>], level: &str, base: f64, height: f64) -> Vec<PartRecipe> {
    let Some(outer) = rings.first().filter(|o| o.len() >= 3) else { return Vec::new() };
    // A frame along the longest edge, swapped so the pieces always run along its x axis.
    let k = (0..outer.len())
        .max_by(|i, j| {
            let d = |i: usize| (outer[(i + 1) % outer.len()][0] - outer[i][0]).hypot(outer[(i + 1) % outer.len()][1] - outer[i][1]);
            d(*i).total_cmp(&d(*j))
        })
        .unwrap_or(0);
    let (o, q) = (outer[k], outer[(k + 1) % outer.len()]);
    let mut angle = (q[1] - o[1]).atan2(q[0] - o[0]);
    if r.direction == Direction::Across {
        angle += std::f64::consts::FRAC_PI_2;
    }
    let (s, c) = angle.sin_cos();
    let to = |p: [f64; 2]| [c * (p[0] - o[0]) + s * (p[1] - o[1]), -s * (p[0] - o[0]) + c * (p[1] - o[1])];
    let back = |x: f64, y: f64| [o[0] + c * x - s * y, o[1] + s * x + c * y];
    let local: Vec<Vec<[f64; 2]>> = rings.iter().map(|ring| ring.iter().map(|p| to(*p)).collect()).collect();
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for p in &local[0] {
        (x0, x1, y0, y1) = (x0.min(p[0]), x1.max(p[0]), y0.min(p[1]), y1.max(p[1]));
    }
    let (w, cl) = (r.width, r.clearance);
    let mid = (x0 + x1) / 2.0;
    let aisle = [(f64::MIN, mid - r.main_aisle / 2.0), (mid + r.main_aisle / 2.0, f64::MAX)];
    let wire = r.wire.min(height - 0.3);
    let mut out = Vec::new();
    for y in rows(y0 + cl, y1 - cl, w, r.aisle, r.system == System::RollingBenches) {
        // Inside along both long edges of the piece (with the clearance) and its centre line.
        let inside = overlap(&overlap(&scan(&local, y - w / 2.0 - cl), &scan(&local, y + w / 2.0 + cl)), &scan(&local, y));
        for (a, b) in overlap(&inside.iter().map(|(a, b)| (a + cl, b - cl)).collect::<Vec<_>>(), &aisle) {
            if b - a > 0.3 {
                pieces(r, level, base, wire, back(a, y), back(b, y), b - a, &mut out);
            }
        }
    }
    out
}

/// A room's floor: its level, boundary rings, and its floor and height above the level.
struct RoomFloor {
    level: String,
    rings: Vec<Vec<[f64; 2]>>,
    base: f64,
    height: f64,
}

fn room_floor(store: &Store, e: &Element) -> Option<RoomFloor> {
    let rr = crate::rooms::room_recipe(e)?;
    let g = crate::rooms::regenerate(store, e).ok()?;
    let region = g.region.as_ref()?;
    let mut rings = vec![region.outer.iter().map(|x| x.line.start).collect::<Vec<_>>()];
    rings.extend(region.holes.iter().map(|h| h.iter().map(|x| x.line.start).collect()));
    let level_z = crate::levels::elevation_of(store, &rr.level).unwrap_or(0.0);
    let (base, top) = (g.z_base - level_z, g.z_top - level_z);
    Some(RoomFloor { level: rr.level, rings, base, height: top - base })
}

/// Makes a layout's pieces from its recipe.
fn build(txn: &mut Transaction<'_>, guid: &str, r: &LayoutRecipe) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let owner = GlobalId::parse(&r.enclosure)
        .and_then(|g| store.find_by_guid(&g))
        .and_then(|id| store.get(id))
        .ok_or_else(|| Error::new("growing.no_enclosure", "the layout's enclosure or room no longer exists"))?;
    let planned = if let Some(e) = enclosures::recipe(owner) {
        plan(r, &e, &e.level)
    } else if let Some(f) = room_floor(&store, owner) {
        room_plan(r, &f.rings, &f.level, f.base, f.height)
    } else {
        return Err(Error::new("growing.no_enclosure", "growing layouts go in a glazed enclosure or an enclosed room"));
    };
    let mut ids = Vec::new();
    for p in planned {
        ids.push(txn.create(
            NewElement::new(r.system.class())
                .name(match r.system {
                    System::Benches => "Bench",
                    System::RollingBenches => "Rolling Bench",
                    System::EbbFlow => "Ebb and Flow Table",
                    System::Nft => "NFT Table",
                    System::HighWire => "High-Wire Gutter",
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
    /// NFT and high-wire: the fall, rise over run (default 0.02; high-wire 0.005).
    pub slope: Option<f64>,
    /// High-wire: the crop wire's height above the floor (default 3.25 m).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub wire: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LayoutParams {
    /// The glazed enclosure or enclosed room to fill (`enclosure_id`, the older name, too).
    pub host_id: Option<ElementRef>,
    pub enclosure_id: Option<ElementRef>,
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
        // Hanging gutters 250 mm wide, one per row 1.25 m apart (E-55).
        System::HighWire => (0.25, 1.0, 0.6),
    };
    let (direction, slope) = if system == System::HighWire { (Direction::Along, 0.005) } else { (Direction::Across, 0.02) };
    LayoutRecipe { kind: RECIPE.into(), enclosure, system, direction, width, aisle, main_aisle: 1.2, clearance: 0.2, height, slope, wire: wire_height(), parts: Vec::new() }
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
    if let Some(v) = s.wire {
        r.wire = v;
    }
    let bad = |m: &str| Err(Error::new("growing.bad_value", m.to_owned()));
    if !(0.1..=5.0).contains(&r.width) {
        return bad("a piece is 0.1 to 5 m wide");
    }
    if r.system == System::HighWire && !(0.001..=0.02).contains(&r.slope) {
        return bad("high-wire gutters fall 0.1 to 2 %");
    }
    if r.system == System::HighWire && !(r.wire > r.height + 0.5 && r.wire <= 10.0) {
        return bad("the crop wire is at least 0.5 m above the gutter and at most 10 m high");
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
    let host = p.host_id.as_ref().or(p.enclosure_id.as_ref()).ok_or_else(|| Error::new("growing.no_enclosure", "give the host_id: a glazed enclosure or a room"))?;
    let owner = store
        .resolve(host)
        .filter(|id| store.get(*id).is_some_and(|e| enclosures::recipe(e).is_some() || crate::rooms::room_recipe(e).is_some()))
        .ok_or_else(|| Error::new("growing.no_enclosure", "growing layouts go in a glazed enclosure or a room"))?;
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
        return Err(Error::new("growing.nothing_fits", "nothing of that size fits there"));
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
    /// NFT and high-wire: the longest run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longest_run: Option<f64>,
}

/// `growing.summary`.
pub(crate) fn summary(store: &Store, p: SummaryParams) -> Result<Summary> {
    let (_, r) = resolve(store, &p.layout_id)?;
    let parts: Vec<PartRecipe> = r.parts.iter().filter_map(|g| store.get(store.find_by_guid(&GlobalId::parse(g)?)?)).filter_map(part_recipe).collect();
    let host = GlobalId::parse(&r.enclosure).and_then(|g| store.get(store.find_by_guid(&g)?));
    let floor = host.map_or(0.0, |h| match enclosures::recipe(h) {
        Some(e) => e.width() * e.length,
        None => crate::rooms::regenerate(store, h).map_or(0.0, |g| g.area()),
    });
    let area = growing_area(&parts);
    let longest = r.system.falls().then(|| parts.iter().map(|p| (p.end[0] - p.start[0]).hypot(p.end[1] - p.start[1])).fold(0.0, f64::max));
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
