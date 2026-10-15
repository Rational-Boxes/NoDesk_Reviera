// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Ramps (ARC-RAMP-001): sloped runs between two levels, with landings.
//!
//! A ramp is an `IfcRamp` with a `recipe.ramp`: its centreline path in plan (straight runs from
//! point to point), its width, and its vertical extent ([`Extent`]). Its type (`IfcRampType` with a
//! `recipe.ramp_type`) gives the maximum slope (1 in `max_slope`, 12 by default), the maximum
//! incline length (a run longer than it is split by landings), the thickness and the shape: a
//! *thick* slab under the sloped surface, or a *solid* one down to the base.
//!
//! - Each path segment is a run; where two runs meet, a square landing (the ramp's width) turns
//!   the corner.
//! - A run longer than the maximum incline length is split into equal flights with a landing as
//!   deep as the ramp is wide between each two; the landings lengthen the ramp along the path.
//! - The height is shared by the flights in proportion to their lengths, so the slope is the
//!   same on every flight. A slope steeper than the type's maximum is reported as a warning;
//!   the ramp is still made.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::bodies::Mesh;
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::levels::Extent;
use crate::store::Store;
use crate::walls::{RECIPE_KEY, TopConstraint};

pub const RAMP_RECIPE: &str = "recipe.ramp";
pub const RAMP_TYPE_RECIPE: &str = "recipe.ramp_type";
pub const DEFAULT_TYPE_NAME: &str = "Ramp 1:12";

/// How a ramp's body is made.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RampShape {
    /// A slab of the type's thickness under the sloped surface.
    #[default]
    Thick,
    /// Solid down to the base.
    Solid,
}

/// The stored recipe of a ramp type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RampType {
    pub kind: String,
    /// The maximum slope is 1 in this (12 for 1:12).
    pub max_slope: f64,
    /// A run longer than this is split by landings (metres).
    pub max_incline_length: f64,
    pub thickness: f64,
    #[serde(default)]
    pub shape: RampShape,
}

impl Default for RampType {
    fn default() -> Self {
        Self {
            kind: RAMP_TYPE_RECIPE.into(),
            max_slope: 12.0,
            max_incline_length: 9.0,
            thickness: 0.15,
            shape: RampShape::Thick,
        }
    }
}

pub fn ramp_type(e: &Element) -> Option<RampType> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == RAMP_TYPE_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// The stored recipe of a ramp.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RampRecipe {
    pub kind: String,
    /// The centreline in plan, from the bottom up (metres).
    pub path: Vec<[f64; 2]>,
    pub width: f64,
    #[serde(flatten)]
    pub extent: Extent,
}

pub fn ramp_recipe(e: &Element) -> Option<RampRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == RAMP_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

/// One piece of a ramp: a sloped flight or a level landing, as a plan quadrilateral (corners
/// counter-clockwise) with the height at each corner.
#[derive(Debug, Clone, PartialEq)]
pub struct RampPiece {
    pub landing: bool,
    pub corners: [[f64; 2]; 4],
    pub z: [f64; 4],
}

/// A ramp as it is now.
#[derive(Debug, Clone, PartialEq)]
pub struct RampGeometry {
    pub ty: RampType,
    pub z0: f64,
    pub height: f64,
    pub pieces: Vec<RampPiece>,
    /// The sloped length (the flights', horizontal).
    pub sloped_length: f64,
    /// The slope (rise over run).
    pub slope: f64,
    /// The horizontal length the type's maximum slope needs for this height.
    pub required_length: f64,
}

impl RampGeometry {
    /// Whether the ramp is steeper than its type allows.
    pub fn too_steep(&self) -> bool {
        self.slope > 1.0 / self.ty.max_slope + 1e-9
    }

    /// A piece's closed mesh: its top as given, its bottom the thickness below (thick) or the
    /// base (solid).
    pub fn mesh(&self, p: &RampPiece) -> Mesh {
        let bottom = |z: f64| match self.ty.shape {
            RampShape::Thick => (z - self.ty.thickness).max(self.z0 - self.ty.thickness),
            RampShape::Solid => self.z0 - self.ty.thickness.min(0.05),
        };
        let mut positions: Vec<[f64; 3]> =
            (0..4).map(|i| [p.corners[i][0], p.corners[i][1], bottom(p.z[i])]).collect();
        positions.extend((0..4).map(|i| [p.corners[i][0], p.corners[i][1], p.z[i]]));
        let indices = vec![
            0, 2, 1, 0, 3, 2, // bottom
            4, 5, 6, 4, 6, 7, // top
            0, 1, 5, 0, 5, 4, //
            1, 2, 6, 1, 6, 5, //
            2, 3, 7, 2, 7, 6, //
            3, 0, 4, 3, 4, 7,
        ];
        (positions, indices)
    }
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn unit(v: [f64; 2]) -> [f64; 2] {
    let l = v[0].hypot(v[1]).max(f64::EPSILON);
    [v[0] / l, v[1] / l]
}

fn at(p: [f64; 2], d: [f64; 2], k: f64) -> [f64; 2] {
    [p[0] + d[0] * k, p[1] + d[1] * k]
}

/// The pieces of a ramp from its path, width, base, height and type (pure, for tests).
pub fn layout(
    path: &[[f64; 2]],
    width: f64,
    z0: f64,
    height: f64,
    ty: &RampType,
) -> Result<(Vec<RampPiece>, f64)> {
    if path.len() < 2 {
        return Err(Error::new("ramp.bad_path", "a ramp needs at least two points"));
    }
    let half = width / 2.0;
    // Each segment's usable run: less half a landing at each corner.
    let n = path.len() - 1;
    // The flights of each segment (start, end along it), and the landings between them.
    enum Part {
        Flight([f64; 2], [f64; 2], [f64; 2]), // start, end, direction
        Landing([[f64; 2]; 4]),
    }
    let mut parts: Vec<Part> = Vec::new();
    for i in 0..n {
        let (a, b) = (path[i], path[i + 1]);
        let d = unit(sub(b, a));
        let len = sub(b, a)[0].hypot(sub(b, a)[1]);
        let s0 = if i > 0 { half } else { 0.0 };
        let s1 = if i + 1 < n { len - half } else { len };
        if s1 - s0 < 0.1 {
            return Err(Error::new(
                "ramp.too_short",
                "a run is too short for the ramp's width at its corners",
            ));
        }
        // Split by landings into equal flights no longer than the maximum incline length; each
        // landing lengthens the run along the path.
        let run = s1 - s0;
        let count = (run / ty.max_incline_length - 1e-9).ceil().max(1.0) as usize;
        let flight = run / count as f64;
        let mut s = s0;
        let mut origin = a;
        for k in 0..count {
            let start = at(origin, d, s);
            let end = at(origin, d, s + flight);
            parts.push(Part::Flight(start, end, d));
            s += flight;
            if k + 1 < count {
                // A landing as deep as the ramp is wide, pushing the rest along.
                let n = [-d[1], d[0]];
                let l0 = at(origin, d, s);
                let l1 = at(l0, d, width);
                parts.push(Part::Landing([
                    at(l0, n, -half),
                    at(l1, n, -half),
                    at(l1, n, half),
                    at(l0, n, half),
                ]));
                origin = at(origin, d, width);
            }
        }
        // The corner landing to the next segment, centred on the corner (moved with the
        // landings inserted on this segment).
        if i + 1 < n {
            let shift = sub(origin, a);
            let c = [b[0] + shift[0], b[1] + shift[1]];
            let n1 = [-d[1], d[0]];
            parts.push(Part::Landing([
                at(at(c, d, -half), n1, -half),
                at(at(c, d, half), n1, -half),
                at(at(c, d, half), n1, half),
                at(at(c, d, -half), n1, half),
            ]));
            // Later segments start from the moved corner.
            if shift != [0.0, 0.0] {
                return Err(Error::new(
                    "ramp.landing_on_turning_run",
                    "a run that needs landings must be the last one; shorten it or add a corner",
                ));
            }
        }
    }
    let sloped: f64 = parts
        .iter()
        .map(|p| match p {
            Part::Flight(a, b, _) => sub(*b, *a)[0].hypot(sub(*b, *a)[1]),
            Part::Landing(_) => 0.0,
        })
        .sum();
    let slope = height / sloped.max(f64::EPSILON);
    let mut z = z0;
    let mut pieces = Vec::new();
    for p in parts {
        match p {
            Part::Flight(a, b, d) => {
                let n = [-d[1], d[0]];
                let len = sub(b, a)[0].hypot(sub(b, a)[1]);
                let z1 = z + len * slope;
                pieces.push(RampPiece {
                    landing: false,
                    corners: [at(a, n, -half), at(b, n, -half), at(b, n, half), at(a, n, half)],
                    z: [z, z1, z1, z],
                });
                z = z1;
            }
            Part::Landing(corners) => pieces.push(RampPiece {
                landing: true,
                corners,
                z: [z; 4],
            }),
        }
    }
    Ok((pieces, sloped))
}

fn type_of(store: &Store, e: &Element) -> RampType {
    e.param("@type")
        .and_then(Value::as_str)
        .and_then(|g| find(store, g))
        .and_then(|t| store.get(t))
        .and_then(ramp_type)
        .unwrap_or_default()
}

/// Regenerates a ramp from its recipe, type and levels.
pub fn regenerate(store: &Store, e: &Element) -> Result<RampGeometry> {
    let r = ramp_recipe(e).ok_or_else(|| Error::new("ramp.not_reviera", "not a Reviera ramp"))?;
    let ty = type_of(store, e);
    let (z0, z1) = r.extent.z(store)?;
    let height = z1 - z0;
    let (pieces, sloped) = layout(&r.path, r.width, z0, height, &ty)?;
    Ok(RampGeometry {
        z0,
        height,
        pieces,
        sloped_length: sloped,
        slope: height / sloped.max(f64::EPSILON),
        required_length: height * ty.max_slope,
        ty,
    })
}

/// Every Reviera ramp that regenerates.
pub fn all(store: &Store) -> Vec<(ElementId, RampGeometry)> {
    store
        .iter()
        .filter(|e| ramp_recipe(e).is_some())
        .filter_map(|e| Some((e.id(), regenerate(store, e).ok()?)))
        .collect()
}

fn default_type(txn: &mut crate::txn::Transaction<'_>) -> Result<ElementId> {
    if let Some(e) = txn
        .store()
        .iter()
        .filter(|e| ramp_type(e).is_some())
        .min_by_key(|e| e.name.as_deref() != Some(DEFAULT_TYPE_NAME))
    {
        return Ok(e.id());
    }
    txn.create(
        NewElement::new("IfcRampType")
            .name(DEFAULT_TYPE_NAME)
            .param("@PredefinedType", "STRAIGHT_RUN_RAMP")
            .param(
                RECIPE_KEY,
                serde_json::to_value(RampType::default()).expect("serialises"),
            ),
    )
}

fn warn_if_steep(txn: &mut crate::txn::Transaction<'_>, id: ElementId) -> Result<()> {
    let store = txn.store().clone();
    let g = regenerate(&store, store.get(id).expect("exists"))?;
    if g.too_steep() {
        txn.warn(crate::txn::Warning {
            id: "arc.ramp.too_steep".into(),
            message: format!(
                "Ramp slope exceeds maximum: 1:{:.1} where the type allows 1:{:.0}; it needs {:.2} m of run",
                1.0 / g.slope,
                g.ty.max_slope,
                g.required_length
            ),
            element_ids: vec![id],
        });
    }
    Ok(())
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// A ramp type; omitted, the default 1:12 type, made on first use.
    pub type_id: Option<ElementRef>,
    /// The centreline in plan from the bottom up: two points or more (metres).
    pub path: Vec<[f64; 2]>,
    /// Default 1.2 m.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub width: Option<f64>,
    pub base_level: ElementRef,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base_offset: f64,
    pub top_constraint: TopConstraint,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub top_offset: f64,
}

fn resolve_type(store: &Store, r: &ElementRef) -> Result<ElementId> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such ramp type"))?;
    if ramp_type(store.get(id).expect("resolved")).is_none() {
        return Err(Error::new(
            "ramp.not_a_type",
            format!("element {id} is not a ramp type"),
        ));
    }
    Ok(id)
}

/// Makes a ramp (`ramp.create`).
pub(crate) fn create(txn: &mut crate::txn::Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let type_id = match &p.type_id {
        Some(r) => resolve_type(&store, r)?,
        None => default_type(txn)?,
    };
    let width = p.width.unwrap_or(1.2);
    if !(0.3..=20.0).contains(&width) {
        return Err(Error::new("ramp.bad_width", "the width must be between 0.3 m and 20 m"));
    }
    let store = txn.store().clone();
    let extent = Extent::from_args(
        &store,
        &p.base_level,
        p.base_offset,
        p.top_constraint,
        p.top_offset,
    )?;
    let recipe = RampRecipe {
        kind: RAMP_RECIPE.into(),
        path: p.path,
        width,
        extent,
    };
    let guid = store.get(type_id).expect("resolved").guid().to_string();
    let id = txn.create(
        NewElement::new("IfcRamp")
            .param("@PredefinedType", "STRAIGHT_RUN_RAMP")
            .param("@type", guid)
            .param(RECIPE_KEY, serde_json::to_value(&recipe).expect("serialises")),
    )?;
    warn_if_steep(txn, id)?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub ramp_ids: Vec<ElementRef>,
    pub path: Option<Vec<[f64; 2]>>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub width: Option<f64>,
    pub base_level: Option<ElementRef>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base_offset: Option<f64>,
    pub top_constraint: Option<TopConstraint>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub top_offset: Option<f64>,
}

/// Changes ramps (`ramp.set`); omitted values are kept.
pub(crate) fn set(txn: &mut crate::txn::Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    for r in &p.ramp_ids {
        let id = store
            .resolve(r)
            .ok_or_else(|| Error::new("element.not_found", "no such ramp"))?;
        let mut rr = ramp_recipe(store.get(id).expect("resolved")).ok_or_else(|| {
            Error::new("ramp.not_reviera", format!("element {id} is not a Reviera ramp"))
        })?;
        crate::modify::check_unpinned(&store, id)?;
        if let Some(path) = &p.path {
            rr.path = path.clone();
        }
        if let Some(w) = p.width {
            if !(0.3..=20.0).contains(&w) {
                return Err(Error::new("ramp.bad_width", "the width must be between 0.3 m and 20 m"));
            }
            rr.width = w;
        }
        let base = match &p.base_level {
            Some(b) => b.clone(),
            None => ElementRef::Id(find(&store, &rr.extent.base_level).ok_or_else(|| {
                Error::new("level.missing", "the ramp's base level is gone")
            })?),
        };
        let top = match p.top_constraint {
            Some(t) => t,
            None => match rr.extent.top_level().and_then(|g| find(&store, g)) {
                Some(l) => TopConstraint::Level(crate::walls::ElementRefValue::Id(l)),
                None => TopConstraint::Unconnected(
                    rr.extent.top["unconnected"].as_f64().unwrap_or(0.6),
                ),
            },
        };
        rr.extent = Extent::from_args(
            &store,
            &base,
            p.base_offset.unwrap_or(rr.extent.base_offset),
            top,
            p.top_offset.unwrap_or(rr.extent.top_offset),
        )?;
        write(txn, id, &rr)?;
        warn_if_steep(txn, id)?;
    }
    Ok(())
}

fn write(txn: &mut crate::txn::Transaction<'_>, id: ElementId, r: &RampRecipe) -> Result<()> {
    let value = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })?;
    let store = txn.store().clone();
    regenerate(&store, store.get(id).expect("exists")).map(|_| ())
}

/// Moves, rotates or mirrors a ramp's path by `x`, up by `dz`.
pub(crate) fn transform(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    x: &dyn Fn([f64; 2]) -> [f64; 2],
    dz: f64,
) -> Result<()> {
    let store = txn.store().clone();
    let mut r = ramp_recipe(store.get(id).expect("exists")).expect("a ramp");
    r.path = r.path.iter().map(|p| x(*p)).collect();
    r.extent.base_offset += dz;
    if r.extent.top_level().is_some() {
        r.extent.top_offset += dz;
    }
    write(txn, id, &r)
}

/// Sets a ramp type's value (`type.set_param`): `max_slope` (the x of 1:x), `max_incline_length`,
/// `thickness` or `shape` (`thick`, `solid`).
pub(crate) fn set_type_param(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    param: &str,
    value: &Value,
) -> Result<()> {
    let store = txn.store().clone();
    let mut t = ramp_type(store.get(id).expect("checked")).expect("a ramp type");
    let bad = |what: &str| Error::new("type.invalid_value", format!("{param}: {what}"));
    let length = |v: &Value| -> Result<f64> {
        let x = match v {
            Value::Number(n) => n.as_f64().unwrap_or(-1.0),
            Value::String(s) => {
                crate::units::parse(s, "length", &crate::units::settings_of(&store))?
            }
            _ => -1.0,
        };
        if x > 0.0 && x.is_finite() {
            Ok(x)
        } else {
            Err(bad("greater than 0"))
        }
    };
    match param {
        "max_slope" => {
            let x = value
                .as_f64()
                .or_else(|| value.as_str()?.trim().trim_start_matches("1:").parse().ok())
                .filter(|x| *x >= 1.0 && *x <= 100.0)
                .ok_or_else(|| bad("the x of 1:x, from 1 to 100"))?;
            t.max_slope = x;
        }
        "max_incline_length" => t.max_incline_length = length(value)?,
        "thickness" => t.thickness = length(value)?,
        "shape" => {
            t.shape = serde_json::from_value(value.clone()).map_err(|_| bad("thick or solid"))?;
        }
        _ => {
            let value = (!value.is_null()).then_some(value);
            return crate::parameters::set(txn, &[id], param, value).map(|_| ());
        }
    }
    let v = serde_json::to_value(&t).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_run_gets_a_landing_and_the_slope_is_shared() {
        // 0.6 m at 1:12, maximum incline 6 m, drawn as one straight 7.2 m run.
        let ty = RampType {
            max_incline_length: 6.0,
            ..RampType::default()
        };
        let (pieces, sloped) = layout(&[[0.0, 0.0], [7.2, 0.0]], 1.2, 0.0, 0.6, &ty).unwrap();
        assert!((sloped - 7.2).abs() < 1e-9);
        let flights: Vec<&RampPiece> = pieces.iter().filter(|p| !p.landing).collect();
        let landings: Vec<&RampPiece> = pieces.iter().filter(|p| p.landing).collect();
        assert_eq!((flights.len(), landings.len()), (2, 1));
        for f in &flights {
            assert!((f.corners[1][0] - f.corners[0][0] - 3.6).abs() < 1e-9);
        }
        // The landing is at mid height, 1.2 m deep; the top is 0.6 m up at x = 8.4.
        assert!((landings[0].z[0] - 0.3).abs() < 1e-9);
        assert!((flights[1].corners[1][0] - 8.4).abs() < 1e-9);
        assert!((flights[1].z[1] - 0.6).abs() < 1e-9);
        // An L-shaped ramp gets a square landing at its corner.
        let (pieces, _) =
            layout(&[[0.0, 0.0], [5.0, 0.0], [5.0, 4.0]], 1.2, 0.0, 0.6, &RampType::default())
                .unwrap();
        assert_eq!(pieces.iter().filter(|p| p.landing).count(), 1);
        let top = pieces.last().unwrap();
        assert!((top.z[2] - 0.6).abs() < 1e-9);
    }
}
