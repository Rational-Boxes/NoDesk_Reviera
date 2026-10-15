// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Model text (spec Ch 3 §3.10.2, ARC-MTXT-001): three-dimensional lettering on a work plane.
//!
//! A model text is an `IfcBuildingElementProxy` (`ObjectType = "ModelText"`) with
//! `recipe.model_text`: its text (lines split at line breaks), work plane, insertion point in the
//! plane, rotation, letter height, depth and horizontal alignment. The letters are Reviera's
//! stroke font, each stroke thickened into an outline and the outlines of a letter merged, so a
//! letter is one solid with its enclosed holes (the A's triangle); the outline is pushed out of
//! the plane by the depth (a sheet a millimetre thick at depth 0).
//!
//! Not built yet: TrueType fonts (FreeType), bold and italic, the depth-0 `IfcAnnotation`, the
//! cursor preview while placing.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bodies::Mesh;
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::polygons::{self, Shape};
use crate::store::Store;
use crate::stroke_font::{CELL, advance, glyph};
use crate::txn::Transaction;
use crate::walls::RECIPE_KEY;
use crate::work_planes::{PlaneArgs, WorkPlaneRef};

pub const RECIPE: &str = "recipe.model_text";
/// Stroke width as a share of the letter height.
const STROKE: f64 = 0.12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModelTextRecipe {
    pub kind: String,
    pub text: String,
    pub plane: WorkPlaneRef,
    /// The insertion point in the plane's coordinates.
    pub point: [f64; 2],
    #[serde(default)]
    pub rotation: f64,
    /// Letter height (metres).
    pub size: f64,
    /// Out of the plane (metres; 0 for flat).
    #[serde(default)]
    pub depth: f64,
    #[serde(default)]
    pub align: Align,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
}

pub fn recipe(e: &Element) -> Option<ModelTextRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// A line's width in letter heights.
fn width(line: &str) -> f64 {
    let total: f64 = line.chars().map(|c| f64::from(advance(c))).sum();
    (total - 1.5).max(0.0) / f64::from(CELL)
}

/// One letter's outline: its strokes thickened and merged (in letter heights, from its origin).
fn letter(c: char) -> Vec<Shape> {
    let w = STROKE / 2.0;
    let unit = 1.0 / f64::from(CELL);
    let mut rings: Vec<Vec<[f64; 2]>> = Vec::new();
    for line in glyph(c) {
        let pts: Vec<[f64; 2]> = line
            .iter()
            .map(|(x, y)| [f64::from(*x) * unit, f64::from(*y) * unit])
            .collect();
        for pair in pts.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let l = (b[0] - a[0]).hypot(b[1] - a[1]);
            if l < 1e-12 {
                continue;
            }
            let u = [(b[0] - a[0]) / l * w, (b[1] - a[1]) / l * w];
            let n = [-u[1], u[0]];
            // A rectangle with square ends reaching past the points, so joints close.
            rings.push(vec![
                [a[0] - u[0] - n[0], a[1] - u[1] - n[1]],
                [b[0] + u[0] - n[0], b[1] + u[1] - n[1]],
                [b[0] + u[0] + n[0], b[1] + u[1] + n[1]],
                [a[0] - u[0] + n[0], a[1] - u[1] + n[1]],
            ]);
        }
    }
    polygons::union(&rings)
}

/// The letters' outlines in the plane, each `(shape)` per lump.
pub fn outlines(r: &ModelTextRecipe) -> Vec<Shape> {
    let (s, c) = (r.rotation.sin(), r.rotation.cos());
    let mut out = Vec::new();
    for (row, line) in r.text.lines().enumerate() {
        let shift = match r.align {
            Align::Left => 0.0,
            Align::Center => -width(line) / 2.0,
            Align::Right => -width(line),
        };
        let mut x = shift;
        let y = -(row as f64) * 1.6;
        for ch in line.chars() {
            for shape in letter(ch) {
                let place = |p: &[f64; 2]| {
                    let (lx, ly) = ((p[0] + x) * r.size, (p[1] + y) * r.size);
                    [r.point[0] + lx * c - ly * s, r.point[1] + lx * s + ly * c]
                };
                out.push(Shape {
                    outer: shape.outer.iter().map(place).collect(),
                    holes: shape
                        .holes
                        .iter()
                        .map(|h| h.iter().map(place).collect())
                        .collect(),
                });
            }
            x += f64::from(advance(ch)) / f64::from(CELL);
        }
    }
    out
}

/// The text's solids in the model, one per lump.
pub fn regenerate(store: &Store, e: &Element) -> Result<Vec<Mesh>> {
    let r = recipe(e).ok_or_else(|| Error::new("modeltext.not", "not a model text"))?;
    let plane = crate::work_planes::resolve(store, &r.plane)?;
    let depth = if r.depth > 0.0 { r.depth } else { 0.001 };
    Ok(outlines(&r)
        .iter()
        .map(|s| {
            let (pts, idx) = crate::bodies::extrude(&s.outer, &s.holes, 0.0, depth);
            let world = pts
                .iter()
                .map(|p| crate::in_place::lift(&plane, p[0], p[1], p[2]))
                .collect();
            (world, idx)
        })
        .collect())
}

/// Every model text that regenerates.
pub fn all(store: &Store) -> Vec<(ElementId, Vec<Mesh>)> {
    store
        .iter()
        .filter(|e| recipe(e).is_some())
        .filter_map(|e| Some((e.id(), regenerate(store, e).ok()?)))
        .collect()
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub text: String,
    /// The insertion point in the work plane's coordinates (on a level: plan x and y).
    pub point: [f64; 2],
    /// The work plane: `level` (with `offset`), `grid`, `ref_plane` or `plane`; omitted, the
    /// lowest level.
    #[serde(flatten)]
    pub plane: PlaneArgs,
    /// Letter height (default 300 mm).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub size: Option<f64>,
    /// Out of the plane (default 0, flat).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub depth: Option<f64>,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: f64,
    #[serde(default)]
    pub align: Align,
    pub material: Option<String>,
}

/// `modeltext.create`.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    if p.text.trim().is_empty() {
        return Err(Error::new("modeltext.empty", "the text is empty"));
    }
    let plane = if p.plane.given() {
        p.plane.to_ref(&store)?
    } else {
        let lowest = crate::levels::list(&store)
            .into_iter()
            .next()
            .ok_or_else(|| Error::new("level.none", "the model has no level"))?;
        WorkPlaneRef::Level {
            level: lowest.guid,
            offset: p.plane.offset,
        }
    };
    let size = p.size.unwrap_or(0.3);
    let depth = p.depth.unwrap_or(0.0);
    if size <= 0.0 || depth < 0.0 {
        return Err(Error::new(
            "modeltext.size",
            "the size must be greater than 0 and the depth not negative",
        ));
    }
    let r = ModelTextRecipe {
        kind: RECIPE.into(),
        text: p.text,
        plane,
        point: p.point,
        rotation: p.rotation,
        size,
        depth,
        align: p.align,
        material: p.material,
    };
    let id = txn.create(
        NewElement::new("IfcBuildingElementProxy")
            .name(r.text.lines().next().unwrap_or_default())
            .param("@ObjectType", "ModelText")
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )?;
    regenerate(txn.store(), txn.store().get(id).expect("created"))?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub element_id: ElementRef,
    pub text: Option<String>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub size: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub depth: Option<f64>,
    pub align: Option<Align>,
    pub material: Option<String>,
}

/// `modeltext.set_text` (and its other values).
pub(crate) fn set(txn: &mut Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let id = store
        .resolve(&p.element_id)
        .ok_or_else(|| Error::new("element.not_found", "no such model text"))?;
    crate::modify::check_unpinned(&store, id)?;
    let mut r = store
        .get(id)
        .and_then(recipe)
        .ok_or_else(|| Error::new("modeltext.not", "not a model text"))?;
    if let Some(t) = p.text {
        if t.trim().is_empty() {
            return Err(Error::new("modeltext.empty", "the text is empty"));
        }
        r.text = t;
    }
    if let Some(v) = p.size.filter(|v| *v > 0.0) {
        r.size = v;
    }
    if let Some(v) = p.depth.filter(|v| *v >= 0.0) {
        r.depth = v;
    }
    if let Some(a) = p.align {
        r.align = a;
    }
    if let Some(m) = p.material {
        r.material = (!m.trim().is_empty()).then_some(m);
    }
    let name = r.text.lines().next().unwrap_or_default().to_owned();
    let v = serde_json::to_value(&r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
        e.name = Some(name);
    })
}

/// Moves, turns or mirrors a model text by a plan transformation (up by `dz`).
pub(crate) fn transform(
    txn: &mut Transaction<'_>,
    id: ElementId,
    point: &dyn Fn([f64; 2]) -> [f64; 2],
    dz: f64,
) -> Result<()> {
    let store = txn.store().clone();
    let mut r = recipe(store.get(id).expect("exists")).expect("a model text");
    match &mut r.plane {
        WorkPlaneRef::Level { offset, .. } => {
            *offset += dz;
            let (a, b) = (
                point(r.point),
                point([r.point[0] + r.rotation.cos(), r.point[1] + r.rotation.sin()]),
            );
            r.rotation = (b[1] - a[1]).atan2(b[0] - a[0]);
            r.point = a;
        }
        other => {
            *other = crate::work_planes::transform_frame(&store, other, point, dz)?;
        }
    }
    let v = serde_json::to_value(&r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abc_is_three_lumps_and_the_a_has_a_hole() {
        let r = ModelTextRecipe {
            kind: RECIPE.into(),
            text: "ABC".into(),
            plane: WorkPlaneRef::Level {
                level: String::new(),
                offset: 0.0,
            },
            point: [0.0, 0.0],
            rotation: 0.0,
            size: 1.0,
            depth: 0.05,
            align: Align::Left,
            material: None,
        };
        let o = outlines(&r);
        assert_eq!(o.len(), 3, "{o:?}");
        assert_eq!(o[0].holes.len(), 1, "the A's triangle");
    }
}
