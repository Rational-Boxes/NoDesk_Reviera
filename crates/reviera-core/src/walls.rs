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
pub mod joins;
pub mod openings;

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
    Ok(WallGeometry {
        start: r.curve.start,
        end: r.curve.end,
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
fn default_type(txn: &mut crate::txn::Transaction<'_>) -> Result<ElementId> {
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
    if p.location_line != LocationLine::WallCenterline {
        return Err(Error::new(
            "wall.unsupported_location_line",
            "only the wall centreline is supported until layered types exist",
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
    let length = p.curve.line.length();
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
    regenerate(txn.store(), txn.store().get(id).expect("created"))?;
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
    let length = p.curve.line.length();
    if length < TOL_SHORT || length < width {
        return Err(Error::new("wall.too_short", "Wall is too short"));
    }
    let old = recipe.curve;
    recipe.curve = p.curve.line;
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
