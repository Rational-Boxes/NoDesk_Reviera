// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Floors (spec Ch 3 §3.6.1, ARC-FLR-001): slabs defined by a sketched boundary.
//!
//! A floor is an `IfcSlab` (`PredefinedType = FLOOR`) with a `recipe.floor` (`rxd:recipe`): its
//! level (GlobalId) and the height offset of its top from the level, and a sketch
//! (`rxd:sketch`, [`crate::sketch`]) whose outer loops are the floor and inner loops its holes.
//! Its type (`IfcSlabType` with a `recipe.floor_type`) gives the thickness; the floor hangs down
//! from its top. Boundary curves picked from walls with Lock follow the walls.
//!
//! Built so far: flat floors of one thickness. Slopes, layered types (ARC-CMPD-001) and joins
//! that cut walls come later.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::sketch::{self, Profile, SKETCH_KEY};
use crate::store::Store;
use crate::walls::RECIPE_KEY;

pub const FLOOR_RECIPE: &str = "recipe.floor";
pub const FLOOR_TYPE_RECIPE: &str = "recipe.floor_type";
pub const DEFAULT_TYPE_NAME: &str = "Generic Floor - 300mm";

/// The stored recipe of a floor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FloorRecipe {
    pub kind: String,
    /// The level (GlobalId).
    pub level: String,
    /// Height of the top above the level (metres).
    pub offset: f64,
}

pub fn floor_recipe(e: &Element) -> Option<FloorRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")? == FLOOR_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// The thickness of a floor type.
pub fn type_thickness(t: &Element) -> Option<f64> {
    let v = t.param(RECIPE_KEY)?;
    (v.get("kind")? == FLOOR_TYPE_RECIPE).then(|| v.get("thickness")?.as_f64())?
}

/// A floor as it is now.
#[derive(Debug, Clone, PartialEq)]
pub struct FloorGeometry {
    pub profiles: Vec<Profile>,
    /// Elevation of the top (absolute).
    pub top: f64,
    pub thickness: f64,
}

impl FloorGeometry {
    pub fn area(&self) -> f64 {
        self.profiles.iter().map(Profile::area).sum()
    }

    pub fn perimeter(&self) -> f64 {
        self.profiles.iter().map(Profile::perimeter).sum()
    }

    pub fn volume(&self) -> f64 {
        self.area() * self.thickness
    }
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

/// Regenerates a floor from its recipe, sketch, level and type.
pub fn regenerate(store: &Store, e: &Element) -> Result<FloorGeometry> {
    let r =
        floor_recipe(e).ok_or_else(|| Error::new("floor.not_reviera", "not a Reviera floor"))?;
    let level = find(store, &r.level)
        .and_then(|l| store.get(l))
        .ok_or_else(|| Error::new("floor.level_missing", "the floor's level no longer exists"))?;
    let thickness = e
        .param("@type")
        .and_then(Value::as_str)
        .and_then(|g| find(store, g))
        .and_then(|t| store.get(t))
        .and_then(type_thickness)
        .ok_or_else(|| {
            Error::new(
                "floor.no_type_thickness",
                "the floor type defines no thickness",
            )
        })?;
    let sketch = sketch::sketch_of(e)
        .ok_or_else(|| Error::new("floor.no_sketch", "the floor has no boundary"))?;
    Ok(FloorGeometry {
        profiles: sketch::profiles(store, &sketch),
        top: crate::levels::elevation(level) + r.offset,
        thickness,
    })
}

/// The Reviera floors of a model that regenerate.
pub fn all(store: &Store) -> Vec<(ElementId, FloorGeometry)> {
    store
        .iter()
        .filter(|e| floor_recipe(e).is_some())
        .filter_map(|e| Some((e.id(), regenerate(store, e).ok()?)))
        .collect()
}

/// The project's default floor type, created when first needed.
fn default_type(txn: &mut crate::txn::Transaction<'_>) -> Result<ElementId> {
    if let Some(e) = txn
        .store()
        .iter()
        .filter(|e| type_thickness(e).is_some())
        .min_by_key(|e| e.name.as_deref() != Some(DEFAULT_TYPE_NAME))
    {
        return Ok(e.id());
    }
    txn.create(
        NewElement::new("IfcSlabType")
            .name(DEFAULT_TYPE_NAME)
            .param("@PredefinedType", "FLOOR")
            .param(
                RECIPE_KEY,
                json!({ "kind": FLOOR_TYPE_RECIPE, "thickness": 0.3 }),
            ),
    )
}

fn resolve_level(store: &Store, r: &ElementRef) -> Result<String> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such level"))?;
    let e = store.get(id).expect("resolved");
    if e.class != crate::levels::CLASS {
        return Err(Error::new(
            "floor.not_a_level",
            format!("element {id} is not a level"),
        ));
    }
    Ok(e.guid().to_string())
}

fn resolve_floor(store: &Store, r: &ElementRef) -> Result<(ElementId, FloorRecipe)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such floor"))?;
    let recipe = floor_recipe(store.get(id).expect("resolved")).ok_or_else(|| {
        Error::new(
            "floor.not_reviera",
            format!("element {id} is not a Reviera floor"),
        )
    })?;
    Ok((id, recipe))
}

/// Sets the type's thickness (`type.set_param` `thickness`; accepts units).
pub(crate) fn set_type_thickness(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    value: &Value,
) -> Result<()> {
    let t = match value {
        Value::Number(n) => n.as_f64().unwrap_or(-1.0),
        Value::String(s) => {
            crate::units::parse(s, "length", &crate::units::settings_of(txn.store()))?
        }
        _ => -1.0,
    };
    if !(t > 0.0 && t.is_finite()) {
        return Err(Error::new(
            "type.invalid_value",
            "the thickness must be greater than 0",
        ));
    }
    let mut recipe = txn
        .store()
        .get(id)
        .expect("resolved")
        .param(RECIPE_KEY)
        .cloned()
        .expect("a floor type");
    recipe["thickness"] = t.into();
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), recipe);
    })
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// A floor type; omitted, the project's default floor type (created on first use).
    pub type_id: Option<ElementRef>,
    /// Boundary curves, as loops or one list: outer loops are the floor, inner loops holes.
    /// Curves picked from walls may carry a `lock` to follow the wall face.
    pub loops: Vec<Vec<sketch::CurveArg>>,
    pub level_id: ElementRef,
    /// Height of the top above the level (accepts units).
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: f64,
}

pub(crate) fn create(txn: &mut crate::txn::Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let curves: Vec<sketch::CurveArg> = p.loops.into_iter().flatten().collect();
    let sketch = sketch::from_args(&store, &curves)?;
    let level = resolve_level(&store, &p.level_id)?;
    let type_id = match &p.type_id {
        Some(r) => {
            let id = store
                .resolve(r)
                .ok_or_else(|| Error::new("element.not_found", "no such floor type"))?;
            if store.get(id).and_then(type_thickness).is_none() {
                return Err(Error::new(
                    "type.wrong_family",
                    format!("element {id} is not a floor type"),
                ));
            }
            id
        }
        None => default_type(txn)?,
    };
    let type_guid = txn.store().get(type_id).expect("exists").guid().to_string();
    let recipe = FloorRecipe {
        kind: FLOOR_RECIPE.into(),
        level,
        offset: p.offset,
    };
    txn.create(
        NewElement::new("IfcSlab")
            .param("@PredefinedType", "FLOOR")
            .param("@type", type_guid)
            .param(
                RECIPE_KEY,
                serde_json::to_value(&recipe).expect("serialises"),
            )
            .param(
                SKETCH_KEY,
                serde_json::to_value(&sketch).expect("serialises"),
            ),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditBoundaryParams {
    pub floor_id: ElementRef,
    pub loops: Vec<Vec<sketch::CurveArg>>,
}

/// Replaces a floor's boundary (Edit Boundary › Finish).
pub(crate) fn edit_boundary(
    txn: &mut crate::txn::Transaction<'_>,
    p: EditBoundaryParams,
) -> Result<()> {
    let store = txn.store().clone();
    let (id, _) = resolve_floor(&store, &p.floor_id)?;
    crate::modify::check_unpinned(&store, id)?;
    let curves: Vec<sketch::CurveArg> = p.loops.into_iter().flatten().collect();
    let sketch = sketch::from_args(&store, &curves)?;
    let value = serde_json::to_value(&sketch).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(SKETCH_KEY.into(), value);
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetLevelParams {
    pub floor_ids: Vec<ElementRef>,
    /// A new level; omitted, the floors stay on theirs.
    pub level_id: Option<ElementRef>,
    /// Height of the top above the level (accepts units); omitted, unchanged.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: Option<f64>,
}

/// Sets floors' level and height offset (the Level and Height Offset from Level parameters).
pub(crate) fn set_level(txn: &mut crate::txn::Transaction<'_>, p: SetLevelParams) -> Result<()> {
    let store = txn.store().clone();
    let level = p
        .level_id
        .as_ref()
        .map(|l| resolve_level(&store, l))
        .transpose()?;
    for r in &p.floor_ids {
        let (id, mut recipe) = resolve_floor(&store, r)?;
        crate::modify::check_unpinned(&store, id)?;
        if let Some(l) = &level {
            recipe.level = l.clone();
        }
        if let Some(o) = p.offset {
            recipe.offset = o;
        }
        let value = serde_json::to_value(&recipe).expect("serialises");
        txn.modify(id, |e| {
            e.params.insert(RECIPE_KEY.into(), value);
        })?;
    }
    Ok(())
}

/// Moves a floor's boundary by a plan vector (`element.move`); locked curves stay locked.
pub(crate) fn translate(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    x: &dyn Fn([f64; 2]) -> [f64; 2],
    reflects: bool,
) -> Result<()> {
    let mut s = sketch::sketch_of(txn.store().get(id).expect("exists")).unwrap_or_default();
    for c in &mut s.curves {
        c.line = crate::walls::Line {
            start: x(c.line.start),
            end: x(c.line.end),
        };
        // A transformed curve no longer lies on its wall face.
        c.lock = None;
    }
    if reflects {
        // Mirroring reverses the loops; keep outer loops counter-clockwise.
        for c in &mut s.curves {
            c.line = crate::walls::Line {
                start: c.line.end,
                end: c.line.start,
            };
        }
        for lp in &mut s.loops {
            lp.reverse();
        }
    }
    let value = serde_json::to_value(&s).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(SKETCH_KEY.into(), value);
    })
}
