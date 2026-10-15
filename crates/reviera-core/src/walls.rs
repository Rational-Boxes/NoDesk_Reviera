// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Walls (spec Ch 3 §3.3, ARC-WALL-002): the wall recipe, its regeneration into geometry, and the
//! `wall.create` / `wall.set_location` commands.
//!
//! A Reviera wall keeps its parametric intent in RXD element data (`rxd:recipe`, kind
//! `recipe.wall`): the location curve, base level and offset, top constraint, location line and
//! flip. Geometry is regenerated from the recipe and the levels it references, so the viewport shows
//! a new or moved wall at once; saving writes the IFC placement, axis and swept-solid body.
//!
//! Built so far: straight walls on the wall centreline, with L and T joins of single-layer walls
//! (see [`joins`]). Layered types, other location lines, arcs and profiles come with the rest of
//! Chapter 3.

pub mod attach;
pub mod cross_section;
pub mod curved;
pub mod joins;
pub mod layered;
pub mod linings;
pub mod openings;
pub mod profile;
pub mod stacked;

pub use joins::{End, Join, Solved, WallBody, solve};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;

/// Element parameter holding the recipe (RXD element data section `recipe`).
pub const RECIPE_KEY: &str = "rxd:recipe";
pub const WALL_RECIPE: &str = "recipe.wall";
pub const WALL_TYPE_RECIPE: &str = "recipe.wall_type";
/// The shortest location line allowed (§3.0, `TOL_SHORT`).
pub const TOL_SHORT: f64 = 0.0008;
pub const DEFAULT_TYPE_NAME: &str = "Generic - 200mm";

/// A straight location curve in plan, in metres (project coordinates).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Line {
    pub start: [f64; 2],
    pub end: [f64; 2],
}

impl Line {
    pub fn length(&self) -> f64 {
        (self.end[0] - self.start[0]).hypot(self.end[1] - self.start[1])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TopConstraint {
    /// Up to a level (by internal ID or GlobalId), plus `top_offset`.
    Level(ElementRefValue),
    /// A fixed height above the base.
    Unconnected(#[schemars(extend("x-reviera-data-type" = "length"))] f64),
}

/// An element reference inside a recipe argument (ID or GlobalId).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ElementRefValue {
    Id(ElementId),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LocationLine {
    #[default]
    WallCenterline,
    CoreCenterline,
    FinishFaceExterior,
    FinishFaceInterior,
    CoreFaceExterior,
    CoreFaceInterior,
}

/// The stored recipe of a wall. Levels are referenced by GlobalId (RXD references, Ch 2a §6.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WallRecipe {
    pub kind: String,
    pub curve: Line,
    pub base_level: String,
    pub base_offset: f64,
    /// `{"level": guid}` or `{"unconnected": height}`.
    pub top: Value,
    pub top_offset: f64,
    pub location_line: LocationLine,
    pub flip: bool,
    /// Join status of the start and end (Allow = `true`, ARC-JOIN-001).
    #[serde(default = "both_allowed")]
    pub join_allowed: [bool; 2],
    /// The roof the top is attached to (GlobalId, ARC-WALL-008).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attach_top: Option<String>,
    /// Points the location curve passes through between its start and end: one makes an arc,
    /// more a spline (ARC-WALL-002). Empty for a straight wall.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub through: Vec<[f64; 2]>,
    /// A slanted or tapered cross-section (ARC-WALL-005); vertical when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cross_section: Option<cross_section::CrossSection>,
    /// An edited elevation profile (ARC-WALL-009); the rectangle when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<profile::Profile>,
    /// The wall this one is hosted on (ARC-WALL-016).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<linings::WallHost>,
    /// Walls joined to this one by Join Geometry (GlobalIds, ARC-WALL-015).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub geometry_joins: Vec<String>,
    /// Base and Top Extension Distances of the extendable layers (ARC-CMPD-004).
    #[serde(default, skip_serializing_if = "no_extension")]
    pub extension: [f64; 2],
    /// Each end's wrapping, by type unless overridden (ARC-WRAP-001).
    #[serde(default, skip_serializing_if = "by_type")]
    pub end_wrap: [layered::EndWrap; 2],
}

fn no_extension(e: &[f64; 2]) -> bool {
    *e == [0.0, 0.0]
}

fn by_type(w: &[layered::EndWrap; 2]) -> bool {
    *w == [layered::EndWrap::ByType; 2]
}

fn both_allowed() -> [bool; 2] {
    [true, true]
}

/// Regenerated wall geometry in project coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WallGeometry {
    pub start: [f64; 2],
    pub end: [f64; 2],
    pub base_z: f64,
    pub height: f64,
    pub width: f64,
}

impl WallGeometry {
    pub fn length(&self) -> f64 {
        (self.end[0] - self.start[0]).hypot(self.end[1] - self.start[1])
    }

    /// Unit direction along the wall in plan.
    pub fn direction(&self) -> [f64; 2] {
        let l = self.length().max(f64::EPSILON);
        [
            (self.end[0] - self.start[0]) / l,
            (self.end[1] - self.start[1]) / l,
        ]
    }

    /// Unit normal to the left of the direction: the exterior side (+Y local).
    pub fn normal(&self) -> [f64; 2] {
        let [ux, uy] = self.direction();
        [-uy, ux]
    }

    /// The unjoined plan outline: start-interior, end-interior, end-exterior, start-exterior.
    pub fn footprint(&self) -> [[f64; 2]; 4] {
        let [nx, ny] = self.normal().map(|v| v * self.width / 2.0);
        let ([sx, sy], [ex, ey]) = (self.start, self.end);
        [
            [sx - nx, sy - ny],
            [ex - nx, ey - ny],
            [ex + nx, ey + ny],
            [sx + nx, sy + ny],
        ]
    }

    /// The unjoined body as a closed box: 8 corners and 12 triangles (outward winding).
    pub fn mesh(&self) -> (Vec<[f64; 3]>, Vec<u32>) {
        prism(&self.footprint(), self.base_z, self.base_z + self.height)
    }
}

/// A vertical prism over a convex plan quadrilateral ordered as [`WallGeometry::footprint`].
pub fn prism(f: &[[f64; 2]; 4], z0: f64, z1: f64) -> (Vec<[f64; 3]>, Vec<u32>) {
    {
        let v: Vec<[f64; 3]> = [z0, z1]
            .into_iter()
            .flat_map(|z| f.iter().map(move |[x, y]| [*x, *y, z]))
            .collect();
        let i = vec![
            0, 2, 1, 0, 3, 2, // bottom
            4, 5, 6, 4, 6, 7, // top
            0, 1, 5, 0, 5, 4, // interior side
            1, 2, 6, 1, 6, 5, // end
            2, 3, 7, 2, 7, 6, // exterior side
            3, 0, 4, 3, 4, 7, // start
        ];
        (v, i)
    }
}

fn level_elevation(store: &Store, guid: &str) -> Result<f64> {
    let g = GlobalId::parse(guid).ok_or_else(|| {
        Error::new(
            "wall.bad_level",
            format!("`{guid}` is not a level GlobalId"),
        )
    })?;
    let level = store
        .find_by_guid(&g)
        .and_then(|id| store.get(id))
        .ok_or_else(|| {
            Error::new(
                "wall.level_missing",
                format!("level {guid} no longer exists"),
            )
        })?;
    Ok(level
        .param("@Elevation")
        .and_then(Value::as_f64)
        .unwrap_or(0.0))
}

/// The recipe of an element, if it is a Reviera wall.
pub fn wall_recipe(e: &Element) -> Option<WallRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")? == WALL_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// The width of a wall type (from its `recipe.wall_type`).
pub fn type_width(t: &Element) -> Option<f64> {
    let v = t.param(RECIPE_KEY)?;
    (v.get("kind")? == WALL_TYPE_RECIPE).then(|| v.get("width")?.as_f64())?
}

fn type_of(store: &Store, e: &Element) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(e.param("@type")?.as_str()?)?)
}

/// Where a location line sits across a wall of type `t`, from the centreline toward the
/// exterior (metres).
pub fn line_offset(t: &Element, line: LocationLine) -> f64 {
    let c = crate::compound::of_wall_type(t);
    let half = type_width(t).unwrap_or(0.0) / 2.0;
    // Core faces, measured from the exterior face (layers run exterior first).
    let (core_ext, core_int) = match &c {
        Some(c) if c.core[1] > c.core[0] => {
            let before: f64 = c.layers[..c.core[0]].iter().map(|l| l.thickness).sum();
            let core: f64 = c.layers[c.core[0]..c.core[1]]
                .iter()
                .map(|l| l.thickness)
                .sum();
            (half - before, half - before - core)
        }
        _ => (half, -half),
    };
    match line {
        LocationLine::WallCenterline => 0.0,
        LocationLine::FinishFaceExterior => half,
        LocationLine::FinishFaceInterior => -half,
        LocationLine::CoreFaceExterior => core_ext,
        LocationLine::CoreFaceInterior => core_int,
        LocationLine::CoreCenterline => (core_ext + core_int) / 2.0,
    }
}

/// The move from a straight wall's location line to its centreline, in plan (ARC-WALL-002).
pub fn to_centreline(store: &Store, e: &Element, r: &WallRecipe) -> [f64; 2] {
    if r.location_line == LocationLine::WallCenterline || !r.through.is_empty() {
        return [0.0, 0.0];
    }
    let Some(t) = type_of(store, e).and_then(|t| store.get(t)) else {
        return [0.0, 0.0];
    };
    let d = line_offset(t, r.location_line) * if r.flip { -1.0 } else { 1.0 };
    let l = r.curve.length().max(f64::EPSILON);
    let n = [
        -(r.curve.end[1] - r.curve.start[1]) / l,
        (r.curve.end[0] - r.curve.start[0]) / l,
    ];
    [-n[0] * d, -n[1] * d]
}

/// Regenerates a wall's geometry from its recipe, type and levels.
pub fn regenerate(store: &Store, e: &Element) -> Result<WallGeometry> {
    let r = wall_recipe(e)
        .ok_or_else(|| Error::new("wall.not_reviera", "the element is not a Reviera wall"))?;
    let width = type_of(store, e)
        .and_then(|t| store.get(t))
        .and_then(type_width)
        .ok_or_else(|| Error::new("wall.no_type_width", "the wall type defines no width"))?;
    let base_z = level_elevation(store, &r.base_level)? + r.base_offset;
    let height = if let Some(h) = r.top.get("unconnected").and_then(Value::as_f64) {
        h
    } else if let Some(g) = r.top.get("level").and_then(Value::as_str) {
        level_elevation(store, g)? + r.top_offset - base_z
    } else {
        return Err(Error::new(
            "wall.bad_top",
            "the top constraint is neither a level nor unconnected",
        ));
    };
    if height <= 0.0 {
        return Err(Error::new(
            "wall.non_positive_height",
            "the wall's top is at or below its base",
        ));
    }
    // The curve is the location line; the geometry is about the centreline.
    let shift = to_centreline(store, e, &r);
    Ok(WallGeometry {
        start: [r.curve.start[0] + shift[0], r.curve.start[1] + shift[1]],
        end: [r.curve.end[0] + shift[0], r.curve.end[1] + shift[1]],
        base_z,
        height,
        width,
    })
}

// ---- Commands ----

/// `curve` argument of `wall.create` (straight lines for now).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CurveArg {
    pub line: Line,
    /// Points the wall passes through between the line's start and end: one makes an arc, two
    /// or more a smooth spline (ARC-WALL-002).
    #[serde(default)]
    pub through: Vec<[f64; 2]>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// A wall type with a width; omitted, the project's default wall type is used (created on first use).
    pub type_id: Option<ElementRef>,
    pub curve: CurveArg,
    pub base_level: ElementRef,
    pub top_constraint: TopConstraint,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base_offset: f64,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub top_offset: f64,
    #[serde(default)]
    pub location_line: LocationLine,
    #[serde(default)]
    pub flip: bool,
    /// Join Geometry with walls the new one touches face to face, and lock them (ARC-WALL-015).
    #[serde(default)]
    pub auto_join: linings::AutoJoin,
}

fn resolve(store: &Store, r: &ElementRef, what: &str) -> Result<ElementId> {
    store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", format!("no such {what}")))
}

fn level_guid(store: &Store, id: ElementId) -> Result<String> {
    let e = store
        .get(id)
        .ok_or_else(|| Error::new("element.not_found", "no such level"))?;
    if e.class != "IfcBuildingStorey" {
        return Err(Error::new(
            "wall.not_a_level",
            format!("{} is not a level", e.class),
        ));
    }
    Ok(e.guid().to_string())
}

/// The project's default wall type, created when first needed.
pub(crate) fn default_type(txn: &mut crate::txn::Transaction<'_>) -> Result<ElementId> {
    let existing = txn
        .store()
        .iter()
        .find(|e| {
            e.class == "IfcWallType"
                && e.name.as_deref() == Some(DEFAULT_TYPE_NAME)
                && type_width(e).is_some()
        })
        .map(Element::id);
    match existing {
        Some(id) => Ok(id),
        None => txn.create(
            NewElement::new("IfcWallType")
                .name(DEFAULT_TYPE_NAME)
                .param("@PredefinedType", "STANDARD")
                .param(
                    RECIPE_KEY,
                    json!({ "kind": WALL_TYPE_RECIPE, "width": 0.2 }),
                ),
        ),
    }
}

pub(crate) fn create(txn: &mut crate::txn::Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    if p.location_line != LocationLine::WallCenterline && !p.curve.through.is_empty() {
        return Err(Error::new(
            "wall.unsupported_location_line",
            "curved walls are drawn on their centreline (for now)",
        ));
    }
    let store = txn.store().clone();
    let type_id = match &p.type_id {
        Some(r) => resolve(&store, r, "wall type")?,
        None => default_type(txn)?,
    };
    let store = txn.store().clone();
    let wall_type = store.get(type_id).expect("resolved");
    let width = type_width(wall_type).ok_or_else(|| {
        Error::new(
            "wall.no_type_width",
            format!(
                "{} has no width; choose a Reviera wall type",
                wall_type.name.as_deref().unwrap_or("the type")
            ),
        )
    })?;
    // A curve through points in line with its ends is straight.
    let through = if curved::path(p.curve.line, &p.curve.through).is_some() {
        p.curve.through.clone()
    } else {
        Vec::new()
    };
    let length = curved::path(p.curve.line, &through).map_or(p.curve.line.length(), |c| c.length());
    if length < TOL_SHORT || length < width {
        return Err(Error::new("wall.too_short", "Wall is too short"));
    }
    let base_level = level_guid(&store, resolve(&store, &p.base_level, "level")?)?;
    let top = match p.top_constraint {
        TopConstraint::Unconnected(h) if h > 0.0 => json!({ "unconnected": h }),
        TopConstraint::Unconnected(_) => {
            return Err(Error::new(
                "wall.non_positive_height",
                "the height must be greater than 0",
            ));
        }
        TopConstraint::Level(ElementRefValue::Id(id)) => {
            json!({ "level": level_guid(&store, id)? })
        }
    };
    let recipe = WallRecipe {
        kind: WALL_RECIPE.into(),
        curve: p.curve.line,
        base_level,
        base_offset: p.base_offset,
        top,
        top_offset: p.top_offset,
        location_line: p.location_line,
        flip: p.flip,
        join_allowed: both_allowed(),
        attach_top: None,
        through,
        cross_section: None,
        profile: None,
        host: None,
        geometry_joins: Vec::new(),
        extension: [0.0, 0.0],
        end_wrap: Default::default(),
    };
    let id = txn.create(
        NewElement::new("IfcWall")
            .param("@PredefinedType", "STANDARD")
            .param("@type", wall_type.guid().as_str())
            .param(
                RECIPE_KEY,
                serde_json::to_value(&recipe).expect("recipes serialise"),
            ),
    )?;
    // Check that the new wall regenerates (for example, a top level below the base fails).
    let e = txn.store().get(id).expect("created");
    let g = regenerate(txn.store(), e)?;
    // A stacked wall must be taller than its fixed rows (ARC-STKW-001).
    if stacked::of_wall(txn.store(), e).is_some() {
        stacked::layout(txn.store(), e, &g)?;
    }
    linings::auto_join(txn, id, p.auto_join)?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetLocationParams {
    pub wall_id: ElementRef,
    pub curve: CurveArg,
}

pub(crate) fn set_location(
    txn: &mut crate::txn::Transaction<'_>,
    p: SetLocationParams,
) -> Result<()> {
    let store = txn.store().clone();
    let id = resolve(&store, &p.wall_id, "wall")?;
    crate::modify::check_unpinned(&store, id)?;
    let wall = store.get(id).expect("resolved");
    let mut recipe = wall_recipe(wall).ok_or_else(|| {
        Error::new(
            "wall.not_reviera",
            "only Reviera walls can be moved this way",
        )
    })?;
    let width = regenerate(&store, wall)?.width;
    // A curve through points in line with its ends is straight.
    let through = if curved::path(p.curve.line, &p.curve.through).is_some() {
        p.curve.through.clone()
    } else {
        Vec::new()
    };
    let length = curved::path(p.curve.line, &through).map_or(p.curve.line.length(), |c| c.length());
    if length < TOL_SHORT || length < width {
        return Err(Error::new("wall.too_short", "Wall is too short"));
    }
    let old = recipe.curve;
    recipe.curve = p.curve.line;
    recipe.through = through;
    let value = serde_json::to_value(&recipe).expect("recipes serialise");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })?;
    // Doors and windows stay where they are in the model.
    let guid = wall.guid().to_string();
    crate::inserts::keep_in_place(txn, &guid, old, p.curve.line)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetConstraintsParams {
    pub wall_id: ElementRef,
    /// The level the base is measured from.
    pub base_level: Option<ElementRef>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base_offset: Option<f64>,
    /// Up to a level, or a fixed height above the base.
    pub top_constraint: Option<TopConstraint>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub top_offset: Option<f64>,
}

/// Changes a wall's base and top (`wall.set_constraints`); omitted fields keep their values.
pub(crate) fn set_constraints(
    txn: &mut crate::txn::Transaction<'_>,
    p: SetConstraintsParams,
) -> Result<()> {
    let store = txn.store().clone();
    let id = resolve(&store, &p.wall_id, "wall")?;
    crate::modify::check_unpinned(&store, id)?;
    let mut recipe = wall_recipe(store.get(id).expect("resolved")).ok_or_else(|| {
        Error::new(
            "wall.not_reviera",
            "only Reviera walls have base and top constraints",
        )
    })?;
    if let Some(r) = &p.base_level {
        recipe.base_level = level_guid(&store, resolve(&store, r, "level")?)?;
    }
    if let Some(o) = p.base_offset {
        recipe.base_offset = o;
    }
    match p.top_constraint {
        Some(TopConstraint::Unconnected(h)) if h <= 0.0 => {
            return Err(Error::new(
                "wall.non_positive_height",
                "the height must be greater than 0",
            ));
        }
        Some(TopConstraint::Unconnected(h)) => recipe.top = json!({ "unconnected": h }),
        Some(TopConstraint::Level(ElementRefValue::Id(l))) => {
            recipe.top = json!({ "level": level_guid(&store, l)? });
        }
        None => {}
    }
    if let Some(o) = p.top_offset {
        recipe.top_offset = o;
    }
    let value = serde_json::to_value(&recipe).expect("recipes serialise");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })?;
    // The top must stay above the base.
    regenerate(txn.store(), txn.store().get(id).expect("modified")).map(|_| ())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetJoinAllowedParams {
    pub wall_id: ElementRef,
    pub end: End,
    pub allowed: bool,
}

pub(crate) fn set_join_allowed(
    txn: &mut crate::txn::Transaction<'_>,
    p: SetJoinAllowedParams,
) -> Result<()> {
    let store = txn.store().clone();
    let id = resolve(&store, &p.wall_id, "wall")?;
    let mut recipe = wall_recipe(store.get(id).expect("resolved"))
        .ok_or_else(|| Error::new("wall.not_reviera", "only Reviera walls have join settings"))?;
    recipe.join_allowed[p.end.index()] = p.allowed;
    let value = serde_json::to_value(&recipe).expect("recipes serialise");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JoinsQueryParams {
    pub wall_id: ElementRef,
}

pub(crate) fn joins_query(store: &Store, p: JoinsQueryParams) -> Result<Vec<Join>> {
    let id = resolve(store, &p.wall_id, "wall")?;
    Ok(solve(store).joins_of(id))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FlipParams {
    pub wall_ids: Vec<ElementRef>,
}

/// Flips walls about their location line: the exterior moves to the other side (ARC-WALL-002).
pub(crate) fn flip(txn: &mut crate::txn::Transaction<'_>, p: FlipParams) -> Result<()> {
    let store = txn.store().clone();
    for r in &p.wall_ids {
        let id = resolve(&store, r, "wall")?;
        crate::modify::check_unpinned(&store, id)?;
        let mut recipe = wall_recipe(store.get(id).expect("resolved"))
            .ok_or_else(|| Error::new("wall.not_reviera", "only Reviera walls can be flipped"))?;
        recipe.flip = !recipe.flip;
        let value = serde_json::to_value(&recipe).expect("recipes serialise");
        txn.modify(id, |e| {
            e.params.insert(RECIPE_KEY.into(), value);
        })?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetLocationLineParams {
    pub wall_ids: Vec<ElementRef>,
    pub location_line: LocationLine,
}

/// `wall.set_location_line`: the walls keep their bodies where they are; their location
/// curves move to the new line (ARC-WALL-002).
pub(crate) fn set_location_line(
    txn: &mut crate::txn::Transaction<'_>,
    p: SetLocationLineParams,
) -> Result<()> {
    let store = txn.store().clone();
    for w in &p.wall_ids {
        let id = resolve(&store, w, "wall")?;
        let e = store.get(id).expect("resolved");
        let mut r = wall_recipe(e)
            .ok_or_else(|| Error::new("wall.not_reviera", "the element is not a Reviera wall"))?;
        if !r.through.is_empty() && p.location_line != LocationLine::WallCenterline {
            return Err(Error::new(
                "wall.unsupported_location_line",
                "curved walls are drawn on their centreline (for now)",
            ));
        }
        let old = to_centreline(&store, e, &r);
        r.location_line = p.location_line;
        let new = to_centreline(&store, e, &r);
        // Centre = curve + shift, kept: the curve moves by old − new.
        let d = [old[0] - new[0], old[1] - new[1]];
        r.curve.start = [r.curve.start[0] + d[0], r.curve.start[1] + d[1]];
        r.curve.end = [r.curve.end[0] + d[0], r.curve.end[1] + d[1]];
        let value = serde_json::to_value(&r).expect("recipes serialise");
        txn.modify(id, |el| {
            el.params.insert(RECIPE_KEY.into(), value);
        })?;
    }
    Ok(())
}
