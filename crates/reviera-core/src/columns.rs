// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Architectural columns (ARC-COL-001): non-structural columns, casings and pilasters.
//!
//! A column is an `IfcColumn` with a `recipe.column`: its centre in plan, its rotation, and its
//! vertical extent between levels ([`Extent`]). Its type (`IfcColumnType` with a
//! `recipe.column_type`) gives its section: rectangular (width × depth) or round (diameter).
//!
//! Its top can be attached to a roof, floor or ceiling above it (`column.attach`): the top then
//! follows the target's underside over the column's section, at its lowest point (*minimum*), its
//! middle (*midline*) or its highest point (*maximum*), plus an offset; with *maximum* and
//! *midline* the top is cut by the target's underside, so it meets a sloped roof flush. The
//! column regenerates when the target changes; when the target goes, it takes its own top.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::levels::Extent;
use crate::store::Store;
use crate::walls::{RECIPE_KEY, TopConstraint};

pub const COLUMN_RECIPE: &str = "recipe.column";
pub const COLUMN_TYPE_RECIPE: &str = "recipe.column_type";
pub const DEFAULT_TYPE_NAME: &str = "Rectangular Column 400 x 400";
/// Sides of a round column's section.
const ROUND_SIDES: usize = 32;

/// A column type's section.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    #[default]
    Rectangular,
    Round,
}

/// The stored recipe of a column type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ColumnType {
    pub kind: String,
    #[serde(default)]
    pub section: Section,
    /// Across its local x (metres); a round column's diameter.
    pub width: f64,
    /// Across its local y (metres); ignored for a round column.
    pub depth: f64,
}

impl Default for ColumnType {
    fn default() -> Self {
        Self {
            kind: COLUMN_TYPE_RECIPE.into(),
            section: Section::Rectangular,
            width: 0.4,
            depth: 0.4,
        }
    }
}

impl ColumnType {
    /// The section's outline about its centre, counter-clockwise.
    pub fn outline(&self) -> Vec<[f64; 2]> {
        match self.section {
            Section::Rectangular => {
                let (w, d) = (self.width / 2.0, self.depth / 2.0);
                vec![[-w, -d], [w, -d], [w, d], [-w, d]]
            }
            Section::Round => {
                let r = self.width / 2.0;
                (0..ROUND_SIDES)
                    .map(|i| {
                        let a = std::f64::consts::TAU * i as f64 / ROUND_SIDES as f64;
                        [r * a.cos(), r * a.sin()]
                    })
                    .collect()
            }
        }
    }
}

pub fn column_type(e: &Element) -> Option<ColumnType> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == COLUMN_TYPE_RECIPE)
        .then(|| serde_json::from_value(v.clone()).ok())?
}

/// Where along a target's underside an attached top goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Justification {
    /// The lowest point of the underside over the section.
    Minimum,
    /// Its middle (between the lowest and highest).
    Midline,
    /// Its highest point; the top is cut by the underside.
    #[default]
    Maximum,
}

/// A column top attached to a roof, floor or ceiling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Attach {
    /// The target (GlobalId).
    pub target: String,
    #[serde(default)]
    pub justification: Justification,
    #[serde(default)]
    pub offset: f64,
}

/// The stored recipe of a column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ColumnRecipe {
    pub kind: String,
    /// Its centre in plan (metres).
    pub point: [f64; 2],
    /// Counter-clockwise from the x axis (radians).
    #[serde(default)]
    pub rotation: f64,
    #[serde(flatten)]
    pub extent: Extent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attach_top: Option<Attach>,
}

pub fn column_recipe(e: &Element) -> Option<ColumnRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == COLUMN_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

/// A column as it is now.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnGeometry {
    pub ty: ColumnType,
    /// The section in plan, counter-clockwise.
    pub ring: Vec<[f64; 2]>,
    pub z0: f64,
    /// The top's height at each corner of `ring`.
    pub tops: Vec<f64>,
}

impl ColumnGeometry {
    /// The closed body: bottom, top (following an attached target) and sides.
    pub fn mesh(&self) -> crate::bodies::Mesh {
        let n = self.ring.len();
        let mut positions: Vec<[f64; 3]> =
            self.ring.iter().map(|p| [p[0], p[1], self.z0]).collect();
        positions.extend(
            self.ring
                .iter()
                .zip(&self.tops)
                .map(|(p, z)| [p[0], p[1], *z]),
        );
        let mut indices = Vec::new();
        let n32 = n as u32;
        // A convex section: fans for the caps.
        for i in 1..n32 - 1 {
            indices.extend([0, i + 1, i]);
            indices.extend([n32, n32 + i, n32 + i + 1]);
        }
        for i in 0..n32 {
            let j = (i + 1) % n32;
            indices.extend([i, j, n32 + j, i, n32 + j, n32 + i]);
        }
        (positions, indices)
    }

    /// The highest point of its top.
    pub fn top(&self) -> f64 {
        self.tops.iter().copied().fold(f64::NEG_INFINITY, f64::max)
    }
}

/// The underside of a roof, floor or ceiling (GlobalId) over plan point `p`, if it covers it.
fn underside(store: &Store, target: &str, p: [f64; 2]) -> Option<f64> {
    let e = store.get(find(store, target)?)?;
    if crate::roofs::roof_recipe(e).is_some() {
        // The lowest of the roof over `p`: its faces' underside, or an eave's fascia or soffit.
        let g = crate::roofs::regenerate(store, e).ok()?;
        let mut z = g.underside(p);
        for t in &g.trims {
            let (pts, idx) = &t.mesh;
            for tri in idx.chunks(3) {
                let [a, b, c] = [
                    pts[tri[0] as usize],
                    pts[tri[1] as usize],
                    pts[tri[2] as usize],
                ];
                if let Some(h) = height_in(p, a, b, c) {
                    z = Some(z.map_or(h, |z: f64| z.min(h)));
                }
            }
        }
        return z;
    }
    let f = crate::floors::regenerate(store, e).ok()?;
    f.profiles
        .iter()
        .any(|pr| inside(p, &pr.outer) && !pr.holes.iter().any(|h| inside(p, h)))
        .then_some(f.top - f.thickness)
}

/// The height over plan point `p` of triangle `a b c`, if `p` is inside its plan (and it is not
/// vertical).
fn height_in(p: [f64; 2], a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Option<f64> {
    let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
    if d.abs() < 1e-12 {
        return None;
    }
    let l1 = ((b[1] - c[1]) * (p[0] - c[0]) + (c[0] - b[0]) * (p[1] - c[1])) / d;
    let l2 = ((c[1] - a[1]) * (p[0] - c[0]) + (a[0] - c[0]) * (p[1] - c[1])) / d;
    let l3 = 1.0 - l1 - l2;
    (l1 >= -1e-9 && l2 >= -1e-9 && l3 >= -1e-9).then(|| l1 * a[2] + l2 * b[2] + l3 * c[2])
}

fn inside(p: [f64; 2], ring: &[[f64; 2]]) -> bool {
    let mut c = false;
    let n = ring.len();
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + n - 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            c = !c;
        }
    }
    c
}

fn type_of(store: &Store, e: &Element) -> ColumnType {
    e.param("@type")
        .and_then(Value::as_str)
        .and_then(|g| find(store, g))
        .and_then(|t| store.get(t))
        .and_then(column_type)
        .unwrap_or_default()
}

/// The roof, floor or ceiling directly over a column (the lowest underside above its base over
/// its centre), to attach its top to.
pub fn target_above(store: &Store, column: ElementId) -> Option<ElementId> {
    let e = store.get(column)?;
    let r = column_recipe(e)?;
    let (z0, _) = r.extent.z(store).ok()?;
    let candidates = crate::roofs::all(store)
        .into_iter()
        .map(|(id, _)| id)
        .chain(crate::floors::all(store).into_iter().map(|(id, _)| id));
    candidates
        .filter_map(|id| {
            let guid = store.get(id)?.guid().to_string();
            let z = underside(store, &guid, r.point)?;
            (z > z0 + 0.1).then_some((id, z))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

/// Regenerates a column from its recipe, type, levels and attach target.
pub fn regenerate(store: &Store, e: &Element) -> Result<ColumnGeometry> {
    let r =
        column_recipe(e).ok_or_else(|| Error::new("column.not_reviera", "not a Reviera column"))?;
    let ty = type_of(store, e);
    let (s, c) = r.rotation.sin_cos();
    let ring: Vec<[f64; 2]> = ty
        .outline()
        .into_iter()
        .map(|[x, y]| [r.point[0] + x * c - y * s, r.point[1] + x * s + y * c])
        .collect();
    let (z0, z1) = r.extent.z(store)?;
    let mut tops = vec![z1; ring.len()];
    if let Some(a) = &r.attach_top {
        // The underside over the corners and the centre.
        let mut samples: Vec<[f64; 2]> = ring.clone();
        samples.push(r.point);
        let heights: Vec<Option<f64>> = samples
            .iter()
            .map(|p| underside(store, &a.target, *p))
            .collect();
        let known: Vec<f64> = heights.iter().flatten().copied().collect();
        if !known.is_empty() {
            let lo = known.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = known.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let z = match a.justification {
                Justification::Minimum => lo,
                Justification::Midline => (lo + hi) / 2.0,
                Justification::Maximum => hi,
            } + a.offset;
            tops = heights[..ring.len()]
                .iter()
                .map(|h| match (a.justification, h) {
                    // Cut by the underside, up to the justified height.
                    (Justification::Minimum, _) | (_, None) => z,
                    (_, Some(h)) => z.min(h + a.offset),
                })
                .collect();
        }
    }
    if tops.iter().any(|t| *t <= z0 + 1e-6) {
        return Err(Error::new(
            "column.non_positive_height",
            "the column's top is at or below its base",
        ));
    }
    Ok(ColumnGeometry { ty, ring, z0, tops })
}

/// Every Reviera column that regenerates.
pub fn all(store: &Store) -> Vec<(ElementId, ColumnGeometry)> {
    store
        .iter()
        .filter(|e| column_recipe(e).is_some())
        .filter_map(|e| Some((e.id(), regenerate(store, e).ok()?)))
        .collect()
}

/// The project's default column type, created when first needed.
fn default_type(txn: &mut crate::txn::Transaction<'_>) -> Result<ElementId> {
    if let Some(e) = txn
        .store()
        .iter()
        .filter(|e| column_type(e).is_some())
        .min_by_key(|e| e.name.as_deref() != Some(DEFAULT_TYPE_NAME))
    {
        return Ok(e.id());
    }
    txn.create(
        NewElement::new("IfcColumnType")
            .name(DEFAULT_TYPE_NAME)
            .param("@PredefinedType", "COLUMN")
            .param(
                RECIPE_KEY,
                serde_json::to_value(ColumnType::default()).expect("serialises"),
            ),
    )
}

fn write(txn: &mut crate::txn::Transaction<'_>, id: ElementId, r: &ColumnRecipe) -> Result<()> {
    let value = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })?;
    let store = txn.store().clone();
    regenerate(&store, store.get(id).expect("exists")).map(|_| ())
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceParams {
    /// A column type; omitted, the default (400 × 400 mm), made on first use.
    pub type_id: Option<ElementRef>,
    /// The column's centre in plan (metres).
    pub point: [f64; 2],
    pub base_level: ElementRef,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base_offset: f64,
    /// A level, or a height above the base.
    pub top_constraint: TopConstraint,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub top_offset: f64,
    /// Counter-clockwise; accepts units ("30°").
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: f64,
}

fn resolve_type(store: &Store, r: &ElementRef) -> Result<ElementId> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such column type"))?;
    if column_type(store.get(id).expect("resolved")).is_none() {
        return Err(Error::new(
            "column.not_a_type",
            format!("element {id} is not a column type"),
        ));
    }
    Ok(id)
}

/// Places an architectural column (`column.place_architectural`).
pub(crate) fn place(txn: &mut crate::txn::Transaction<'_>, p: PlaceParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let type_id = match &p.type_id {
        Some(r) => resolve_type(&store, r)?,
        None => default_type(txn)?,
    };
    let store = txn.store().clone();
    let extent = Extent::from_args(
        &store,
        &p.base_level,
        p.base_offset,
        p.top_constraint,
        p.top_offset,
    )?;
    let recipe = ColumnRecipe {
        kind: COLUMN_RECIPE.into(),
        point: p.point,
        rotation: p.rotation,
        extent,
        attach_top: None,
    };
    let guid = store.get(type_id).expect("resolved").guid().to_string();
    let id = txn.create(
        NewElement::new("IfcColumn")
            .param("@PredefinedType", "COLUMN")
            .param("@type", guid)
            .param(
                "Pset_ColumnCommon.LoadBearing",
                json!({ "type": "IfcBoolean", "value": false }),
            )
            .param(
                RECIPE_KEY,
                serde_json::to_value(&recipe).expect("serialises"),
            ),
    )?;
    let store = txn.store().clone();
    regenerate(&store, store.get(id).expect("created"))?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub column_ids: Vec<ElementRef>,
    pub point: Option<[f64; 2]>,
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: Option<f64>,
    pub base_level: Option<ElementRef>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base_offset: Option<f64>,
    pub top_constraint: Option<TopConstraint>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub top_offset: Option<f64>,
}

fn resolve_columns(store: &Store, refs: &[ElementRef]) -> Result<Vec<(ElementId, ColumnRecipe)>> {
    refs.iter()
        .map(|r| {
            let id = store
                .resolve(r)
                .ok_or_else(|| Error::new("element.not_found", "no such column"))?;
            let recipe = column_recipe(store.get(id).expect("resolved")).ok_or_else(|| {
                Error::new(
                    "column.not_reviera",
                    format!("element {id} is not a Reviera column"),
                )
            })?;
            Ok((id, recipe))
        })
        .collect()
}

/// Changes columns' place, rotation or extent (`column.set`); omitted values are kept.
pub(crate) fn set(txn: &mut crate::txn::Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    for (id, mut r) in resolve_columns(&store, &p.column_ids)? {
        crate::modify::check_unpinned(&store, id)?;
        if let Some(pt) = p.point {
            r.point = pt;
        }
        if let Some(a) = p.rotation {
            r.rotation = a;
        }
        let base =
            match &p.base_level {
                Some(b) => b.clone(),
                None => ElementRef::Id(find(&store, &r.extent.base_level).ok_or_else(|| {
                    Error::new("level.missing", "the column's base level is gone")
                })?),
            };
        let top = match p.top_constraint {
            Some(t) => t,
            None => match r.extent.top_level().and_then(|g| find(&store, g)) {
                Some(l) => TopConstraint::Level(crate::walls::ElementRefValue::Id(l)),
                None => {
                    TopConstraint::Unconnected(r.extent.top["unconnected"].as_f64().unwrap_or(3.0))
                }
            },
        };
        r.extent = Extent::from_args(
            &store,
            &base,
            p.base_offset.unwrap_or(r.extent.base_offset),
            top,
            p.top_offset.unwrap_or(r.extent.top_offset),
        )?;
        write(txn, id, &r)?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AttachParams {
    pub column_ids: Vec<ElementRef>,
    /// A Reviera roof, floor or ceiling above the columns.
    pub target_id: ElementRef,
    #[serde(default)]
    pub justification: Justification,
    /// Above (or, negative, below) the attachment.
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: f64,
}

/// Attaches columns' tops to a roof, floor or ceiling (`column.attach`).
pub(crate) fn attach(txn: &mut crate::txn::Transaction<'_>, p: AttachParams) -> Result<()> {
    let store = txn.store().clone();
    let target = store
        .resolve(&p.target_id)
        .ok_or_else(|| Error::new("element.not_found", "no such roof, floor or ceiling"))?;
    let t = store.get(target).expect("resolved");
    if crate::roofs::roof_recipe(t).is_none() && crate::floors::slab_recipe(t).is_none() {
        return Err(Error::new(
            "column.bad_target",
            format!("element {target} is not a Reviera roof, floor or ceiling"),
        ));
    }
    let guid = t.guid().to_string();
    for (id, mut r) in resolve_columns(&store, &p.column_ids)? {
        crate::modify::check_unpinned(&store, id)?;
        r.attach_top = Some(Attach {
            target: guid.clone(),
            justification: p.justification,
            offset: p.offset,
        });
        // It must cover the column.
        if underside(&store, &guid, r.point).is_none() {
            return Err(Error::new(
                "column.not_covered",
                format!("element {target} is not over column {id}"),
            ));
        }
        write(txn, id, &r)?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DetachParams {
    pub column_ids: Vec<ElementRef>,
}

/// Detaches columns' tops (`column.detach`): they go back to their top constraint.
pub(crate) fn detach(txn: &mut crate::txn::Transaction<'_>, p: DetachParams) -> Result<()> {
    let store = txn.store().clone();
    for (id, mut r) in resolve_columns(&store, &p.column_ids)? {
        r.attach_top = None;
        write(txn, id, &r)?;
    }
    Ok(())
}

/// Moves, rotates or mirrors a column: its centre by `x`, its rotation by `turn` (or reflected
/// about a line at angle `mirror`), up by `dz`.
pub(crate) fn transform(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    x: &dyn Fn([f64; 2]) -> [f64; 2],
    turn: f64,
    mirror: Option<f64>,
    dz: f64,
) -> Result<()> {
    let store = txn.store().clone();
    let mut r = column_recipe(store.get(id).expect("exists")).expect("a column");
    r.point = x(r.point);
    r.rotation = match mirror {
        Some(axis) => 2.0 * axis - r.rotation,
        None => r.rotation + turn,
    };
    r.extent.base_offset += dz;
    if r.extent.top_level().is_some() {
        r.extent.top_offset += dz;
    }
    write(txn, id, &r)
}

/// Sets a column type's section value (`type.set_param`: `section`, `width`, `depth` or
/// `diameter`).
pub(crate) fn set_type_param(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    param: &str,
    value: &Value,
) -> Result<()> {
    let store = txn.store().clone();
    let mut t = column_type(store.get(id).expect("checked")).expect("a column type");
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
            Err(Error::new(
                "type.invalid_value",
                "the size must be greater than 0",
            ))
        }
    };
    match param {
        "section" => {
            t.section = match value.as_str() {
                Some("rectangular") => Section::Rectangular,
                Some("round") => Section::Round,
                _ => {
                    return Err(Error::new(
                        "type.invalid_value",
                        "the section is `rectangular` or `round`",
                    ));
                }
            };
        }
        "width" | "diameter" => t.width = length(value)?,
        "depth" => t.depth = length(value)?,
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
    fn sections_and_meshes() {
        let t = ColumnType::default();
        assert_eq!(t.outline().len(), 4);
        let round = ColumnType {
            section: Section::Round,
            width: 0.5,
            ..ColumnType::default()
        };
        assert_eq!(round.outline().len(), ROUND_SIDES);
        let g = ColumnGeometry {
            ty: t.clone(),
            ring: t.outline(),
            z0: 0.0,
            tops: vec![3.0; 4],
        };
        let (pts, idx) = g.mesh();
        assert_eq!((pts.len(), idx.len()), (8, 36));
    }
}
