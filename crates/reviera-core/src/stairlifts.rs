// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Stairlifts and inclined platform lifts (ARC-VT-013 … ARC-VT-015): a seated stairlift or a
//! wheelchair platform lift on one side of a Reviera stair (ARC-STAIR-*), sized from the
//! vertical-transport rule packs ([`crate::elevators`]), made as one custom object
//! (`IfcTransportElement` USERDEFINED "STAIRLIFT" or "INCLINEDPLATFORMLIFT") following the stair,
//! with its running envelope and landing zones, and the stair checks.
//!
//! Along a straight run, `s` is the plan distance from the run's first riser up the run, and the
//! side offset is measured in from the run's edge on the lift's side.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::clearances::{Clearance, ClearanceKind};
use crate::elevators::{Confidence, Layer, Pack, Severity, active, by_guid, measure};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::stairs::{Run, StairRecipe};
use crate::store::Store;
use crate::txn::Transaction;

type V2 = [f64; 2];

/// A stairlift's data, on it.
pub const DATA_KEY: &str = "rxd:stairlift";

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

// ---- Pack blocks ----

/// Rules for stairlifts and inclined platform lifts on a stair. Each pack gives the rules it has.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Rules {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Clear passage left beside the folded unit on a stair in a dwelling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dwelling_clear_width: Option<f64>,
    /// The width a stair elsewhere must keep beside the folded or parked unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_width: Option<f64>,
    /// The width kept on a stair serving fewer than `small_occupants`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub small_width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub small_occupants: Option<u32>,
    /// The guide rail is left out of the width: a unit that parks clear of the stair keeps it all.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rail_ignored: bool,
    /// No unit may stand in an exit enclosure.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_exit_enclosure: bool,
    /// Clear height above any point of a platform's deck along the run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_box: Option<f64>,
    /// The least platform, width × length.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform_min: Option<V2>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_speed: Option<f64>,
    /// The clear wheelchair space at each end of a platform lift, width × depth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wheelchair_space: Option<V2>,
    pub source: String,
    pub confidence: Confidence,
}

/// A generic seated stairlift.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Seated {
    /// From the wall (the stair's edge), folded and in use.
    pub folded: f64,
    pub unfolded: f64,
    /// How far the rail stands in from the wall.
    pub rail_depth: f64,
    pub seat_width: f64,
    /// The parked unit's height above the floor.
    pub parked_height: f64,
    /// The rider's envelope above the nosing line.
    pub envelope_height: f64,
    /// Along the stair, parked.
    pub carriage_length: f64,
    pub speed: f64,
    pub load: f64,
    pub min_incline: f64,
    pub max_incline: f64,
    /// The longest straight rail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<f64>,
    pub swivel_radius: f64,
    /// How far the seat travels past the top nosing.
    pub top_overrun: f64,
    pub supply: String,
    pub source: String,
}

/// One platform size of a family.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlatformSize {
    /// Width × length.
    pub platform: V2,
    pub folded: f64,
    pub unfolded: f64,
    /// The maker's recommended stair width for a straight run.
    pub stair_width: f64,
    /// The least width on the inside of a turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inside_bend: Option<f64>,
}

/// A family of inclined platform lifts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Family {
    pub id: String,
    pub description: String,
    /// Its rail can follow turns and landings.
    pub curved: bool,
    pub load: f64,
    pub speed: f64,
    pub min_incline: f64,
    pub max_incline: f64,
    pub rail_depth: f64,
    /// The highest point of the deck above the nosing line, by stair angle (degrees, metres).
    pub deck_rise: Vec<V2>,
    /// The platform's reach beyond the bottom nosing, by stair angle (degrees, metres).
    pub runout: Vec<V2>,
    /// Landing used beyond the top nosing.
    pub top_space: f64,
    /// The folded platform's height above the floor.
    pub folded_height: f64,
    /// Height × width × depth of a drive box at the top of the rail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drive_box: Option<[f64; 3]>,
    /// Loads on the wall at a support: F1 and F2 (kN), and the moment (kN·m).
    pub wall_loads: [f64; 3],
    pub supply: String,
    pub sizes: Vec<PlatformSize>,
    pub source: String,
}

/// Generic, maker-style stairlift sizes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Sizing {
    pub seated: Seated,
    pub platforms: Vec<Family>,
    pub source: String,
    pub confidence: Confidence,
}

fn sizing(packs: &[Pack]) -> Result<(&Pack, &Sizing)> {
    packs
        .iter()
        .find_map(|p| p.stairlift_sizing.as_ref().map(|s| (p, s)))
        .ok_or_else(|| bad("vt.no_sizing", "no rule pack in use gives stairlift sizes (add practice_us or practice_en)"))
}

/// The first value a pack in use gives.
fn rule<T>(packs: &[Pack], f: impl Fn(&Rules) -> Option<T>) -> Option<T> {
    packs.iter().filter_map(|p| p.stairlift.as_ref()).find_map(f)
}

/// Linear in the first column, held at the ends.
fn interp(t: &[V2], x: f64) -> f64 {
    let Some(first) = t.first() else { return 0.0 };
    if x <= first[0] {
        return first[1];
    }
    for w in t.windows(2) {
        if x <= w[1][0] {
            return w[0][1] + (w[1][1] - w[0][1]) * (x - w[0][0]) / (w[1][0] - w[0][0]);
        }
    }
    t.last().expect("not empty")[1]
}

// ---- The stair ----

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    #[default]
    Left,
    Right,
}

/// What the stair serves, for the width rules.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StairUse {
    /// A stair within a dwelling (US R-3, or within an R-2 unit).
    Dwelling,
    /// A stair in an exit enclosure (a protected escape stair).
    Exit,
    #[default]
    Other,
}

/// What a lift needs to know of its stair.
#[derive(Debug, Clone, PartialEq)]
pub struct StairFacts {
    pub recipe: StairRecipe,
    pub z_base: f64,
    pub riser: f64,
    pub tread: f64,
    /// Degrees.
    pub incline: f64,
    /// The narrowest run.
    pub width: f64,
    /// Its rail turns: winders, a spiral, or runs at an angle.
    pub turns: bool,
    /// Along the nosing line.
    pub length: f64,
}

fn stair_facts(store: &Store, stair: ElementId) -> Result<StairFacts> {
    let e = store.get(stair).ok_or_else(|| bad("element.not_found", "no such stair"))?;
    let recipe = crate::stairs::stair_recipe(e).ok_or_else(|| bad("vt.not_stair", format!("element {stair} is not a Reviera stair")))?;
    let g = crate::stairs::regenerate(store, e)?;
    let (z_base, _) = crate::stairs::elevations(store, e)?;
    let recipe = recipe.extended(g.risers);
    let (riser, tread) = (g.riser_height, recipe.tread_depth);
    let turns = recipe.runs.iter().any(|r| r.spiral.is_some() || r.winder.is_some())
        || recipe.runs.windows(2).any(|w| (w[0].direction[0] * w[1].direction[1] - w[0].direction[1] * w[1].direction[0]).abs() > 1e-6 || w[0].direction[0] * w[1].direction[0] + w[0].direction[1] * w[1].direction[1] < 0.0);
    let width = recipe.runs.iter().map(|r| r.width).fold(f64::INFINITY, f64::min);
    let length = recipe.runs.iter().map(|r| r.length(tread).hypot(f64::from(r.risers.saturating_sub(1)) * riser)).sum();
    Ok(StairFacts { incline: (riser / tread).atan().to_degrees(), recipe, z_base, riser, tread, width, turns, length })
}

// ---- Sizing ----

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A seated stairlift (a stairway chairlift).
    #[default]
    Seated,
    /// An inclined platform lift for a wheelchair.
    Platform,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Seated => "Stairlift",
            Self::Platform => "Platform Stairlift",
        }
    }

    /// IFC's object type for the user-defined transport element.
    pub fn object_type(self) -> &'static str {
        match self {
            Self::Seated => "STAIRLIFT",
            Self::Platform => "INCLINEDPLATFORMLIFT",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SizeParams {
    #[serde(default)]
    pub kind: Kind,
    /// A platform lift's family (`straight` or `curved`); omitted, curved where the stair turns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// A platform's width × length, one of its family's; omitted, the largest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<V2>,
    /// Rated speed (m/s); omitted, the generic model's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
}

/// A stairlift as sized.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Sized {
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    pub curved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<V2>,
    pub folded: f64,
    pub unfolded: f64,
    pub rail_depth: f64,
    /// The maker's recommended stair width, and the inside-bend width.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stair_width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inside_bend: Option<f64>,
    pub speed: f64,
    pub load: f64,
    /// The stair's angle, and the model's range.
    pub incline: f64,
    pub incline_range: V2,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<f64>,
    /// Along the nosing line.
    pub stair_length: f64,
    /// Beyond the bottom nosing, and the landing used beyond the top nosing.
    pub runout: f64,
    pub top_space: f64,
    /// The running envelope's height above the nosing line: a seated rider's, or a platform's
    /// deck rise plus the head box.
    pub envelope_height: f64,
    /// The landing zones' height: a seated rider's, or the head box over a deck at the floor.
    pub landing_height: f64,
    pub parked_height: f64,
    pub carriage_length: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drive_box: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_loads: Option<[f64; 3]>,
    pub swivel_radius: f64,
    pub supply: String,
    pub sizing_pack: String,
}

/// Sizes a stairlift for a stair under the packs in use.
pub fn size(packs: &[Pack], stair: &StairFacts, p: &SizeParams) -> Result<Sized> {
    let (pack, s) = sizing(packs)?;
    let a = stair.incline;
    let out = match p.kind {
        Kind::Seated => {
            let c = &s.seated;
            Sized {
                kind: Kind::Seated,
                family: None,
                curved: stair.turns,
                platform: None,
                folded: c.folded,
                unfolded: c.unfolded,
                rail_depth: c.rail_depth,
                stair_width: None,
                inside_bend: None,
                speed: p.speed.unwrap_or(c.speed),
                load: c.load,
                incline: a,
                incline_range: [c.min_incline, c.max_incline],
                max_length: c.max_length.filter(|_| !stair.turns),
                stair_length: stair.length,
                runout: c.carriage_length,
                top_space: c.top_overrun + c.swivel_radius,
                envelope_height: c.envelope_height,
                landing_height: c.envelope_height,
                parked_height: c.parked_height,
                carriage_length: c.carriage_length,
                drive_box: None,
                wall_loads: None,
                swivel_radius: c.swivel_radius,
                supply: c.supply.clone(),
                sizing_pack: pack.id.clone(),
            }
        }
        Kind::Platform => {
            let want = p.family.clone().unwrap_or_else(|| if stair.turns { "curved".into() } else { "straight".into() });
            let f = s.platforms.iter().find(|f| f.id == want).ok_or_else(|| {
                bad("vt.params", format!("no platform family {want}; there are {}", s.platforms.iter().map(|f| f.id.as_str()).collect::<Vec<_>>().join(", ")))
            })?;
            let size = match p.platform {
                None => f.sizes.first().ok_or_else(|| bad("vt.pack_range", format!("{} has no sizes", f.id)))?,
                Some(w) => f.sizes.iter().find(|z| (z.platform[0] - w[0]).abs() < 1e-3 && (z.platform[1] - w[1]).abs() < 1e-3).ok_or_else(|| {
                    let all: Vec<String> = f.sizes.iter().map(|z| format!("{:.0} × {:.0}", z.platform[0] * 1000.0, z.platform[1] * 1000.0)).collect();
                    bad("vt.params", format!("the {} family has no {:.0} × {:.0} mm platform; it has {}", f.id, w[0] * 1000.0, w[1] * 1000.0, all.join(", ")))
                })?,
            };
            let head_box = rule(packs, |r| r.head_box).unwrap_or(1.5);
            Sized {
                kind: Kind::Platform,
                family: Some(f.id.clone()),
                curved: f.curved,
                platform: Some(size.platform),
                folded: size.folded,
                unfolded: size.unfolded,
                rail_depth: f.rail_depth,
                stair_width: Some(size.stair_width),
                inside_bend: size.inside_bend,
                speed: p.speed.unwrap_or(f.speed),
                load: f.load,
                incline: a,
                incline_range: [f.min_incline, f.max_incline],
                max_length: None,
                stair_length: stair.length,
                runout: interp(&f.runout, a),
                top_space: f.top_space,
                envelope_height: interp(&f.deck_rise, a) + head_box,
                landing_height: head_box + 0.1,
                parked_height: f.folded_height,
                carriage_length: size.platform[1],
                drive_box: f.drive_box,
                wall_loads: Some(f.wall_loads),
                swivel_radius: 0.0,
                supply: f.supply.clone(),
                sizing_pack: pack.id.clone(),
            }
        }
    };
    Ok(out)
}

// ---- The object ----

/// A stairlift's data, kept on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Stairlift {
    #[serde(flatten)]
    pub sized: Sized,
    /// Its stair (GlobalId).
    pub stair: String,
    pub side: Side,
    /// It parks clear of the stair: a hinged or slide-track rail end, or round a corner.
    #[serde(default)]
    pub parks_clear: bool,
    pub stair_use: StairUse,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occupants: Option<u32>,
    /// The rail's length as made.
    pub rail_length: f64,
}

pub fn stairlift(e: &crate::element::Element) -> Option<Stairlift> {
    serde_json::from_value(e.param(DATA_KEY)?.clone()).ok()
}

pub fn stairlifts(store: &Store) -> Vec<(ElementId, Stairlift)> {
    store.iter().filter_map(|e| Some((e.id(), stairlift(e)?))).collect()
}

fn left(d: V2) -> V2 {
    [-d[1], d[0]]
}

/// A point on a straight run: `s` up it from its first riser, `inset` in from its edge on `side`.
fn on_run(run: &Run, side: Side, s: f64, inset: f64) -> V2 {
    let d = run.direction;
    let n = left(d);
    let o = if side == Side::Left { run.width / 2.0 - inset } else { inset - run.width / 2.0 };
    [run.start[0] + d[0] * s + n[0] * o, run.start[1] + d[1] * s + n[1] * o]
}

/// A plan quad on a straight run between `s0..s1` and insets `i0..i1`, counter-clockwise.
fn quad(run: &Run, side: Side, s: V2, i: V2) -> Vec<V2> {
    let mut q = vec![on_run(run, side, s[0], i[0]), on_run(run, side, s[1], i[0]), on_run(run, side, s[1], i[1]), on_run(run, side, s[0], i[1])];
    let area: f64 = (0..4).map(|k| q[k][0] * q[(k + 1) % 4][1] - q[(k + 1) % 4][0] * q[k][1]).sum();
    if area < 0.0 {
        q.reverse();
    }
    q
}

/// The run's start elevation and its nosing line's height at `p`.
fn nosing(run: &Run, z0: f64, riser: f64, tread: f64, p: V2) -> f64 {
    let s = (p[0] - run.start[0]) * run.direction[0] + (p[1] - run.start[1]) * run.direction[1];
    z0 + riser + s * riser / tread
}

/// Each run's start elevation.
fn run_bases(st: &StairFacts) -> Vec<f64> {
    let mut done = 0u32;
    st.recipe
        .runs
        .iter()
        .map(|r| {
            let z = st.z_base + f64::from(done) * st.riser;
            done += r.risers;
            z
        })
        .collect()
}

fn append(out: &mut crate::bodies::Mesh, m: crate::bodies::Mesh) {
    let base = out.0.len() as u32;
    out.0.extend(m.0);
    out.1.extend(m.1.into_iter().map(|i| i + base));
}

/// A box along the segment `a`→`b` (plan, with heights), `half` either side of it, from `lo` to
/// `hi` above the line.
fn rail_piece(a: [f64; 3], b: [f64; 3], half: f64, lo: f64, hi: f64) -> Option<crate::bodies::Mesh> {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l = d[0].hypot(d[1]);
    if l < 1e-6 {
        return None;
    }
    let u = [d[0] / l, d[1] / l];
    let n = left(u);
    let ring = vec![[a[0] - n[0] * half, a[1] - n[1] * half], [b[0] - n[0] * half, b[1] - n[1] * half], [b[0] + n[0] * half, b[1] + n[1] * half], [a[0] + n[0] * half, a[1] + n[1] * half]];
    let z = move |p: V2| a[2] + (b[2] - a[2]) * (((p[0] - a[0]) * u[0] + (p[1] - a[1]) * u[1]) / l).clamp(0.0, 1.0);
    Some(crate::bodies::extrude_between(&ring, &[], &|p| z(p) + lo, &|p| z(p) + hi))
}

/// The rail's path: along the stair's side, down to the floor before the first riser and on
/// beyond the top nosing.
fn rail_path(st: &StairFacts, el: &Stairlift) -> Vec<[f64; 3]> {
    let s = &el.sized;
    let inset = s.rail_depth / 2.0;
    let mut pts = crate::stairs::side_path(&st.recipe, st.z_base, st.riser, el.side == Side::Left, inset);
    let (Some(first), Some(last)) = (st.recipe.runs.first(), st.recipe.runs.last()) else { return pts };
    let bottom = on_run(first, el.side, -s.runout.min(2.0), inset);
    pts.insert(0, [bottom[0], bottom[1], st.z_base]);
    let d = last.end_direction(st.tread);
    if let Some(&end) = pts.last() {
        let reach = if s.kind == Kind::Seated { s.top_space - s.swivel_radius } else { s.top_space };
        pts.push([end[0] + d[0] * reach, end[1] + d[1] * reach, end[2]]);
    }
    pts
}

fn path_length(p: &[[f64; 3]]) -> f64 {
    p.windows(2).map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2) + (w[1][2] - w[0][2]).powi(2)).sqrt()).sum()
}

/// The rail (two tubes for a platform lift), the parked unit at the bottom, and a platform's
/// drive box at the top, in world coordinates.
fn mesh(st: &StairFacts, el: &Stairlift) -> crate::bodies::Mesh {
    let s = &el.sized;
    let mut out: crate::bodies::Mesh = (Vec::new(), Vec::new());
    let path = rail_path(st, el);
    let tubes: &[(f64, f64)] = match s.kind {
        Kind::Seated => &[(0.10, 0.22)],
        Kind::Platform => &[(0.47, 0.53), (1.07, 1.13)],
    };
    let half = (s.rail_depth / 2.0 - 0.01).max(0.025);
    for w in path.windows(2) {
        for &(lo, hi) in tubes {
            if let Some(m) = rail_piece(w[0], w[1], half, lo, hi) {
                append(&mut out, m);
            }
        }
    }
    let (Some(first), Some(last)) = (st.recipe.runs.first(), st.recipe.runs.last()) else { return out };
    // Parked at the bottom, on the floor before the first riser.
    let ring = quad(first, el.side, [-s.carriage_length - 0.05, -0.05], [0.0, s.folded]);
    append(&mut out, crate::bodies::extrude(&ring, &[], st.z_base + 0.05, st.z_base + s.parked_height));
    if let Some([h, w, d]) = s.drive_box {
        let bases = run_bases(st);
        let z = bases.last().copied().unwrap_or(st.z_base) + f64::from(last.risers) * st.riser;
        let l = last.length(st.tread);
        if last.spiral.is_none() && last.winder.is_none() {
            let ring = quad(last, el.side, [l + s.top_space - w, l + s.top_space], [0.0, d]);
            append(&mut out, crate::bodies::extrude(&ring, &[], z, z + h));
        }
    }
    out
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// The stair (a Reviera stair).
    pub stair: ElementRef,
    /// The side going up.
    #[serde(default)]
    pub side: Side,
    #[serde(default)]
    pub parks_clear: bool,
    #[serde(default)]
    pub stair_use: StairUse,
    /// The people the stair serves (for the smaller required width).
    #[serde(default)]
    pub occupants: Option<u32>,
    #[serde(flatten)]
    pub size: SizeParams,
}

/// What `stairlift.create` made.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Created {
    pub stairlift: ElementId,
    #[serde(flatten)]
    pub data: Stairlift,
}

/// Makes a stairlift or platform lift on a stair (`stairlift.create`).
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<Created> {
    let store = txn.store().clone();
    let stair = store.resolve(&p.stair).ok_or_else(|| bad("element.not_found", "no such stair"))?;
    let st = stair_facts(&store, stair)?;
    let sized = size(&active(&store), &st, &p.size)?;
    let level = by_guid(&store, &st.recipe.base_level).ok_or_else(|| bad("vt.no_level", "the stair's base level does not exist"))?;
    let elevation = store.get(level).and_then(|l| l.param("@Elevation")).and_then(Value::as_f64).unwrap_or(0.0);
    let mut el = Stairlift {
        sized,
        stair: store.get(stair).expect("resolved").guid().to_string(),
        side: p.side,
        parks_clear: p.parks_clear,
        stair_use: p.stair_use,
        occupants: p.occupants,
        rail_length: 0.0,
    };
    el.rail_length = path_length(&rail_path(&st, &el));

    // The object: a type of its own shape, which follows this stair only.
    let (pts, idx) = mesh(&st, &el);
    let pts: Vec<[f64; 3]> = pts.into_iter().map(|q| [q[0], q[1], q[2] - elevation]).collect();
    let tris: Vec<[u32; 3]> = idx.chunks(3).map(|t| [t[0], t[1], t[2]]).collect();
    let s = &el.sized;
    let what = match s.platform {
        Some(w) => format!("{:.0} × {:.0} mm platform", w[0] * 1000.0, w[1] * 1000.0),
        None => "seated".into(),
    };
    let stair_name = store.get(stair).and_then(|e| e.name.clone()).unwrap_or_else(|| "stair".into());
    let name = format!("{} ({what}), {} side of {stair_name}, {:.2} m rail [{}]", s.kind.label(), if el.side == Side::Left { "left" } else { "right" }, el.rail_length, &el.stair[..8.min(el.stair.len())]);
    let ty = crate::mesh_objects::create_generated_type(txn, &name, crate::mesh_objects::Category::Transport, &pts, &tris, [0.55, 0.6, 0.66, 1.0], Vec::new(), Some("USERDEFINED"))?;
    let object_type = s.kind.object_type();
    txn.modify(ty, |e| {
        e.params.insert("@ElementType".into(), object_type.into());
    })?;
    let id = crate::mesh_objects::place(
        txn,
        crate::mesh_objects::PlaceParams { type_id: ElementRef::Id(ty), point: [0.0, 0.0], level: ElementRef::Id(level), offset: 0.0, rotation: 0.0, mirror: false, scale: 1.0 },
    )?;
    let data = serde_json::to_value(&el).expect("serialises");
    let props = properties(&el);
    let label = s.kind.label();
    txn.modify(id, |e| {
        e.name = Some(label.into());
        e.params.insert(DATA_KEY.into(), data);
        e.params.extend(props);
    })?;
    Ok(Created { stairlift: id, data: el })
}

/// Its property sets: `Reviera_Stairlift`.
pub fn properties(el: &Stairlift) -> Vec<(String, Value)> {
    let s = &el.sized;
    let len = |v: f64| measure("IfcPositiveLengthMeasure", v);
    let label = |v: &str| measure("IfcLabel", v);
    let mut out = vec![
        ("Pset_TransportElementCommon.CapacityPeople".into(), measure("IfcCountMeasure", 1)),
        ("Pset_TransportElementCommon.FireExit".into(), measure("IfcBoolean", false)),
        ("Reviera_Stairlift.Kind".into(), label(s.kind.object_type())),
        ("Reviera_Stairlift.Side".into(), label(if el.side == Side::Left { "LEFT" } else { "RIGHT" })),
        ("Reviera_Stairlift.CurvedRail".into(), measure("IfcBoolean", s.curved)),
        ("Reviera_Stairlift.FoldedDepth".into(), len(s.folded)),
        ("Reviera_Stairlift.UnfoldedDepth".into(), len(s.unfolded)),
        ("Reviera_Stairlift.RailDepth".into(), len(s.rail_depth)),
        ("Reviera_Stairlift.RailLength".into(), len(el.rail_length)),
        ("Reviera_Stairlift.Incline".into(), measure("IfcPlaneAngleMeasure", s.incline)),
        ("Reviera_Stairlift.Speed".into(), measure("IfcLinearVelocityMeasure", s.speed)),
        ("Reviera_Stairlift.RatedLoad".into(), measure("IfcMassMeasure", s.load)),
        ("Reviera_Stairlift.ParksClear".into(), measure("IfcBoolean", el.parks_clear)),
        ("Reviera_Stairlift.Supply".into(), label(&s.supply)),
        ("Reviera_Stairlift.SizingPack".into(), label(&s.sizing_pack)),
    ];
    if let Some(w) = s.platform {
        out.push(("Reviera_Stairlift.PlatformWidth".into(), len(w[0])));
        out.push(("Reviera_Stairlift.PlatformLength".into(), len(w[1])));
    }
    if let Some([f1, f2, m]) = s.wall_loads {
        out.push(("Reviera_Stairlift.WallLoadFirst".into(), measure("IfcForceMeasure", f1 * 1000.0)));
        out.push(("Reviera_Stairlift.WallLoadSecond".into(), measure("IfcForceMeasure", f2 * 1000.0)));
        out.push(("Reviera_Stairlift.WallMoment".into(), measure("IfcTorqueMeasure", m * 1000.0)));
    }
    out
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StairliftParams {
    pub stairlift: ElementRef,
}

fn of(store: &Store, r: &ElementRef) -> Result<(ElementId, Stairlift)> {
    let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such element"))?;
    let data = store.get(id).and_then(stairlift).ok_or_else(|| bad("vt.not_stairlift", format!("element {id} is not a stairlift")))?;
    Ok((id, data))
}

/// Deletes a stairlift (`stairlift.delete`), with its type, which nothing else uses.
pub(crate) fn delete(txn: &mut Transaction<'_>, p: StairliftParams) -> Result<usize> {
    let store = txn.store().clone();
    let (id, _) = of(&store, &p.stairlift)?;
    let ty = store.get(id).and_then(|e| e.param("@type")).and_then(Value::as_str).map(str::to_owned);
    txn.delete(id)?;
    let mut n = 1;
    if let Some(g) = ty
        && !txn.store().iter().any(|e| e.param("@type").and_then(Value::as_str) == Some(g.as_str()))
        && let Some(t) = by_guid(txn.store(), &g)
    {
        txn.delete(t)?;
        n += 1;
    }
    Ok(n)
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Listed {
    pub stairlift: ElementId,
    pub name: String,
    #[serde(flatten)]
    pub data: Stairlift,
}

/// `stairlift.list`.
pub(crate) fn list(store: &Store, _: Option<crate::elevators::NoParams>) -> Result<Vec<Listed>> {
    Ok(stairlifts(store).into_iter().map(|(id, data)| Listed { stairlift: id, name: store.get(id).and_then(|e| e.name.clone()).unwrap_or_default(), data }).collect())
}

// ---- Clear zones (ARC-VT-014) ----

/// Every stairlift's running envelope over each straight run of its stair (as deep as the unit
/// unfolded, as high as a seated rider or a platform's deck and head box), and its landing zones:
/// the bottom run-out and the top landing, with a platform's wheelchair space beyond each.
pub fn zones(store: &Store) -> Vec<Clearance> {
    let packs = active(store);
    let wheelchair = rule(&packs, |r| r.wheelchair_space);
    let head_rule = packs.iter().filter_map(|p| p.stairlift.as_ref().filter(|r| r.head_box.is_some()).map(|r| format!("{} {}", p.id, r.id))).next();
    let mut out = Vec::new();
    for (id, el) in stairlifts(store) {
        let Some(st) = by_guid(store, &el.stair).and_then(|s| stair_facts(store, s).ok()) else { continue };
        let s = &el.sized;
        let rule = match (s.kind, &head_rule) {
            (Kind::Platform, Some(r)) => r.clone(),
            _ => format!("{} generic envelope", s.sizing_pack),
        };
        let bases = run_bases(&st);
        for (run, &z0) in st.recipe.runs.iter().zip(&bases) {
            if run.spiral.is_some() || run.winder.is_some() {
                continue;
            }
            let l = run.length(st.tread);
            let ring = quad(run, el.side, [0.0, l], [0.0, s.unfolded]);
            let (r, t) = (st.riser, st.tread);
            let h = s.envelope_height;
            out.push(Clearance {
                element: id,
                kind: ClearanceKind::StairliftRun,
                rule: rule.clone(),
                mesh: crate::bodies::extrude_between(&ring, &[], &|p| nosing(run, z0, r, t, p) + 0.02, &|p| nosing(run, z0, r, t, p) + h),
                door_swing_forbidden: false,
            });
        }
        let platform = s.kind == Kind::Platform;
        let (wc_w, wc_d) = match wheelchair {
            Some([w, d]) if platform => (w, d),
            _ => (0.0, 0.0),
        };
        let width = s.unfolded.max(wc_w);
        let height = s.landing_height;
        if let Some(first) = st.recipe.runs.first().filter(|r| r.spiral.is_none() && r.winder.is_none()) {
            let ring = quad(first, el.side, [-(s.runout + wc_d), 0.0], [0.0, width]);
            out.push(landing(id, &rule, ring, st.z_base, height));
        }
        if let (Some(last), Some(&z0)) = (st.recipe.runs.last().filter(|r| r.spiral.is_none() && r.winder.is_none()), bases.last()) {
            let l = last.length(st.tread);
            let z = z0 + f64::from(last.risers) * st.riser;
            let ring = quad(last, el.side, [l, l + s.top_space + wc_d], [0.0, width]);
            out.push(landing(id, &rule, ring, z, height));
        }
    }
    out
}

fn landing(id: ElementId, rule: &str, ring: Vec<V2>, z: f64, height: f64) -> Clearance {
    Clearance { element: id, kind: ClearanceKind::StairliftLanding, rule: rule.into(), mesh: crate::bodies::extrude(&ring, &[], z + 0.01, z + height), door_swing_forbidden: false }
}

/// What may stand in a stairlift's zones besides itself: stairs, railings (a wall handrail beside
/// a seated unit stays), and floor openings.
pub fn may_share(store: &Store, other: ElementId) -> bool {
    store.get(other).is_some_and(|e| {
        crate::stairs::stair_recipe(e).is_some() || crate::railings::railing_recipe(e).is_some() || crate::slab_openings::shaft_recipe(e).is_some()
            || crate::slab_openings::opening_recipe(e).is_some()
    })
}

// ---- Checks (ARC-VT-015) ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    /// The width left beside the folded or parked unit.
    ClearWidth,
    ExitEnclosure,
    PlatformSize,
    Speed,
    /// The stair's angle against the model's range.
    Incline,
    /// A straight rail longer than the model's longest.
    RailLength,
    /// The stair against the maker's recommended width, or the inside-bend width at a turn.
    StairWidth,
    /// A straight-rail model on a stair that turns.
    CurvedRail,
    StairGone,
}

/// One finding of `stairlift.check`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Finding {
    pub severity: Severity,
    pub kind: FindingKind,
    pub stairlift: ElementId,
    pub pack: String,
    pub edition: String,
    pub layer: Layer,
    pub rule: String,
    pub description: String,
    pub confidence: Confidence,
    pub source: String,
    pub message: String,
    pub measured: f64,
    pub required: f64,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckParams {
    /// The stairlifts to check; omitted, all.
    #[serde(default)]
    pub stairlifts: Vec<ElementRef>,
}

struct Ctx<'a> {
    pack: &'a Pack,
    id: String,
    description: String,
    source: String,
    confidence: Confidence,
}

fn finding(c: &Ctx<'_>, lift: ElementId, kind: FindingKind, message: String, measured: f64, required: f64) -> Finding {
    let note = c.pack.layer == Layer::Recommended || c.confidence == Confidence::Advisory;
    Finding {
        severity: if note { Severity::Note } else { Severity::Error },
        kind,
        stairlift: lift,
        pack: c.pack.id.clone(),
        edition: c.pack.edition.clone(),
        layer: c.pack.layer,
        rule: c.id.clone(),
        description: c.description.clone(),
        confidence: c.confidence,
        source: c.source.clone(),
        message,
        measured,
        required,
    }
}

/// Checks stairlifts and their stairs against the packs in use (`stairlift.check`): the width
/// left beside the unit, exit enclosures, platform size and speed, and the generic model's
/// incline, length, stair-width and turn limits. Running envelopes and landings are checked by
/// `clearance.check`.
pub(crate) fn check(store: &Store, p: Option<CheckParams>) -> Result<Vec<Finding>> {
    let p = p.unwrap_or_default();
    let packs = active(store);
    let chosen: Vec<(ElementId, Stairlift)> = if p.stairlifts.is_empty() { stairlifts(store) } else { p.stairlifts.iter().map(|r| of(store, r)).collect::<Result<_>>()? };
    let mut out = Vec::new();
    for (id, el) in &chosen {
        let s = &el.sized;
        let Some(st) = by_guid(store, &el.stair).and_then(|x| stair_facts(store, x).ok()) else {
            if let Some(pack) = packs.first() {
                let c = Ctx { pack, id: "stair".into(), description: String::new(), source: String::new(), confidence: Confidence::Verified };
                out.push(finding(&c, *id, FindingKind::StairGone, "its stair is gone or no longer a Reviera stair".into(), 0.0, 0.0));
            }
            continue;
        };
        let mm = |v: f64| v * 1000.0;
        for pack in &packs {
            if let Some(r) = &pack.stairlift {
                let c = Ctx { pack, id: r.id.clone(), description: r.description.clone().unwrap_or_default(), source: r.source.clone(), confidence: r.confidence };
                // The width beside the unit: the folded unit where it parks on the stair; only
                // the rail where it parks clear (nothing where the rail is ignored).
                let projection = match (el.parks_clear, r.rail_ignored) {
                    (false, _) => s.folded,
                    (true, false) => s.rail_depth,
                    (true, true) => 0.0,
                };
                let left = st.width - projection;
                let need = match el.stair_use {
                    StairUse::Dwelling => r.dwelling_clear_width,
                    _ => match (r.small_width, r.small_occupants, el.occupants) {
                        (Some(w), Some(n), Some(o)) if o < n => Some(w),
                        _ => r.required_width,
                    },
                };
                if let Some(need) = need.filter(|n| left < n - 1e-6) {
                    let by = if projection > 0.0 { format!(" beside the {} ({:.0} mm)", if el.parks_clear { "rail" } else { "folded unit" }, mm(projection)) } else { String::new() };
                    out.push(finding(&c, *id, FindingKind::ClearWidth, format!("{:.0} mm of the {:.0} mm stair is left{by}; {:.0} mm is required", mm(left), mm(st.width), mm(need)), left, need));
                }
                if r.no_exit_enclosure && el.stair_use == StairUse::Exit {
                    out.push(finding(&c, *id, FindingKind::ExitEnclosure, "a stairlift in an exit enclosure may interfere with egress; ask the authority having jurisdiction".into(), 1.0, 0.0));
                }
                if let (Some(min), Some(w)) = (r.platform_min, s.platform)
                    && (w[0] < min[0] - 1e-6 || w[1] < min[1] - 1e-6)
                {
                    out.push(finding(&c, *id, FindingKind::PlatformSize, format!("a {:.0} × {:.0} mm platform is smaller than {:.0} × {:.0} mm", mm(w[0]), mm(w[1]), mm(min[0]), mm(min[1])), w[0] * w[1], min[0] * min[1]));
                }
                if let Some(max) = r.max_speed.filter(|m| s.speed > m + 1e-6) {
                    out.push(finding(&c, *id, FindingKind::Speed, format!("{:.2} m/s is faster than {max:.2} m/s", s.speed), s.speed, max));
                }
            }
            if pack.id == s.sizing_pack
                && let Some(z) = &pack.stairlift_sizing
            {
                let source = match (s.kind, &s.family) {
                    (Kind::Platform, Some(f)) => z.platforms.iter().find(|x| &x.id == f).map_or(z.source.clone(), |x| x.source.clone()),
                    _ => z.seated.source.clone(),
                };
                let c = Ctx { pack, id: format!("generic {}", s.family.as_deref().unwrap_or("seated")), description: "the generic model's limits; a chosen product's own replace them".into(), source, confidence: z.confidence };
                let [lo, hi] = s.incline_range;
                if s.incline < lo - 1e-6 || s.incline > hi + 1e-6 {
                    out.push(finding(&c, *id, FindingKind::Incline, format!("the stair's {:.1}° is outside the model's {lo:.0}–{hi:.0}°", s.incline), s.incline, if s.incline > hi { hi } else { lo }));
                }
                if let Some(max) = s.max_length.filter(|m| st.length > m + 1e-6) {
                    out.push(finding(&c, *id, FindingKind::RailLength, format!("the {:.2} m flight is longer than the {max:.2} m straight rail", st.length), st.length, max));
                }
                if let Some(w) = s.stair_width.filter(|w| st.width < w - 1e-6) {
                    out.push(finding(&c, *id, FindingKind::StairWidth, format!("the stair is {:.0} mm wide; the maker recommends {:.0} mm for this platform", mm(st.width), mm(w)), st.width, w));
                }
                if st.turns {
                    if s.kind == Kind::Platform && !s.curved {
                        out.push(finding(&c, *id, FindingKind::CurvedRail, "the stair turns; this platform family runs on straight rails only (choose the curved family)".into(), 1.0, 0.0));
                    }
                    if let Some(w) = s.inside_bend.filter(|w| st.width < w - 1e-6) {
                        out.push(finding(&c, *id, FindingKind::StairWidth, format!("the stair is {:.0} mm wide; a turn needs {:.0} mm on its inside", mm(st.width), mm(w)), st.width, w));
                    }
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packs(ids: &[&str]) -> Vec<Pack> {
        crate::elevators::shipped().iter().filter(|p| ids.contains(&p.id.as_str())).cloned().collect()
    }

    fn straight(width: f64, risers: u32) -> StairFacts {
        let recipe = StairRecipe {
            kind: crate::stairs::STAIR_RECIPE.into(),
            base_level: String::new(),
            base_offset: 0.0,
            top: crate::stairs::StairTop::Unconnected(0.18 * f64::from(risers)),
            top_offset: 0.0,
            desired_risers: None,
            tread_depth: 0.28,
            runs: vec![Run { start: [0.0, 0.0], direction: [1.0, 0.0], width, risers, spiral: None, winder: None }],
            auto_extend: false,
        };
        StairFacts { recipe, z_base: 0.0, riser: 0.18, tread: 0.28, incline: (0.18f64 / 0.28).atan().to_degrees(), width, turns: false, length: 3.0 }
    }

    #[test]
    fn interpolation_holds_at_the_ends() {
        let t = [[10.0, 2.0], [30.0, 1.0]];
        assert_eq!((interp(&t, 0.0), interp(&t, 20.0), interp(&t, 45.0)), (2.0, 1.5, 1.0));
    }

    #[test]
    fn a_platform_takes_its_family_and_largest_size() {
        let us = packs(&["us_a18", "practice_us"]);
        let st = straight(1.1, 16);
        let s = size(&us, &st, &SizeParams { kind: Kind::Platform, ..SizeParams::default() }).unwrap();
        assert_eq!((s.family.as_deref(), s.platform, s.curved), (Some("straight"), Some([0.8, 1.25]), false));
        // The head box rides on the deck's rise at this angle (32.7°, between the 30° and 35° rows).
        let rise = 0.834 + (0.992 - 0.834) * (s.incline - 30.0) / 5.0;
        assert!((s.envelope_height - (1.524 + rise)).abs() < 1e-9, "{}", s.envelope_height);
        assert!(size(&us, &st, &SizeParams { kind: Kind::Platform, platform: Some([0.9, 0.9]), ..SizeParams::default() }).is_err());
        let turning = StairFacts { turns: true, ..st };
        let c = size(&us, &turning, &SizeParams { kind: Kind::Platform, ..SizeParams::default() }).unwrap();
        assert_eq!((c.family.as_deref(), c.inside_bend), (Some("curved"), Some(1.225)));
    }

    #[test]
    fn the_published_definitions_cover_what_is_written() {
        let defs: Value = serde_json::from_str(include_str!("../../../schemas/psets/reviera-psets-1.json")).unwrap();
        let set = defs["property_sets"].as_array().unwrap().iter().find(|s| s["name"] == "Reviera_Stairlift").expect("Reviera_Stairlift");
        let s = size(&packs(&["practice_us"]), &straight(1.1, 16), &SizeParams { kind: Kind::Platform, ..SizeParams::default() }).unwrap();
        let el = Stairlift { sized: s, stair: String::new(), side: Side::Left, parks_clear: false, stair_use: StairUse::Other, occupants: None, rail_length: 5.0 };
        for (key, value) in properties(&el) {
            let Some(name) = key.strip_prefix("Reviera_Stairlift.") else { continue };
            let def = set["properties"].as_array().unwrap().iter().find(|p| p["name"] == name).unwrap_or_else(|| panic!("{name} is not published"));
            assert_eq!(def["measure"], value["type"], "{name}");
        }
    }
}
