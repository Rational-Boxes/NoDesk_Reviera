// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Model lines (spec Ch 3 §3.10.3, ARC-MLIN-001): 3D lines, arcs and splines on a work plane
//! (ARC-WP-001), shown in every view, unlike a view's detail lines.
//!
//! Each curve is its own `IfcAnnotation` (`ObjectType = "ModelLine"`) whose `Annotation`
//! representation is the curve as a 3D polyline, styled with its line style. Its recipe
//! (`recipe.model_line`) holds the work plane and the curve in the plane's coordinates (a level's
//! plane: plan x and y), so the line follows its plane: raise the level, move the reference
//! plane, and the line goes with it.
//!
//! Line styles are the project's `Lines` subcategories ([`STYLES`]): a pen and a pattern.
//!
//! Not built yet: converting to detail lines (`line.convert`), user line styles.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::{CurveArg, Line, RECIPE_KEY};
use crate::work_planes::{self, PlaneArgs, WorkPlaneRef};

pub const RECIPE: &str = "recipe.model_line";
pub const OBJECT_TYPE: &str = "ModelLine";
pub const DEFAULT_STYLE: &str = "Lines";
const MIN_LENGTH: f64 = 0.001;

/// A line pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Pattern {
    Solid,
    /// Dashes (paper millimetres: 3 on, 1.5 off).
    Dashed,
    /// Long dash, short dash (centre lines).
    Centre,
}

/// A line style: a `Lines` subcategory.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
pub struct LineStyle {
    pub name: &'static str,
    /// Projection pen (1 thinnest … 16).
    pub pen: u8,
    pub pattern: Pattern,
    /// Colour, sRGB 0–255.
    pub color: [u8; 3],
}

pub const STYLES: [LineStyle; 7] = [
    LineStyle {
        name: "Lines",
        pen: 1,
        pattern: Pattern::Solid,
        color: [0, 0, 0],
    },
    LineStyle {
        name: "Thin Lines",
        pen: 1,
        pattern: Pattern::Solid,
        color: [0, 0, 0],
    },
    LineStyle {
        name: "Medium Lines",
        pen: 3,
        pattern: Pattern::Solid,
        color: [0, 0, 0],
    },
    LineStyle {
        name: "Wide Lines",
        pen: 5,
        pattern: Pattern::Solid,
        color: [0, 0, 0],
    },
    LineStyle {
        name: "Hidden Lines",
        pen: 2,
        pattern: Pattern::Dashed,
        color: [0, 0, 0],
    },
    LineStyle {
        name: "Centre Lines",
        pen: 1,
        pattern: Pattern::Centre,
        color: [0, 0, 0],
    },
    LineStyle {
        name: "Overhead Lines",
        pen: 1,
        pattern: Pattern::Dashed,
        color: [64, 64, 64],
    },
];

pub fn style(name: &str) -> Option<&'static LineStyle> {
    STYLES.iter().find(|s| s.name.eq_ignore_ascii_case(name))
}

/// The curve of a model line in its plane's coordinates (a level's plane: plan x and y): from
/// `start` to `end`, through `through` (one point: an arc; more: a spline).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LocalCurve {
    pub start: [f64; 2],
    pub end: [f64; 2],
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub through: Vec<[f64; 2]>,
}

impl LocalCurve {
    /// The curve sampled as a polyline (plane coordinates).
    pub fn points(&self) -> Vec<[f64; 2]> {
        let line = Line {
            start: self.start,
            end: self.end,
        };
        let mut pts = vec![self.start];
        pts.extend(crate::sketch::interior(line, &self.through));
        pts.push(self.end);
        pts
    }

    fn map(&self, f: &dyn Fn([f64; 2]) -> [f64; 2]) -> Self {
        Self {
            start: f(self.start),
            end: f(self.end),
            through: self.through.iter().map(|p| f(*p)).collect(),
        }
    }
}

/// The stored recipe of a model line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModelLineRecipe {
    pub kind: String,
    pub plane: WorkPlaneRef,
    pub curve: LocalCurve,
    pub line_style: String,
}

pub fn recipe(e: &Element) -> Option<ModelLineRecipe> {
    let r: ModelLineRecipe = serde_json::from_value(e.param(RECIPE_KEY)?.clone()).ok()?;
    (r.kind == RECIPE).then_some(r)
}

/// A model line as it is now: its world polyline and style.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ModelLineInfo {
    pub id: ElementId,
    pub points: Vec<[f64; 3]>,
    pub line_style: String,
    /// The work plane's label ("Level: Level 1", "<not associated>").
    pub work_plane: String,
}

/// The world polyline of a model line.
pub fn world_points(store: &Store, r: &ModelLineRecipe) -> Result<Vec<[f64; 3]>> {
    let plane = work_planes::resolve(store, &r.plane)?;
    Ok(r.curve
        .points()
        .into_iter()
        .map(|p| plane.to_world(p))
        .collect())
}

/// Every model line whose plane resolves.
pub fn all(store: &Store) -> Vec<ModelLineInfo> {
    store
        .iter()
        .filter_map(|e| {
            let r = recipe(e)?;
            Some(ModelLineInfo {
                id: e.id(),
                points: world_points(store, &r).ok()?,
                work_plane: work_planes::label(store, &r.plane),
                line_style: r.line_style,
            })
        })
        .collect()
}

/// `modelline.list`.
pub(crate) fn list_command(
    store: &Store,
    _: crate::builtin::NoParams,
) -> Result<Vec<ModelLineInfo>> {
    Ok(all(store))
}

/// `modelline.styles`.
pub(crate) fn styles_command(_: &Store, _: crate::builtin::NoParams) -> Result<Vec<LineStyle>> {
    Ok(STYLES.to_vec())
}

fn check_curve(c: &LocalCurve) -> Result<()> {
    let pts = c.points();
    let length: f64 = pts
        .windows(2)
        .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
        .sum();
    if length < MIN_LENGTH {
        return Err(Error::new(
            "modelline.too_short",
            "a model line must be at least 1 mm long",
        ));
    }
    Ok(())
}

fn checked_style(name: Option<String>) -> Result<String> {
    let name = name.unwrap_or_else(|| DEFAULT_STYLE.to_owned());
    style(&name).map(|s| s.name.to_owned()).ok_or_else(|| {
        Error::new(
            "modelline.style",
            format!(
                "no line style {name:?}; the styles are {}",
                STYLES.map(|s| s.name).join(", ")
            ),
        )
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// The curves, in the work plane's coordinates (on a level: plan x and y; on a vertical plane:
    /// along its line from its start, and up). `through` points make arcs and splines.
    pub curves: Vec<CurveArg>,
    /// The work plane: one of `level` (with `offset`), `grid`, `ref_plane` or `plane`. Omitted:
    /// the lowest level.
    #[serde(flatten)]
    pub plane: PlaneArgs,
    /// A line style (`modelline.styles`); "Lines" by default.
    pub line_style: Option<String>,
}

/// `modelline.create`: one model line per curve.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    if p.curves.is_empty() {
        return Err(Error::new("modelline.no_curves", "give at least one curve"));
    }
    let plane = if p.plane.given() {
        p.plane.to_ref(&store)?
    } else {
        let lowest = crate::levels::list(&store)
            .into_iter()
            .next()
            .ok_or_else(|| Error::new("level.none", "the model has no level to draw on"))?;
        WorkPlaneRef::Level {
            level: lowest.guid,
            offset: p.plane.offset,
        }
    };
    let line_style = checked_style(p.line_style)?;
    let mut out = Vec::new();
    for c in p.curves {
        let curve = LocalCurve {
            start: c.line.start,
            end: c.line.end,
            through: c.through,
        };
        check_curve(&curve)?;
        let r = ModelLineRecipe {
            kind: RECIPE.into(),
            plane: plane.clone(),
            curve,
            line_style: line_style.clone(),
        };
        let id = txn.create(
            NewElement::new("IfcAnnotation")
                .name("Model Line")
                .param("@ObjectType", OBJECT_TYPE),
        )?;
        write(txn, id, &r)?;
        out.push(id);
    }
    Ok(out)
}

pub(crate) fn write(txn: &mut Transaction<'_>, id: ElementId, r: &ModelLineRecipe) -> Result<()> {
    check_curve(&r.curve)?;
    world_points(txn.store(), r)?;
    let value = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub model_line_ids: Vec<ElementRef>,
    /// A new curve (work plane coordinates); only with one line.
    pub curve: Option<CurveArg>,
    pub line_style: Option<String>,
}

/// `modelline.set`.
pub(crate) fn set(txn: &mut Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    if p.curve.is_some() && p.model_line_ids.len() != 1 {
        return Err(Error::new(
            "modelline.one_curve",
            "a new curve is given for one model line at a time",
        ));
    }
    let style = p.line_style.map(|s| checked_style(Some(s))).transpose()?;
    for r in &p.model_line_ids {
        let (id, mut rec) = store
            .resolve(r)
            .and_then(|id| Some((id, recipe(store.get(id)?)?)))
            .ok_or_else(|| Error::new("modelline.not_found", format!("no model line {r:?}")))?;
        if let Some(c) = &p.curve {
            rec.curve = LocalCurve {
                start: c.line.start,
                end: c.line.end,
                through: c.through.clone(),
            };
        }
        if let Some(s) = &style {
            rec.line_style = s.clone();
        }
        write(txn, id, &rec)?;
    }
    Ok(())
}

/// Puts a model line on another work plane where it stands (its points projected onto the
/// plane).
pub(crate) fn set_plane(
    txn: &mut Transaction<'_>,
    id: ElementId,
    plane: WorkPlaneRef,
) -> Result<()> {
    let store = txn.store().clone();
    let mut r = recipe(store.get(id).expect("exists")).expect("a model line");
    let from = work_planes::resolve(&store, &r.plane)?;
    let to = work_planes::resolve(&store, &plane)?;
    r.curve = r.curve.map(&|p| to.to_local(from.to_world(p)));
    r.plane = plane;
    write(txn, id, &r)
}

/// Moves, turns or mirrors a model line in plan (and raises it by `dz`). On a level its points
/// move in the level's plane (raised by the level offset); on another plane the line keeps its
/// place in a plane fixed in space, moved with it.
pub(crate) fn transform(
    txn: &mut Transaction<'_>,
    id: ElementId,
    point: &dyn Fn([f64; 2]) -> [f64; 2],
    dz: f64,
) -> Result<()> {
    let store = txn.store().clone();
    let mut r = recipe(store.get(id).expect("exists")).expect("a model line");
    match &mut r.plane {
        WorkPlaneRef::Level { offset, .. } => {
            *offset += dz;
            r.curve = r.curve.map(point);
        }
        other => {
            *other = work_planes::transform_frame(&store, other, point, dz)?;
        }
    }
    write(txn, id, &r)
}

/// Whether a model line is drawn on the element with GlobalId `guid`.
pub fn uses(r: &ModelLineRecipe, guid: &str) -> bool {
    work_planes::host_of(&r.plane) == Some(guid)
}

/// A polyline cut into dashes of `on` and `off` lengths (model metres) following its pattern.
pub fn dashes(points: &[[f64; 3]], pattern: Pattern, unit: f64) -> Vec<[[f64; 3]; 2]> {
    let seq: &[(f64, bool)] = match pattern {
        Pattern::Solid => {
            return points.windows(2).map(|w| [w[0], w[1]]).collect();
        }
        Pattern::Dashed => &[(3.0, true), (1.5, false)],
        Pattern::Centre => &[(8.0, true), (1.5, false), (2.0, true), (1.5, false)],
    };
    let mut out = Vec::new();
    let (mut k, mut left) = (0usize, seq[0].0 * unit);
    for w in points.windows(2) {
        let d = work_planes::sub(w[1], w[0]);
        let len = work_planes::dot(d, d).sqrt();
        let mut t = 0.0;
        while t < len - 1e-12 {
            let step = left.min(len - t);
            if seq[k].1 {
                let a = t / len;
                let b = (t + step) / len;
                let at = |s: f64| [w[0][0] + d[0] * s, w[0][1] + d[1] * s, w[0][2] + d[2] * s];
                out.push([at(a), at(b)]);
            }
            t += step;
            left -= step;
            if left <= 1e-12 {
                k = (k + 1) % seq.len();
                left = seq[k].0 * unit;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashes_follow_the_pattern() {
        let pts = [[0.0, 0.0, 0.0], [9.0, 0.0, 0.0]];
        let d = dashes(&pts, Pattern::Dashed, 1.0);
        // 3 on, 1.5 off: [0,3], [4.5,7.5], then 9 is reached in the gap.
        assert_eq!(d.len(), 2);
        assert!((d[1][0][0] - 4.5).abs() < 1e-9 && (d[1][1][0] - 7.5).abs() < 1e-9);
        assert_eq!(dashes(&pts, Pattern::Solid, 1.0).len(), 1);
    }
}
