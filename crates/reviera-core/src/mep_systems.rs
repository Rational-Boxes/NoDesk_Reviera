// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! MEP systems and system types (MEP-SYS-001, MEP-SYS-002).
//!
//! - A **system type** belongs to one fixed classification (Supply Air, Domestic Cold Water, …)
//!   and carries its graphics (colour, line pattern), material, calculations, abbreviation, rise
//!   and drop symbols and, for piping, the fluid and its temperature. Each classification has a
//!   built-in type of its name; more are made by duplicating (`system_type.create`). Types are kept
//!   in the RXD document `rxd:mep_settings` on the project (only those made or changed).
//! - A **system** is an `IfcDistributionSystem` with `recipe.mep_system`: its classification, its
//!   type, its member ports (an element may be in several systems through different connectors;
//!   a port is in one) and the equipment that serves it. Members join by their connectors of the
//!   system's classification (Fitting and Global connectors join any). Names are unique within a
//!   classification.
//! - **Divide** splits a system whose members form several networks into "Name 1", "Name 2", ….
//!
//! In IFC each system is written with its classification's `PredefinedType`, the type's name as
//! `ObjectType`, its members by `IfcRelAssignsToGroup` and the building it serves by
//! `IfcRelServicesBuildings`.
//!
//! Not built yet: electrical circuits and switch systems (§4.9), systems made by drawing ducts
//! and pipes from a connector, the System Editor mode, Generate Placeholder.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::mep::{self, Classification, Domain, StoredPort};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::RECIPE_KEY;

pub const SETTINGS_KEY: &str = "rxd:mep_settings";
pub const RECIPE: &str = "recipe.mep_system";
pub const CLASS: &str = "IfcDistributionSystem";

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Calculations {
    #[default]
    All,
    FlowOnly,
    Performance,
    None,
}

/// A system type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SystemType {
    /// Stable id (`supply_air` for the built-in Supply Air type; `t1`, `t2`, … for made ones).
    pub id: String,
    pub name: String,
    pub classification: Classification,
    /// The colour of its members in views (RGBA 0–1).
    pub color: [f32; 4],
    #[serde(default)]
    pub line_pattern: String,
    #[serde(default)]
    pub material: String,
    #[serde(default)]
    pub calculations: Calculations,
    #[serde(default)]
    pub abbreviation: String,
    #[serde(default)]
    pub rise_drop: String,
    /// Piping: the fluid and its temperature (°C).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fluid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fluid_temperature: Option<f64>,
    /// Piping: the fixture-unit-to-flow method.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub flow_conversion: String,
    /// Hydronic: the IFC system type, when it should not follow the fluid temperature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ifc_predefined: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct MepSettings {
    /// The types made or changed; the built-in ones not here are as shipped.
    pub system_types: Vec<SystemType>,
}

fn slug(c: Classification) -> String {
    serde_json::to_value(c).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

/// The built-in type of a classification: its name, abbreviation, colour and fluid.
pub fn builtin(c: Classification) -> SystemType {
    use Classification as K;
    let (abbr, color, fluid, temp): (&str, [f32; 4], Option<&str>, Option<f64>) = match c {
        K::SupplyAir => ("SA", [0.20, 0.45, 0.85, 1.0], None, None),
        K::ReturnAir => ("RA", [0.85, 0.30, 0.65, 1.0], None, None),
        K::ExhaustAir => ("EA", [0.55, 0.40, 0.20, 1.0], None, None),
        K::OtherAir => ("OA", [0.45, 0.70, 0.40, 1.0], None, None),
        K::HydronicSupply => ("HS", [0.85, 0.20, 0.15, 1.0], Some("Water"), Some(70.0)),
        K::HydronicReturn => ("HR", [0.95, 0.55, 0.20, 1.0], Some("Water"), Some(50.0)),
        K::DomesticHotWater => ("DHW", [0.90, 0.35, 0.25, 1.0], Some("Water"), Some(60.0)),
        K::DomesticColdWater => ("DCW", [0.15, 0.55, 0.85, 1.0], Some("Water"), Some(10.0)),
        K::Sanitary => ("SAN", [0.45, 0.35, 0.25, 1.0], Some("Water"), Some(20.0)),
        K::Vent => ("V", [0.55, 0.55, 0.55, 1.0], None, None),
        K::FireProtectionWet => ("FPW", [0.90, 0.10, 0.10, 1.0], Some("Water"), Some(10.0)),
        K::FireProtectionDry => ("FPD", [0.80, 0.25, 0.25, 1.0], None, None),
        K::FireProtectionPreAction => ("FPP", [0.75, 0.20, 0.35, 1.0], None, None),
        K::FireProtectionOther => ("FPO", [0.70, 0.30, 0.30, 1.0], None, None),
        K::Other | K::Fitting | K::Global => ("OTH", [0.50, 0.50, 0.55, 1.0], None, None),
    };
    SystemType {
        id: slug(c),
        name: c.label().into(),
        classification: c,
        color,
        line_pattern: "Solid".into(),
        material: String::new(),
        calculations: Calculations::All,
        abbreviation: abbr.into(),
        rise_drop: String::new(),
        fluid: fluid.map(str::to_owned),
        fluid_temperature: temp,
        flow_conversion: String::new(),
        ifc_predefined: None,
    }
}

fn project(store: &Store) -> Option<&Element> {
    store.iter().find(|e| e.class == "IfcProject")
}

pub fn settings(store: &Store) -> MepSettings {
    project(store)
        .and_then(|p| p.param(SETTINGS_KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

fn save_settings(txn: &mut Transaction<'_>, s: &MepSettings) -> Result<()> {
    let pid = project(txn.store())
        .map(|p| p.id())
        .ok_or_else(|| bad("project.missing", "the model has no project"))?;
    txn.set_param(pid, SETTINGS_KEY, serde_json::to_value(s).expect("serialises"))
}

/// Every system type: the built-in ones (as changed) and those made.
pub fn system_types(store: &Store) -> Vec<SystemType> {
    let s = settings(store);
    let mut out: Vec<SystemType> = Classification::ALL
        .iter()
        .filter(|c| !c.is_wildcard())
        .map(|c| {
            let b = builtin(*c);
            s.system_types.iter().find(|t| t.id == b.id).cloned().unwrap_or(b)
        })
        .collect();
    let made: Vec<SystemType> = s.system_types.iter().filter(|t| !out.iter().any(|o| o.id == t.id)).cloned().collect();
    out.extend(made);
    out
}

pub fn system_type(store: &Store, id: &str) -> Option<SystemType> {
    system_types(store).into_iter().find(|t| t.id == id)
}

/// `system_type.list`.
pub(crate) fn list_types(store: &Store, _: Empty) -> Result<Vec<SystemType>> {
    Ok(system_types(store))
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}

/// Values of a system type that may be given; omitted ones are kept (or the classification's).
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TypeParams {
    pub name: Option<String>,
    pub color: Option<[f32; 4]>,
    pub line_pattern: Option<String>,
    pub material: Option<String>,
    pub calculations: Option<Calculations>,
    pub abbreviation: Option<String>,
    pub rise_drop: Option<String>,
    pub fluid: Option<String>,
    pub fluid_temperature: Option<f64>,
    pub flow_conversion: Option<String>,
    /// Hydronic only: `HEATING` or `CHILLEDWATER`, overriding the choice by temperature.
    pub ifc_predefined: Option<String>,
}

impl TypeParams {
    fn apply(&self, t: &mut SystemType) {
        if let Some(v) = &self.name {
            t.name = v.trim().to_owned();
        }
        if let Some(v) = self.color {
            t.color = v;
        }
        macro_rules! take {
            ($($f:ident),*) => { $( if let Some(v) = &self.$f { t.$f = v.clone(); } )* };
        }
        take!(line_pattern, material, calculations, abbreviation, rise_drop, flow_conversion);
        if let Some(v) = &self.fluid {
            t.fluid = Some(v.clone());
        }
        if let Some(v) = self.fluid_temperature {
            t.fluid_temperature = Some(v);
        }
        if let Some(v) = &self.ifc_predefined {
            t.ifc_predefined = Some(v.clone());
        }
    }
}

fn check_type(store: &Store, t: &SystemType) -> Result<()> {
    if t.name.is_empty() {
        return Err(bad("system_type.name", "a system type needs a name"));
    }
    if system_types(store).iter().any(|o| o.id != t.id && o.name.eq_ignore_ascii_case(&t.name)) {
        return Err(bad("system_type.duplicate", format!("there is already a system type named {:?}", t.name)));
    }
    if t.color.iter().any(|c| !(0.0..=1.0).contains(c)) {
        return Err(bad("system_type.color", "colour components are from 0 to 1"));
    }
    if let Some(p) = &t.ifc_predefined
        && !matches!(p.as_str(), "HEATING" | "CHILLEDWATER")
    {
        return Err(bad("system_type.ifc", "the IFC system type override is HEATING or CHILLEDWATER"));
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTypeParams {
    pub classification: Classification,
    pub name: String,
    #[serde(flatten)]
    pub params: TypeParams,
}

/// Makes a system type of a classification from its built-in type (`system_type.create`).
pub(crate) fn create_type(txn: &mut Transaction<'_>, p: CreateTypeParams) -> Result<String> {
    if p.classification.is_wildcard() {
        return Err(bad("system_type.classification", "Fitting and Global are not system classifications"));
    }
    let store = txn.store().clone();
    let base = system_types(&store)
        .into_iter()
        .find(|t| t.id == slug(p.classification))
        .unwrap_or_else(|| builtin(p.classification));
    let mut s = settings(&store);
    let n = (1..).find(|k| !system_types(&store).iter().any(|t| t.id == format!("t{k}"))).expect("free");
    let mut t = SystemType { id: format!("t{n}"), ..base };
    p.params.apply(&mut t);
    t.name = p.name.trim().to_owned();
    check_type(&store, &t)?;
    let id = t.id.clone();
    s.system_types.push(t);
    save_settings(txn, &s)?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetTypeParams {
    pub type_id: String,
    #[serde(flatten)]
    pub params: TypeParams,
}

/// Changes a system type (`system_type.set`): its systems' members show the new colour.
pub(crate) fn set_type(txn: &mut Transaction<'_>, p: SetTypeParams) -> Result<()> {
    let store = txn.store().clone();
    let mut t = system_type(&store, &p.type_id)
        .ok_or_else(|| bad("system_type.not_found", format!("no system type {:?}", p.type_id)))?;
    p.params.apply(&mut t);
    check_type(&store, &t)?;
    let mut s = settings(&store);
    match s.system_types.iter_mut().find(|o| o.id == t.id) {
        Some(o) => *o = t,
        None => s.system_types.push(t),
    }
    save_settings(txn, &s)
}

// ---- Systems ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SystemKind {
    Duct,
    Piping,
    Power,
    Data,
    Switch,
}

/// The stored recipe of a system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SystemRecipe {
    pub kind: String,
    pub classification: Classification,
    pub system_type: String,
    pub members: Vec<StoredPort>,
    /// The equipment that serves it (GlobalId).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equipment: Option<String>,
}

pub fn recipe(e: &Element) -> Option<SystemRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// A system as it is now.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct SystemInfo {
    pub id: ElementId,
    pub name: String,
    pub classification: Classification,
    pub system_type: String,
    pub type_name: String,
    /// Its member ports (element, connector index) that still exist.
    pub ports: Vec<(ElementId, usize)>,
    /// Its member elements, each once.
    pub elements: Vec<ElementId>,
    pub equipment: Option<ElementId>,
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

fn info(store: &Store, e: &Element, r: &SystemRecipe) -> SystemInfo {
    let ports: Vec<(ElementId, usize)> = r
        .members
        .iter()
        .filter_map(|m| {
            let id = find(store, &m.element)?;
            mep::port(store, id, m.port).map(|_| (id, m.port))
        })
        .collect();
    let mut elements: Vec<ElementId> = Vec::new();
    for (id, _) in &ports {
        if !elements.contains(id) {
            elements.push(*id);
        }
    }
    SystemInfo {
        id: e.id(),
        name: e.name.clone().unwrap_or_default(),
        classification: r.classification,
        type_name: system_type(store, &r.system_type).map(|t| t.name).unwrap_or_else(|| r.system_type.clone()),
        system_type: r.system_type.clone(),
        ports,
        elements,
        equipment: r.equipment.as_deref().and_then(|g| find(store, g)),
    }
}

/// Every system.
pub fn systems(store: &Store) -> Vec<SystemInfo> {
    store
        .iter()
        .filter(|e| e.class == CLASS)
        .filter_map(|e| Some(info(store, e, &recipe(e)?)))
        .collect()
}

/// The systems each port is in.
pub fn port_systems(store: &Store) -> HashMap<(ElementId, usize), Vec<ElementId>> {
    let mut m: HashMap<(ElementId, usize), Vec<ElementId>> = HashMap::new();
    for s in systems(store) {
        for p in s.ports {
            m.entry(p).or_default().push(s.id);
        }
    }
    m
}

/// The colour each element is drawn in by its systems' types (the first system's).
pub fn element_colors(store: &Store) -> HashMap<ElementId, [f32; 4]> {
    let types: HashMap<String, SystemType> = system_types(store).into_iter().map(|t| (t.id.clone(), t)).collect();
    let mut m = HashMap::new();
    for s in systems(store) {
        if let Some(t) = types.get(&s.system_type) {
            for e in &s.elements {
                m.entry(*e).or_insert(t.color);
            }
        }
    }
    m
}

/// Ports with a duct or pipe classification that are in no system (the System Browser's
/// Unassigned node).
pub fn unassigned(store: &Store) -> Vec<mep::Port> {
    let assigned = port_systems(store);
    mep::all_ports(store)
        .into_iter()
        .filter(|p| matches!(p.connector.domain, Domain::Duct | Domain::Pipe))
        .filter(|p| !assigned.contains_key(&(p.element, p.index)))
        .collect()
}

/// The `IfcDistributionSystemEnum` of a system.
pub fn ifc_predefined(t: &SystemType) -> &'static str {
    use Classification as K;
    match t.classification {
        K::SupplyAir => "AIRCONDITIONING",
        K::ReturnAir | K::OtherAir => "VENTILATION",
        K::ExhaustAir => "EXHAUST",
        K::HydronicSupply | K::HydronicReturn => match t.ifc_predefined.as_deref() {
            Some("CHILLEDWATER") => "CHILLEDWATER",
            Some("HEATING") => "HEATING",
            // Chilled below 20 °C, heating otherwise.
            _ if t.fluid_temperature.is_some_and(|v| v < 20.0) => "CHILLEDWATER",
            _ => "HEATING",
        },
        K::DomesticColdWater => "DOMESTICCOLDWATER",
        K::DomesticHotWater => "DOMESTICHOTWATER",
        K::Sanitary => "DRAINAGE",
        K::Vent => "VENT",
        K::FireProtectionWet | K::FireProtectionDry | K::FireProtectionPreAction | K::FireProtectionOther => "FIREPROTECTION",
        K::Other | K::Fitting | K::Global => "USERDEFINED",
    }
}

fn kind_fits(kind: SystemKind, c: Classification) -> Result<()> {
    match kind {
        SystemKind::Duct if c.is_air() || c == Classification::Other => Ok(()),
        SystemKind::Piping if !c.is_air() => Ok(()),
        SystemKind::Duct | SystemKind::Piping => Err(bad(
            "system.kind",
            format!("a {} system cannot be {}", if kind == SystemKind::Duct { "duct" } else { "piping" }, c.label()),
        )),
        _ => Err(bad("system.not_built", "power, data and switch systems come with circuits (MEP-ELEC), which are not built yet")),
    }
}

/// The ports of `id` that may join a system of `c`: of its domain and classification (or a
/// wildcard).
fn joinable(store: &Store, id: ElementId, c: Classification) -> Vec<mep::Port> {
    let domain = if c.is_air() || (c == Classification::Other) { Domain::Duct } else { Domain::Pipe };
    store
        .get(id)
        .map(|e| mep::ports(store, e))
        .unwrap_or_default()
        .into_iter()
        .filter(|p| {
            let pc = p.connector.classification;
            (p.connector.domain == domain || (c == Classification::Other && p.connector.domain == Domain::Pipe))
                && pc.is_some_and(|pc| pc == c || pc.is_wildcard())
        })
        .collect()
}

fn element_label(store: &Store, id: ElementId) -> String {
    store
        .get(id)
        .map(|e| format!("{} {}", e.name.clone().unwrap_or_else(|| e.class.clone()), id.0))
        .unwrap_or_else(|| id.0.to_string())
}

/// The member ports for `ids` joining a system of `c`, refusing an element with none free.
fn member_ports(
    store: &Store,
    ids: &[ElementId],
    c: Classification,
    except: Option<ElementId>,
) -> Result<Vec<StoredPort>> {
    let taken = port_systems(store);
    let mut out = Vec::new();
    for id in ids {
        let ports = joinable(store, *id, c);
        if ports.is_empty() {
            return Err(bad(
                "system.incompatible",
                format!("{} has no {} connector", element_label(store, *id), c.label()),
            ));
        }
        let free: Vec<&mep::Port> = ports
            .iter()
            .filter(|p| taken.get(&(p.element, p.index)).is_none_or(|s| s.iter().all(|s| Some(*s) == except)))
            .collect();
        if free.is_empty() {
            let other = taken.get(&(ports[0].element, ports[0].index)).and_then(|s| s.first()).copied();
            let name = other.and_then(|s| store.get(s)).and_then(|e| e.name.clone()).unwrap_or_default();
            return Err(bad(
                "system.taken",
                format!("{}'s {} connector is already in the system {name:?}", element_label(store, *id), c.label()),
            ));
        }
        let guid = store.get(*id).expect("exists").guid().to_string();
        out.extend(free.iter().map(|p| StoredPort { element: guid.clone(), port: p.index }));
    }
    Ok(out)
}

fn unique_name(store: &Store, c: Classification, base: &str, except: Option<ElementId>) -> String {
    let used: BTreeSet<String> = systems(store)
        .iter()
        .filter(|s| s.classification == c && Some(s.id) != except)
        .map(|s| s.name.to_lowercase())
        .collect();
    (1..).map(|k| format!("{base} {k}")).find(|n| !used.contains(&n.to_lowercase())).expect("free")
}

fn check_name(store: &Store, c: Classification, name: &str, except: Option<ElementId>) -> Result<()> {
    if name.trim().is_empty() {
        return Err(bad("system.name", "a system needs a name"));
    }
    if systems(store)
        .iter()
        .any(|s| s.classification == c && Some(s.id) != except && s.name.eq_ignore_ascii_case(name.trim()))
    {
        return Err(bad("system.duplicate_name", "A system with this name already exists"));
    }
    Ok(())
}

fn ids(store: &Store, refs: &[ElementRef]) -> Result<Vec<ElementId>> {
    let mut out = Vec::new();
    for r in refs {
        let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such element"))?;
        if !out.contains(&id) {
            out.push(id);
        }
    }
    Ok(out)
}

fn write(txn: &mut Transaction<'_>, id: ElementId, r: &SystemRecipe) -> Result<()> {
    let v = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub kind: SystemKind,
    pub element_ids: Vec<ElementRef>,
    /// The system type (`supply_air`, `domestic_cold_water`, … or one made); the members'
    /// classification's built-in type when omitted.
    pub system_type_id: Option<String>,
    pub name: Option<String>,
}

/// Makes a system of elements (`system.create`).
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let members = ids(&store, &p.element_ids)?;
    if members.is_empty() {
        return Err(bad("system.empty", "select the terminals, fixtures or equipment to put in the system"));
    }
    let t = match &p.system_type_id {
        Some(t) => system_type(&store, t).ok_or_else(|| bad("system_type.not_found", format!("no system type {t:?}")))?,
        None => {
            // The classification of the first member's first duct or pipe connector.
            let c = members
                .iter()
                .filter_map(|id| store.get(*id))
                .flat_map(|e| mep::ports(&store, e))
                .filter_map(|p| p.connector.classification)
                .find(|c| !c.is_wildcard() && kind_fits(p.kind, *c).is_ok())
                .ok_or_else(|| bad("system.incompatible", "the elements have no connector for this kind of system"))?;
            system_type(&store, &slug(c)).expect("built in")
        }
    };
    kind_fits(p.kind, t.classification)?;
    let ports = member_ports(&store, &members, t.classification, None)?;
    let name = match &p.name {
        Some(n) => {
            check_name(&store, t.classification, n, None)?;
            n.trim().to_owned()
        }
        None => unique_name(&store, t.classification, &t.name, None),
    };
    let r = SystemRecipe {
        kind: RECIPE.into(),
        classification: t.classification,
        system_type: t.id.clone(),
        members: ports,
        equipment: None,
    };
    txn.create(
        NewElement::new(CLASS)
            .name(name)
            .param("@PredefinedType", ifc_predefined(&t))
            .param("@ObjectType", t.name.clone())
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )
}

fn system(store: &Store, r: &ElementRef) -> Result<(ElementId, SystemRecipe)> {
    let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such system"))?;
    let rec = store
        .get(id)
        .and_then(recipe)
        .ok_or_else(|| bad("system.not_a_system", format!("element {id} is not an MEP system")))?;
    Ok((id, rec))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MembersParams {
    pub system_id: ElementRef,
    pub element_ids: Vec<ElementRef>,
}

/// Adds elements to a system (`system.add`).
pub(crate) fn add(txn: &mut Transaction<'_>, p: MembersParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = system(&store, &p.system_id)?;
    let new = ids(&store, &p.element_ids)?;
    let ports = member_ports(&store, &new, r.classification, Some(id))?;
    for port in ports {
        if !r.members.contains(&port) {
            r.members.push(port);
        }
    }
    write(txn, id, &r)
}

/// Takes elements out of a system (`system.remove`); the equipment among them stops serving it.
pub(crate) fn remove(txn: &mut Transaction<'_>, p: MembersParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = system(&store, &p.system_id)?;
    let guids: BTreeSet<String> = ids(&store, &p.element_ids)?
        .iter()
        .filter_map(|i| store.get(*i).map(|e| e.guid().to_string()))
        .collect();
    let before = r.members.len();
    r.members.retain(|m| !guids.contains(&m.element));
    if r.members.len() == before {
        return Err(bad("system.not_member", "none of the elements is in the system"));
    }
    if r.equipment.as_ref().is_some_and(|g| guids.contains(g)) {
        r.equipment = None;
    }
    write(txn, id, &r)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EquipmentParams {
    pub system_id: ElementRef,
    pub equipment_id: ElementRef,
}

/// Sets the equipment that serves a system (`system.select_equipment`); it joins the system by
/// its connector of the system's classification.
pub(crate) fn select_equipment(txn: &mut Transaction<'_>, p: EquipmentParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = system(&store, &p.system_id)?;
    let eq = store.resolve(&p.equipment_id).ok_or_else(|| bad("element.not_found", "no such equipment"))?;
    let guid = store.get(eq).expect("resolved").guid().to_string();
    if !r.members.iter().any(|m| m.element == guid) {
        let ports = member_ports(&store, &[eq], r.classification, Some(id))?;
        r.members.extend(ports);
    }
    r.equipment = Some(guid);
    write(txn, id, &r)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SystemParams {
    pub system_id: ElementRef,
}

/// Stops the equipment serving a system (`system.disconnect_equipment`); it leaves the system.
pub(crate) fn disconnect_equipment(txn: &mut Transaction<'_>, p: SystemParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = system(&store, &p.system_id)?;
    let g = r.equipment.take().ok_or_else(|| bad("system.no_equipment", "the system has no equipment"))?;
    r.members.retain(|m| m.element != g);
    write(txn, id, &r)
}

/// Splits a system whose members form several networks (`system.divide`): "Name 1", "Name 2", …;
/// the equipment goes with its network. Returns the systems (the first is the original).
pub(crate) fn divide(txn: &mut Transaction<'_>, p: SystemParams) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let (id, r) = system(&store, &p.system_id)?;
    let e = store.get(id).expect("resolved");
    let base = e.name.clone().unwrap_or_default();
    // Group member ports by the network of their element.
    let mut groups: Vec<(BTreeSet<ElementId>, Vec<StoredPort>)> = Vec::new();
    for m in &r.members {
        let Some(el) = find(&store, &m.element) else { continue };
        match groups.iter_mut().find(|(n, _)| n.contains(&el)) {
            Some((_, ports)) => ports.push(m.clone()),
            None => {
                let net: BTreeSet<ElementId> = mep::network(&store, el).into_iter().collect();
                groups.push((net, vec![m.clone()]));
            }
        }
    }
    if groups.len() < 2 {
        return Err(bad("system.one_network", "the system's members are already one network"));
    }
    let mut out = Vec::new();
    for (k, (net, ports)) in groups.into_iter().enumerate() {
        let name = format!("{base} {}", k + 1);
        let equipment = r
            .equipment
            .clone()
            .filter(|g| find(&store, g).is_some_and(|eq| net.contains(&eq)));
        let rec = SystemRecipe { members: ports, equipment, ..r.clone() };
        if k == 0 {
            write(txn, id, &rec)?;
            txn.modify(id, |e| e.name = Some(name))?;
            out.push(id);
        } else {
            let params = e.params.clone();
            let new = txn.create(NewElement {
                class: CLASS.into(),
                name: Some(name),
                params,
                guid: None,
            })?;
            write(txn, new, &rec)?;
            out.push(new);
        }
    }
    Ok(out)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub system_id: ElementRef,
    /// Unique within the classification.
    pub name: Option<String>,
    /// Another type of the same classification.
    pub system_type_id: Option<String>,
}

/// Renames a system or changes its type (`system.set`).
pub(crate) fn set(txn: &mut Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = system(&store, &p.system_id)?;
    if let Some(t) = &p.system_type_id {
        let t = system_type(&store, t).ok_or_else(|| bad("system_type.not_found", format!("no system type {t:?}")))?;
        if t.classification != r.classification {
            return Err(bad(
                "system.classification",
                format!("the system is {}; the type is {}", r.classification.label(), t.classification.label()),
            ));
        }
        r.system_type = t.id.clone();
        write(txn, id, &r)?;
        txn.set_param(id, "@ObjectType", t.name.clone())?;
        txn.set_param(id, "@PredefinedType", ifc_predefined(&t))?;
    }
    if let Some(n) = &p.name {
        check_name(&store, r.classification, n, Some(id))?;
        let n = n.trim().to_owned();
        txn.modify(id, |e| e.name = Some(n))?;
    }
    Ok(())
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    /// Only systems with this element.
    pub element_id: Option<ElementRef>,
}

/// `system.list`.
pub(crate) fn list(store: &Store, p: ListParams) -> Result<Vec<SystemInfo>> {
    let only = p
        .element_id
        .map(|r| store.resolve(&r).ok_or_else(|| bad("element.not_found", "no such element")))
        .transpose()?;
    Ok(systems(store)
        .into_iter()
        .filter(|s| only.is_none_or(|e| s.elements.contains(&e)))
        .collect())
}

/// Unassigned ports by domain, for the System Browser (`system.unassigned`).
pub(crate) fn list_unassigned(store: &Store, _: Empty) -> Result<BTreeMap<String, Vec<(ElementId, usize)>>> {
    let mut m: BTreeMap<String, Vec<(ElementId, usize)>> = BTreeMap::new();
    for p in unassigned(store) {
        m.entry(p.connector.domain.label().to_owned()).or_default().push((p.element, p.index));
    }
    Ok(m)
}

// ---- Checks (MEP-SYS-005) ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CheckKind {
    Duct,
    Pipe,
    Circuit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IssueKind {
    NotWellConnected,
    FlowMismatch,
    FlowDirection,
}

/// A problem a system check finds.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Issue {
    pub kind: IssueKind,
    pub element_ids: Vec<ElementId>,
    /// The connectors concerned (element, index).
    pub connector_refs: Vec<(ElementId, usize)>,
    pub message: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckParams {
    pub kind: CheckKind,
    /// The view the markers are for (the check itself is model-wide).
    #[serde(default)]
    pub view_id: Option<ElementRef>,
    /// Markers on or off in the view (for the window; the issues are returned either way).
    #[serde(default = "yes")]
    pub on: bool,
}

fn yes() -> bool {
    true
}

/// Checks duct or pipe systems (`mep_check.systems`): systems not well connected (no equipment,
/// or members not in the equipment's network), flow and demand mismatches (preset equipment flow
/// unlike its preset terminals' total; a calculated outlet with nothing downstream) and flow
/// direction mismatches (two connected In or two connected Out connectors).
pub fn check(store: &Store, p: CheckParams) -> Result<Vec<Issue>> {
    let air = match p.kind {
        CheckKind::Duct => true,
        CheckKind::Pipe => false,
        CheckKind::Circuit => {
            return Err(bad("mep_check.not_built", "circuits (MEP-ELEC) are not built yet"));
        }
    };
    let domain = if air { Domain::Duct } else { Domain::Pipe };
    let mut out = Vec::new();
    for s in systems(store).into_iter().filter(|s| s.classification.is_air() == air) {
        let Some(eq) = s.equipment else {
            out.push(Issue {
                kind: IssueKind::NotWellConnected,
                element_ids: s.elements.clone(),
                connector_refs: s.ports.clone(),
                message: format!("{} has no equipment", s.name),
            });
            continue;
        };
        let net: BTreeSet<ElementId> = mep::network(store, eq).into_iter().collect();
        let apart: Vec<ElementId> = s.elements.iter().copied().filter(|e| !net.contains(e)).collect();
        if !apart.is_empty() {
            out.push(Issue {
                kind: IssueKind::NotWellConnected,
                connector_refs: s.ports.iter().copied().filter(|(e, _)| apart.contains(e)).collect(),
                message: format!("{} is not well connected: {} of its elements are not connected to its equipment", s.name, apart.len()),
                element_ids: apart,
            });
        }
        // The equipment's preset flow against its preset terminals' total.
        let ports: Vec<mep::Port> = s.ports.iter().filter_map(|(e, k)| mep::port(store, *e, *k)).collect();
        let (eq_ports, terminals): (Vec<&mep::Port>, Vec<&mep::Port>) = ports.iter().partition(|p| p.element == eq);
        if let Some(ep) = eq_ports.iter().find(|p| p.connector.flow_config == mep::FlowConfig::Preset) {
            let presets: Vec<&&mep::Port> = terminals.iter().filter(|p| p.connector.flow_config == mep::FlowConfig::Preset).collect();
            let total: f64 = presets.iter().map(|p| p.connector.flow).sum();
            if !presets.is_empty() && (total - ep.connector.flow).abs() > 0.01 * ep.connector.flow.max(total).max(1e-9) {
                out.push(Issue {
                    kind: IssueKind::FlowMismatch,
                    element_ids: vec![eq],
                    connector_refs: vec![(ep.element, ep.index)],
                    message: format!(
                        "{}: the equipment's preset flow {:.3} m³/s differs from its terminals' {:.3} m³/s",
                        s.name, ep.connector.flow, total
                    ),
                });
            }
        }
    }
    let partners = mep::partners(store);
    let all = mep::all_ports(store);
    let by: HashMap<(ElementId, usize), &mep::Port> = all.iter().map(|p| ((p.element, p.index), p)).collect();
    for p in all.iter().filter(|p| p.connector.domain == domain) {
        let key = (p.element, p.index);
        let mine = partners.get(&key);
        if p.connector.flow_config == mep::FlowConfig::Calculated
            && p.connector.flow_direction == mep::FlowDirection::Out
            && mine.is_none()
        {
            out.push(Issue {
                kind: IssueKind::FlowMismatch,
                element_ids: vec![p.element],
                connector_refs: vec![key],
                message: format!("connector {} of {} is calculated but has nothing downstream", p.index, element_label(store, p.element)),
            });
        }
        for other in mine.into_iter().flatten() {
            if other < &key {
                continue;
            }
            let Some(q) = by.get(other) else { continue };
            let (a, b) = (p.connector.flow_direction, q.connector.flow_direction);
            if a == b && a != mep::FlowDirection::Bidirectional {
                let dir = if a == mep::FlowDirection::In { "In" } else { "Out" };
                out.push(Issue {
                    kind: IssueKind::FlowDirection,
                    element_ids: vec![p.element, q.element],
                    connector_refs: vec![key, *other],
                    message: format!(
                        "{} and {} are connected but both flow {dir}",
                        element_label(store, p.element),
                        element_label(store, q.element)
                    ),
                });
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DisconnectsParams {
    #[serde(default)]
    pub view_id: Option<ElementRef>,
    /// Duct, pipe, cable tray, conduit (electrical ports join through circuits); all when empty.
    #[serde(default)]
    pub domains: Vec<Domain>,
    #[serde(default = "yes")]
    pub on: bool,
}

/// The open connectors (`mep_check.show_disconnects`).
pub(crate) fn disconnects(store: &Store, p: DisconnectsParams) -> Result<Vec<mep::Port>> {
    Ok(mep::open_ports(store, &p.domains))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predefined_types_follow_the_classification() {
        let t = builtin(Classification::DomesticColdWater);
        assert_eq!(ifc_predefined(&t), "DOMESTICCOLDWATER");
        let mut h = builtin(Classification::HydronicSupply);
        assert_eq!(ifc_predefined(&h), "HEATING");
        h.fluid_temperature = Some(7.0);
        assert_eq!(ifc_predefined(&h), "CHILLEDWATER");
        h.ifc_predefined = Some("HEATING".into());
        assert_eq!(ifc_predefined(&h), "HEATING");
        assert_eq!(builtin(Classification::SupplyAir).id, "supply_air");
    }
}
