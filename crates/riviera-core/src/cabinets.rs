// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Cabinet runs and countertops (ARC-KIT-001/002, `KITCHENS_PLAN.md` §4.1–4.2; the counter tool
//! `INTERIORS_PLAN.md` §4.4 builds on). A run is drawn along a wall from a start to an end point
//! (its back line) and filled with base, wall or tall units in the region's standard widths, a
//! filler taking up what is left; its front faces away from the wall. A countertop covers a run's
//! base units with an overhang at the front.
//!
//! Units are `IfcFurniture` USERDEFINED (BaseCabinet, WallCabinet, TallCabinet, Filler), built in
//! (`Pset_FurnitureTypeCommon.IsBuiltIn`), so the Casework category picks them up; a countertop
//! is an `IfcSystemFurnitureElement` WORKSURFACE (the owner's accepted mapping, §4.3).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::bath_components::Shape;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::mesh_objects::Category;
use crate::store::Store;
use crate::txn::Transaction;

/// The tag on a run's units: `{run, kind, index}`.
pub const RUN_KEY: &str = "rxd:cabinet_run";
const IN: f64 = 0.0254;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Base,
    Wall,
    Tall,
}

/// Whose standard sizes: US (inches, research/14 A4) or EU (EN 1116 and common practice).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Region {
    Us,
    Eu,
}

/// A kind of unit's sizes in a region (metres).
struct Spec {
    depth: f64,
    height: f64,
    /// A wall unit's bottom above the floor; 0 for others.
    bottom: f64,
    /// Toe kick (height, depth) under base and tall units.
    toe: Option<(f64, f64)>,
    /// Standard widths, widest first; the first is the one runs are filled with.
    widths: Vec<f64>,
    /// A countertop's thickness.
    top: f64,
}

fn spec(region: Region, kind: Kind) -> Spec {
    let inch_widths = |from: u32, to: u32| (from..=to).rev().step_by(3).map(|w| f64::from(w) * IN).collect::<Vec<_>>();
    match (region, kind) {
        // 34½ in high, 24 in deep, 9–48 in in 3 in steps; a 4 × 3 in toe kick [U] (S1199, S1200).
        (Region::Us, Kind::Base) => Spec { depth: 24.0 * IN, height: 34.5 * IN, bottom: 0.0, toe: Some((4.0 * IN, 3.0 * IN)), widths: preferred(36.0 * IN, inch_widths(9, 48)), top: 1.5 * IN },
        // 12 in deep, 30 in high, 18 in above a 36 in counter (S1199, Whirlpool sheet).
        (Region::Us, Kind::Wall) => Spec { depth: 12.0 * IN, height: 30.0 * IN, bottom: 54.0 * IN, toe: None, widths: preferred(36.0 * IN, inch_widths(9, 48)), top: 1.5 * IN },
        // 24 in deep, 84 in high, 18–36 in wide.
        (Region::Us, Kind::Tall) => Spec { depth: 24.0 * IN, height: 84.0 * IN, bottom: 0.0, toe: Some((4.0 * IN, 3.0 * IN)), widths: preferred(24.0 * IN, inch_widths(18, 36)), top: 1.5 * IN },
        // A 720 mm carcass on a 150 mm plinth under a 40 mm worktop: 910 mm, within EN 1116's 900
        // (+50) [U]; 600 mm modules [U] (S1322–S1324).
        (Region::Eu, Kind::Base) => Spec { depth: 0.6, height: 0.87, bottom: 0.0, toe: Some((0.15, 0.06)), widths: preferred(0.6, vec![1.2, 1.0, 0.9, 0.8, 0.6, 0.5, 0.45, 0.4, 0.3, 0.2, 0.15]), top: 0.04 },
        // 350 mm deep, 720 mm high, 550 mm above the worktop (50–65 cm, Konsument [S]).
        (Region::Eu, Kind::Wall) => Spec { depth: 0.35, height: 0.72, bottom: 1.46, toe: None, widths: preferred(0.6, vec![1.2, 1.0, 0.9, 0.8, 0.6, 0.5, 0.45, 0.4, 0.3, 0.2, 0.15]), top: 0.04 },
        (Region::Eu, Kind::Tall) => Spec { depth: 0.6, height: 2.1, bottom: 0.0, toe: Some((0.15, 0.06)), widths: preferred(0.6, vec![0.6, 0.5, 0.45, 0.4]), top: 0.04 },
    }
}

/// The widths, the preferred one first, then the rest widest first.
fn preferred(p: f64, mut all: Vec<f64>) -> Vec<f64> {
    all.sort_by(|a, b| b.total_cmp(a));
    all.retain(|w| (w - p).abs() > 1e-9);
    std::iter::once(p).chain(all).collect()
}

/// Fills a run with the least filler: as many of the preferred width as fit (or up to two
/// fewer), then one or two standard widths in what is left; ties keep more preferred units, then
/// fewer units. A filler under 3 mm is dropped.
pub fn fill(length: f64, widths: &[f64]) -> (Vec<f64>, f64) {
    let p = widths[0];
    let n = ((length + 1e-9) / p).floor() as usize;
    let mut best: Option<((i64, i64, usize), Vec<f64>)> = None;
    for k in (n.saturating_sub(2)..=n).rev() {
        let rest = length - p * k as f64;
        let mut tails: Vec<Vec<f64>> = vec![Vec::new()];
        for (i, a) in widths.iter().enumerate() {
            if *a <= rest + 1e-9 {
                tails.push(vec![*a]);
                tails.extend(widths[i..].iter().filter(|b| *a + **b <= rest + 1e-9).map(|b| vec![a.max(*b), a.min(*b)]));
            }
        }
        for tail in tails {
            let left = rest - tail.iter().sum::<f64>();
            let score = ((left * 1e4).round() as i64, -(k as i64), tail.len());
            if best.as_ref().is_none_or(|(s, _)| score < *s) {
                best = Some((score, std::iter::repeat_n(p, k).chain(tail).collect()));
            }
        }
    }
    let out = best.map(|(_, w)| w).unwrap_or_default();
    let filler = length - out.iter().sum::<f64>();
    (out, if filler < 0.003 { 0.0 } else { filler })
}

fn label(region: Region, w: f64) -> String {
    match region {
        Region::Us => format!("{:.0} in", w / IN),
        Region::Eu => format!("{:.0}", w * 1000.0),
    }
}

fn kind_name(kind: Kind) -> (&'static str, &'static str) {
    match kind {
        Kind::Base => ("Base cabinet", "BaseCabinet"),
        Kind::Wall => ("Wall cabinet", "WallCabinet"),
        Kind::Tall => ("Tall cabinet", "TallCabinet"),
    }
}

fn shape(w: f64, s: &Spec) -> Shape {
    let (x, y) = ([-w / 2.0, w / 2.0], [-s.depth / 2.0, s.depth / 2.0]);
    match s.toe {
        Some((h, d)) => Shape::default().cuboid(x, [y[0] + d, y[1]], [0.0, h]).cuboid(x, y, [h, s.height]),
        None => Shape::default().cuboid(x, y, [0.0, s.height]),
    }
}

/// A generated type of this name, made once (built in, with its nominal sizes).
fn unit_type(txn: &mut Transaction<'_>, name: &str, category: Category, predefined: &str, kind: &str, sh: &Shape, size: [f64; 3], color: [f32; 4]) -> Result<ElementId> {
    if let Some(e) = txn.store().iter().find(|e| e.name.as_deref() == Some(name) && crate::mesh_objects::mesh_object_type(e).is_some()) {
        return Ok(e.id());
    }
    let id = crate::mesh_objects::create_generated_type(txn, name, category, &sh.p, &sh.t, color, Vec::new(), Some(predefined))?;
    let len = |v: f64| json!({ "type": "IfcPositiveLengthMeasure", "value": v });
    let kind = kind.to_owned();
    txn.modify(id, |e| {
        if predefined == "USERDEFINED" {
            e.params.insert("@ElementType".into(), kind.clone().into());
        }
        e.params.insert("Pset_FurnitureTypeCommon.IsBuiltIn".into(), json!({ "type": "IfcBoolean", "value": true }));
        e.params.insert("Pset_FurnitureTypeCommon.NominalLength".into(), len(size[0]));
        e.params.insert("Pset_FurnitureTypeCommon.NominalDepth".into(), len(size[1]));
        e.params.insert("Pset_FurnitureTypeCommon.NominalHeight".into(), len(size[2]));
    })?;
    Ok(id)
}

const CARCASS: [f32; 4] = [0.86, 0.84, 0.80, 1.0];
const WORKTOP: [f32; 4] = [0.45, 0.45, 0.47, 1.0];

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunParams {
    /// The run's back line, along the wall: from here …
    pub start: [f64; 2],
    /// … to here.
    pub end: [f64; 2],
    pub level: ElementRef,
    pub kind: Kind,
    /// Default from the building code: US for the IBC and IRC, else EU.
    pub region: Option<Region>,
    /// The units' widths in order (metres); default filled from the standard widths.
    pub widths: Option<Vec<f64>>,
    /// A point in front of the run; default away from the wall behind it.
    pub toward: Option<[f64; 2]>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Unit {
    pub element: ElementId,
    pub width: f64,
    pub filler: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Run {
    /// The run's id (its units' tag).
    pub run: String,
    pub region: Region,
    pub length: f64,
    pub units: Vec<Unit>,
    /// The filler's width (0 when none).
    pub filler: f64,
}

/// Whether a plan point is inside a wall's footprint at this height.
fn in_wall(store: &crate::Store, p: [f64; 2], z: f64) -> bool {
    store.iter().filter(|e| crate::walls::wall_recipe(e).is_some()).any(|w| {
        let Ok(g) = crate::walls::regenerate(store, w) else { return false };
        if z < g.base_z || z > g.base_z + g.height {
            return false;
        }
        let f = g.footprint();
        let mut inside = false;
        for k in 0..f.len() {
            let (a, b) = (f[k], f[(k + 1) % f.len()]);
            if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
                inside = !inside;
            }
        }
        inside
    })
}

/// Lays out a run of cabinets (`cabinet.run`).
pub(crate) fn run(txn: &mut Transaction<'_>, p: RunParams) -> Result<Run> {
    let store = txn.store().clone();
    let level = store.resolve(&p.level).and_then(|l| store.get(l)).ok_or_else(|| bad("element.not_found", "no such level"))?;
    let z0 = crate::levels::elevation(level);
    let region = p.region.unwrap_or(match crate::codes::settings(&store).code {
        crate::codes::Code::Ibc | crate::codes::Code::Irc => Region::Us,
        _ => Region::Eu,
    });
    let s = spec(region, p.kind);
    let v = [p.end[0] - p.start[0], p.end[1] - p.start[1]];
    let length = v[0].hypot(v[1]);
    if length < 0.1 {
        return Err(bad("cabinet.too_short", "a run needs a start and an end at least 100 mm apart"));
    }
    let d = [v[0] / length, v[1] / length];
    let left = [-d[1], d[0]];
    let mid = [p.start[0] + v[0] / 2.0, p.start[1] + v[1] / 2.0];
    // Its front: toward the point given, else away from a wall just behind the line.
    let sign = match p.toward {
        Some(q) => ((q[0] - mid[0]) * left[0] + (q[1] - mid[1]) * left[1]).signum(),
        None => {
            // Probed 50 mm to each side, so a line on a wall's face is behind on one only.
            let wall = |s: f64| {
                let q = [mid[0] + left[0] * s * 0.05, mid[1] + left[1] * s * 0.05];
                in_wall(&store, q, z0 + 0.5) || crate::bath_rules::ray_to_walls(&store, q, [left[0] * s, left[1] * s], z0 + 0.5, 0.25).is_some()
            };
            if wall(1.0) && !wall(-1.0) { -1.0 } else { 1.0 }
        }
    };
    let front = [left[0] * sign, left[1] * sign];
    let rotation = front[0].atan2(-front[1]);
    let (widths, filler) = match &p.widths {
        Some(ws) => {
            let sum: f64 = ws.iter().sum();
            if ws.iter().any(|w| *w <= 0.0) || sum > length + 1e-6 {
                return Err(bad("cabinet.too_long", format!("the units are {sum:.3} m, longer than the run's {length:.3} m")));
            }
            (ws.clone(), if length - sum < 0.003 { 0.0 } else { length - sum })
        }
        None => fill(length, &s.widths),
    };
    let (name, kind) = kind_name(p.kind);
    let run_id = crate::id::GlobalId::new().as_str().to_owned();
    let mut units = Vec::new();
    let mut at = 0.0;
    let pieces: Vec<(f64, bool)> = widths.iter().map(|w| (*w, false)).chain((filler > 0.0).then_some((filler, true))).collect();
    for (k, (w, is_filler)) in pieces.into_iter().enumerate() {
        let sh = if is_filler { Shape::default().cuboid([-w / 2.0, w / 2.0], [-s.depth / 2.0, -s.depth / 2.0 + 0.02], [s.toe.map_or(0.0, |t| t.0), s.height]) } else { shape(w, &s) };
        let ty_name = if is_filler { format!("Filler {} × {}", label(region, w), label(region, s.height)) } else { format!("{name} {}", label(region, w)) };
        let ty = unit_type(txn, &ty_name, Category::Furniture, "USERDEFINED", if is_filler { "Filler" } else { kind }, &sh, [w, s.depth, s.height], CARCASS)?;
        let c = at + w / 2.0;
        let point = [p.start[0] + d[0] * c + front[0] * s.depth / 2.0, p.start[1] + d[1] * c + front[1] * s.depth / 2.0];
        let id = crate::mesh_objects::place(txn, crate::mesh_objects::PlaceParams { type_id: ElementRef::Id(ty), point, level: p.level.clone(), offset: s.bottom, rotation, mirror: false, scale: 1.0 })?;
        let tag = json!({ "run": run_id, "kind": p.kind, "index": k, "region": region });
        txn.modify(id, |e| {
            e.params.insert(RUN_KEY.into(), tag);
        })?;
        units.push(Unit { element: id, width: w, filler: is_filler });
        at += w;
    }
    Ok(Run { run: run_id, region, length, units, filler })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TopsParams {
    /// Runs (their ids), or any of their units, to top; every run's base units when both are
    /// empty.
    #[serde(default)]
    pub runs: Vec<String>,
    #[serde(default)]
    pub element_ids: Vec<ElementRef>,
    /// Past the units' fronts (default 1 in US, 20 mm EU).
    #[schemars(extend("x-riviera-data-type" = "length"))]
    pub overhang: Option<f64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Top {
    pub element: ElementId,
    pub run: String,
    pub length: f64,
    pub depth: f64,
    /// Its top above the level.
    pub height: f64,
}

fn tag(e: &crate::element::Element) -> Option<(String, Kind, Region)> {
    let v = e.param(RUN_KEY)?;
    Some((v.get("run")?.as_str()?.to_owned(), serde_json::from_value(v.get("kind")?.clone()).ok()?, serde_json::from_value(v.get("region")?.clone()).ok()?))
}

/// Tops runs of base units (`countertop.from_runs`): one top over each run, along its units from
/// the first to the last, overhanging their fronts.
pub(crate) fn tops(txn: &mut Transaction<'_>, p: TopsParams) -> Result<Vec<Top>> {
    let store = txn.store().clone();
    let mut runs: Vec<String> = p.runs.clone();
    for r in &p.element_ids {
        let id = store.resolve(r).ok_or_else(|| bad("element.not_found", format!("no element {r:?}")))?;
        let (run, _, _) = store.get(id).and_then(tag).ok_or_else(|| bad("countertop.not_a_unit", format!("element {id} is not a cabinet run's unit")))?;
        runs.push(run);
    }
    if runs.is_empty() {
        runs = store.iter().filter_map(tag).filter(|t| t.1 == Kind::Base).map(|t| t.0).collect();
    }
    runs.sort();
    runs.dedup();
    let mut out = Vec::new();
    for run in runs {
        let units: Vec<(crate::rules::Item, Region, String)> = store
            .iter()
            .filter_map(|e| tag(e).filter(|t| t.0 == run && t.1 == Kind::Base).map(|t| (e, t.2)))
            .filter_map(|(e, region)| Some((crate::rules::item(&store, e)?, region, crate::mesh_objects::mesh_object_recipe(e)?.level)))
            .collect();
        let Some((first, region, level)) = units.first().cloned() else {
            return Err(bad("countertop.no_base_units", format!("run {run} has no base units")));
        };
        let s = spec(region, Kind::Base);
        let overhang = p.overhang.unwrap_or(match region {
            Region::Us => 1.0 * IN,
            Region::Eu => 0.02,
        });
        // Along the run: the units' centres projected on its direction (their local x).
        let (c, sn) = (first.frame.rotation.cos(), first.frame.rotation.sin());
        let dir = [c, sn];
        let along = |q: [f64; 3]| q[0] * dir[0] + q[1] * dir[1];
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        for (u, _, _) in &units {
            let a = along(u.frame.origin);
            lo = lo.min(a + u.x[0]);
            hi = hi.max(a + u.x[1]);
        }
        let length = hi - lo;
        let depth = s.depth + overhang;
        let name = format!("Countertop {:.0} × {:.0}", length * 1000.0, depth * 1000.0);
        let sh = Shape::default().cuboid([-length / 2.0, length / 2.0], [-depth / 2.0, depth / 2.0], [0.0, s.top]);
        let ty = unit_type(txn, &name, Category::SystemFurniture, "WORKSURFACE", "", &sh, [length, depth, s.top], WORKTOP)?;
        // Centred along the run; its back on the units' backs, its front past theirs.
        let o = first.frame.origin;
        let back = [-sn, c];
        let shift = (lo + hi) / 2.0 - along(o);
        let centre = [o[0] + dir[0] * shift - back[0] * overhang / 2.0, o[1] + dir[1] * shift - back[1] * overhang / 2.0];
        let height = first.z[1] - first.floor;
        let level_ref = crate::id::GlobalId::parse(&level).and_then(|g| store.find_by_guid(&g)).ok_or_else(|| bad("element.not_found", "the run's level"))?;
        let id = crate::mesh_objects::place(txn, crate::mesh_objects::PlaceParams { type_id: ElementRef::Id(ty), point: centre, level: ElementRef::Id(level_ref), offset: height, rotation: first.frame.rotation, mirror: false, scale: 1.0 })?;
        let tagv: Value = json!({ "run": run, "kind": "top", "region": region });
        txn.modify(id, |e| {
            e.params.insert(RUN_KEY.into(), tagv);
        })?;
        out.push(Top { element: id, run, length, depth, height: height + s.top });
    }
    Ok(out)
}

/// The runs in a model: their ids, kinds and units (`cabinet.runs`).
#[derive(Debug, Serialize, JsonSchema)]
pub struct RunInfo {
    pub run: String,
    pub kind: Value,
    pub units: Vec<ElementId>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunsParams {}

pub(crate) fn runs(store: &Store, _: Option<RunsParams>) -> Result<Vec<RunInfo>> {
    let mut out: Vec<RunInfo> = Vec::new();
    for e in store.iter() {
        let Some(v) = e.param(RUN_KEY) else { continue };
        let (Some(run), Some(kind)) = (v.get("run").and_then(Value::as_str), v.get("kind")) else { continue };
        // A countertop carries its run's id too; it is not a run of its own.
        if kind == "top" {
            continue;
        }
        match out.iter_mut().find(|r| r.run == run && &r.kind == kind) {
            Some(r) => r.units.push(e.id()),
            None => out.push(RunInfo { run: run.to_owned(), kind: kind.clone(), units: vec![e.id()] }),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_fill_with_the_least_filler() {
        let s = spec(Region::Us, Kind::Base);
        // 3.6 m: three 36 in units, a 33 in unit, and a 19 mm filler.
        let (w, f) = fill(3.6, &s.widths);
        assert_eq!(w.iter().map(|w| (w / IN).round() as u32).collect::<Vec<_>>(), [36, 36, 36, 33]);
        assert!((f - (3.6 - 141.0 * IN)).abs() < 1e-9, "{f}");
        // 3.8 m: a 39 in unit in place of the fourth 36 in leaves 2.6 in, not 5.6 in.
        let (w, f) = fill(3.8, &s.widths);
        assert_eq!(w.iter().map(|w| (w / IN).round() as u32).collect::<Vec<_>>(), [36, 36, 36, 39]);
        assert!((f - (3.8 - 147.0 * IN)).abs() < 1e-9, "{f}");
        // EU 2.5 m: three 600 units, a 500 and a 200, and no filler.
        let (w, f) = fill(2.5, &spec(Region::Eu, Kind::Base).widths);
        assert_eq!((w.iter().map(|w| (w * 1000.0).round() as u32).collect::<Vec<_>>(), f), (vec![600, 600, 600, 500, 200], 0.0));
        // A 0.8 m tall run: one 30 in unit rather than a 24 in and 190 mm of filler.
        let (w, _) = fill(0.8, &spec(Region::Us, Kind::Tall).widths);
        assert_eq!(w.iter().map(|w| (w / IN).round() as u32).collect::<Vec<_>>(), [30]);
    }
}
