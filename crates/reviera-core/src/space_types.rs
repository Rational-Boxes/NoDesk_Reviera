// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Building and space types (MEP-SET-004): the design loads, occupancy and set points that
//! energy and load analysis read for a building or a room.
//!
//! - The types live on the project (`rxd:space_types`, read from `rxd:analysis` where an earlier
//!   build kept them, a key structural load settings also use), as two lists: building types and
//!   space types. Reviera ships none (published tables of densities are standards'
//!   material); a project makes its own.
//! - A room takes a space type (`space.set_type`), a building a building type
//!   (`building.set_type`), as RXD element data naming the type's id.
//! - A type still in use cannot be deleted; the refusal names how many rooms or buildings use it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::store::Store;
use crate::txn::Transaction;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

pub const ANALYSIS_KEY: &str = "rxd:space_types";
/// Where the first build kept them (shared with the structural load settings, so it is only read).
const OLD_KEY: &str = "rxd:analysis";
/// On a room: its space type's id; on a building: its building type's id.
pub const SPACE_TYPE_KEY: &str = "rxd:space_type";
pub const BUILDING_TYPE_KEY: &str = "rxd:building_type";

/// A building's or space's design values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct TypeValues {
    /// W/m².
    pub lighting_load_density: f64,
    pub power_load_density: f64,
    /// m² per person.
    pub area_per_person: f64,
    /// W per person.
    pub sensible_heat_per_person: f64,
    pub latent_heat_per_person: f64,
    /// The share of lighting heat that goes to the plenum (0–1).
    pub plenum_lighting: f64,
    /// Schedule names (from the schedule library, ANA-EM-002).
    pub occupancy_schedule: String,
    pub lighting_schedule: String,
    pub power_schedule: String,
    /// °C.
    pub heating_set_point: f64,
    pub cooling_set_point: f64,
    /// L/s per person and per m².
    pub outside_air_per_person: f64,
    pub outside_air_per_area: f64,
    /// "08:00".
    pub opening_time: String,
    pub closing_time: String,
}

impl Default for TypeValues {
    fn default() -> Self {
        TypeValues {
            lighting_load_density: 0.0,
            power_load_density: 0.0,
            area_per_person: 0.0,
            sensible_heat_per_person: 0.0,
            latent_heat_per_person: 0.0,
            plenum_lighting: 0.0,
            occupancy_schedule: String::new(),
            lighting_schedule: String::new(),
            power_schedule: String::new(),
            heating_set_point: 20.0,
            cooling_set_point: 24.0,
            outside_air_per_person: 0.0,
            outside_air_per_area: 0.0,
            opening_time: "08:00".into(),
            closing_time: "18:00".into(),
        }
    }
}

impl TypeValues {
    fn check(&self) -> Result<()> {
        let nonneg = [
            ("lighting load density", self.lighting_load_density),
            ("power load density", self.power_load_density),
            ("area per person", self.area_per_person),
            ("sensible heat per person", self.sensible_heat_per_person),
            ("latent heat per person", self.latent_heat_per_person),
            ("outside air per person", self.outside_air_per_person),
            ("outside air per area", self.outside_air_per_area),
        ];
        if let Some((name, _)) = nonneg.iter().find(|(_, v)| !(v.is_finite() && *v >= 0.0)) {
            return Err(bad("space_type.value", format!("the {name} is zero or more")));
        }
        if !(0.0..=1.0).contains(&self.plenum_lighting) {
            return Err(bad("space_type.value", "the plenum lighting share runs from 0 to 1"));
        }
        if !(self.heating_set_point.is_finite() && self.cooling_set_point.is_finite()) || self.heating_set_point > self.cooling_set_point {
            return Err(bad("space_type.set_points", "the heating set point is at most the cooling set point"));
        }
        for t in [&self.opening_time, &self.closing_time] {
            if !time_ok(t) {
                return Err(bad("space_type.time", format!("{t:?} is not a time such as 08:00")));
            }
        }
        Ok(())
    }
}

fn time_ok(t: &str) -> bool {
    let Some((h, m)) = t.split_once(':') else { return false };
    matches!((h.parse::<u32>(), m.parse::<u32>()), (Ok(h), Ok(m)) if h < 24 && m < 60 && m.to_string().len() <= 2 && h.to_string().len() <= 2)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AnalysisType {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub values: TypeValues,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Analysis {
    pub building_types: Vec<AnalysisType>,
    pub space_types: Vec<AnalysisType>,
}

/// Building or space types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Which {
    Building,
    Space,
}

impl Which {
    fn list(self, a: &Analysis) -> &Vec<AnalysisType> {
        match self {
            Which::Building => &a.building_types,
            Which::Space => &a.space_types,
        }
    }
    fn list_mut(self, a: &mut Analysis) -> &mut Vec<AnalysisType> {
        match self {
            Which::Building => &mut a.building_types,
            Which::Space => &mut a.space_types,
        }
    }
    fn key(self) -> &'static str {
        match self {
            Which::Building => BUILDING_TYPE_KEY,
            Which::Space => SPACE_TYPE_KEY,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Which::Building => "building type",
            Which::Space => "space type",
        }
    }
}

fn project(store: &Store) -> Option<ElementId> {
    store.iter().find(|e| e.class == "IfcProject").map(|e| e.id())
}

pub fn analysis(store: &Store) -> Analysis {
    project(store)
        .and_then(|p| store.get(p))
        .and_then(|p| p.param(ANALYSIS_KEY).or_else(|| p.param(OLD_KEY).filter(|v| v.get("space_types").is_some() || v.get("building_types").is_some())))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

fn save(txn: &mut Transaction<'_>, a: &Analysis) -> Result<()> {
    let pid = project(txn.store()).ok_or_else(|| bad("project.missing", "the model has no project"))?;
    txn.set_param(pid, ANALYSIS_KEY, serde_json::to_value(a).expect("serialises"))
}

/// The rooms or buildings using a type.
pub fn users(store: &Store, which: Which, id: &str) -> Vec<ElementId> {
    store.iter().filter(|e| e.param(which.key()).and_then(|v| v.as_str()) == Some(id)).map(|e| e.id()).collect()
}

fn find(a: &Analysis, which: Which, r: &str) -> Result<usize> {
    which
        .list(a)
        .iter()
        .position(|t| t.id == r || t.name.eq_ignore_ascii_case(r))
        .ok_or_else(|| bad("space_type.not_found", format!("no {} {r:?}", which.label())))
}

fn free_id(a: &Analysis, name: &str) -> String {
    let base: String = name.trim().to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    let taken = |id: &str| a.building_types.iter().chain(&a.space_types).any(|t| t.id == id);
    std::iter::once(base.clone()).chain((2..).map(|n| format!("{base}_{n}"))).find(|id| !taken(id)).expect("a free id")
}

fn check_name(a: &Analysis, which: Which, name: &str, except: Option<&str>) -> Result<String> {
    let n = name.trim();
    if n.is_empty() {
        return Err(bad("space_type.name", "name the type"));
    }
    if which.list(a).iter().any(|t| Some(t.id.as_str()) != except && t.name.eq_ignore_ascii_case(n)) {
        return Err(bad("space_type.exists", format!("a {} {n:?} exists", which.label())));
    }
    Ok(n.to_owned())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub name: String,
    #[serde(default)]
    pub values: Option<TypeValues>,
    /// Copy this type's values (its id or name): Duplicate.
    #[serde(default)]
    pub from: Option<String>,
}

fn create(txn: &mut Transaction<'_>, which: Which, p: CreateParams) -> Result<String> {
    let mut a = analysis(txn.store());
    let name = check_name(&a, which, &p.name, None)?;
    let values = match (&p.values, &p.from) {
        (Some(v), _) => v.clone(),
        (None, Some(f)) => which.list(&a)[find(&a, which, f)?].values.clone(),
        (None, None) => TypeValues::default(),
    };
    values.check()?;
    let id = free_id(&a, &name);
    which.list_mut(&mut a).push(AnalysisType { id: id.clone(), name, values });
    save(txn, &a)?;
    Ok(id)
}

pub(crate) fn create_space(txn: &mut Transaction<'_>, p: CreateParams) -> Result<String> {
    create(txn, Which::Space, p)
}

pub(crate) fn create_building(txn: &mut Transaction<'_>, p: CreateParams) -> Result<String> {
    create(txn, Which::Building, p)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub which: Which,
    /// The type's id or name.
    pub type_id: String,
    /// A new name (Rename).
    pub name: Option<String>,
    pub values: Option<TypeValues>,
}

/// `space_type.set`: renames a building or space type or changes its values.
pub(crate) fn set(txn: &mut Transaction<'_>, p: SetParams) -> Result<()> {
    let mut a = analysis(txn.store());
    let k = find(&a, p.which, &p.type_id)?;
    let id = p.which.list(&a)[k].id.clone();
    if let Some(n) = &p.name {
        let n = check_name(&a, p.which, n, Some(&id))?;
        p.which.list_mut(&mut a)[k].name = n;
    }
    if let Some(v) = p.values {
        v.check()?;
        p.which.list_mut(&mut a)[k].values = v;
    }
    save(txn, &a)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteParams {
    pub which: Which,
    pub type_id: String,
}

/// `space_type.delete`: refused while rooms or buildings use the type.
pub(crate) fn delete(txn: &mut Transaction<'_>, p: DeleteParams) -> Result<()> {
    let mut a = analysis(txn.store());
    let k = find(&a, p.which, &p.type_id)?;
    let t = p.which.list(&a)[k].clone();
    let n = users(txn.store(), p.which, &t.id).len();
    if n > 0 {
        let what = match (p.which, n) {
            (Which::Space, 1) => "1 space uses it".to_owned(),
            (Which::Space, n) => format!("{n} spaces use it"),
            (Which::Building, 1) => "1 building uses it".to_owned(),
            (Which::Building, n) => format!("{n} buildings use it"),
        };
        return Err(bad("space_type.in_use", format!("The {} {:?} cannot be deleted: {what}", p.which.label(), t.name)));
    }
    p.which.list_mut(&mut a).remove(k);
    save(txn, &a)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    pub which: Which,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TypeInfo {
    #[serde(flatten)]
    pub ty: AnalysisType,
    /// The rooms or buildings using it.
    pub used_by: usize,
}

/// `space_type.list`.
pub(crate) fn list(store: &Store, p: ListParams) -> Result<Vec<TypeInfo>> {
    Ok(p.which.list(&analysis(store)).iter().map(|t| TypeInfo { used_by: users(store, p.which, &t.id).len(), ty: t.clone() }).collect())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssignParams {
    /// Rooms (`space.set_type`) or buildings (`building.set_type`).
    pub element_ids: Vec<ElementRef>,
    /// The type's id or name; none clears it.
    pub type_id: Option<String>,
}

fn assign(txn: &mut Transaction<'_>, which: Which, p: AssignParams) -> Result<()> {
    let store = txn.store().clone();
    let a = analysis(&store);
    let id = p.type_id.as_deref().map(|t| find(&a, which, t).map(|k| which.list(&a)[k].id.clone())).transpose()?;
    for r in &p.element_ids {
        let e = store.resolve(r).and_then(|i| store.get(i)).ok_or_else(|| bad("element.not_found", "no such element"))?;
        let fits = match which {
            Which::Space => e.class == "IfcSpace",
            Which::Building => e.class == "IfcBuilding",
        };
        if !fits {
            return Err(bad("space_type.element", format!("element {} is not a {}", e.id(), if which == Which::Space { "room" } else { "building" })));
        }
        let (eid, v) = (e.id(), id.clone());
        txn.modify(eid, |e| match v {
            Some(v) => {
                e.params.insert(which.key().into(), serde_json::Value::from(v));
            }
            None => {
                e.params.remove(which.key());
            }
        })?;
    }
    Ok(())
}

pub(crate) fn assign_space(txn: &mut Transaction<'_>, p: AssignParams) -> Result<()> {
    assign(txn, Which::Space, p)
}

pub(crate) fn assign_building(txn: &mut Transaction<'_>, p: AssignParams) -> Result<()> {
    assign(txn, Which::Building, p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_and_values() {
        assert!(time_ok("08:00") && time_ok("23:59") && !time_ok("24:00") && !time_ok("8") && !time_ok("08:600"));
        let mut v = TypeValues::default();
        assert!(v.check().is_ok());
        v.heating_set_point = 26.0;
        assert!(v.check().is_err());
    }
}
