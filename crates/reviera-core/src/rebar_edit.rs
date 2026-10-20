// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Rebar editing, propagation and splitting (STR-RBR-006), splices and couplers (STR-RBR-015).
//!
//! **Editing** keeps a set parametric: a segment set by hand (rounded to the segment
//! increment), bars moved along the array or removed, and Reset Bars to undo those; **Split
//! Set** keeps bars `0..at` in the set and makes a new set of the rest; **Propagate** copies sets
//! to other hosts of the same kind, each worked out in its own host (sets are placed relative to
//! their host, so *Align by Host* is the copy itself; *Align by Face* moves bars at a face to the
//! face given), reporting the hosts where they do not fit.
//!
//! **Splices** cut a set of straight bars (shape 00) at a point along them, by a distance or
//! where a plan line crosses them, into two sets that overlap by the lap (by default the bar
//! type's, 40 diameters); a stagger moves every second bar's splice. The two keep each other
//! (GlobalIds) and change together.
//!
//! **Couplers** (`IfcMechanicalFastener`, ObjectType COUPLER) join two bar ends no more than
//! 10 diameters apart and 3 off line, or sit on one bar end; they are sized to the bar and go
//! with their bars.
//!
//! Not yet: in-canvas segment controls and shape handles, splice types, coupler types and their
//! end treatments, arc bar couplers, coupler schedules and *Align to View*.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::rebar::{self, RebarRecipe, Splice, V3, add, len, mul, sub, unit};
use crate::store::Store;
use crate::walls::RECIPE_KEY;

pub const COUPLER_RECIPE: &str = "recipe.rebar_coupler";

fn set_of(store: &Store, r: &ElementRef) -> Result<(ElementId, RebarRecipe)> {
    let id = store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
    let rec = store.get(id).and_then(rebar::rebar_recipe).ok_or_else(|| Error::new("rebar.not_rebar", format!("element {id} is not a rebar set")))?;
    Ok((id, rec))
}

/// Writes a set's recipe after checking it works out, and its splice partner's alike.
fn write(txn: &mut crate::txn::Transaction<'_>, id: ElementId, rec: &RebarRecipe) -> Result<()> {
    let store = txn.store().clone();
    rebar::geometry(&store, rec)?;
    txn.set_param(id, RECIPE_KEY, serde_json::to_value(rec).expect("serialises"))?;
    crate::rebar_numbers::persist(txn)
}

/// The number of layout positions a set has (before its edits).
fn positions(store: &Store, rec: &RebarRecipe) -> Result<usize> {
    let bare = RebarRecipe { edits: Default::default(), splice: None, ..rec.clone() };
    Ok(rebar::geometry(store, &bare)?.bars.len())
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SegmentParams {
    pub rebar_id: ElementRef,
    /// A, B, C…
    pub segment: String,
    /// Outside dimension (metres); rounded to the segment increment. `null` resets it.
    #[serde(default)]
    pub length: Option<f64>,
}

/// Sets one segment of a set's shape (`rebar.set_segment_length`).
pub(crate) fn set_segment_length(txn: &mut crate::txn::Transaction<'_>, p: SegmentParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut rec) = set_of(&store, &p.rebar_id)?;
    if matches!(rec.placement, rebar::Placement::Sketch(_)) {
        return Err(Error::new("rebar.segment", "a sketched bar's segments follow its sketch and constraints; edit those instead"));
    }
    let s = rebar::settings(&store);
    match p.length {
        Some(v) if v <= 0.0 => return Err(Error::new("rebar.segment", "a segment is more than 0 long")),
        Some(v) => {
            let v = if s.rounding.on { s.rounding.round(v, s.rounding.segment_length) } else { v };
            rec.edits.segments.insert(p.segment.to_uppercase(), v);
        }
        None => {
            rec.edits.segments.remove(&p.segment.to_uppercase());
        }
    }
    write(txn, id, &rec)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct MoveBarParams {
    pub rebar_id: ElementRef,
    /// The bar's layout position (from 0).
    pub bar_index: usize,
    /// Along the array (metres).
    pub delta: f64,
}

/// Moves one bar along the set's array (`rebar.move_bar_in_set`).
pub(crate) fn move_bar(txn: &mut crate::txn::Transaction<'_>, p: MoveBarParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut rec) = set_of(&store, &p.rebar_id)?;
    if p.bar_index >= positions(&store, &rec)? {
        return Err(Error::new("rebar.bar_index", format!("the set has no bar {}", p.bar_index)));
    }
    *rec.edits.moved.entry(p.bar_index).or_default() += p.delta;
    write(txn, id, &rec)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BarParams {
    pub rebar_id: ElementRef,
    pub index: usize,
}

/// Removes one bar of a set (`rebar.remove_bar`); the quantity drops.
pub(crate) fn remove_bar(txn: &mut crate::txn::Transaction<'_>, p: BarParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut rec) = set_of(&store, &p.rebar_id)?;
    if p.index >= positions(&store, &rec)? {
        return Err(Error::new("rebar.bar_index", format!("the set has no bar {}", p.index)));
    }
    if !rec.edits.removed.contains(&p.index) {
        rec.edits.removed.push(p.index);
        rec.edits.removed.sort_unstable();
    }
    write(txn, id, &rec)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ResetParams {
    pub rebar_id: ElementRef,
}

/// Brings removed and moved bars back (`rebar.reset_bars`); a split set keeps its part.
pub(crate) fn reset_bars(txn: &mut crate::txn::Transaction<'_>, p: ResetParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut rec) = set_of(&store, &p.rebar_id)?;
    rec.edits.removed.clear();
    rec.edits.moved.clear();
    write(txn, id, &rec)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SplitParams {
    pub rebar_id: ElementRef,
    /// The first bar of the new set, counted in this set's bars (from 0).
    pub at_index: usize,
}

/// Splits a set at a bar (`rebar.split_set`): the set keeps the bars before it, a new set takes
/// the rest. Returns the new set.
pub(crate) fn split_set(txn: &mut crate::txn::Transaction<'_>, p: SplitParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let (id, rec) = set_of(&store, &p.rebar_id)?;
    let n = positions(&store, &rec)?;
    let (from, to) = rec.edits.range.unwrap_or((0, n));
    // The set's own bars, by layout position.
    let own: Vec<usize> = (from..to.min(n)).filter(|i| !rec.edits.removed.contains(i)).collect();
    if p.at_index == 0 || p.at_index >= own.len() {
        return Err(Error::new("rebar.split", format!("a set of {} bars splits at bar 1 to {}", own.len(), own.len().saturating_sub(1))));
    }
    let cut = own[p.at_index];
    let mut first = rec.clone();
    first.edits.range = Some((from, cut));
    let mut second = rec.clone();
    second.edits.range = Some((cut, to));
    write(txn, id, &first)?;
    let e = store.get(id).expect("resolved");
    let new = txn.create(
        NewElement::new("IfcReinforcingBar")
            .param("@PredefinedType", e.param("@PredefinedType").cloned().unwrap_or(json!("MAIN")))
            .param(RECIPE_KEY, serde_json::to_value(&second).expect("serialises")),
    )?;
    if let Some(part) = e.param(crate::rebar_numbers::PARTITION_KEY).cloned() {
        txn.set_param(new, crate::rebar_numbers::PARTITION_KEY, part)?;
    }
    crate::rebar_numbers::persist(txn)?;
    Ok(new)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PropagateMode {
    /// Matched to each host's own frame.
    #[default]
    Host,
    /// Bars at a face moved to the face given.
    Face,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PropagateParams {
    pub rebar_ids: Vec<ElementRef>,
    pub target_host_ids: Vec<ElementRef>,
    #[serde(default)]
    pub mode: PropagateMode,
    /// For `face`: top, bottom, left or right (members); top or bottom (floors); exterior or
    /// interior (walls).
    #[serde(default)]
    pub face_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PropagateOut {
    pub created: Vec<ElementId>,
    /// Hosts where a set does not fit, with why.
    pub failed: Vec<(ElementId, String)>,
}

/// Copies sets to other hosts of the same kind (`rebar.propagate`).
pub(crate) fn propagate(txn: &mut crate::txn::Transaction<'_>, p: PropagateParams) -> Result<PropagateOut> {
    let store = txn.store().clone();
    let mut out = PropagateOut { created: Vec::new(), failed: Vec::new() };
    let sets: Vec<(ElementId, RebarRecipe)> = p.rebar_ids.iter().map(|r| set_of(&store, r)).collect::<Result<_>>()?;
    for t in &p.target_host_ids {
        let hid = store.resolve(t).ok_or_else(|| Error::new("element.not_found", format!("no element {t:?}")))?;
        let host = store.get(hid).expect("resolved");
        let kind = match rebar::host_check(&store, host) {
            Ok(k) => k,
            Err(why) => {
                out.failed.push((hid, why));
                continue;
            }
        };
        for (sid, rec) in &sets {
            let source = GlobalId::parse(&rec.host).and_then(|g| store.find_by_guid(&g)).and_then(|i| store.get(i));
            if source.and_then(|s| rebar::host_check(&store, s).ok()) != Some(kind) {
                out.failed.push((hid, format!("set {sid} is in a different kind of host")));
                continue;
            }
            if matches!(rec.placement, rebar::Placement::Sketch(_)) {
                out.failed.push((hid, format!("set {sid} is sketched; sketched sets are not propagated yet")));
                continue;
            }
            let mut copy = RebarRecipe { host: host.guid().to_string(), splice: None, ..rec.clone() };
            copy.edits = rebar::BarEdits { segments: rec.edits.segments.clone(), ..Default::default() };
            if p.mode == PropagateMode::Face
                && let Some(f) = &p.face_ref
            {
                match &mut copy.placement {
                    rebar::Placement::Face { face } => *face = f.clone(),
                    rebar::Placement::Layer { face, .. } => *face = f.clone(),
                    rebar::Placement::Section | rebar::Placement::Through { .. } | rebar::Placement::Sketch(_) => {}
                }
            }
            match rebar::geometry(&store, &copy) {
                Ok(_) => {
                    let e = store.get(*sid).expect("a set");
                    let id = txn.create(
                        NewElement::new("IfcReinforcingBar")
                            .param("@PredefinedType", e.param("@PredefinedType").cloned().unwrap_or(json!("MAIN")))
                            .param(RECIPE_KEY, serde_json::to_value(&copy).expect("serialises")),
                    )?;
                    out.created.push(id);
                }
                Err(e) => out.failed.push((hid, e.message)),
            }
        }
    }
    crate::rebar_numbers::persist(txn)?;
    Ok(out)
}

// ---- Splices (STR-RBR-015) ----

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SpliceMode {
    #[default]
    Length,
    Line,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SpliceParams {
    pub rebar_id: ElementRef,
    #[serde(default)]
    pub mode: SpliceMode,
    /// For `line`: two plan points; the splice is where it crosses the first bar.
    #[serde(default)]
    pub line: Option<[[f64; 2]; 2]>,
    /// For `length`: from the bars' start (metres).
    #[serde(default)]
    pub length: Option<f64>,
    /// The lap (metres); default 40 bar diameters.
    #[serde(default)]
    pub lap: Option<f64>,
    /// Every second bar's splice moves this far along (metres).
    #[serde(default)]
    pub stagger: f64,
}

/// Splices a set of straight bars (`rebar.splice`): the set keeps the first part and a new set
/// the second, overlapping by the lap. Returns both.
pub(crate) fn splice(txn: &mut crate::txn::Transaction<'_>, p: SpliceParams) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let (id, rec) = set_of(&store, &p.rebar_id)?;
    if rec.shape != "00" || rec.splice.is_some() {
        return Err(Error::new("rebar.splice", "splices are on straight bars (shape 00) not already spliced; links, bent, area, path and free-form bars are not spliced"));
    }
    if matches!(&rec.placement, rebar::Placement::Layer { hook: Some(_), .. }) {
        return Err(Error::new("rebar.splice", "hooked bars are not spliced"));
    }
    let g = rebar::geometry(&store, &rec)?;
    let bar = g.bars.first().ok_or_else(|| Error::new("rebar.no_bars", "the set has no bars"))?;
    let (a, e) = (bar.points[0], bar.points[bar.points.len() - 1]);
    let full = len(sub(e, a));
    let at = match (p.mode, p.length, p.line) {
        (SpliceMode::Length, Some(l), _) => l,
        (SpliceMode::Line, _, Some([q0, q1])) => crossing(a, e, q0, q1).ok_or_else(|| Error::new("rebar.splice", "the line does not cross the bars"))?,
        _ => return Err(Error::new("rebar.splice", "give a length, or a line crossing the bars")),
    };
    let d = rebar::bar_types(&store).into_iter().find(|b| b.id == rec.bar_type).map_or(0.012, |b| b.diameter);
    let lap = p.lap.unwrap_or(40.0 * d);
    if lap <= 0.0 || at - lap / 2.0 <= d || at + lap / 2.0 + p.stagger >= full - d {
        return Err(Error::new("rebar.splice", format!("the splice and its {:.0} mm lap must lie inside the {:.0} mm bars", lap * 1000.0, full * 1000.0)));
    }
    let e0 = store.get(id).expect("resolved");
    let second_guid = GlobalId::new();
    let first = RebarRecipe { splice: Some(Splice { first: true, at, lap, stagger: p.stagger, partner: second_guid.to_string() }), ..rec.clone() };
    let second = RebarRecipe { splice: Some(Splice { first: false, at, lap, stagger: p.stagger, partner: e0.guid().to_string() }), ..rec.clone() };
    write(txn, id, &first)?;
    let new = txn.create(
        NewElement::new("IfcReinforcingBar")
            .guid(second_guid)
            .param("@PredefinedType", "MAIN")
            .param(RECIPE_KEY, serde_json::to_value(&second).expect("serialises")),
    )?;
    if let Some(part) = e0.param(crate::rebar_numbers::PARTITION_KEY).cloned() {
        txn.set_param(new, crate::rebar_numbers::PARTITION_KEY, part)?;
    }
    crate::rebar_numbers::persist(txn)?;
    Ok(vec![id, new])
}

/// Where a plan line crosses a bar, as a distance along it.
fn crossing(a: V3, e: V3, q0: [f64; 2], q1: [f64; 2]) -> Option<f64> {
    let (d, w) = ([e[0] - a[0], e[1] - a[1]], [q1[0] - q0[0], q1[1] - q0[1]]);
    let den = d[0] * w[1] - d[1] * w[0];
    if den.abs() < 1e-12 {
        return None;
    }
    let r = [q0[0] - a[0], q0[1] - a[1]];
    let t = (r[0] * w[1] - r[1] * w[0]) / den;
    (0.0..=1.0).contains(&t).then(|| t * len(sub(e, a)))
}

/// A spliced set's partner, which follows its layout, bar type and placement.
pub fn partner(store: &Store, rec: &RebarRecipe) -> Option<ElementId> {
    let g = GlobalId::parse(&rec.splice.as_ref()?.partner)?;
    store.find_by_guid(&g)
}

// ---- Couplers (STR-RBR-015) ----

/// A bar end: a set, a bar (its index among the set's bars) and which end.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BarEnd {
    pub rebar: String,
    pub bar: usize,
    /// `start` or `end`.
    pub end: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CouplerRecipe {
    pub kind: String,
    pub ends: Vec<BarEnd>,
}

pub fn coupler_recipe(e: &Element) -> Option<CouplerRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == COUPLER_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// A bar end's point, outward direction and diameter.
fn end_point(store: &Store, b: &BarEnd) -> Result<(V3, V3, f64)> {
    let e = GlobalId::parse(&b.rebar).and_then(|g| store.find_by_guid(&g)).and_then(|i| store.get(i)).ok_or_else(|| Error::new("rebar_coupler.no_bar", "the coupler's set is gone"))?;
    let rec = rebar::rebar_recipe(e).ok_or_else(|| Error::new("rebar.not_rebar", "not a rebar set"))?;
    let g = rebar::geometry(store, &rec)?;
    let bar = g.bars.get(b.bar).ok_or_else(|| Error::new("rebar.bar_index", format!("the set has no bar {}", b.bar)))?;
    let d = rebar::bar_types(store).into_iter().find(|t| t.id == rec.bar_type).map_or(0.012, |t| t.diameter);
    let n = bar.points.len();
    Ok(match b.end.as_str() {
        "start" => (bar.points[0], unit(sub(bar.points[0], bar.points[1])), d),
        _ => (bar.points[n - 1], unit(sub(bar.points[n - 1], bar.points[n - 2])), d),
    })
}

/// A coupler's axis (from, to) and its outside diameter: sized to the bar, a sleeve 1.6 bar
/// diameters across and 6 long.
pub fn coupler_geometry(store: &Store, r: &CouplerRecipe) -> Result<(V3, V3, f64)> {
    let (p, out, d) = end_point(store, &r.ends[0])?;
    let (a, b) = match r.ends.get(1) {
        Some(other) => {
            let (q, _, _) = end_point(store, other)?;
            let mid = mul(add(p, q), 0.5);
            let dir = if len(sub(q, p)) > 1e-9 { unit(sub(q, p)) } else { out };
            (add(mid, mul(dir, -3.0 * d)), add(mid, mul(dir, 3.0 * d)))
        }
        None => (add(p, mul(out, -3.0 * d)), add(p, mul(out, 3.0 * d))),
    };
    Ok((a, b, 1.6 * d))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CouplerEndRef {
    pub rebar_id: ElementRef,
    pub bar: usize,
    /// `start` or `end`.
    pub end: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CouplerParams {
    /// One bar end (On Bar End) or two (Between Two Bars).
    pub bar_end_refs: Vec<CouplerEndRef>,
}

/// Places a coupler (`rebar_coupler.place`).
pub(crate) fn place_coupler(txn: &mut crate::txn::Transaction<'_>, p: CouplerParams) -> Result<ElementId> {
    let store = txn.store().clone();
    if !(1..=2).contains(&p.bar_end_refs.len()) {
        return Err(Error::new("rebar_coupler.ends", "a coupler takes one bar end or two"));
    }
    let mut ends = Vec::new();
    for r in &p.bar_end_refs {
        let (id, _) = set_of(&store, &r.rebar_id)?;
        if r.end != "start" && r.end != "end" {
            return Err(Error::new("rebar_coupler.ends", "a bar end is start or end"));
        }
        ends.push(BarEnd { rebar: store.get(id).expect("a set").guid().to_string(), bar: r.bar, end: r.end.clone() });
    }
    let rec = CouplerRecipe { kind: COUPLER_RECIPE.into(), ends };
    if rec.ends.len() == 2 {
        let (p0, out, d) = end_point(&store, &rec.ends[0])?;
        let (p1, _, d1) = end_point(&store, &rec.ends[1])?;
        if (d - d1).abs() > 1e-6 {
            return Err(Error::new("rebar_coupler.size", "the two bars are of different sizes"));
        }
        let gap = sub(p1, p0);
        let along = gap[0] * out[0] + gap[1] * out[1] + gap[2] * out[2];
        let off = len(sub(gap, mul(out, along)));
        if len(gap) > 10.0 * d + 1e-9 {
            return Err(Error::new(
                "rebar_coupler.gap",
                format!("the bar ends are {:.0} mm apart, more than 10 diameters ({:.0} mm)", len(gap) * 1000.0, 10.0 * d * 1000.0),
            ));
        }
        if off > 3.0 * d + 1e-9 {
            return Err(Error::new(
                "rebar_coupler.offset",
                format!("the bar ends are {:.0} mm off line, more than 3 diameters ({:.0} mm)", off * 1000.0, 3.0 * d * 1000.0),
            ));
        }
    }
    coupler_geometry(&store, &rec)?;
    txn.create(
        NewElement::new("IfcMechanicalFastener")
            .name("Coupler")
            .param("@PredefinedType", "USERDEFINED")
            .param("@ObjectType", "COUPLER")
            .param(RECIPE_KEY, serde_json::to_value(&rec).expect("serialises")),
    )
}

/// A coupler's body.
pub fn coupler_mesh(store: &Store, e: &Element) -> Option<crate::bodies::Mesh> {
    let r = coupler_recipe(e)?;
    let (a, b, d) = coupler_geometry(store, &r).ok()?;
    Some(crate::steel_fab::cylinder(a, b, d / 2.0, 12))
}

/// Whether an element goes with set `guid`: a coupler on its bars.
pub fn uses(e: &Element, guid: &str) -> bool {
    coupler_recipe(e).is_some_and(|r| r.ends.iter().any(|b| b.rebar == guid))
}

/// The sets a coupler joins (for IFC `IfcRelConnectsWithRealizingElements`).
pub fn coupled(store: &Store, e: &Element) -> Vec<ElementId> {
    coupler_recipe(e)
        .map(|r| r.ends.iter().filter_map(|b| GlobalId::parse(&b.rebar).and_then(|g| store.find_by_guid(&g))).collect())
        .unwrap_or_default()
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CouplerInfoParams {
    pub coupler_ids: Vec<ElementRef>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CouplerInfo {
    pub id: ElementId,
    pub sets: Vec<ElementId>,
    pub diameter: f64,
    pub length: f64,
    pub axis: [V3; 2],
}

pub fn coupler_info(store: &Store, p: CouplerInfoParams) -> Result<Vec<CouplerInfo>> {
    p.coupler_ids
        .iter()
        .map(|r| {
            let id = store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
            let e = store.get(id).expect("resolved");
            let rec = coupler_recipe(e).ok_or_else(|| Error::new("rebar_coupler.not_coupler", format!("element {id} is not a coupler")))?;
            let (a, b, d) = coupler_geometry(store, &rec)?;
            Ok(CouplerInfo { id, sets: coupled(store, e), diameter: d, length: len(sub(b, a)), axis: [a, b] })
        })
        .collect()
}
