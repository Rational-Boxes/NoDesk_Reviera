// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Spaces (MEP-SPACE-001/002): MEP volume elements independent of architectural rooms, holding
//! load, airflow and lighting data for analysis and exchange.
//!
//! - A space is an `IfcSpace` with ObjectType "MEPSpace" (PredefinedType SPACE) and a
//!   `recipe.mep_space`: the same level, point, upper limit and offsets as a room, so its
//!   boundary is found as a room's is (walls and room separation lines on its level). Rooms and
//!   spaces at one place are separate elements; room tools never see spaces.
//! - **Occupiable** when a room is placed at its location.
//! - **Unplaced** spaces keep their data and stay in schedules as "Not Placed" until deleted.
//! - **Properties** (`rxd:space`): condition type, return-air plenum, reflectances, the lighting
//!   workplane height, specified airflows and per-space overrides of the space type's values
//!   (lighting and power density, people, schedules, set points, outside air). A value not
//!   overridden is the space type's (MEP-SET-004), or the defaults without one.
//! - **Actual values**: lighting and power loads from the fixtures and devices whose calculation
//!   points are in the space; supply, return and exhaust airflow from its air terminals' preset
//!   flows.
//! - In IFC each space has a body (base to upper limit), `Qto_SpaceBaseQuantities` (NetFloorArea,
//!   NetVolume, Height), `Pset_SpaceCommon.IsExternal` and, with airflows, `Pset_SpaceThermalDesign`.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::mesh_objects::{self, Category};
use crate::rooms::{self, RoomGeometry, RoomRecipe};
use crate::space_types::{self, TypeValues};
use crate::store::Store;
use crate::txn::{Transaction, Warning};
use crate::walls::RECIPE_KEY;

pub const SPACE_RECIPE: &str = "recipe.mep_space";
pub const OBJECT_TYPE: &str = "MEPSpace";
pub const PROPS_KEY: &str = "rxd:space";
/// Regions smaller than this get no space when placing automatically (m²).
pub const MIN_AREA: f64 = 0.025;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// What conditions a space.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    #[default]
    HeatedAndCooled,
    Heated,
    Cooled,
    Unconditioned,
    Vented,
    NaturallyVentedOnly,
}

/// Per-space values that replace the space type's.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Overrides {
    pub lighting_load_density: Option<f64>,
    pub power_load_density: Option<f64>,
    pub area_per_person: Option<f64>,
    pub sensible_heat_per_person: Option<f64>,
    pub latent_heat_per_person: Option<f64>,
    pub occupancy_schedule: Option<String>,
    pub lighting_schedule: Option<String>,
    pub power_schedule: Option<String>,
    pub heating_set_point: Option<f64>,
    pub cooling_set_point: Option<f64>,
    pub outside_air_per_person: Option<f64>,
    pub outside_air_per_area: Option<f64>,
}

impl Overrides {
    /// The type's values with these laid over them; the names of the overridden ones.
    fn apply(&self, mut v: TypeValues) -> (TypeValues, Vec<&'static str>) {
        let mut over = Vec::new();
        macro_rules! lay {
            ($($f:ident),*) => {$(
                if let Some(x) = &self.$f {
                    v.$f = x.clone();
                    over.push(stringify!($f));
                }
            )*};
        }
        lay!(
            lighting_load_density,
            power_load_density,
            area_per_person,
            sensible_heat_per_person,
            latent_heat_per_person,
            occupancy_schedule,
            lighting_schedule,
            power_schedule,
            heating_set_point,
            cooling_set_point,
            outside_air_per_person,
            outside_air_per_area
        );
        (v, over)
    }
}

/// A space's own properties.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct SpaceProps {
    /// Unplaced: kept, without a location.
    pub unplaced: bool,
    pub condition: Condition,
    pub return_air_plenum: bool,
    pub ceiling_reflectance: f64,
    pub wall_reflectance: f64,
    pub floor_reflectance: f64,
    /// Metres above the floor.
    pub workplane_height: f64,
    /// Specified airflows (m³/s).
    pub specified_supply: Option<f64>,
    pub specified_return: Option<f64>,
    pub specified_exhaust: Option<f64>,
    pub overrides: Overrides,
}

impl Default for SpaceProps {
    fn default() -> Self {
        SpaceProps {
            unplaced: false,
            condition: Condition::HeatedAndCooled,
            return_air_plenum: false,
            ceiling_reflectance: 0.8,
            wall_reflectance: 0.5,
            floor_reflectance: 0.2,
            workplane_height: 0.76,
            specified_supply: None,
            specified_return: None,
            specified_exhaust: None,
            overrides: Overrides::default(),
        }
    }
}

/// The recipe of a space (a room's, of the space kind).
pub fn space_recipe(e: &Element) -> Option<RoomRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")? == SPACE_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

pub fn props(e: &Element) -> SpaceProps {
    e.param(PROPS_KEY).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

/// A space's geometry now: none when unplaced.
pub fn geometry(store: &Store, e: &Element) -> Option<RoomGeometry> {
    let r = space_recipe(e)?;
    if props(e).unplaced {
        return None;
    }
    rooms::regenerate_recipe(store, &r).ok()
}

/// Every space with its geometry (none when unplaced or broken).
pub fn all(store: &Store) -> Vec<(ElementId, RoomRecipe, Option<RoomGeometry>)> {
    store.iter().filter_map(|e| Some((e.id(), space_recipe(e)?, geometry(store, e)))).collect()
}

/// The space around a plan point on a level.
pub fn space_at(store: &Store, level: ElementId, p: [f64; 2]) -> Option<ElementId> {
    let guid = store.get(level)?.guid().to_string();
    all(store).into_iter().find(|(_, r, g)| r.level == guid && g.as_ref().and_then(|g| g.region.as_ref()).is_some_and(|reg| reg.contains(p))).map(|s| s.0)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceParams {
    pub level_id: ElementRef,
    /// A plan point inside the region to fill (metres).
    pub point: [f64; 2],
    /// The upper limit level; the space's own level when omitted.
    pub upper_limit: Option<ElementRef>,
    /// Height of the top above the upper limit (metres or text with units); default 4000 mm.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub limit_offset: Option<Value>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base_offset: Option<Value>,
    pub name: Option<String>,
    /// Default: one more than the highest space number in use.
    pub number: Option<String>,
}

fn next_number(store: &Store) -> String {
    let n = store
        .iter()
        .filter(|e| space_recipe(e).is_some())
        .filter_map(|e| e.name.as_deref().and_then(|n| n.trim().parse::<u64>().ok()))
        .max()
        .unwrap_or(0);
    (n + 1).to_string()
}

/// `space.place`.
pub(crate) fn place(txn: &mut Transaction<'_>, p: PlaceParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let (level, level_guid) = rooms::resolve_level(&store, &p.level_id)?;
    let level_z = crate::levels::elevation(store.get(level).expect("resolved"));
    if rooms::in_wall(&store, level_z + rooms::COMPUTATION_HEIGHT, p.point) {
        return Err(bad("space.inside_element", "Space placement point is inside an element"));
    }
    let upper_level = p.upper_limit.as_ref().map(|u| rooms::resolve_level(&store, u).map(|(_, g)| g)).transpose()?;
    let num = |v: &Option<Value>, what: &str| v.as_ref().map(|v| rooms::length(&store, v, what)).transpose();
    let recipe = RoomRecipe {
        kind: SPACE_RECIPE.into(),
        level: level_guid,
        point: p.point,
        upper_level,
        limit_offset: num(&p.limit_offset, "the limit offset")?.unwrap_or(rooms::DEFAULT_LIMIT_OFFSET),
        base_offset: num(&p.base_offset, "the base offset")?.unwrap_or(0.0),
    };
    let g = rooms::regenerate_recipe(&store, &recipe)?;
    if g.unbounded_height() <= 0.0 {
        return Err(bad("space.bad_limits", "the space's upper limit is at or below its base"));
    }
    let number = p.number.unwrap_or_else(|| next_number(&store));
    let id = txn.create(
        NewElement::new(rooms::CLASS)
            .name(number)
            .param(rooms::NAME_KEY, p.name.unwrap_or_else(|| "Space".to_owned()))
            .param("@PredefinedType", "SPACE")
            .param("@ObjectType", OBJECT_TYPE)
            .param(RECIPE_KEY, serde_json::to_value(&recipe).expect("serialises")),
    )?;
    if !g.enclosed() {
        txn.warn(Warning { id: "space.not_enclosed".into(), message: "Space is not in a properly enclosed region".into(), element_ids: vec![id] });
    }
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceAutoParams {
    pub level_id: ElementRef,
}

/// `space.place_auto`: a space in every enclosed region of a level larger than 0.025 m² that has
/// none, numbered in sequence (top to bottom, then left to right).
pub(crate) fn place_auto(txn: &mut Transaction<'_>, p: PlaceAutoParams) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let (level, _) = rooms::resolve_level(&store, &p.level_id)?;
    let level_z = crate::levels::elevation(store.get(level).expect("resolved"));
    let z = level_z + rooms::COMPUTATION_HEIGHT;
    let edges = rooms::plane_edges(&store, level_z);
    let mut candidates: Vec<(crate::arrangement::Region, [f64; 2])> = crate::arrangement::regions(&edges)
        .into_iter()
        .filter(|r| r.area() > MIN_AREA)
        .filter_map(|r| {
            let p = rooms::interior_point(&r)?;
            Some((r, p))
        })
        .collect();
    let key = |r: &crate::arrangement::Region| {
        let ring = r.outer_ring();
        let top = ring.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
        let left = ring.iter().map(|p| p[0]).fold(f64::MAX, f64::min);
        ((-top * 2.0).round(), left)
    };
    candidates.sort_by(|a, b| {
        let (ka, kb) = (key(&a.0), key(&b.0));
        ka.0.total_cmp(&kb.0).then(ka.1.total_cmp(&kb.1))
    });
    let mut out = Vec::new();
    for (_, point) in candidates {
        if rooms::in_wall(txn.store(), z, point) || space_at(txn.store(), level, point).is_some() {
            continue;
        }
        out.push(place(
            txn,
            PlaceParams { level_id: ElementRef::Id(level), point, upper_limit: None, limit_offset: None, base_offset: None, name: None, number: None },
        )?);
    }
    Ok(out)
}

fn spaces_of(store: &Store, ids: &[ElementRef]) -> Result<Vec<ElementId>> {
    ids.iter()
        .map(|r| {
            let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such space"))?;
            store.get(id).and_then(space_recipe).ok_or_else(|| bad("space.not_a_space", format!("element {id} is not a space")))?;
            Ok(id)
        })
        .collect()
}

fn write_props(txn: &mut Transaction<'_>, id: ElementId, p: &SpaceProps) -> Result<()> {
    let v = serde_json::to_value(p).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(PROPS_KEY.into(), v);
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IdsParams {
    pub ids: Vec<ElementRef>,
}

/// `space.unplace`: the spaces keep their data without a location ("Not Placed").
pub(crate) fn unplace(txn: &mut Transaction<'_>, p: IdsParams) -> Result<()> {
    let store = txn.store().clone();
    for id in spaces_of(&store, &p.ids)? {
        let mut pr = props(store.get(id).expect("resolved"));
        pr.unplaced = true;
        write_props(txn, id, &pr)?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub ids: Vec<ElementRef>,
    pub number: Option<String>,
    pub name: Option<String>,
    /// Place an unplaced space again at this point.
    pub point: Option<[f64; 2]>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub limit_offset: Option<Value>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base_offset: Option<Value>,
    pub condition: Option<Condition>,
    pub return_air_plenum: Option<bool>,
    pub ceiling_reflectance: Option<f64>,
    pub wall_reflectance: Option<f64>,
    pub floor_reflectance: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub workplane_height: Option<f64>,
    /// m³/s; a negative value clears it.
    pub specified_supply: Option<f64>,
    pub specified_return: Option<f64>,
    pub specified_exhaust: Option<f64>,
    /// Values replacing the space type's; each given value overrides, a `null` one goes back to
    /// the type's.
    #[serde(default)]
    pub overrides: serde_json::Map<String, Value>,
}

/// `space.set`: spaces' number, name, limits and properties, and overrides of their type.
pub(crate) fn set(txn: &mut Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let ids = spaces_of(&store, &p.ids)?;
    if p.number.is_some() && ids.len() > 1 {
        return Err(bad("space.number_not_unique", "several spaces cannot take one number"));
    }
    for r in [p.ceiling_reflectance, p.wall_reflectance, p.floor_reflectance].into_iter().flatten() {
        if !(0.0..=1.0).contains(&r) {
            return Err(bad("space.reflectance", "reflectances run from 0 to 1"));
        }
    }
    for id in ids {
        let e = store.get(id).expect("resolved");
        let mut pr = props(e);
        let mut recipe = space_recipe(e).expect("a space");
        if let Some(c) = p.condition {
            pr.condition = c;
        }
        if let Some(b) = p.return_air_plenum {
            pr.return_air_plenum = b;
        }
        pr.ceiling_reflectance = p.ceiling_reflectance.unwrap_or(pr.ceiling_reflectance);
        pr.wall_reflectance = p.wall_reflectance.unwrap_or(pr.wall_reflectance);
        pr.floor_reflectance = p.floor_reflectance.unwrap_or(pr.floor_reflectance);
        pr.workplane_height = p.workplane_height.unwrap_or(pr.workplane_height);
        for (slot, v) in [(&mut pr.specified_supply, p.specified_supply), (&mut pr.specified_return, p.specified_return), (&mut pr.specified_exhaust, p.specified_exhaust)] {
            if let Some(v) = v {
                *slot = (v >= 0.0).then_some(v);
            }
        }
        // Overrides: merged into the stored ones, nulls removing.
        let mut o = serde_json::to_value(&pr.overrides).expect("serialises");
        for (k, v) in &p.overrides {
            if o.get(k).is_none() {
                return Err(bad("space.override", format!("{k:?} is not a space type value")));
            }
            o[k] = v.clone();
        }
        pr.overrides = serde_json::from_value(o).map_err(|e| bad("space.override", e.to_string()))?;
        let (values, _) = pr.overrides.apply(type_values(&store, e));
        check_values(&values)?;
        if let Some(pt) = p.point {
            recipe.point = pt;
            pr.unplaced = false;
        }
        if let Some(v) = &p.limit_offset {
            recipe.limit_offset = rooms::length(&store, v, "the limit offset")?;
        }
        if let Some(v) = &p.base_offset {
            recipe.base_offset = rooms::length(&store, v, "the base offset")?;
        }
        if rooms::regenerate_recipe(&store, &recipe)?.unbounded_height() <= 0.0 {
            return Err(bad("space.bad_limits", "the space's upper limit is at or below its base"));
        }
        let rv = serde_json::to_value(&recipe).expect("serialises");
        let (number, name) = (p.number.clone(), p.name.clone());
        txn.modify(id, |e| {
            e.params.insert(RECIPE_KEY.into(), rv);
            if let Some(n) = number {
                e.name = Some(n);
            }
            if let Some(n) = name {
                e.params.insert(rooms::NAME_KEY.into(), Value::from(n));
            }
        })?;
        write_props(txn, id, &pr)?;
    }
    Ok(())
}

fn check_values(v: &TypeValues) -> Result<()> {
    let neg = [v.lighting_load_density, v.power_load_density, v.area_per_person, v.sensible_heat_per_person, v.latent_heat_per_person, v.outside_air_per_person, v.outside_air_per_area];
    if neg.iter().any(|x| !(x.is_finite() && *x >= 0.0)) {
        return Err(bad("space.override", "loads, people and outside air are zero or more"));
    }
    if v.heating_set_point > v.cooling_set_point {
        return Err(bad("space.override", "the heating set point is at most the cooling set point"));
    }
    Ok(())
}

/// The values of a space's (or room's) type, or the defaults.
fn type_values(store: &Store, e: &Element) -> TypeValues {
    let a = space_types::analysis(store);
    e.param(space_types::SPACE_TYPE_KEY)
        .and_then(Value::as_str)
        .and_then(|id| a.space_types.iter().find(|t| t.id == id))
        .map(|t| t.values.clone())
        .unwrap_or_default()
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NameParams {
    /// Spaces to name; every space when empty.
    #[serde(default)]
    pub ids: Vec<ElementRef>,
}

/// `space.name_from_rooms`: each space takes the number and name of the room at its location.
/// Returns the spaces named.
pub(crate) fn name_from_rooms(txn: &mut Transaction<'_>, p: NameParams) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let ids: Vec<ElementId> = if p.ids.is_empty() { all(&store).into_iter().map(|s| s.0).collect() } else { spaces_of(&store, &p.ids)? };
    let mut named = Vec::new();
    for id in ids {
        let Some(room) = room_under(&store, id) else { continue };
        let (number, name) = rooms::number_and_name(store.get(room).expect("listed"));
        txn.modify(id, |e| {
            e.name = Some(number);
            e.params.insert(rooms::NAME_KEY.into(), Value::from(name));
        })?;
        named.push(id);
    }
    Ok(named)
}

/// The room placed where a space is.
fn room_under(store: &Store, space: ElementId) -> Option<ElementId> {
    let e = store.get(space)?;
    let r = space_recipe(e)?;
    let level = GlobalId::parse(&r.level).and_then(|g| store.find_by_guid(&g))?;
    let g = geometry(store, e)?;
    let probe = g.region.as_ref().and_then(rooms::interior_point).unwrap_or(r.point);
    rooms::room_at(store, level, probe)
}

// ---- Information ----

/// A space as analysis and schedules read it.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SpaceInfo {
    pub id: ElementId,
    pub number: String,
    pub name: String,
    pub placed: bool,
    pub enclosed: bool,
    /// m², m³, m.
    pub area: f64,
    pub volume: f64,
    pub height: f64,
    pub occupiable: bool,
    pub space_type: Option<String>,
    pub props: SpaceProps,
    /// The type's values with the overrides laid over them, and which are overridden.
    pub values: TypeValues,
    pub overridden: Vec<String>,
    /// Lighting and power from the fixtures and devices in it (W), and per m².
    pub actual_lighting_load: f64,
    pub actual_power_load: f64,
    pub actual_lighting_density: f64,
    pub actual_power_density: f64,
    /// From its air terminals' preset flows (m³/s).
    pub actual_supply: f64,
    pub actual_return: f64,
    pub actual_exhaust: f64,
    /// People by area per person.
    pub people: f64,
}

/// Information on a space.
pub fn info(store: &Store, id: ElementId) -> Option<SpaceInfo> {
    let e = store.get(id)?;
    let r = space_recipe(e)?;
    let pr = props(e);
    let g = geometry(store, e);
    let level = GlobalId::parse(&r.level).and_then(|g| store.find_by_guid(&g));
    let (area, volume, height, enclosed) = g.as_ref().map_or((0.0, 0.0, 0.0, false), |g| (g.area(), g.volume(), g.unbounded_height(), g.enclosed()));
    let (values, over) = pr.overrides.apply(type_values(store, e));
    let (number, name) = rooms::number_and_name(e);
    let mut out = SpaceInfo {
        id,
        number,
        name,
        placed: !pr.unplaced,
        enclosed,
        area,
        volume,
        height,
        occupiable: room_under(store, id).is_some(),
        space_type: e.param(space_types::SPACE_TYPE_KEY).and_then(Value::as_str).map(str::to_owned),
        people: if values.area_per_person > 0.0 { area / values.area_per_person } else { 0.0 },
        props: pr,
        values,
        overridden: over.into_iter().map(str::to_owned).collect(),
        actual_lighting_load: 0.0,
        actual_power_load: 0.0,
        actual_lighting_density: 0.0,
        actual_power_density: 0.0,
        actual_supply: 0.0,
        actual_return: 0.0,
        actual_exhaust: 0.0,
    };
    let (Some(level), Some(region)) = (level, g.as_ref().and_then(|g| g.region.as_ref())) else { return Some(out) };
    for c in store.iter() {
        let Some(cr) = mesh_objects::mesh_object_recipe(c) else { continue };
        if GlobalId::parse(&cr.level).and_then(|g| store.find_by_guid(&g)) != Some(level) {
            continue;
        }
        let Some((_, t)) = mesh_objects::type_of(store, c) else { continue };
        let Some(p) = crate::mep_lighting::calc_point(store, c) else { continue };
        if !region.contains([p[0], p[1]]) {
            continue;
        }
        for port in crate::mep::ports(store, c) {
            let k = &port.connector;
            if let Some(el) = &k.electrical {
                let va: f64 = el.load.iter().sum();
                if t.category == Category::LightingFixture {
                    out.actual_lighting_load += va;
                } else if t.category.is_electrical_device() || t.category.is_electrical_equipment() {
                    out.actual_power_load += va;
                }
            }
            if t.category == Category::AirTerminal && k.domain == crate::mep::Domain::Duct && k.flow_config == crate::mep::FlowConfig::Preset {
                match k.classification {
                    Some(crate::mep::Classification::SupplyAir) => out.actual_supply += k.flow,
                    Some(crate::mep::Classification::ReturnAir) => out.actual_return += k.flow,
                    Some(crate::mep::Classification::ExhaustAir) => out.actual_exhaust += k.flow,
                    _ => {}
                }
            }
        }
    }
    if area > 0.0 {
        out.actual_lighting_density = out.actual_lighting_load / area;
        out.actual_power_density = out.actual_power_load / area;
    }
    Some(out)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InfoParams {
    /// Spaces; every space when empty.
    #[serde(default)]
    pub ids: Vec<ElementRef>,
}

/// `space.info`.
pub(crate) fn infos(store: &Store, p: InfoParams) -> Result<Vec<SpaceInfo>> {
    let ids: Vec<ElementId> = if p.ids.is_empty() { all(store).into_iter().map(|s| s.0).collect() } else { spaces_of(store, &p.ids)? };
    Ok(ids.into_iter().filter_map(|id| info(store, id)).collect())
}

/// Every space's information, by id (for schedules).
pub fn infos_by_id(store: &Store) -> BTreeMap<ElementId, SpaceInfo> {
    all(store).into_iter().filter_map(|(id, _, _)| Some((id, info(store, id)?))).collect()
}
