// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Rebar documentation (STR-RBR-018): rebar tags, multi-rebar annotations, bending details and
//! rebar schedules.
//!
//! - **Schedules and tags** read the rebar fields (`rebar.number`, `rebar.shape`,
//!   `rebar.bar_type`, `rebar.quantity`, `rebar.spacing`, `rebar.bar_length`,
//!   `rebar.total_length`, `rebar.mass`, `rebar.A` … `rebar.R`, `rebar.partition`,
//!   `rebar.description` such as "15 Ø12 @ 150"). Mass is the rounded total length times the
//!   bar's nominal mass per metre. The *Rebar Tag* reads the number and description.
//! - A **multi-rebar annotation** (`anno.multi_rebar`) is a dimension across one or more parallel
//!   sets, measuring their extent (from the first bar to the last), with the description as its
//!   text; *aligned* runs square to the bars, *linear* along x or y. It follows the sets and goes
//!   with the last of them.
//! - A **bending detail** (`anno.bending_detail`) draws a set's bar flattened in its own plane at
//!   a point, scaled and turned, with the shape code and the segment values after rounding.
//!
//! All three are plan annotations (view data); they read the sets when drawn, so a change to a
//! set shows in its detail, tags and schedules in the same regeneration.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::annotations::{ANNO_KEY, CLASS_PREFIX, is_annotation};
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::rebar::{self, RebarInfo};
use crate::store::Store;

pub const KIND_MULTI: &str = "anno.multi_rebar";
pub const KIND_BENDING: &str = "anno.bending_detail";

/// The schedule fields of rebar sets: (key, name, summed).
pub const FIELDS: &[(&str, &str, bool)] = &[
    ("rebar.number", "Rebar Number", false),
    ("rebar.partition", "Partition", false),
    ("rebar.shape", "Shape Code", false),
    ("rebar.bar_type", "Bar Type", false),
    ("rebar.quantity", "Quantity", true),
    ("rebar.spacing", "Spacing", false),
    ("rebar.bar_length", "Bar Length", false),
    ("rebar.total_length", "Total Length", true),
    ("rebar.mass", "Mass", true),
    ("rebar.description", "Description", false),
];

pub const SEGMENTS: &str = "ABCDEFGHJKLMNOPQR";

/// A set's nominal mass per metre (kg): its area times steel's density.
pub fn mass_per_metre(diameter: f64) -> f64 {
    std::f64::consts::PI * diameter * diameter / 4.0 * 7850.0
}

/// A segment value after segment rounding.
pub fn segment(store: &Store, v: f64) -> f64 {
    let s = rebar::settings(store);
    if s.rounding.on { s.rounding.round(v, s.rounding.segment_length) } else { v }
}

/// The set's spacing: the layout's, else its array length over its spaces.
pub fn spacing(i: &RebarInfo) -> Option<f64> {
    i.layout.spacing.or_else(|| (i.count > 1).then(|| i.array_length / (i.count - 1) as f64)).filter(|s| *s > 0.0)
}

/// "15 Ø12 @ 150": quantity, bar type and spacing in mm.
pub fn description(i: &RebarInfo) -> String {
    match spacing(i) {
        Some(s) if i.count > 1 => format!("{} {} @ {:.0}", i.count, i.bar_type, s * 1000.0),
        _ => format!("{} {}", i.count, i.bar_type),
    }
}

/// Every set's info, for schedules and tags.
pub fn infos(store: &Store) -> std::collections::HashMap<ElementId, RebarInfo> {
    let ids: Vec<ElementId> = store.iter().filter(|e| rebar::rebar_recipe(e).is_some()).map(|e| e.id()).collect();
    if ids.is_empty() {
        return Default::default();
    }
    // One call numbers them all; a set that cannot be worked out is left out.
    match rebar::info(store, rebar::InfoParams { rebar_ids: ids.iter().map(|i| ElementRef::Id(*i)).collect() }) {
        Ok(v) => v.into_iter().map(|i| (i.id, i)).collect(),
        Err(_) => ids
            .into_iter()
            .filter_map(|id| rebar::info(store, rebar::InfoParams { rebar_ids: vec![ElementRef::Id(id)] }).ok()?.into_iter().next())
            .map(|i| (i.id, i))
            .collect(),
    }
}

/// A rebar field of a set: its text and number (for sorting and totals).
pub fn field(store: &Store, i: &RebarInfo, key: &str) -> Option<(String, Option<f64>)> {
    let mm = |v: f64| (format!("{:.0}", v * 1000.0), Some(v));
    Some(match key {
        "rebar.number" => (i.number.map(|n| n.to_string()).unwrap_or_default(), i.number.map(f64::from)),
        "rebar.partition" => (i.partition.clone(), None),
        "rebar.shape" => (i.shape.clone(), None),
        "rebar.bar_type" => (i.bar_type.clone(), None),
        "rebar.quantity" => (i.count.to_string(), Some(i.count as f64)),
        "rebar.spacing" => spacing(i).map_or((String::new(), None), mm),
        "rebar.bar_length" => i.rounded_lengths.iter().copied().reduce(f64::max).map_or((String::new(), None), mm),
        "rebar.total_length" => mm(i.total_length),
        "rebar.mass" => (format!("{:.1}", i.total_length * mass_per_metre(i.diameter)), Some(i.total_length * mass_per_metre(i.diameter))),
        "rebar.description" => (description(i), None),
        k => {
            let letter = k.strip_prefix("rebar.")?;
            let (_, v) = i.dimensions.iter().find(|(n, _)| n == letter)?;
            mm(segment(store, *v))
        }
    })
}

/// Formats a summed rebar field's total.
pub fn total_text(key: &str, v: f64) -> String {
    match key {
        "rebar.quantity" => format!("{v:.0}"),
        "rebar.mass" => format!("{v:.1}"),
        _ => format!("{:.0}", v * 1000.0),
    }
}

/// Where a rebar tag hangs from: the middle bar's middle (plan).
pub fn anchor(store: &Store, e: &Element) -> Option<[f64; 2]> {
    let r = rebar::rebar_recipe(e)?;
    let g = rebar::geometry(store, &r).ok()?;
    let b = g.bars.get(g.bars.len() / 2)?;
    let p = b.points.get(b.points.len() / 2)?;
    let q = b.points.get(b.points.len().saturating_sub(1) / 2)?;
    Some([(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0])
}

// ---- Annotations ----

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MultiKind {
    #[default]
    Aligned,
    Linear,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MultiData {
    pub kind: String,
    pub view: String,
    /// The sets (GlobalIds).
    pub rebar: Vec<String>,
    pub style: MultiKind,
    /// A point the dimension line passes through (plan).
    pub line: [f64; 2],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BendingData {
    pub kind: String,
    pub view: String,
    pub rebar: String,
    /// Where the detail's first point goes (plan).
    pub point: [f64; 2],
    /// Drawn size over real size.
    #[serde(default = "quarter")]
    pub scale: f64,
    /// Degrees.
    #[serde(default)]
    pub rotation: f64,
}

fn quarter() -> f64 {
    0.25
}

fn anno<T: serde::de::DeserializeOwned>(e: &Element, kind: &str) -> Option<T> {
    if !is_annotation(e) {
        return None;
    }
    let v = e.param(ANNO_KEY)?;
    (v.get("kind")?.as_str()? == kind).then(|| serde_json::from_value(v.clone()).ok())?
}

pub fn multi_data(e: &Element) -> Option<MultiData> {
    anno(e, KIND_MULTI)
}

pub fn bending_data(e: &Element) -> Option<BendingData> {
    anno(e, KIND_BENDING)
}

fn by_guid<'a>(store: &'a Store, guid: &str) -> Option<&'a Element> {
    GlobalId::parse(guid).and_then(|g| store.find_by_guid(&g)).and_then(|i| store.get(i))
}

/// Whether an annotation goes with `guid`: a multi-rebar annotation whose last live set it is,
/// or a bending detail of it.
pub fn uses(store: &Store, e: &Element, guid: &str) -> bool {
    if let Some(d) = bending_data(e) {
        return d.rebar == guid;
    }
    multi_data(e).is_some_and(|d| {
        let live: Vec<&String> = d.rebar.iter().filter(|g| by_guid(store, g).is_some()).collect();
        live.len() == 1 && live[0] == guid
    })
}

/// A multi-rebar annotation as drawn: the dimension's points (the extent's ends on the
/// references), the witness direction, the line point and the text.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MultiDrawn {
    pub points: Vec<[f64; 2]>,
    pub along: [f64; 2],
    pub line: [f64; 2],
    pub extent: f64,
    pub text: String,
}

/// The sets' bar direction in plan and extent ends, refusing sets that are not parallel.
fn measure(store: &Store, sets: &[&Element], style: MultiKind, line: [f64; 2]) -> Result<MultiDrawn> {
    let mut dir: Option<[f64; 2]> = None;
    let mut mids: Vec<[f64; 2]> = Vec::new();
    let mut infos = Vec::new();
    for e in sets {
        let i = rebar::info(store, rebar::InfoParams { rebar_ids: vec![ElementRef::Id(e.id())] })?.remove(0);
        for b in &i.bars {
            let (p, q) = (b.points[0], b.points[b.points.len() - 1]);
            let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
            let l = dx.hypot(dy);
            if l > 1e-6 {
                let d = [dx / l, dy / l];
                match dir {
                    None => dir = Some(d),
                    Some(o) if (o[0] * d[1] - o[1] * d[0]).abs() > 1e-3 => {
                        return Err(Error::new("multi_rebar.not_parallel", "the sets' bars are not parallel"));
                    }
                    _ => {}
                }
            }
            let m = b.points[b.points.len() / 2];
            let n = b.points[(b.points.len() - 1) / 2];
            mids.push([(m[0] + n[0]) / 2.0, (m[1] + n[1]) / 2.0]);
        }
        infos.push(i);
    }
    let dir = dir.ok_or_else(|| Error::new("multi_rebar.no_direction", "the sets have no bars running in plan"))?;
    // Witness lines run along the bars (aligned) or along the nearest axis (linear).
    let along = match style {
        MultiKind::Aligned => dir,
        MultiKind::Linear if dir[0].abs() >= dir[1].abs() => [1.0, 0.0],
        MultiKind::Linear => [0.0, 1.0],
    };
    let across = [-along[1], along[0]];
    let t = |p: &[f64; 2]| p[0] * across[0] + p[1] * across[1];
    let lo = mids.iter().copied().min_by(|a, b| t(a).total_cmp(&t(b))).expect("bars");
    let hi = mids.iter().copied().max_by(|a, b| t(a).total_cmp(&t(b))).expect("bars");
    let text = if infos.len() == 1 {
        description(&infos[0])
    } else {
        let n: usize = infos.iter().map(|i| i.count).sum();
        let same = infos.windows(2).all(|w| w[0].bar_type == w[1].bar_type && spacing(&w[0]).map(|s| (s * 1000.0).round()) == spacing(&w[1]).map(|s| (s * 1000.0).round()));
        match spacing(&infos[0]) {
            Some(s) if same => format!("{n} {} @ {:.0}", infos[0].bar_type, s * 1000.0),
            _ => infos.iter().map(description).collect::<Vec<_>>().join(" + "),
        }
    };
    Ok(MultiDrawn { points: vec![lo, hi], along, line, extent: (t(&hi) - t(&lo)).abs(), text })
}

/// A bending detail as drawn: line segments, and labelled text lines with their positions.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct BendingDrawn {
    pub segments: Vec<[[f64; 2]; 2]>,
    /// The shape code and each segment value, from the detail's corner downwards.
    pub lines: Vec<String>,
    /// Where the text starts (plan) and the text height (model metres at the detail's scale).
    pub text_at: [f64; 2],
    pub dimensions: Vec<(String, f64)>,
}

fn bending(store: &Store, d: &BendingData) -> Result<BendingDrawn> {
    let e = by_guid(store, &d.rebar).ok_or_else(|| Error::new("bending_detail.no_rebar", "the bending detail's set is gone"))?;
    let i = rebar::info(store, rebar::InfoParams { rebar_ids: vec![ElementRef::Id(e.id())] })?.remove(0);
    let bar = i.bars.get(i.bars.len() / 2).ok_or_else(|| Error::new("bending_detail.no_bars", "the set has no bars"))?;
    let pts = &bar.points;
    // The bar's own plane: along its first run, and across in the plane of its bends.
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let norm = |a: [f64; 3]| {
        let l = dot(a, a).sqrt().max(1e-12);
        [a[0] / l, a[1] / l, a[2] / l]
    };
    let u = norm(sub(pts[pts.len() - 1], pts[0]));
    let u = if dot(u, u) < 0.5 { norm(sub(pts[1], pts[0])) } else { u };
    let mut v = [0.0, 0.0, 0.0];
    for p in pts {
        let w = sub(*p, pts[0]);
        let off = sub(w, [u[0] * dot(w, u), u[1] * dot(w, u), u[2] * dot(w, u)]);
        if dot(off, off) > dot(v, v) {
            v = off;
        }
    }
    let v = if dot(v, v) < 1e-12 { [-u[1], u[0], 0.0] } else { norm(v) };
    let (c, s) = (d.rotation.to_radians().cos(), d.rotation.to_radians().sin());
    let flat = |p: [f64; 3]| {
        let w = sub(p, pts[0]);
        let (x, y) = (dot(w, u) * d.scale, dot(w, v) * d.scale);
        [d.point[0] + x * c - y * s, d.point[1] + x * s + y * c]
    };
    let segments: Vec<[[f64; 2]; 2]> = pts.windows(2).map(|w| [flat(w[0]), flat(w[1])]).collect();
    let dims: Vec<(String, f64)> = i.dimensions.iter().map(|(n, x)| (n.clone(), segment(store, *x))).collect();
    let mut lines = vec![format!("{} {} shape {}", i.number.map(|n| format!("{n}:")).unwrap_or_default(), i.bar_type, i.shape).trim().to_owned()];
    lines.extend(dims.iter().map(|(n, x)| format!("{n} {:.0}", x * 1000.0)));
    let lowest = segments.iter().flatten().map(|p| p[1]).fold(d.point[1], f64::min);
    Ok(BendingDrawn { segments, lines, text_at: [d.point[0], lowest - 0.05], dimensions: dims })
}

/// One rebar annotation of a view, as drawn now.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Drawn {
    Multi { id: ElementId, data: MultiData, drawn: MultiDrawn },
    Bending { id: ElementId, data: BendingData, drawn: BendingDrawn },
}

/// The rebar annotations of a view (by GlobalId) that can be drawn.
pub fn of_view(store: &Store, view: &str) -> Vec<Drawn> {
    let mut out = Vec::new();
    for e in store.iter().filter(|e| is_annotation(e)) {
        if let Some(d) = multi_data(e).filter(|d| d.view == view) {
            let sets: Vec<&Element> = d.rebar.iter().filter_map(|g| by_guid(store, g)).collect();
            if let Ok(m) = measure(store, &sets, d.style, d.line) {
                out.push(Drawn::Multi { id: e.id(), data: d, drawn: m });
            }
        } else if let Some(d) = bending_data(e).filter(|d| d.view == view)
            && let Ok(b) = bending(store, &d)
        {
            out.push(Drawn::Bending { id: e.id(), data: d, drawn: b });
        }
    }
    out
}

fn plan(store: &Store, r: &ElementRef) -> Result<String> {
    let (id, data) = crate::views::resolve_view(store, r)?;
    if !crate::plans::is_plan(&data.kind) {
        return Err(Error::new("anno.view_not_supported", "rebar annotations are placed in plans for now"));
    }
    Ok(store.get(id).expect("resolved").guid().to_string())
}

fn sets<'a>(store: &'a Store, ids: &[ElementRef]) -> Result<Vec<&'a Element>> {
    ids.iter()
        .map(|r| {
            let id = store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
            let e = store.get(id).expect("resolved");
            if rebar::rebar_recipe(e).is_none() {
                return Err(Error::new("rebar.not_rebar", format!("element {id} is not a rebar set")));
            }
            Ok(e)
        })
        .collect()
}

fn create(txn: &mut crate::txn::Transaction<'_>, view: &str, value: Value) -> Result<ElementId> {
    let guid = GlobalId::new();
    txn.create(NewElement::new(format!("{CLASS_PREFIX}{view}/{}", guid.as_str())).guid(guid).param(ANNO_KEY, value))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct MultiParams {
    pub rebar_ids: Vec<ElementRef>,
    pub view_id: ElementRef,
    #[serde(default)]
    pub kind: MultiKind,
    /// A point the dimension line passes through (plan).
    pub line: [f64; 2],
}

/// Places a multi-rebar annotation (`multi_rebar_annotation.place`).
pub(crate) fn place_multi(txn: &mut crate::txn::Transaction<'_>, p: MultiParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let view = plan(&store, &p.view_id)?;
    let s = sets(&store, &p.rebar_ids)?;
    if s.is_empty() {
        return Err(Error::new("multi_rebar.no_sets", "pick one or more rebar sets"));
    }
    measure(&store, &s, p.kind, p.line)?;
    let d = MultiData { kind: KIND_MULTI.into(), view: view.clone(), rebar: s.iter().map(|e| e.guid().to_string()).collect(), style: p.kind, line: p.line };
    create(txn, &view, serde_json::to_value(&d).expect("serialises"))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BendingParams {
    pub rebar_id: ElementRef,
    pub view_id: ElementRef,
    pub point: [f64; 2],
    /// Drawn size over real size (default 0.25).
    #[serde(default)]
    pub scale: Option<f64>,
    #[serde(default)]
    pub rotation: f64,
}

/// Places a bending detail (`bending_detail.place`).
pub(crate) fn place_bending(txn: &mut crate::txn::Transaction<'_>, p: BendingParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let view = plan(&store, &p.view_id)?;
    let e = sets(&store, std::slice::from_ref(&p.rebar_id))?[0];
    let scale = p.scale.unwrap_or(0.25);
    if !(scale > 0.0 && scale <= 10.0) {
        return Err(Error::new("bending_detail.scale", "a bending detail's scale is more than 0 and at most 10"));
    }
    let d = BendingData { kind: KIND_BENDING.into(), view: view.clone(), rebar: e.guid().to_string(), point: p.point, scale, rotation: p.rotation };
    bending(&store, &d)?;
    create(txn, &view, serde_json::to_value(&d).expect("serialises"))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct TagParams {
    pub rebar_id: ElementRef,
    pub view_id: ElementRef,
    /// The tag's centre (plan); on the middle bar when omitted.
    #[serde(default)]
    pub point: Option<[f64; 2]>,
    /// Default the Rebar Tag.
    #[serde(default)]
    pub tag_type_id: Option<String>,
}

/// Tags a rebar set (`rebar_tag.place`).
pub(crate) fn place_tag(txn: &mut crate::txn::Transaction<'_>, p: TagParams) -> Result<ElementId> {
    crate::notes::create_tag(
        txn,
        crate::notes::CreateTagParams {
            view_id: p.view_id,
            host_ref: p.rebar_id,
            position: p.point,
            tag_type_id: Some(p.tag_type_id.unwrap_or_else(|| "Rebar Tag".into())),
            orientation: None,
            leader: Some(p.point.is_some()),
        },
    )
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct InfoParams {
    pub view_id: ElementRef,
}

/// The rebar annotations of a plan as drawn (`rebar_annotation.list`).
pub fn list(store: &Store, p: InfoParams) -> Result<Vec<Drawn>> {
    Ok(of_view(store, &plan(store, &p.view_id)?))
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ScheduleParams {
    #[serde(default)]
    pub name: Option<String>,
}

/// Makes a bar bending schedule (`rebar_schedule.create`): rebar sets grouped by partition and
/// number with shape, bar type, segments A–E, quantity, lengths and mass.
pub(crate) fn create_schedule(txn: &mut crate::txn::Transaction<'_>, p: ScheduleParams) -> Result<ElementId> {
    let fields = [
        "rebar.partition",
        "rebar.number",
        "rebar.shape",
        "rebar.bar_type",
        "rebar.A",
        "rebar.B",
        "rebar.C",
        "rebar.D",
        "rebar.E",
        "rebar.quantity",
        "rebar.bar_length",
        "rebar.total_length",
        "rebar.mass",
    ];
    let id = crate::schedules::create(
        txn,
        crate::schedules::CreateParams {
            category: "cat.structural_rebar".into(),
            name: Some(p.name.unwrap_or_else(|| "Bar Bending Schedule".into())),
            fields: Some(fields.iter().map(|f| (*f).to_owned()).collect()),
        },
    )?;
    let store = txn.store().clone();
    let mut def = store.get(id).and_then(crate::schedules::schedule_def).expect("made");
    def.sort = vec![
        crate::schedules::SortLevel { field: "rebar.partition".into(), descending: false },
        crate::schedules::SortLevel { field: "rebar.number".into(), descending: false },
    ];
    def.itemize = false;
    txn.set_param(id, crate::schedules::SCHEDULE_KEY, json!(def))?;
    Ok(id)
}
