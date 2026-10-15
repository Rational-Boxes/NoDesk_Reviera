// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Datum extents (spec Ch 3 §3.2.4, ARC-DATUM-001) and scope box assignments (ARC-SCOPE-001) of
//! levels, grids and reference planes.
//!
//! A datum has its **3D (model) extent**, shared by every view that shows it: a grid's or
//! reference plane's line in plan and, in sections and elevations, its height range
//! (`rxd:datum_extent.z`); a level's extent in plan (`rxd:datum_extent.plan`; none: across every
//! view). A view may override it with a **2D extent** of its own (`rxd:datum_views`, keyed by
//! the view's GlobalId): a plan its line, a section or elevation its range along the view
//! (`range`: heights for grids and reference planes, distances across the view for levels).
//! Each view also has its own bubble flags and elbows (jogs near a bubble).
//!
//! A datum may be assigned to a scope box: it shows only in views whose plane crosses the box, and
//! its 3D extent is the box's (its line clipped to the box, its heights the box's).
//!
//! Not built yet: alignment locks between datum ends, and elbows on levels.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::Line;

/// Parameter holding a datum's 3D extent and scope box.
pub const EXTENT_KEY: &str = "rxd:datum_extent";
/// Parameter holding a datum's 2D extents, bubbles and elbows per view.
pub const VIEWS_KEY: &str = "rxd:datum_views";
/// An elbow's default offset (paper millimetres).
pub const ELBOW_MM: f64 = 5.0;
/// How far beyond the model a maximized extent reaches (metres).
const MARGIN: f64 = 1.0;

/// A datum's 3D extent beyond its own line (grids, reference planes) or level.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DatumExtent {
    /// Heights in sections and elevations (grids, reference planes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub z: Option<[f64; 2]>,
    /// A level's extent in plan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<Line>,
    /// The scope box (GlobalId).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_box: Option<String>,
}

/// A datum in one view: its 2D extent, bubbles and elbows.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ViewExtent {
    /// A plan's own line (grids, reference planes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<Line>,
    /// A section's or elevation's own range: heights (grids, reference planes) or distances
    /// across the view (levels).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<[f64; 2]>,
    /// Bubbles at the start and the end in this view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bubbles: Option<[bool; 2]>,
    /// Elbows near the start and the end: the jog's offset (paper millimetres).
    #[serde(default, skip_serializing_if = "is_none2")]
    pub elbows: [Option<f64>; 2],
}

fn is_none2(e: &[Option<f64>; 2]) -> bool {
    e[0].is_none() && e[1].is_none()
}

impl ViewExtent {
    fn is_empty(&self) -> bool {
        self.line.is_none()
            && self.range.is_none()
            && self.bubbles.is_none()
            && is_none2(&self.elbows)
    }

    /// Whether it overrides the 3D extent.
    pub fn is_2d(&self) -> bool {
        self.line.is_some() || self.range.is_some()
    }
}

/// What kind of datum an element is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Level,
    Grid,
    RefPlane,
}

pub fn kind_of(e: &Element) -> Option<Kind> {
    if e.class == crate::levels::CLASS {
        Some(Kind::Level)
    } else if crate::grids::is_grid(e) {
        Some(Kind::Grid)
    } else if crate::ref_planes::recipe(e).is_some() {
        Some(Kind::RefPlane)
    } else {
        None
    }
}

pub fn extent(e: &Element) -> DatumExtent {
    e.param(EXTENT_KEY)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

pub fn views(e: &Element) -> BTreeMap<String, ViewExtent> {
    e.param(VIEWS_KEY)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// A datum's settings in one view (GlobalId).
pub fn in_view(e: &Element, view: &str) -> ViewExtent {
    views(e).remove(view).unwrap_or_default()
}

/// A grid's or reference plane's 3D line in plan.
pub fn model_line(e: &Element) -> Option<Line> {
    if let Some(g) = crate::grids::grid_data(e) {
        return Some(g.curve);
    }
    crate::ref_planes::recipe(e).map(|r| Line {
        start: [r.start[0], r.start[1]],
        end: [r.end[0], r.end[1]],
    })
}

/// A datum's line in a plan view (GlobalId): its 2D line there, else its 3D line.
pub fn plan_line(e: &Element, view: Option<&str>) -> Option<Line> {
    view.and_then(|v| in_view(e, v).line)
        .or_else(|| model_line(e))
}

/// The scope box a datum is assigned to, as a box, if it still exists.
pub fn scope_box_of(store: &Store, e: &Element) -> Option<crate::scope_boxes::ScopeBox> {
    let guid = extent(e).scope_box?;
    let id = GlobalId::parse(&guid).and_then(|g| store.find_by_guid(&g))?;
    crate::scope_boxes::recipe(store.get(id)?)
}

/// Whether a datum shows in a plan cut at `cut` (its scope box must reach the cut).
pub fn shows_in_plan(store: &Store, e: &Element, cut: f64) -> bool {
    scope_box_of(store, e)
        .is_none_or(|b| cut >= b.bottom - 1e-9 && cut <= b.bottom + b.height + 1e-9)
}

/// Whether a datum shows in a section along `frame` (its scope box must reach the view's plane
/// or depth).
pub fn shows_in_section(store: &Store, e: &Element, frame: &crate::sections::SectionFrame) -> bool {
    scope_box_of(store, e).is_none_or(|b| {
        b.corners().iter().any(|c| {
            let q = frame.to_local([c[0], c[1], 0.0]);
            q[2] <= 1e-9 && q[2] >= -frame.far - 1e-9
        })
    })
}

fn write_views(
    txn: &mut Transaction<'_>,
    id: ElementId,
    v: BTreeMap<String, ViewExtent>,
) -> Result<()> {
    let v: BTreeMap<String, ViewExtent> = v.into_iter().filter(|(_, x)| !x.is_empty()).collect();
    txn.modify(id, |e| {
        if v.is_empty() {
            e.params.remove(VIEWS_KEY);
        } else {
            e.params.insert(
                VIEWS_KEY.into(),
                serde_json::to_value(&v).expect("serialises"),
            );
        }
    })
}

fn write_extent(txn: &mut Transaction<'_>, id: ElementId, x: &DatumExtent) -> Result<()> {
    let empty = x == &DatumExtent::default();
    let value = serde_json::to_value(x).expect("serialises");
    txn.modify(id, |e| {
        if empty {
            e.params.remove(EXTENT_KEY);
        } else {
            e.params.insert(EXTENT_KEY.into(), value);
        }
    })
}

fn resolve_datum(store: &Store, r: &ElementRef) -> Result<(ElementId, Kind)> {
    store
        .resolve(r)
        .and_then(|id| Some((id, kind_of(store.get(id)?)?)))
        .ok_or_else(|| {
            Error::new(
                "datum.not_found",
                format!("{r:?} is not a level, grid or reference plane"),
            )
        })
}

/// A view: its GlobalId and whether it is a plan (else a section or elevation).
fn resolve_view(
    store: &Store,
    r: &ElementRef,
) -> Result<(String, Option<crate::sections::SectionFrame>)> {
    let e = store
        .resolve(r)
        .and_then(|id| store.get(id))
        .filter(|e| crate::views::is_view(e))
        .ok_or_else(|| Error::new("view.not_found", format!("no view {r:?}")))?;
    let data = crate::views::view_data(e).expect("a view");
    let frame = crate::sections::section_of(e).map(|(_, d)| d.frame());
    if frame.is_none() && !crate::plans::is_plan(&data.kind) {
        return Err(Error::new(
            "datum.view_kind",
            "datum extents are set in plans, sections and elevations",
        ));
    }
    Ok((e.guid().to_string(), frame))
}

/// The default heights a grid spans in sections: from below the lowest level to above the
/// highest (and the model).
pub fn default_heights(store: &Store) -> [f64; 2] {
    let levels = crate::levels::list(store);
    let lo = levels.first().map_or(0.0, |l| l.elevation);
    let hi = levels.last().map_or(3.0, |l| l.elevation);
    [lo - MARGIN, hi + MARGIN]
}

/// A grid's or reference plane's heights in a section (view GlobalId): its 2D range there, its
/// scope box's heights or its own 3D heights; `None` when none is set (the view's default).
pub fn heights(store: &Store, e: &Element, view: Option<&str>) -> Option<[f64; 2]> {
    if let Some(r) = view.and_then(|v| in_view(e, v).range) {
        return Some(r);
    }
    if let Some(b) = scope_box_of(store, e) {
        return Some([b.bottom, b.bottom + b.height]);
    }
    extent(e).z
}

/// A level's range across a section (frame distances), or `None` to cross the whole view.
pub fn level_range(
    store: &Store,
    e: &Element,
    view: Option<&str>,
    frame: &crate::sections::SectionFrame,
) -> Option<[f64; 2]> {
    if let Some(r) = view.and_then(|v| in_view(e, v).range) {
        return Some(r);
    }
    let _ = store;
    let plan = extent(e).plan?;
    let (a, b) = (
        frame.to_local([plan.start[0], plan.start[1], 0.0])[0],
        frame.to_local([plan.end[0], plan.end[1], 0.0])[0],
    );
    Some([a.min(b), a.max(b)])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum End {
    Start,
    End,
}

impl End {
    fn index(self) -> usize {
        match self {
            End::Start => 0,
            End::End => 1,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetExtentParams {
    pub datum_id: ElementRef,
    /// The view the end is dragged in; omitted, the 3D extent in plan.
    pub view_id: Option<ElementRef>,
    /// Change the 3D extent from this view (in a section: a grid's heights, a level's plan extent)
    /// rather than the view's own 2D extent.
    #[serde(default)]
    pub model: bool,
    pub end: End,
    /// In a plan (or without a view): `[x, y]`. In a section or elevation: `[across, height]`.
    pub point: [f64; 2],
}

/// `datum.set_extent`.
pub(crate) fn set_extent(txn: &mut Transaction<'_>, p: SetExtentParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, kind) = resolve_datum(&store, &p.datum_id)?;
    let e = store.get(id).expect("resolved");
    let i = p.end.index();
    let view = p
        .view_id
        .as_ref()
        .map(|v| resolve_view(&store, v))
        .transpose()?;
    match (kind, &view) {
        // Plan ends of grids and reference planes.
        (Kind::Grid | Kind::RefPlane, None | Some((_, None))) => {
            let base = plan_line(e, view.as_ref().map(|v| v.0.as_str()).filter(|_| !p.model))
                .expect("a line");
            let mut l = base;
            if i == 0 {
                l.start = p.point;
            } else {
                l.end = p.point;
            }
            if l.length() < 0.01 {
                return Err(Error::new(
                    "datum.too_short",
                    "the datum would be shorter than 10 mm",
                ));
            }
            match view {
                Some((guid, _)) if !p.model => {
                    let mut all = views(e);
                    all.entry(guid.clone()).or_default().line = Some(l);
                    write_views(txn, id, all)
                }
                _ => set_model_line(txn, id, kind, l),
            }
        }
        // Heights of grids and reference planes in a section.
        (Kind::Grid | Kind::RefPlane, Some((guid, Some(_)))) => {
            let mut r = heights(&store, e, (!p.model).then_some(guid.as_str()))
                .unwrap_or_else(|| default_heights(&store));
            r[i] = p.point[1];
            if r[1] - r[0] < 0.01 {
                return Err(Error::new(
                    "datum.too_short",
                    "the top must be above the bottom",
                ));
            }
            if p.model {
                let mut x = extent(e);
                x.z = Some(r);
                write_extent(txn, id, &x)
            } else {
                let mut all = views(e);
                all.entry(guid.clone()).or_default().range = Some(r);
                write_views(txn, id, all)
            }
        }
        // A level's extent across a section, or in plan.
        (Kind::Level, Some((guid, Some(frame)))) => {
            let current = level_range(&store, e, (!p.model).then_some(guid.as_str()), frame)
                .unwrap_or([0.0, frame.width]);
            let mut r = current;
            r[i] = p.point[0];
            if r[1] - r[0] < 0.01 {
                return Err(Error::new("datum.too_short", "the ends would cross"));
            }
            if p.model {
                let mut x = extent(e);
                let a = frame.to_world([r[0], 0.0, 0.0]);
                let b = frame.to_world([r[1], 0.0, 0.0]);
                x.plan = Some(Line {
                    start: [a[0], a[1]],
                    end: [b[0], b[1]],
                });
                write_extent(txn, id, &x)
            } else {
                let mut all = views(e);
                all.entry(guid.clone()).or_default().range = Some(r);
                write_views(txn, id, all)
            }
        }
        (Kind::Level, _) => {
            // In plan: the 3D extent's ends.
            let mut x = extent(e);
            let mut l = x.plan.unwrap_or(Line {
                start: p.point,
                end: p.point,
            });
            if i == 0 {
                l.start = p.point;
            } else {
                l.end = p.point;
            }
            x.plan = (l.length() >= 0.01).then_some(l);
            write_extent(txn, id, &x)
        }
    }
}

fn set_model_line(txn: &mut Transaction<'_>, id: ElementId, kind: Kind, l: Line) -> Result<()> {
    match kind {
        Kind::Grid => crate::grids::set_curve(
            txn,
            crate::grids::SetCurveParams {
                grid_id: ElementRef::Id(id),
                curve: crate::walls::CurveArg {
                    line: l,
                    through: Vec::new(),
                },
            },
        ),
        Kind::RefPlane => {
            let mut r = crate::ref_planes::recipe(txn.store().get(id).expect("exists"))
                .expect("a reference plane");
            r.start = [l.start[0], l.start[1], r.start[2]];
            r.end = [l.end[0], l.end[1], r.end[2]];
            crate::ref_planes::write(txn, id, &r)
        }
        Kind::Level => Ok(()),
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewDatumsParams {
    pub datum_ids: Vec<ElementRef>,
    pub view_id: ElementRef,
}

/// `datum.reset_to_3d`: removes the datums' 2D extents in a view (bubbles and elbows stay).
pub(crate) fn reset_to_3d(txn: &mut Transaction<'_>, p: ViewDatumsParams) -> Result<()> {
    let store = txn.store().clone();
    let (guid, _) = resolve_view(&store, &p.view_id)?;
    for r in &p.datum_ids {
        let (id, _) = resolve_datum(&store, r)?;
        let mut all = views(store.get(id).expect("resolved"));
        if let Some(v) = all.get_mut(&guid) {
            v.line = None;
            v.range = None;
        }
        write_views(txn, id, all)?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DatumsParams {
    pub datum_ids: Vec<ElementRef>,
}

/// The model's bounding box in plan and height.
fn model_bounds(store: &Store) -> Option<([f64; 3], [f64; 3])> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for b in crate::bodies::all(store) {
        if let Some((a, c)) = b.bounds() {
            for k in 0..3 {
                lo[k] = lo[k].min(a[k]);
                hi[k] = hi[k].max(c[k]);
            }
        }
    }
    lo[0].is_finite().then_some((lo, hi))
}

/// `datum.maximize_3d`: stretches grids and reference planes over the model (in plan and height);
/// a level crosses every view again.
pub(crate) fn maximize_3d(txn: &mut Transaction<'_>, p: DatumsParams) -> Result<()> {
    let store = txn.store().clone();
    let bounds = model_bounds(&store);
    for r in &p.datum_ids {
        let (id, kind) = resolve_datum(&store, r)?;
        let e = store.get(id).expect("resolved");
        let mut x = extent(e);
        match kind {
            Kind::Level => x.plan = None,
            Kind::Grid | Kind::RefPlane => {
                let Some((lo, hi)) = bounds else { continue };
                let l = model_line(e).expect("a line");
                let len = l.length().max(1e-9);
                let d = [(l.end[0] - l.start[0]) / len, (l.end[1] - l.start[1]) / len];
                let along = |p: [f64; 2]| (p[0] - l.start[0]) * d[0] + (p[1] - l.start[1]) * d[1];
                let corners = [
                    [lo[0], lo[1]],
                    [hi[0], lo[1]],
                    [hi[0], hi[1]],
                    [lo[0], hi[1]],
                ];
                let (mut a, mut b) = (0.0f64, len);
                for c in corners {
                    a = a.min(along(c) - MARGIN);
                    b = b.max(along(c) + MARGIN);
                }
                let at = |t: f64| [l.start[0] + d[0] * t, l.start[1] + d[1] * t];
                set_model_line(
                    txn,
                    id,
                    kind,
                    Line {
                        start: at(a),
                        end: at(b),
                    },
                )?;
                let levels = default_heights(&store);
                x.z = Some([lo[2].min(levels[0]) - MARGIN, hi[2].max(levels[1]) + MARGIN]);
            }
        }
        write_extent(txn, id, &x)?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PropagateParams {
    pub datum_ids: Vec<ElementRef>,
    pub source_view_id: ElementRef,
    pub target_view_ids: Vec<ElementRef>,
}

/// `datum.propagate_extents`: copies the datums' 2D extents (and bubbles and elbows) in one view
/// to other views of the same kind (plans to plans; sections and elevations to parallel ones).
pub(crate) fn propagate(txn: &mut Transaction<'_>, p: PropagateParams) -> Result<()> {
    let store = txn.store().clone();
    let (source, sframe) = resolve_view(&store, &p.source_view_id)?;
    let mut targets = Vec::new();
    for t in &p.target_view_ids {
        let (guid, frame) = resolve_view(&store, t)?;
        let compatible = match (&sframe, &frame) {
            (None, None) => true,
            (Some(a), Some(b)) => (a.right[0] * b.right[1] - a.right[1] * b.right[0]).abs() < 1e-6,
            _ => false,
        };
        if !compatible {
            return Err(Error::new(
                "datum.not_parallel",
                "extents propagate between plans, or between parallel sections and elevations",
            ));
        }
        if guid != source {
            targets.push(guid);
        }
    }
    for r in &p.datum_ids {
        let (id, _) = resolve_datum(&store, r)?;
        let mut all = views(store.get(id).expect("resolved"));
        let v = all.get(&source).cloned().unwrap_or_default();
        for t in &targets {
            all.insert(t.clone(), v.clone());
        }
        write_views(txn, id, all)?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BubbleParams {
    pub datum_id: ElementRef,
    pub view_id: ElementRef,
    pub end: End,
    pub visible: bool,
}

/// `datum.set_bubble`: shows or hides a datum's bubble at one end in one view.
pub(crate) fn set_bubble(txn: &mut Transaction<'_>, p: BubbleParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, kind) = resolve_datum(&store, &p.datum_id)?;
    let (guid, _) = resolve_view(&store, &p.view_id)?;
    let e = store.get(id).expect("resolved");
    let mut all = views(e);
    let current = all
        .get(&guid)
        .and_then(|v| v.bubbles)
        .unwrap_or_else(|| match kind {
            Kind::Grid => crate::grids::list(&store)
                .into_iter()
                .find(|g| g.id == id)
                .map_or([false, true], |g| g.bubbles),
            _ => [false, true],
        });
    let mut b = current;
    b[p.end.index()] = p.visible;
    all.entry(guid).or_default().bubbles = Some(b);
    write_views(txn, id, all)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ElbowParams {
    pub datum_id: ElementRef,
    pub view_id: ElementRef,
    pub end: End,
    /// The jog's offset to the left of the line's direction (paper millimetres; negative: to
    /// the right). Omitted: 5. Zero removes the elbow.
    pub offset: Option<f64>,
}

/// `datum.add_elbow`: a jog near a grid's bubble in one view.
pub(crate) fn add_elbow(txn: &mut Transaction<'_>, p: ElbowParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, kind) = resolve_datum(&store, &p.datum_id)?;
    if kind != Kind::Grid {
        return Err(Error::new("datum.elbow", "elbows are on grids for now"));
    }
    let (guid, frame) = resolve_view(&store, &p.view_id)?;
    if frame.is_some() {
        return Err(Error::new("datum.elbow", "grid elbows are added in plans"));
    }
    let offset = p.offset.unwrap_or(ELBOW_MM);
    let mut all = views(store.get(id).expect("resolved"));
    all.entry(guid).or_default().elbows[p.end.index()] = (offset != 0.0).then_some(offset);
    write_views(txn, id, all)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssignParams {
    pub datum_ids: Vec<ElementRef>,
    /// The scope box; `null` clears the assignment (the datums keep their extents).
    pub scope_box_id: Option<ElementRef>,
}

/// `datum.assign_scope_box`.
pub(crate) fn assign_scope_box(txn: &mut Transaction<'_>, p: AssignParams) -> Result<()> {
    let store = txn.store().clone();
    let guid = p
        .scope_box_id
        .as_ref()
        .map(|r| {
            store
                .resolve(r)
                .and_then(|id| store.get(id))
                .filter(|e| crate::scope_boxes::recipe(e).is_some())
                .map(|e| e.guid().to_string())
                .ok_or_else(|| Error::new("scopebox.not_found", format!("no scope box {r:?}")))
        })
        .transpose()?;
    for r in &p.datum_ids {
        let (id, kind) = resolve_datum(&store, r)?;
        let e = store.get(id).expect("resolved");
        let mut x = extent(e);
        // Clearing keeps the box's extent as the datum's own.
        if guid.is_none()
            && let Some(b) = scope_box_of(&store, e)
            && kind != Kind::Level
        {
            x.z = Some([b.bottom, b.bottom + b.height]);
        }
        x.scope_box = guid.clone();
        write_extent(txn, id, &x)?;
        if kind != Kind::Level
            && let Some(g) = &guid
        {
            let b = crate::scope_boxes::recipe(
                store
                    .get(
                        GlobalId::parse(g)
                            .and_then(|g| store.find_by_guid(&g))
                            .expect("checked"),
                    )
                    .expect("exists"),
            )
            .expect("a box");
            fit_to_box(txn, id, kind, &b)?;
        }
    }
    Ok(())
}

/// Clips a grid's or reference plane's line to a scope box's footprint (a datum that misses it
/// keeps its line).
pub(crate) fn fit_to_box(
    txn: &mut Transaction<'_>,
    id: ElementId,
    kind: Kind,
    b: &crate::scope_boxes::ScopeBox,
) -> Result<()> {
    let e = txn.store().get(id).expect("exists").clone();
    let Some(l) = model_line(&e) else {
        return Ok(());
    };
    if let Some(clipped) = b.clip(l) {
        set_model_line(txn, id, kind, clipped)?;
    }
    Ok(())
}

/// Datums assigned to the scope box with GlobalId `guid`.
pub fn assigned_to(store: &Store, guid: &str) -> Vec<ElementId> {
    store
        .iter()
        .filter(|e| kind_of(e).is_some() && extent(e).scope_box.as_deref() == Some(guid))
        .map(Element::id)
        .collect()
}

/// A datum's state in a view, as `datum.get` reports it.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DatumState {
    pub extent: DatumExtentReport,
    /// The view's settings, when a view is given.
    pub view: Option<ViewExtentReport>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DatumExtentReport {
    pub z: Option<[f64; 2]>,
    pub plan: Option<Line>,
    pub line: Option<Line>,
    pub scope_box: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ViewExtentReport {
    /// "3D" or "2D".
    pub mode: String,
    pub line: Option<Line>,
    pub range: Option<[f64; 2]>,
    pub bubbles: Option<[bool; 2]>,
    pub elbows: [Option<f64>; 2],
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetParams {
    pub datum_id: ElementRef,
    pub view_id: Option<ElementRef>,
}

/// `datum.get`.
pub(crate) fn get(store: &Store, p: GetParams) -> Result<DatumState> {
    let (id, _) = resolve_datum(store, &p.datum_id)?;
    let e = store.get(id).expect("resolved");
    let x = extent(e);
    let view = p
        .view_id
        .as_ref()
        .map(|v| resolve_view(store, v))
        .transpose()?
        .map(|(guid, _)| {
            let v = in_view(e, &guid);
            ViewExtentReport {
                mode: if v.is_2d() { "2D" } else { "3D" }.to_owned(),
                line: v.line,
                range: v.range,
                bubbles: v.bubbles,
                elbows: v.elbows,
            }
        });
    Ok(DatumState {
        extent: DatumExtentReport {
            z: x.z,
            plan: x.plan,
            line: model_line(e),
            scope_box: x.scope_box,
        },
        view,
    })
}

/// Keeps a datum's view settings when views are deleted (the stale keys are dropped).
pub fn prune(store: &Store, e: &Element) -> Option<BTreeMap<String, ViewExtent>> {
    let all = views(e);
    let kept: BTreeMap<String, ViewExtent> = all
        .iter()
        .filter(|(k, _)| {
            GlobalId::parse(k)
                .and_then(|g| store.find_by_guid(&g))
                .is_some()
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    (kept.len() != all.len()).then_some(kept)
}
