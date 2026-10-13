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

/// A reference of a dimension: a straight line in plan (all references of an aligned dimension
/// are parallel).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DimRef {
    /// The model element (GlobalId), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element: Option<String>,
    /// `face:ext`, `face:int`, `axis` (Reviera walls; `axis` for grids too), or `line`.
    pub sub: String,
    /// A point on the reference line where it was picked (metres, plan).
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
    let g = walls::regenerate(store, e).ok()?;
    let recipe = walls::wall_recipe(e)?;
    let n = g.normal();
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

/// Measures a dimension against the current model.
pub fn measure(store: &Store, d: &DimensionData) -> Option<Measured> {
    let first = d.refs.first()?;
    let along = first.direction;
    let across = [-along[1], along[0]];
    let mut points = Vec::new();
    let mut orphaned = Vec::new();
    for r in &d.refs {
        let (p, dir) = match resolve_ref(store, r) {
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
    /// `face:ext`, `face:int`, `axis` (Reviera walls), or `line` (with `point` and `direction`).
    pub sub: String,
    /// Where the reference was picked (plan, metres).
    pub point: [f64; 2],
    /// Direction of a `line` reference; taken from the wall for wall references.
    pub direction: Option<[f64; 2]>,
}

/// A stored reference from its argument: the element's GlobalId, and the direction taken from
/// the element (or normalised, for a `line`).
pub fn ref_of(store: &Store, a: &DimRefArg) -> Result<DimRef> {
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
        let (_, dir) = resolve_ref(store, &r).ok_or_else(|| {
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
    if p.refs.len() < 2 {
        return Err(Error::new(
            "dim.too_few_refs",
            "a dimension needs at least two references",
        ));
    }
    let refs = p
        .refs
        .iter()
        .map(|a| ref_of(&store, a))
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
    let view_guid = store.get(view_id).expect("resolved").guid().to_string();
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

/// The annotation of a view element, for `element.delete` dependents.
pub fn owned_by(store: &Store, view: &Element) -> Vec<ElementId> {
    let guid = view.guid().to_string();
    of_view(store, &guid)
        .into_iter()
        .map(|(id, _)| id)
        .collect()
}
