// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Foundations (STR-FND-001 … 003): isolated footings, wall foundations and foundation slabs.
//!
//! An **isolated footing** is an `IfcFooting` (`PAD_FOOTING`) with a `recipe.footing`: a plan
//! point and rotation, and its top at a level plus an offset; its type (`IfcFootingType` with a
//! `recipe.footing_type`) gives its width, length and thickness. A footing placed under a column
//! is joined to it: it stays centred under the column, turned with it, its top at the column's
//! base, and follows the column; if the column goes, it stays where it was.
//!
//! A **wall foundation** is an `IfcFooting` (`STRIP_FOOTING`) with a `recipe.wall_foundation`
//! naming its wall: it runs the wall's length (plus the type's end extension) under its base. A
//! *bearing* type is a width centred on the wall; a *retaining* type has a toe beyond the wall's
//! exterior face and a heel beyond its interior face. It follows the wall and goes with it.
//!
//! A **foundation slab** is a floor (`floors`) with PredefinedType `BASESLAB` and a foundation
//! slab type: its top at the level plus an offset, its thickness below.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::walls::RECIPE_KEY;

pub const FOOTING_RECIPE: &str = "recipe.footing";
pub const FOOTING_TYPE_RECIPE: &str = "recipe.footing_type";
pub const WALL_FOUNDATION_RECIPE: &str = "recipe.wall_foundation";
pub const WALL_FOUNDATION_TYPE_RECIPE: &str = "recipe.wall_foundation_type";

/// An isolated footing type: its plan size and thickness (metres).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FootingType {
    pub kind: String,
    /// Along the footing's local x.
    pub width: f64,
    /// Along its local y.
    pub length: f64,
    pub thickness: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// A pile cap's piles (STR-FND-004): the footing is a `PILE_CAP` over them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub piles: Option<PileLayout>,
}

/// The piles under a pile cap: a grid of `rows` × `columns` at `spacing`, centred.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PileLayout {
    pub rows: usize,
    pub columns: usize,
    pub spacing: f64,
    /// The piles' type (GlobalId).
    pub pile_type: String,
}

/// How a pile is made.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PileKind {
    #[default]
    Bored,
    Driven,
}

impl PileKind {
    fn predefined(self) -> &'static str {
        match self {
            PileKind::Bored => "BORED",
            PileKind::Driven => "DRIVEN",
        }
    }
}

/// A pile type: its diameter, length and kind (metres).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PileType {
    pub kind: String,
    pub diameter: f64,
    pub length: f64,
    #[serde(default)]
    pub pile_kind: PileKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
}

/// A pile's place under its cap.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CapSlot {
    /// The cap (GlobalId).
    pub cap: String,
    pub index: usize,
}

/// The stored recipe of a pile: its top's centre at a level plus an offset, or under its cap.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PileRecipe {
    pub kind: String,
    pub point: [f64; 2],
    pub level: String,
    #[serde(default)]
    pub offset: f64,
    /// Its own length, else the type's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cap: Option<CapSlot>,
}

pub const PILE_RECIPE: &str = "recipe.pile";
pub const PILE_TYPE_RECIPE: &str = "recipe.pile_type";

pub fn pile_recipe(e: &Element) -> Option<PileRecipe> {
    of_kind(e, PILE_RECIPE)
}
pub fn pile_type(e: &Element) -> Option<PileType> {
    of_kind(e, PILE_TYPE_RECIPE)
}

/// What a wall foundation does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Function {
    /// A width centred on the wall.
    #[default]
    Bearing,
    /// A toe beyond the wall's exterior face and a heel beyond its interior face.
    Retaining,
}

/// A wall foundation type (metres).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WallFoundationType {
    pub kind: String,
    #[serde(default)]
    pub function: Function,
    /// Bearing: the overall width.
    #[serde(default)]
    pub width: f64,
    /// Retaining: beyond the exterior face, and beyond the interior face.
    #[serde(default)]
    pub toe: f64,
    #[serde(default)]
    pub heel: f64,
    pub thickness: f64,
    /// Past each end of the wall.
    #[serde(default)]
    pub end_extension: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
}

/// The stored recipe of an isolated footing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FootingRecipe {
    pub kind: String,
    /// Its centre in plan.
    pub point: [f64; 2],
    #[serde(default)]
    pub rotation: f64,
    /// Its top's level (GlobalId) and height above it.
    pub level: String,
    #[serde(default)]
    pub offset: f64,
    /// The column it is joined to (GlobalId): it follows the column's base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
}

/// The stored recipe of a wall foundation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WallFoundationRecipe {
    pub kind: String,
    /// Its wall (GlobalId).
    pub wall: String,
}

fn of_kind<T: for<'de> Deserialize<'de>>(e: &Element, kind: &str) -> Option<T> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == kind).then(|| serde_json::from_value(v.clone()).ok())?
}

pub fn footing_recipe(e: &Element) -> Option<FootingRecipe> {
    of_kind(e, FOOTING_RECIPE)
}
pub fn footing_type(e: &Element) -> Option<FootingType> {
    of_kind(e, FOOTING_TYPE_RECIPE)
}
pub fn wall_foundation_recipe(e: &Element) -> Option<WallFoundationRecipe> {
    of_kind(e, WALL_FOUNDATION_RECIPE)
}
pub fn wall_foundation_type(e: &Element) -> Option<WallFoundationType> {
    of_kind(e, WALL_FOUNDATION_TYPE_RECIPE)
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

fn type_element<'a>(store: &'a Store, e: &Element) -> Option<&'a Element> {
    let g = e.param("@type")?.as_str()?;
    store.get(find(store, g)?)
}

/// A foundation as it is now: its plan outline (counter-clockwise) between its bottom and top.
#[derive(Debug, Clone, PartialEq)]
pub struct FoundationGeometry {
    pub ring: Vec<[f64; 2]>,
    pub top: f64,
    pub bottom: f64,
}

impl FoundationGeometry {
    pub fn mesh(&self) -> crate::bodies::Mesh {
        crate::bodies::extrude(&self.ring, &[], self.bottom, self.top)
    }
}

/// Regenerates an isolated footing or a wall foundation.
pub fn regenerate(store: &Store, e: &Element) -> Result<FoundationGeometry> {
    if let Some(r) = pile_recipe(e) {
        let t = type_element(store, e)
            .and_then(pile_type)
            .ok_or_else(|| Error::new("pile.no_type", "the pile has no pile type"))?;
        let under = r.cap.as_ref().and_then(|slot| {
            let cap = store.get(find(store, &slot.cap)?)?;
            let layout = type_element(store, cap).and_then(footing_type)?.piles?;
            let g = regenerate(store, cap).ok()?;
            let n = g.ring.len() as f64;
            let c = [
                g.ring.iter().map(|p| p[0]).sum::<f64>() / n,
                g.ring.iter().map(|p| p[1]).sum::<f64>() / n,
            ];
            let edge = [g.ring[1][0] - g.ring[0][0], g.ring[1][1] - g.ring[0][1]];
            let a = edge[1].atan2(edge[0]);
            let (i, j) = (
                slot.index % layout.columns.max(1),
                slot.index / layout.columns.max(1),
            );
            let x = (i as f64 - (layout.columns as f64 - 1.0) / 2.0) * layout.spacing;
            let y = (j as f64 - (layout.rows as f64 - 1.0) / 2.0) * layout.spacing;
            let (s, k) = a.sin_cos();
            Some(([c[0] + x * k - y * s, c[1] + x * s + y * k], g.bottom))
        });
        let (point, top) = match under {
            Some(u) => u,
            None => (
                r.point,
                crate::levels::elevation_of(store, &r.level)? + r.offset,
            ),
        };
        let radius = t.diameter / 2.0;
        let ring = (0..24)
            .map(|k| {
                let a = std::f64::consts::TAU * k as f64 / 24.0;
                [point[0] + radius * a.cos(), point[1] + radius * a.sin()]
            })
            .collect();
        return Ok(FoundationGeometry {
            ring,
            top,
            bottom: top - r.length.unwrap_or(t.length),
        });
    }
    if let Some(r) = footing_recipe(e) {
        let t = type_element(store, e)
            .and_then(footing_type)
            .ok_or_else(|| Error::new("footing.no_type", "the footing has no footing type"))?;
        let column = r
            .column
            .as_deref()
            .and_then(|g| find(store, g))
            .and_then(|c| store.get(c))
            .and_then(|c| {
                let cr = crate::columns::column_recipe(c)?;
                let g = crate::columns::regenerate(store, c).ok()?;
                Some((g.point, cr.rotation, g.z0))
            });
        let (point, rotation, top) = match column {
            Some((p, turn, z)) => (p, turn + r.rotation, z),
            None => (
                r.point,
                r.rotation,
                crate::levels::elevation_of(store, &r.level)? + r.offset,
            ),
        };
        let (s, c) = rotation.sin_cos();
        let (w, l) = (t.width / 2.0, t.length / 2.0);
        let ring = [[-w, -l], [w, -l], [w, l], [-w, l]]
            .into_iter()
            .map(|[x, y]| [point[0] + x * c - y * s, point[1] + x * s + y * c])
            .collect();
        return Ok(FoundationGeometry {
            ring,
            top,
            bottom: top - t.thickness,
        });
    }
    if let Some(r) = wall_foundation_recipe(e) {
        let t = type_element(store, e)
            .and_then(wall_foundation_type)
            .ok_or_else(|| {
                Error::new("wall_foundation.no_type", "the wall foundation has no type")
            })?;
        let wall = find(store, &r.wall)
            .and_then(|w| store.get(w))
            .ok_or_else(|| Error::new("wall_foundation.no_wall", "its wall no longer exists"))?;
        let g = crate::walls::regenerate(store, wall)?;
        let (d, n) = (g.direction(), g.normal());
        let (lo, hi) = match t.function {
            Function::Bearing => (-t.width / 2.0, t.width / 2.0),
            Function::Retaining => (-(g.width / 2.0 + t.heel), g.width / 2.0 + t.toe),
        };
        let a = [
            g.start[0] - d[0] * t.end_extension,
            g.start[1] - d[1] * t.end_extension,
        ];
        let b = [
            g.end[0] + d[0] * t.end_extension,
            g.end[1] + d[1] * t.end_extension,
        ];
        let at = |p: [f64; 2], k: f64| [p[0] + n[0] * k, p[1] + n[1] * k];
        return Ok(FoundationGeometry {
            ring: vec![at(a, lo), at(b, lo), at(b, hi), at(a, hi)],
            top: g.base_z,
            bottom: g.base_z - t.thickness,
        });
    }
    Err(Error::new(
        "foundation.not_reviera",
        "not a Reviera footing or wall foundation",
    ))
}

/// Every Reviera footing and wall foundation that regenerates.
pub fn all(store: &Store) -> Vec<(ElementId, FoundationGeometry)> {
    store
        .iter()
        .filter(|e| {
            footing_recipe(e).is_some()
                || wall_foundation_recipe(e).is_some()
                || pile_recipe(e).is_some()
        })
        .filter_map(|e| Some((e.id(), regenerate(store, e).ok()?)))
        .collect()
}

/// The level an element of this module is on (its own, or its wall's base level).
pub fn level_of(store: &Store, e: &Element) -> Option<String> {
    if let Some(r) = footing_recipe(e) {
        return Some(r.level);
    }
    if let Some(r) = pile_recipe(e) {
        return Some(r.level);
    }
    let r = wall_foundation_recipe(e)?;
    let wall = store.get(find(store, &r.wall)?)?;
    Some(crate::walls::wall_recipe(wall)?.base_level)
}

/// Whether an element depends on `guid` (deleted with it): a footing on its level, a wall
/// foundation on its wall.
pub fn uses(e: &Element, guid: &str) -> bool {
    footing_recipe(e).is_some_and(|r| r.level == guid)
        || wall_foundation_recipe(e).is_some_and(|r| r.wall == guid)
        || pile_recipe(e)
            .is_some_and(|r| r.level == guid || r.cap.as_ref().is_some_and(|c| c.cap == guid))
}

// ---- Types ----

fn imperial(store: &Store) -> bool {
    crate::codes::settings(store).code.structural().region == crate::codes::Region::UnitedStates
}

fn concrete(store: &Store) -> String {
    crate::codes::settings(store)
        .code
        .structural()
        .concrete
        .into()
}

/// The default footing type, made on first use: 6'-0" × 6'-0" × 1'-6" in US models, else
/// 1800 × 1800 × 500 mm, in the region's concrete.
fn default_footing_type(txn: &mut crate::txn::Transaction<'_>) -> Result<ElementId> {
    let store = txn.store().clone();
    let (name, w, t) = if imperial(&store) {
        ("Footing 6'-0\" x 6'-0\" x 1'-6\"", 1.8288, 0.4572)
    } else {
        ("Footing 1800 x 1800 x 500", 1.8, 0.5)
    };
    if let Some(e) = store
        .iter()
        .filter(|e| footing_type(e).is_some())
        .min_by_key(|e| e.name.as_deref() != Some(name))
    {
        return Ok(e.id());
    }
    let ty = FootingType {
        kind: FOOTING_TYPE_RECIPE.into(),
        width: w,
        length: w,
        thickness: t,
        material: Some(concrete(&store)),
        piles: None,
    };
    txn.create(
        NewElement::new("IfcFootingType")
            .name(name)
            .param("@PredefinedType", "PAD_FOOTING")
            .param(RECIPE_KEY, serde_json::to_value(ty).expect("serialises")),
    )
}

/// The default wall foundation type: a bearing footing 24" × 12" in US models, else
/// 600 × 300 mm.
fn default_wall_foundation_type(txn: &mut crate::txn::Transaction<'_>) -> Result<ElementId> {
    let store = txn.store().clone();
    let (name, w, t) = if imperial(&store) {
        ("Bearing Footing 24\" x 12\"", 0.6096, 0.3048)
    } else {
        ("Bearing Footing 600 x 300", 0.6, 0.3)
    };
    if let Some(e) = store
        .iter()
        .filter(|e| wall_foundation_type(e).is_some())
        .min_by_key(|e| e.name.as_deref() != Some(name))
    {
        return Ok(e.id());
    }
    let ty = WallFoundationType {
        kind: WALL_FOUNDATION_TYPE_RECIPE.into(),
        function: Function::Bearing,
        width: w,
        toe: 0.0,
        heel: 0.0,
        thickness: t,
        end_extension: 0.0,
        material: Some(concrete(&store)),
    };
    txn.create(
        NewElement::new("IfcFootingType")
            .name(name)
            .param("@PredefinedType", "STRIP_FOOTING")
            .param(RECIPE_KEY, serde_json::to_value(ty).expect("serialises")),
    )
}

/// The default foundation slab type: 12" in US models, else 300 mm.
fn default_slab_type(txn: &mut crate::txn::Transaction<'_>) -> Result<ElementId> {
    let store = txn.store().clone();
    let (name, t) = if imperial(&store) {
        ("Foundation Slab 12\"", 0.3048)
    } else {
        ("Foundation Slab 300", 0.3)
    };
    if let Some(e) = store
        .iter()
        .find(|e| e.name.as_deref() == Some(name) && crate::floors::type_kind(e).is_some())
    {
        return Ok(e.id());
    }
    txn.create(
        NewElement::new("IfcSlabType")
            .name(name)
            .param("@PredefinedType", "BASESLAB")
            .param(
                RECIPE_KEY,
                json!({ "kind": crate::floors::FLOOR_TYPE_RECIPE, "thickness": t, "material": concrete(&store) }),
            ),
    )
}

fn resolve_type(
    store: &Store,
    r: &ElementRef,
    ok: fn(&Element) -> bool,
    what: &str,
) -> Result<ElementId> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", format!("no such {what}")))?;
    if !ok(store.get(id).expect("resolved")) {
        return Err(Error::new(
            "type.wrong_family",
            format!("element {id} is not a {what}"),
        ));
    }
    Ok(id)
}

fn resolve_level(store: &Store, r: &ElementRef) -> Result<String> {
    store
        .resolve(r)
        .and_then(|id| store.get(id))
        .filter(|e| e.class == crate::levels::CLASS)
        .map(|e| e.guid().to_string())
        .ok_or_else(|| Error::new("level.not_found", format!("no level {r:?}")))
}

fn new_footing(
    txn: &mut crate::txn::Transaction<'_>,
    type_id: ElementId,
    recipe: FootingRecipe,
) -> Result<ElementId> {
    let guid = txn
        .store()
        .get(type_id)
        .expect("resolved")
        .guid()
        .to_string();
    let layout = txn
        .store()
        .get(type_id)
        .and_then(footing_type)
        .and_then(|t| t.piles);
    let id = txn.create(
        NewElement::new("IfcFooting")
            .param(
                "@PredefinedType",
                if layout.is_some() {
                    "PILE_CAP"
                } else {
                    "PAD_FOOTING"
                },
            )
            .param("@type", guid)
            .param(
                "Pset_FootingCommon.LoadBearing",
                json!({ "type": "IfcBoolean", "value": true }),
            )
            .param(
                RECIPE_KEY,
                serde_json::to_value(&recipe).expect("serialises"),
            ),
    )?;
    let store = txn.store().clone();
    regenerate(&store, store.get(id).expect("created"))?;
    // A pile cap's piles, laid out under it.
    if let Some(l) = layout {
        let cap = store.get(id).expect("created").guid().to_string();
        let pile_type = find(&store, &l.pile_type)
            .ok_or_else(|| Error::new("pile.no_type", "the pile cap's pile type is gone"))?;
        for index in 0..l.rows * l.columns {
            new_pile(
                txn,
                pile_type,
                PileRecipe {
                    kind: PILE_RECIPE.into(),
                    point: recipe.point,
                    level: recipe.level.clone(),
                    offset: recipe.offset,
                    length: None,
                    cap: Some(CapSlot {
                        cap: cap.clone(),
                        index,
                    }),
                },
            )?;
        }
    }
    Ok(id)
}

fn new_pile(
    txn: &mut crate::txn::Transaction<'_>,
    type_id: ElementId,
    recipe: PileRecipe,
) -> Result<ElementId> {
    let store = txn.store().clone();
    let t = store.get(type_id).expect("resolved");
    let kind = pile_type(t).map(|p| p.pile_kind).unwrap_or_default();
    let id = txn.create(
        NewElement::new("IfcPile")
            .param("@PredefinedType", kind.predefined())
            .param("@type", t.guid().to_string())
            .param(
                "Pset_PileCommon.LoadBearing",
                json!({ "type": "IfcBoolean", "value": true }),
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

/// The default pile type: 18" bored, 40 ft long in US models, else 450 mm bored, 12 m long.
fn default_pile_type(txn: &mut crate::txn::Transaction<'_>) -> Result<ElementId> {
    let store = txn.store().clone();
    let (name, d, l) = if imperial(&store) {
        ("Bored Pile 18\" x 40'", 0.4572, 12.192)
    } else {
        ("Bored Pile 450 x 12000", 0.45, 12.0)
    };
    if let Some(e) = store
        .iter()
        .filter(|e| pile_type(e).is_some())
        .min_by_key(|e| e.name.as_deref() != Some(name))
    {
        return Ok(e.id());
    }
    let ty = PileType {
        kind: PILE_TYPE_RECIPE.into(),
        diameter: d,
        length: l,
        pile_kind: PileKind::Bored,
        material: Some(concrete(&store)),
    };
    txn.create(
        NewElement::new("IfcPileType")
            .name(name)
            .param("@PredefinedType", "BORED")
            .param(RECIPE_KEY, serde_json::to_value(ty).expect("serialises")),
    )
}

/// The default pile cap type: four piles at three diameters, the cap one diameter beyond them
/// and twice a diameter deep.
fn default_pile_cap_type(txn: &mut crate::txn::Transaction<'_>) -> Result<ElementId> {
    let pile = default_pile_type(txn)?;
    let store = txn.store().clone();
    let p = pile_type(store.get(pile).expect("made")).expect("a pile type");
    let spacing = 3.0 * p.diameter;
    let width = spacing + 2.0 * p.diameter;
    let depth = (2.0 * p.diameter).max(0.6);
    let name = "Pile Cap 4 Piles";
    if let Some(e) = store.iter().find(|e| {
        e.name.as_deref() == Some(name) && footing_type(e).is_some_and(|t| t.piles.is_some())
    }) {
        return Ok(e.id());
    }
    let ty = FootingType {
        kind: FOOTING_TYPE_RECIPE.into(),
        width,
        length: width,
        thickness: depth,
        material: Some(concrete(&store)),
        piles: Some(PileLayout {
            rows: 2,
            columns: 2,
            spacing,
            pile_type: store.get(pile).expect("made").guid().to_string(),
        }),
    };
    txn.create(
        NewElement::new("IfcFootingType")
            .name(name)
            .param("@PredefinedType", "PILE_CAP")
            .param(RECIPE_KEY, serde_json::to_value(ty).expect("serialises")),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PileParams {
    /// A pile type; omitted, the default (450 mm bored, 12 m; or 18", 40 ft).
    pub type_id: Option<ElementRef>,
    pub point: [f64; 2],
    /// Its top's level and height above it.
    pub level: ElementRef,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: f64,
    /// Its length; default the type's.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub length: Option<f64>,
}

/// Places a single pile (`pile.place`, STR-FND-004).
pub(crate) fn place_pile(
    txn: &mut crate::txn::Transaction<'_>,
    p: PileParams,
) -> Result<ElementId> {
    let type_id = match &p.type_id {
        Some(r) => resolve_type(txn.store(), r, |e| pile_type(e).is_some(), "pile type")?,
        None => default_pile_type(txn)?,
    };
    if p.length.is_some_and(|l| l <= 0.0) {
        return Err(Error::new(
            "pile.length",
            "a pile's length must be above zero",
        ));
    }
    let level = resolve_level(txn.store(), &p.level)?;
    new_pile(
        txn,
        type_id,
        PileRecipe {
            kind: PILE_RECIPE.into(),
            point: p.point,
            level,
            offset: p.offset,
            length: p.length,
            cap: None,
        },
    )
}

/// Places a pile cap with its piles (`pile_cap.place`, STR-FND-004): an `IfcFooting`
/// (`PILE_CAP`) whose `IfcPile`s are laid out under it.
pub(crate) fn place_pile_cap(
    txn: &mut crate::txn::Transaction<'_>,
    p: PlaceParams,
) -> Result<ElementId> {
    let type_id = match &p.type_id {
        Some(r) => resolve_type(
            txn.store(),
            r,
            |e| footing_type(e).is_some_and(|t| t.piles.is_some()),
            "pile cap type",
        )?,
        None => default_pile_cap_type(txn)?,
    };
    place(
        txn,
        PlaceParams {
            type_id: Some(ElementRef::Id(type_id)),
            ..p
        },
    )
}

fn footing_type_id(
    txn: &mut crate::txn::Transaction<'_>,
    r: &Option<ElementRef>,
) -> Result<ElementId> {
    match r {
        Some(r) => resolve_type(
            txn.store(),
            r,
            |e| footing_type(e).is_some(),
            "footing type",
        ),
        None => default_footing_type(txn),
    }
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceParams {
    /// A footing type; omitted, the default (1800 × 1800 × 500 mm, or 6' × 6' × 1'-6").
    pub type_id: Option<ElementRef>,
    /// Its centre in plan.
    pub point: [f64; 2],
    /// Its top's level and height above it.
    pub level: ElementRef,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: f64,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: f64,
}

/// Places an isolated footing (`footing.place`, STR-FND-001).
pub(crate) fn place(txn: &mut crate::txn::Transaction<'_>, p: PlaceParams) -> Result<ElementId> {
    let type_id = footing_type_id(txn, &p.type_id)?;
    let level = resolve_level(txn.store(), &p.level)?;
    new_footing(
        txn,
        type_id,
        FootingRecipe {
            kind: FOOTING_RECIPE.into(),
            point: p.point,
            rotation: p.rotation,
            level,
            offset: p.offset,
            column: None,
        },
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AtColumnsParams {
    pub type_id: Option<ElementRef>,
    pub column_ids: Vec<ElementRef>,
}

/// Places an isolated footing under each column, joined to it (`footing.place_at_columns`); one
/// undo step. Columns that already have one are skipped.
pub(crate) fn place_at_columns(
    txn: &mut crate::txn::Transaction<'_>,
    p: AtColumnsParams,
) -> Result<Vec<ElementId>> {
    let type_id = footing_type_id(txn, &p.type_id)?;
    let store = txn.store().clone();
    let mut out = Vec::new();
    for r in &p.column_ids {
        let id = store
            .resolve(r)
            .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
        let e = store.get(id).expect("resolved");
        let cr = crate::columns::column_recipe(e).ok_or_else(|| {
            Error::new(
                "footing.not_a_column",
                format!("element {id} is not a column"),
            )
        })?;
        let guid = e.guid().to_string();
        if store
            .iter()
            .any(|f| footing_recipe(f).is_some_and(|r| r.column.as_deref() == Some(guid.as_str())))
        {
            continue;
        }
        out.push(new_footing(
            txn,
            type_id,
            FootingRecipe {
                kind: FOOTING_RECIPE.into(),
                point: cr.point,
                rotation: 0.0,
                level: cr.extent.base_level.clone(),
                offset: cr.extent.base_offset,
                column: Some(guid),
            },
        )?);
    }
    Ok(out)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AtGridsParams {
    pub type_id: Option<ElementRef>,
    /// A footing goes at every point where two of them cross.
    pub grid_ids: Vec<ElementRef>,
    pub level: ElementRef,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: f64,
}

/// Places an isolated footing at every crossing of the given grid lines, turned to them
/// (`footing.place_at_grids`); one undo step.
pub(crate) fn place_at_grids(
    txn: &mut crate::txn::Transaction<'_>,
    p: AtGridsParams,
) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let ids: Vec<ElementId> = p
        .grid_ids
        .iter()
        .map(|r| {
            store
                .resolve(r)
                .filter(|id| store.get(*id).is_some_and(crate::grids::is_grid))
                .ok_or_else(|| Error::new("grid.not_found", format!("no grid line {r:?}")))
        })
        .collect::<Result<_>>()?;
    let points = crate::columns::grid_crossings(&store, Some(&ids));
    if points.is_empty() {
        return Err(Error::new(
            "footing.no_crossings",
            "the chosen grid lines do not cross",
        ));
    }
    let type_id = footing_type_id(txn, &p.type_id)?;
    let level = resolve_level(&store, &p.level)?;
    let mut out = Vec::new();
    for (point, _, _, angle) in points {
        out.push(new_footing(
            txn,
            type_id,
            FootingRecipe {
                kind: FOOTING_RECIPE.into(),
                point,
                rotation: angle,
                level: level.clone(),
                offset: p.offset,
                column: None,
            },
        )?);
    }
    Ok(out)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WallFoundationParams {
    /// A wall foundation type; omitted, the default bearing footing (600 × 300 mm, or 24" × 12").
    pub type_id: Option<ElementRef>,
    pub wall_ids: Vec<ElementRef>,
}

/// Places a wall foundation under each wall (`wall_foundation.create`, STR-FND-002); walls that
/// already have one are skipped.
pub(crate) fn create_wall_foundations(
    txn: &mut crate::txn::Transaction<'_>,
    p: WallFoundationParams,
) -> Result<Vec<ElementId>> {
    let type_id = match &p.type_id {
        Some(r) => resolve_type(
            txn.store(),
            r,
            |e| wall_foundation_type(e).is_some(),
            "wall foundation type",
        )?,
        None => default_wall_foundation_type(txn)?,
    };
    let store = txn.store().clone();
    let type_guid = store.get(type_id).expect("resolved").guid().to_string();
    let mut out = Vec::new();
    for r in &p.wall_ids {
        let id = store
            .resolve(r)
            .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
        let e = store.get(id).expect("resolved");
        if crate::walls::wall_recipe(e).is_none() {
            return Err(Error::new(
                "wall_foundation.not_a_wall",
                format!("element {id} is not a Reviera wall"),
            ));
        }
        let guid = e.guid().to_string();
        if store
            .iter()
            .any(|f| wall_foundation_recipe(f).is_some_and(|r| r.wall == guid))
        {
            continue;
        }
        let recipe = WallFoundationRecipe {
            kind: WALL_FOUNDATION_RECIPE.into(),
            wall: guid,
        };
        let f = txn.create(
            NewElement::new("IfcFooting")
                .param("@PredefinedType", "STRIP_FOOTING")
                .param("@type", type_guid.clone())
                .param(
                    "Pset_FootingCommon.LoadBearing",
                    json!({ "type": "IfcBoolean", "value": true }),
                )
                .param(
                    RECIPE_KEY,
                    serde_json::to_value(&recipe).expect("serialises"),
                ),
        )?;
        let s = txn.store().clone();
        regenerate(&s, s.get(f).expect("created"))?;
        out.push(f);
    }
    Ok(out)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SlabParams {
    /// A floor type; omitted, the default foundation slab type (300 mm, or 12").
    pub type_id: Option<ElementRef>,
    /// Boundary curves: outer loops are the slab, inner loops openings.
    pub boundary: Vec<Vec<crate::sketch::CurveArg>>,
    /// Its top's level and height above it; the slab extends below.
    pub level: ElementRef,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: f64,
}

/// Creates a foundation slab (`foundation_slab.create`, STR-FND-003): a floor with PredefinedType
/// `BASESLAB`.
pub(crate) fn create_slab(
    txn: &mut crate::txn::Transaction<'_>,
    p: SlabParams,
) -> Result<ElementId> {
    let type_id = match p.type_id {
        Some(r) => r,
        None => ElementRef::Id(default_slab_type(txn)?),
    };
    let id = crate::floors::create(
        txn,
        crate::floors::CreateParams {
            type_id: Some(type_id),
            loops: p.boundary,
            level_id: p.level,
            offset: Some(p.offset),
            structural: false,
            span_direction_edge: None,
        },
    )?;
    txn.modify(id, |e| {
        e.params.insert("@PredefinedType".into(), "BASESLAB".into());
    })?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InfoParams {
    pub ids: Vec<ElementRef>,
}

/// A foundation as reported (`foundation.info`).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Info {
    pub id: ElementId,
    /// Its top, and its *Elevation at Bottom* (metres, project).
    pub top: f64,
    pub bottom: f64,
    /// Plan area (m²) and volume (m³).
    pub area: f64,
    pub volume: f64,
    /// The column or wall it is joined to.
    pub host: Option<ElementId>,
}

/// Reports footings' and wall foundations' top, elevation at bottom, area, volume and host
/// (`foundation.info`).
pub fn info(store: &Store, p: &InfoParams) -> Result<Vec<Info>> {
    p.ids
        .iter()
        .map(|r| {
            let id = store
                .resolve(r)
                .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
            let e = store.get(id).expect("resolved");
            let g = regenerate(store, e)?;
            let n = g.ring.len();
            let area = (0..n)
                .map(|i| {
                    let (a, b) = (g.ring[i], g.ring[(i + 1) % n]);
                    a[0] * b[1] - b[0] * a[1]
                })
                .sum::<f64>()
                / 2.0;
            let host = footing_recipe(e)
                .and_then(|r| r.column)
                .or_else(|| wall_foundation_recipe(e).map(|r| r.wall))
                .and_then(|g| find(store, &g));
            Ok(Info {
                id,
                top: g.top,
                bottom: g.bottom,
                area,
                volume: area * (g.top - g.bottom),
                host,
            })
        })
        .collect()
}

/// Sets a footing or wall foundation type's value (`type.set_param`: `width`, `length`,
/// `thickness`, `toe`, `heel`, `end_extension`, `function`, `material`).
pub(crate) fn set_type_param(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    param: &str,
    value: &Value,
) -> Result<()> {
    let store = txn.store().clone();
    let e = store.get(id).expect("checked");
    let length = |v: &Value, min: f64| -> Result<f64> {
        let x = match v {
            Value::Number(n) => n.as_f64().unwrap_or(-1.0),
            Value::String(s) => {
                crate::units::parse(s, "length", &crate::units::settings_of(&store))?
            }
            _ => -1.0,
        };
        if x.is_finite() && x >= min {
            Ok(x)
        } else {
            Err(Error::new(
                "type.invalid_value",
                format!("`{param}` is out of range"),
            ))
        }
    };
    let material = |v: &Value| v.as_str().map(str::to_owned).filter(|s| !s.is_empty());
    let value_json = if let Some(mut t) = footing_type(e) {
        match param {
            "width" => t.width = length(value, 1e-3)?,
            "length" => t.length = length(value, 1e-3)?,
            "thickness" => t.thickness = length(value, 1e-3)?,
            "material" => t.material = material(value),
            _ => {
                let value = (!value.is_null()).then_some(value);
                return crate::parameters::set(txn, &[id], param, value).map(|_| ());
            }
        }
        serde_json::to_value(&t).expect("serialises")
    } else {
        let mut t = wall_foundation_type(e).expect("checked");
        match param {
            "width" => t.width = length(value, 1e-3)?,
            "toe" => t.toe = length(value, 0.0)?,
            "heel" => t.heel = length(value, 0.0)?,
            "thickness" => t.thickness = length(value, 1e-3)?,
            "end_extension" => t.end_extension = length(value, 0.0)?,
            "function" => {
                t.function = serde_json::from_value(value.clone()).map_err(|_| {
                    Error::new(
                        "type.invalid_value",
                        "the function is `bearing` or `retaining`",
                    )
                })?;
            }
            "material" => t.material = material(value),
            _ => {
                let value = (!value.is_null()).then_some(value);
                return crate::parameters::set(txn, &[id], param, value).map(|_| ());
            }
        }
        serde_json::to_value(&t).expect("serialises")
    };
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value_json);
    })
}

/// Moves or turns a footing (`element.move`, `.rotate`, `.mirror`, `.copy`); a moved footing
/// leaves its column.
pub(crate) fn transform(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    x: &dyn Fn([f64; 2]) -> [f64; 2],
    turn: f64,
    mirror: Option<f64>,
    dz: f64,
) -> Result<()> {
    let store = txn.store().clone();
    let e = store.get(id).expect("exists");
    let Some(mut r) = footing_recipe(e) else {
        return Err(Error::new(
            "wall_foundation.follows_wall",
            "a wall foundation follows its wall: move the wall",
        ));
    };
    if r.column.is_some() {
        freeze(&store, e, &mut r)?;
    }
    r.point = x(r.point);
    r.rotation = match mirror {
        Some(axis) => 2.0 * axis - r.rotation,
        None => r.rotation + turn,
    };
    r.offset += dz;
    let v = serde_json::to_value(&r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })
}

/// A footing joined to a column, made free where it is now.
fn freeze(store: &Store, e: &Element, r: &mut FootingRecipe) -> Result<()> {
    let g = regenerate(store, e)?;
    let c = [
        g.ring.iter().map(|p| p[0]).sum::<f64>() / 4.0,
        g.ring.iter().map(|p| p[1]).sum::<f64>() / 4.0,
    ];
    let edge = [g.ring[1][0] - g.ring[0][0], g.ring[1][1] - g.ring[0][1]];
    r.point = c;
    r.rotation = edge[1].atan2(edge[0]);
    r.offset = g.top - crate::levels::elevation_of(store, &r.level)?;
    r.column = None;
    Ok(())
}

/// Frees footings joined to columns about to be deleted: each stays where it is.
pub(crate) fn release(
    txn: &mut crate::txn::Transaction<'_>,
    going: &std::collections::BTreeSet<ElementId>,
) -> Result<()> {
    let store = txn.store().clone();
    let guids: std::collections::HashSet<String> = going
        .iter()
        .filter_map(|id| store.get(*id).map(|e| e.guid().to_string()))
        .collect();
    for e in store.iter() {
        let Some(mut r) = footing_recipe(e) else {
            continue;
        };
        if going.contains(&e.id()) || !r.column.as_ref().is_some_and(|c| guids.contains(c)) {
            continue;
        }
        freeze(&store, e, &mut r)?;
        let v = serde_json::to_value(&r).expect("serialises");
        txn.modify(e.id(), |e| {
            e.params.insert(RECIPE_KEY.into(), v);
        })?;
    }
    Ok(())
}
