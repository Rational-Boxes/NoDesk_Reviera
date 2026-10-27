// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! View annotation (spec Ch 5 §5.13): aligned dimensions (DOC-ANN-001).
//!
//! Annotation objects are RXD-only elements owned by a view: class
//! `rxd:views/annotation/<viewId>/<objectId>`, data in the `rxd:anno` parameter. Each object is
//! its own RXD document under the view's annotation prefix, so objects merge one by one.
//!
//! A dimension references model geometry by stable sub-references: for Reviera walls,
//! `face:ext`, `face:int` and `axis` (the location line), which follow the wall when it moves.
//! Grids are referenced by their line (`axis`), which follows the grid.
//! Other geometry (cut edges of imported elements) is referenced as `line` with the line kept in
//! the reference. A reference whose element no longer exists is *orphaned*: the dimension keeps
//! its last position (DOC-ANN-000).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::views;
use crate::walls;

pub const CLASS_PREFIX: &str = "rxd:views/annotation/";
pub const ANNO_KEY: &str = "rxd:anno";
pub const KIND_DIMENSION: &str = "anno.dimension";

/// A reference of a dimension: a straight line in the view's 2D coordinates, plan x and y in plans
/// and the frame's x (to the right) and height in sections and elevations (all references of an
/// aligned dimension are parallel).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DimRef {
    /// The model element (GlobalId), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element: Option<String>,
    /// `face:ext`, `face:int`, `axis`, `end:start`, `end:end` (Reviera walls; `axis` for grids
    /// too), or `line`; in sections and elevations also `base` and `top` (walls) and `level`
    /// (levels).
    pub sub: String,
    /// A point on the reference line where it was picked (metres, the view's coordinates).
    pub point: [f64; 2],
    /// The reference line's direction, unit length.
    pub direction: [f64; 2],
}

/// Stored data of an aligned dimension (`anno.dimension`, mode `aligned`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DimensionData {
    pub kind: String,
    pub mode: String,
    /// The view (GlobalId) that owns the dimension.
    pub view: String,
    pub refs: Vec<DimRef>,
    /// The dimension line: a point it passes through (plan, metres).
    pub line: [f64; 2],
}

/// A dimension as it measures now.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Measured {
    /// Unit direction of measurement (across the references).
    pub across: [f64; 2],
    /// Unit direction of the references (along the dimension line's witness lines).
    pub along: [f64; 2],
    /// Each reference: a point on its line nearest to where it was picked, now.
    pub points: Vec<[f64; 2]>,
    /// Each reference's position measured across.
    pub positions: Vec<f64>,
    /// Segment values (distances between consecutive references), metres.
    pub values: Vec<f64>,
    /// References whose element no longer exists.
    pub orphaned: Vec<bool>,
}

fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

/// Whether an element is annotation.
pub fn is_annotation(e: &Element) -> bool {
    e.class.starts_with(CLASS_PREFIX)
}

pub fn dimension_data(e: &Element) -> Option<DimensionData> {
    if !is_annotation(e) {
        return None;
    }
    let d: DimensionData = serde_json::from_value(e.param(ANNO_KEY)?.clone()).ok()?;
    (d.kind == KIND_DIMENSION).then_some(d)
}

/// The current line of a reference: a point and a direction; `None` when its element is gone.
pub fn resolve_ref(store: &Store, r: &DimRef) -> Option<([f64; 2], [f64; 2])> {
    let Some(guid) = &r.element else {
        return Some((r.point, r.direction));
    };
    let e = store.get(store.find_by_guid(&GlobalId::parse(guid)?)?)?;
    if r.sub == "line" {
        return Some((r.point, r.direction));
    }
    if let Some(g) = crate::grids::grid_data(e) {
        let c = g.curve;
        let length = c.length().max(f64::EPSILON);
        let dir = [
            (c.end[0] - c.start[0]) / length,
            (c.end[1] - c.start[1]) / length,
        ];
        return (r.sub == "axis").then_some((c.start, dir));
    }
    // A vertical reference plane, by its line (ARC-REFP-001).
    if let Some(rp) = crate::ref_planes::recipe(e) {
        let (a, b) = ([rp.start[0], rp.start[1]], [rp.end[0], rp.end[1]]);
        let length = (b[0] - a[0]).hypot(b[1] - a[1]).max(f64::EPSILON);
        let dir = [(b[0] - a[0]) / length, (b[1] - a[1]) / length];
        return (r.sub == "axis").then_some((a, dir));
    }
    let g = walls::regenerate(store, e).ok()?;
    let recipe = walls::wall_recipe(e)?;
    let n = g.normal();
    // A wall's end, square to it (the gap of Split with Gap, ARC-WALL-011).
    match r.sub.as_str() {
        "end:start" => return Some((g.start, n)),
        "end:end" => return Some((g.end, n)),
        _ => {}
    }
    // The exterior is on the left (+normal) unless the wall is flipped.
    let ext = if recipe.flip { -1.0 } else { 1.0 };
    let offset = match r.sub.as_str() {
        "face:ext" => ext * g.width / 2.0,
        "face:int" => -ext * g.width / 2.0,
        "axis" => 0.0,
        _ => return None,
    };
    let start = [g.start[0] + n[0] * offset, g.start[1] + n[1] * offset];
    Some((start, g.direction()))
}

/// The current line of a reference in a section's or elevation's frame (x to the right, y up),
/// `None` when its element is gone or does not show there as a line:
/// - a level: across the view at its elevation (`level`);
/// - a Reviera wall: its base and top (`base`, `top`); a face or its axis where it meets the
///   view plane, or, seen edge on, where it stands (`face:ext`, `face:int`, `axis`); its ends
///   (`end:start`, `end:end`);
/// - a grid where it crosses the view (`axis`), and a free `line` as it is.
pub fn resolve_ref_in_section(store: &Store, r: &DimRef, frame: &crate::sections::SectionFrame) -> Option<([f64; 2], [f64; 2])> {
    const ACROSS: [f64; 2] = [1.0, 0.0];
    const UP: [f64; 2] = [0.0, 1.0];
    let Some(guid) = &r.element else {
        return Some((r.point, r.direction));
    };
    let e = store.get(store.find_by_guid(&GlobalId::parse(guid)?)?)?;
    if r.sub == "line" {
        return Some((r.point, r.direction));
    }
    if e.class == crate::levels::CLASS {
        return (r.sub == "level").then(|| ([0.0, crate::levels::elevation(e)], ACROSS));
    }
    if let Some(g) = crate::grids::grid_data(e) {
        let x = frame.grid_crossing(g.curve.start, g.curve.end)?;
        return (r.sub == "axis").then_some(([x, 0.0], UP));
    }
    let recipe = walls::wall_recipe(e)?;
    let g = walls::regenerate(store, e).ok()?;
    let x_of = |p: [f64; 2]| frame.to_local([p[0], p[1], 0.0])[0];
    match r.sub.as_str() {
        "base" => return Some(([0.0, g.base_z], ACROSS)),
        "top" => return Some(([0.0, g.base_z + g.height], ACROSS)),
        "end:start" => return Some(([x_of(g.start), 0.0], UP)),
        "end:end" => return Some(([x_of(g.end), 0.0], UP)),
        _ => {}
    }
    let ext = if recipe.flip { -1.0 } else { 1.0 };
    let offset = match r.sub.as_str() {
        "face:ext" => ext * g.width / 2.0,
        "face:int" => -ext * g.width / 2.0,
        "axis" => 0.0,
        _ => return None,
    };
    let n = g.normal();
    let shift = |p: [f64; 2]| [p[0] + n[0] * offset, p[1] + n[1] * offset];
    let (a, b) = (shift(g.start), shift(g.end));
    if let Some(x) = frame.crossing(a, b) {
        return Some(([x, 0.0], UP));
    }
    // Seen edge on: the face runs along the view direction.
    let dir = g.direction();
    (dot(dir, frame.look).abs() > 1.0 - 1e-6).then(|| ([x_of(a), 0.0], UP))
}

/// The line of a reference in the view `view` (GlobalId): in plan, or in a section's frame.
pub fn resolve_ref_in(store: &Store, r: &DimRef, view: &str) -> Option<([f64; 2], [f64; 2])> {
    match crate::sections::frame_of_view(store, view) {
        Some((frame, _)) => resolve_ref_in_section(store, r, &frame),
        None => resolve_ref(store, r),
    }
}

/// Measures a dimension against the current model.
pub fn measure(store: &Store, d: &DimensionData) -> Option<Measured> {
    let frame = crate::sections::frame_of_view(store, &d.view).map(|(f, _)| f);
    let resolve = |r: &DimRef| match &frame {
        Some(f) => resolve_ref_in_section(store, r, f),
        None => resolve_ref(store, r),
    };
    let first = d.refs.first()?;
    let along = first.direction;
    let across = [-along[1], along[0]];
    let mut points = Vec::new();
    let mut orphaned = Vec::new();
    for r in &d.refs {
        let (p, dir) = match resolve(r) {
            Some(line) => {
                orphaned.push(false);
                line
            }
            None => {
                orphaned.push(true);
                (r.point, r.direction)
            }
        };
        // The point of the reference line nearest to where it was picked.
        let t = dot([r.point[0] - p[0], r.point[1] - p[1]], dir);
        points.push([p[0] + dir[0] * t, p[1] + dir[1] * t]);
    }
    let positions: Vec<f64> = points.iter().map(|p| dot(*p, across)).collect();
    let values = positions.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
    Some(Measured {
        across,
        along,
        points,
        positions,
        values,
        orphaned,
    })
}

/// The annotation of a view.
pub fn of_view(store: &Store, view_guid: &str) -> Vec<(ElementId, DimensionData)> {
    store
        .iter()
        .filter_map(|e| {
            let d = dimension_data(e)?;
            (d.view == view_guid).then(|| (e.id(), d))
        })
        .collect()
}

// ---- Commands ----

/// A reference argument of `dim.create_aligned`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DimRefArg {
    /// The element (ID or GlobalId); omitted for a free line.
    pub element: Option<ElementRef>,
    /// `face:ext`, `face:int`, `axis` (Reviera walls), or `line` (with `point` and `direction`);
    /// in sections and elevations also `base`, `top` (walls), `end:start`, `end:end` and `level`.
    pub sub: String,
    /// Where the reference was picked (metres; plan, or the section's x and height).
    pub point: [f64; 2],
    /// Direction of a `line` reference; taken from the wall for wall references.
    pub direction: Option<[f64; 2]>,
}

/// A stored reference from its argument: the element's GlobalId, and the direction taken from
/// the element (or normalised, for a `line`), in plan.
pub fn ref_of(store: &Store, a: &DimRefArg) -> Result<DimRef> {
    ref_in(store, a, None)
}

/// A stored reference in view `view` (GlobalId), a plan or a section.
pub fn ref_in(store: &Store, a: &DimRefArg, view: Option<&str>) -> Result<DimRef> {
    let element = match &a.element {
        Some(r) => {
            let id = store
                .resolve(r)
                .ok_or_else(|| Error::new("element.not_found", "no such element"))?;
            Some(store.get(id).expect("resolved").guid().to_string())
        }
        None => None,
    };
    let mut r = DimRef {
        element,
        sub: a.sub.clone(),
        point: a.point,
        direction: a.direction.unwrap_or([1.0, 0.0]),
    };
    if r.sub != "line" {
        let line = match view {
            Some(v) => resolve_ref_in(store, &r, v),
            None => resolve_ref(store, &r),
        };
        let (_, dir) = line.ok_or_else(|| {
            Error::new(
                "dim.bad_ref",
                format!("`{}` is not a reference of that element", r.sub),
            )
        })?;
        r.direction = dir;
    } else {
        let d = r.direction;
        let len = d[0].hypot(d[1]);
        if len < 1e-9 {
            return Err(Error::new(
                "dim.bad_ref",
                "a line reference needs a direction",
            ));
        }
        r.direction = [d[0] / len, d[1] / len];
    }
    Ok(r)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateAlignedParams {
    pub view_id: ElementRef,
    /// Two or more parallel references, in order.
    pub refs: Vec<DimRefArg>,
    /// A point the dimension line passes through.
    pub line_point: [f64; 2],
}

pub(crate) fn create_aligned(
    txn: &mut crate::txn::Transaction<'_>,
    p: CreateAlignedParams,
) -> Result<ElementId> {
    let store = txn.store().clone();
    let (view_id, _) = views::resolve_view(&store, &p.view_id)?;
    let view_guid = store.get(view_id).expect("resolved").guid().to_string();
    if p.refs.len() < 2 {
        return Err(Error::new(
            "dim.too_few_refs",
            "a dimension needs at least two references",
        ));
    }
    let refs = p
        .refs
        .iter()
        .map(|a| ref_in(&store, a, Some(&view_guid)))
        .collect::<Result<Vec<_>>>()?;
    let along = refs[0].direction;
    if refs
        .iter()
        .any(|r| (along[0] * r.direction[1] - along[1] * r.direction[0]).abs() > 1e-3)
    {
        return Err(Error::new(
            "dim.not_parallel",
            "the references of an aligned dimension must be parallel",
        ));
    }
    let data = DimensionData {
        kind: KIND_DIMENSION.into(),
        mode: "aligned".into(),
        view: view_guid.clone(),
        refs,
        line: p.line_point,
    };
    let measured = measure(&store, &data).expect("two references");
    if measured.values.iter().any(|v| *v < 1e-6) {
        return Err(Error::new(
            "dim.zero_length",
            "two references are at the same place",
        ));
    }
    let guid = GlobalId::new();
    txn.create(
        NewElement::new(format!("{CLASS_PREFIX}{view_guid}/{}", guid.as_str()))
            .guid(guid)
            .param(ANNO_KEY, serde_json::to_value(&data).expect("serialises")),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    pub view_id: ElementRef,
}

/// One dimension of `anno.list`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DimensionInfo {
    pub id: ElementId,
    pub data: DimensionData,
    pub measured: Option<Measured>,
}

pub(crate) fn list(store: &Store, p: ListParams) -> Result<Vec<DimensionInfo>> {
    let (id, _) = views::resolve_view(store, &p.view_id)?;
    let guid = store.get(id).expect("resolved").guid().to_string();
    Ok(of_view(store, &guid)
        .into_iter()
        .map(|(id, data)| DimensionInfo {
            id,
            measured: measure(store, &data),
            data,
        })
        .collect())
}

/// The annotation of a view element (dimensions, tags, text), for `element.delete` dependents.
pub fn owned_by(store: &Store, view: &Element) -> Vec<ElementId> {
    let prefix = format!("{CLASS_PREFIX}{}/", view.guid());
    store
        .iter()
        .filter(|e| e.class.starts_with(&prefix))
        .map(Element::id)
        .collect()
}
