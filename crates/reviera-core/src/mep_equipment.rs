// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Air terminals and mechanical equipment (MEP-DUCT-009).
//!
//! - **Air terminals** are components of the Air Terminal category. A terminal placed with a
//!   `flow` gets it as a preset flow on its duct connector. On a duct, it goes on the duct's
//!   bottom face under the point, its upward connector joined to a tap port the duct gains there.
//! - **Space airflow**: each terminal's preset flow counts toward the supply, return or exhaust
//!   total (by its connector's classification) of the room it stands in.
//! - **Mechanical equipment**: air handlers and other unitary equipment, VAV boxes, fans,
//!   boilers, chillers, pumps and radiators, placed as components of those categories.
//! - **Equipment sets** group duty and standby units (pumps, typically) as an `IfcGroup` whose
//!   ObjectType is "EquipmentSet"; the roles are RXD element data. Only duty units count in a
//!   system's equipment flow total.
//! - **Connect Into** draws a duct or pipe from an open connector into a level main: out along
//!   the connector (straight up or down to the main's height for a vertical one), then square to
//!   the main, ending on its side, where the routing preferences put a tee or tap.

use std::collections::{BTreeMap, HashSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::NewElement;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::mep::{self, Classification, Domain, FlowConfig, Shape};
use crate::mep_runs::{self, CreateParams, RunReport, TapPort, segment_recipe};
use crate::mesh_objects::{self, Category};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::RECIPE_KEY;

type V3 = [f64; 3];

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

fn resolve(store: &Store, r: &ElementRef) -> Result<ElementId> {
    store.resolve(r).ok_or_else(|| bad("element.not_found", "no such element"))
}

/// A component's category, if it is a placed component.
fn category(store: &Store, id: ElementId) -> Option<Category> {
    let e = store.get(id)?;
    mesh_objects::mesh_object_recipe(e)?;
    Some(mesh_objects::type_of(store, e)?.1.category)
}

fn type_category(store: &Store, r: &ElementRef) -> Result<Category> {
    let id = resolve(store, r)?;
    store
        .get(id)
        .and_then(mesh_objects::mesh_object_type)
        .map(|t| t.category)
        .ok_or_else(|| bad("mesh_object_type.not_found", format!("element {id} is not a component type")))
}

// ---- Air terminals ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AirTerminalParams {
    /// An Air Terminal component type.
    pub type_id: ElementRef,
    /// The point in plan (metres); on a duct, the point on its axis nearest to it.
    pub point: [f64; 2],
    pub level: ElementRef,
    /// Height of its insertion point above the level (metres); ignored on a duct.
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: f64,
    /// Counter-clockwise; accepts units ("90°").
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: f64,
    /// Its preset flow (m³/s) on its duct connector; the type's when omitted.
    pub flow: Option<f64>,
    /// A straight level duct to mount it on (its bottom face).
    pub duct_id: Option<ElementRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AirTerminalReport {
    pub id: ElementId,
    /// The duct it is mounted on.
    pub duct: Option<ElementId>,
    /// The room it stands in.
    pub room: Option<ElementId>,
}

/// Places an air terminal (`air_terminal.place`).
pub(crate) fn place_air_terminal(txn: &mut Transaction<'_>, p: AirTerminalParams) -> Result<AirTerminalReport> {
    let store = txn.store().clone();
    if type_category(&store, &p.type_id)? != Category::AirTerminal {
        return Err(bad("air_terminal.category", "choose an Air Terminal type"));
    }
    let ty = store.get(resolve(&store, &p.type_id)?).and_then(mesh_objects::mesh_object_type).expect("a type");
    let index = ty
        .connectors
        .iter()
        .position(|c| c.domain == Domain::Duct)
        .ok_or_else(|| bad("air_terminal.no_connector", "the type has no duct connector"))?;
    let level = resolve(&store, &p.level)?;
    let (_, z0) = mep_runs::level_of(&store, &p.level)?;
    let mut point = p.point;
    let mut offset = p.offset;
    let mut host = None;
    if let Some(d) = &p.duct_id {
        let id = resolve(&store, d)?;
        let s = store.get(id).and_then(segment_recipe).ok_or_else(|| bad("air_terminal.not_a_duct", format!("element {id} is not a duct")))?;
        let axis = [s.end[0] - s.start[0], s.end[1] - s.start[1], s.end[2] - s.start[2]];
        let l = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
        if s.domain != Domain::Duct || s.placeholder || s.flex.is_some() || s.corner.is_some() || l < 1e-9 || axis[2].abs() > 1e-6 * l {
            return Err(bad("air_terminal.duct", "a terminal goes on a straight level duct"));
        }
        let c = &ty.connectors[index];
        if c.direction[2] < 0.999 {
            return Err(bad("air_terminal.duct_connector", "a terminal on a duct's bottom needs a connector facing up"));
        }
        let u = [axis[0] / l, axis[1] / l];
        let t = (point[0] - s.start[0]) * u[0] + (point[1] - s.start[1]) * u[1];
        let half = match s.size.shape {
            Shape::Round => s.size.diameter / 2.0,
            _ => s.size.height / 2.0,
        };
        let margin = mep_runs::leg_length(&s.size).max(c.diameter.max(c.width));
        if t < margin || t > l - margin {
            return Err(bad("air_terminal.duct_end", "too near the duct's end; place it further along"));
        }
        point = [s.start[0] + u[0] * t, s.start[1] + u[1] * t];
        let bottom = s.start[2] - half;
        offset = bottom - z0 - c.origin[2];
        let mut h = s.clone();
        h.taps.push(TapPort { at: [point[0], point[1], bottom], direction: [0.0, 0.0, -1.0], size: size_of(c) });
        let v = serde_json::to_value(&h).expect("serialises");
        txn.modify(id, |e| {
            e.params.insert(RECIPE_KEY.into(), v);
        })?;
        host = Some(id);
    }
    let id = mesh_objects::place(
        txn,
        serde_json::from_value(serde_json::json!({ "type_id": p.type_id, "point": point, "level": level, "offset": offset, "rotation": p.rotation }))
            .expect("place parameters"),
    )?;
    if let Some(flow) = p.flow {
        if !(flow >= 0.0 && flow.is_finite()) {
            return Err(bad("air_terminal.flow", "the flow is zero or more"));
        }
        mep::set(
            txn,
            mep::SetParams {
                element_id: ElementRef::Id(id),
                connector_index: index,
                params: mep::ConnectorParams { flow: Some(flow), flow_config: Some(FlowConfig::Preset), ..Default::default() },
            },
        )?;
    }
    mep::connect_coincident(txn, &[id])?;
    if host.is_some()
        && !mep::partners(txn.store()).contains_key(&(id, index))
    {
        return Err(bad("air_terminal.not_connected", "the terminal's connector does not fit the duct's tap"));
    }
    let room = crate::rooms::room_at(txn.store(), level, point);
    Ok(AirTerminalReport { id, duct: host, room })
}

fn size_of(c: &mep::Connector) -> mep_runs::Size {
    mep_runs::Size { shape: c.shape, diameter: c.diameter, width: c.width, height: c.height }
}

// ---- Space airflow ----

/// A room's air terminal flows (m³/s).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SpaceAirflow {
    pub room: ElementId,
    pub supply: f64,
    #[serde(rename = "return")]
    pub return_air: f64,
    pub exhaust: f64,
    /// The terminals counted.
    pub terminals: Vec<ElementId>,
}

fn empty(room: ElementId) -> SpaceAirflow {
    SpaceAirflow { room, supply: 0.0, return_air: 0.0, exhaust: 0.0, terminals: Vec::new() }
}

/// Each room's supply, return and exhaust airflow from the preset flows of the air terminals
/// standing in it.
pub fn airflows(store: &Store) -> BTreeMap<ElementId, SpaceAirflow> {
    let mut out: BTreeMap<ElementId, SpaceAirflow> = BTreeMap::new();
    for e in store.iter() {
        let Some(r) = mesh_objects::mesh_object_recipe(e) else { continue };
        if category(store, e.id()) != Some(Category::AirTerminal) {
            continue;
        }
        let Some(level) = GlobalId::parse(&r.level).and_then(|g| store.find_by_guid(&g)) else { continue };
        let Some(room) = crate::rooms::room_at(store, level, r.point) else { continue };
        let a = out.entry(room).or_insert_with(|| empty(room));
        for p in mep::ports(store, e) {
            if p.connector.domain != Domain::Duct || p.connector.flow_config != FlowConfig::Preset {
                continue;
            }
            match p.connector.classification {
                Some(Classification::SupplyAir) => a.supply += p.connector.flow,
                Some(Classification::ReturnAir) => a.return_air += p.connector.flow,
                Some(Classification::ExhaustAir) => a.exhaust += p.connector.flow,
                _ => continue,
            }
            if !a.terminals.contains(&e.id()) {
                a.terminals.push(e.id());
            }
        }
    }
    out
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AirflowParams {
    /// Rooms; every room with terminals when empty.
    #[serde(default)]
    pub room_ids: Vec<ElementRef>,
}

/// `space.airflow`: rooms' actual supply, return and exhaust airflow.
pub(crate) fn airflow(store: &Store, p: AirflowParams) -> Result<Vec<SpaceAirflow>> {
    let all = airflows(store);
    if p.room_ids.is_empty() {
        return Ok(all.into_values().collect());
    }
    p.room_ids
        .iter()
        .map(|r| {
            let id = resolve(store, r)?;
            if store.get(id).and_then(crate::rooms::room_recipe).is_none() && store.get(id).is_none_or(|e| e.class != "IfcSpace") {
                return Err(bad("space.not_a_room", format!("element {id} is not a room")));
            }
            Ok(all.get(&id).cloned().unwrap_or_else(|| empty(id)))
        })
        .collect()
}

// ---- Mechanical equipment ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EquipmentParams {
    /// A component type of a mechanical equipment category.
    pub type_id: ElementRef,
    pub point: [f64; 2],
    pub level: ElementRef,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: f64,
    /// Counter-clockwise; accepts units ("90°").
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: f64,
}

/// Places mechanical equipment (`mech_equipment.place`).
pub(crate) fn place_equipment(txn: &mut Transaction<'_>, p: EquipmentParams) -> Result<ElementId> {
    let store = txn.store().clone();
    if !type_category(&store, &p.type_id)?.is_mechanical_equipment() {
        return Err(bad(
            "mech_equipment.category",
            "choose a mechanical equipment type (air handler, VAV box, fan, boiler, chiller, pump or radiator)",
        ));
    }
    let level = resolve(&store, &p.level)?;
    mesh_objects::place(
        txn,
        serde_json::from_value(serde_json::json!({ "type_id": p.type_id, "point": p.point, "level": level, "offset": p.offset, "rotation": p.rotation }))
            .expect("place parameters"),
    )
}

// ---- Equipment sets ----

const SET_KIND: &str = "recipe.equipment_set";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EquipmentSetRecipe {
    pub kind: String,
    /// Its units (GlobalIds).
    pub members: Vec<String>,
    /// The duty units among them; the rest stand by.
    pub duty: Vec<String>,
}

pub fn set_recipe(e: &crate::element::Element) -> Option<EquipmentSetRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == SET_KIND).then(|| serde_json::from_value(v.clone()).ok())?
}

/// An equipment set as listed.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EquipmentSetInfo {
    pub id: ElementId,
    pub name: String,
    pub duty: Vec<ElementId>,
    pub standby: Vec<ElementId>,
}

fn guid(store: &Store, id: ElementId) -> String {
    store.get(id).map(|e| e.guid().to_string()).unwrap_or_default()
}

fn find(store: &Store, g: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(g)?)
}

/// Every equipment set.
pub fn sets(store: &Store) -> Vec<EquipmentSetInfo> {
    store
        .iter()
        .filter_map(|e| {
            let r = set_recipe(e)?;
            let ids = |v: &[String]| v.iter().filter_map(|g| find(store, g)).collect::<Vec<_>>();
            let duty = ids(&r.duty);
            let standby = ids(&r.members).into_iter().filter(|m| !duty.contains(m)).collect();
            Some(EquipmentSetInfo { id: e.id(), name: e.name.clone().unwrap_or_default(), duty, standby })
        })
        .collect()
}

/// The units standing by in any set.
pub fn standby(store: &Store) -> HashSet<ElementId> {
    sets(store).into_iter().flat_map(|s| s.standby).collect()
}

fn check_units(store: &Store, ids: &[ElementId], set: Option<ElementId>) -> Result<()> {
    for id in ids {
        if !category(store, *id).is_some_and(Category::is_mechanical_equipment) {
            return Err(bad("equipment_set.not_equipment", format!("element {id} is not mechanical equipment")));
        }
        if let Some(other) = sets(store).into_iter().find(|s| Some(s.id) != set && (s.duty.contains(id) || s.standby.contains(id))) {
            return Err(bad("equipment_set.taken", format!("element {id} is already in {}", other.name)));
        }
    }
    Ok(())
}

fn write_set(txn: &mut Transaction<'_>, id: ElementId, r: &EquipmentSetRecipe) -> Result<()> {
    if r.duty.is_empty() {
        return Err(bad("equipment_set.no_duty", "a set keeps at least one duty unit"));
    }
    let v = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateSetParams {
    /// Its units: pumps, fans or other mechanical equipment.
    pub equipment_ids: Vec<ElementRef>,
    /// The duty units among them; the rest stand by.
    pub duty_ids: Vec<ElementRef>,
    pub name: Option<String>,
}

/// `equipment_set.create`.
pub(crate) fn create_set(txn: &mut Transaction<'_>, p: CreateSetParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let mut members: Vec<ElementId> = Vec::new();
    for r in &p.equipment_ids {
        let id = resolve(&store, r)?;
        if !members.contains(&id) {
            members.push(id);
        }
    }
    let duty: Vec<ElementId> = p.duty_ids.iter().map(|r| resolve(&store, r)).collect::<Result<_>>()?;
    if members.len() < 2 {
        return Err(bad("equipment_set.too_few", "a set has two units or more"));
    }
    if let Some(d) = duty.iter().find(|d| !members.contains(d)) {
        return Err(bad("equipment_set.duty_not_member", format!("duty unit {d} is not one of the set's units")));
    }
    check_units(&store, &members, None)?;
    let n = sets(&store).len() + 1;
    let r = EquipmentSetRecipe {
        kind: SET_KIND.into(),
        members: members.iter().map(|id| guid(&store, *id)).collect(),
        duty: duty.iter().map(|id| guid(&store, *id)).collect(),
    };
    if r.duty.is_empty() {
        return Err(bad("equipment_set.no_duty", "a set keeps at least one duty unit"));
    }
    txn.create(
        NewElement::new("IfcGroup")
            .name(p.name.unwrap_or_else(|| format!("Equipment Set {n}")).as_str())
            .param("@ObjectType", "EquipmentSet")
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditSetParams {
    pub set_id: ElementRef,
    pub equipment_ids: Vec<ElementRef>,
    /// Adding: the added units are duty units (standby by default).
    #[serde(default)]
    pub duty: bool,
}

fn open_set(store: &Store, r: &ElementRef) -> Result<(ElementId, EquipmentSetRecipe)> {
    let id = resolve(store, r)?;
    let rec = store.get(id).and_then(set_recipe).ok_or_else(|| bad("equipment_set.not_found", format!("element {id} is not an equipment set")))?;
    Ok((id, rec))
}

/// `equipment_set.add`: adds units, standing by unless `duty`.
pub(crate) fn add_to_set(txn: &mut Transaction<'_>, p: EditSetParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = open_set(&store, &p.set_id)?;
    let units: Vec<ElementId> = p.equipment_ids.iter().map(|x| resolve(&store, x)).collect::<Result<_>>()?;
    check_units(&store, &units, Some(id))?;
    for u in units {
        let g = guid(&store, u);
        if !r.members.contains(&g) {
            r.members.push(g.clone());
        }
        if p.duty && !r.duty.contains(&g) {
            r.duty.push(g);
        }
    }
    write_set(txn, id, &r)
}

/// `equipment_set.remove`: takes units out of the set.
pub(crate) fn remove_from_set(txn: &mut Transaction<'_>, p: EditSetParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = open_set(&store, &p.set_id)?;
    let out: Vec<String> = p.equipment_ids.iter().map(|x| resolve(&store, x).map(|u| guid(&store, u))).collect::<Result<_>>()?;
    r.members.retain(|g| !out.contains(g));
    r.duty.retain(|g| !out.contains(g));
    write_set(txn, id, &r)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetDutyParams {
    pub set_id: ElementRef,
    /// The duty units; the set's other units stand by.
    pub duty_ids: Vec<ElementRef>,
}

/// `equipment_set.set_duty`: which units are on duty.
pub(crate) fn set_duty(txn: &mut Transaction<'_>, p: SetDutyParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = open_set(&store, &p.set_id)?;
    let duty: Vec<String> = p.duty_ids.iter().map(|x| resolve(&store, x).map(|u| guid(&store, u))).collect::<Result<_>>()?;
    if let Some(d) = duty.iter().find(|d| !r.members.contains(d)) {
        return Err(bad("equipment_set.duty_not_member", format!("{} is not one of the set's units", find(&store, d).map_or(d.clone(), |i| i.to_string()))));
    }
    r.duty = duty;
    write_set(txn, id, &r)
}

/// `equipment_set.list`.
pub(crate) fn list_sets(store: &Store, _: crate::builtin::NoParams) -> Result<Vec<EquipmentSetInfo>> {
    Ok(sets(store))
}

// ---- System flow totals ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TotalsParams {
    pub system_id: ElementRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EquipmentFlow {
    pub element: ElementId,
    /// Its largest connector flow in the system (m³/s).
    pub flow: f64,
    /// Standing by in an equipment set: not counted.
    pub standby: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FlowTotals {
    /// The terminals' preset flows (m³/s).
    pub terminal_flow: f64,
    /// Duty equipment's flows (m³/s).
    pub equipment_flow: f64,
    pub equipment: Vec<EquipmentFlow>,
}

/// `system.flow_totals`: a system's terminal and equipment flow; standby units do not count.
pub(crate) fn flow_totals(store: &Store, p: TotalsParams) -> Result<FlowTotals> {
    let id = resolve(store, &p.system_id)?;
    let sys = crate::mep_systems::systems(store).into_iter().find(|s| s.id == id).ok_or_else(|| bad("system.not_found", format!("element {id} is not a system")))?;
    let standing = standby(store);
    let mut ports = sys.ports.clone();
    if let Some(eq) = sys.equipment {
        for p in mep::ports(store, store.get(eq).expect("listed")) {
            if p.connector.classification == Some(sys.classification) && !ports.contains(&(eq, p.index)) {
                ports.push((eq, p.index));
            }
        }
    }
    let mut terminal_flow = 0.0;
    let mut equipment: Vec<EquipmentFlow> = Vec::new();
    for (e, k) in ports {
        let Some(el) = store.get(e) else { continue };
        if mep_runs::is_run_element(el) {
            continue;
        }
        let Some(port) = mep::port(store, e, k) else { continue };
        if category(store, e).is_some_and(Category::is_mechanical_equipment) {
            match equipment.iter_mut().find(|x| x.element == e) {
                Some(x) => x.flow = x.flow.max(port.connector.flow),
                None => equipment.push(EquipmentFlow { element: e, flow: port.connector.flow, standby: standing.contains(&e) }),
            }
        } else if port.connector.flow_config == FlowConfig::Preset {
            terminal_flow += port.connector.flow;
        }
    }
    let equipment_flow = equipment.iter().filter(|x| !x.standby).map(|x| x.flow).sum();
    Ok(FlowTotals { terminal_flow, equipment_flow, equipment })
}

// ---- Connect Into ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConnectIntoParams {
    /// A terminal, fixture or piece of equipment.
    pub element_id: ElementRef,
    /// Its open duct or pipe connector (0 is the first).
    pub connector_index: usize,
    /// A straight level duct or pipe main.
    pub target_id: ElementRef,
}

/// `mep.connect_into`: a run from a connector into a main, sized as the connector, of the
/// main's type.
pub(crate) fn connect_into(txn: &mut Transaction<'_>, p: ConnectIntoParams) -> Result<RunReport> {
    let store = txn.store().clone();
    let el = resolve(&store, &p.element_id)?;
    let port = mep::port(&store, el, p.connector_index).ok_or_else(|| bad("connector.no_index", format!("element {el} has no connector {}", p.connector_index)))?;
    let domain = port.connector.domain;
    if !matches!(domain, Domain::Duct | Domain::Pipe) {
        return Err(bad("connect_into.domain", "Connect Into joins duct and pipe connectors"));
    }
    if mep::partners(&store).contains_key(&(el, p.connector_index)) {
        return Err(bad("connect_into.connected", "that connector is already connected"));
    }
    let target = resolve(&store, &p.target_id)?;
    let ts = store.get(target).and_then(segment_recipe).ok_or_else(|| bad("connect_into.target", format!("element {target} is not a duct or pipe")))?;
    let axis = [ts.end[0] - ts.start[0], ts.end[1] - ts.start[1], ts.end[2] - ts.start[2]];
    let l = (axis[0] * axis[0] + axis[1] * axis[1]).sqrt();
    if ts.domain != domain || ts.placeholder || ts.flex.is_some() || ts.corner.is_some() || l < 1e-9 || axis[2].abs() > 1e-6 * l {
        return Err(bad("connect_into.target", format!("connect a {} into a straight level {}", domain.label().to_lowercase(), domain.label().to_lowercase())));
    }
    let u = [axis[0] / l, axis[1] / l];
    let zt = ts.start[2];
    let foot = |q: [f64; 2]| -> Result<V3> {
        let t = (q[0] - ts.start[0]) * u[0] + (q[1] - ts.start[1]) * u[1];
        let margin = mep_runs::leg_length(&ts.size) + port.connector.diameter.max(port.connector.width);
        if t < margin || t > l - margin {
            return Err(bad("connect_into.past_end", "the main does not pass the connector; extend it or choose another main"));
        }
        Ok([ts.start[0] + u[0] * t, ts.start[1] + u[1] * t, zt])
    };
    let (pp, d) = (port.position, port.direction);
    let mut pts: Vec<V3> = vec![pp];
    if d[2].abs() > 0.999 {
        if d[2] * (zt - pp[2]) <= 0.0 {
            return Err(bad("connect_into.away", "the connector faces away from the main"));
        }
        pts.push([pp[0], pp[1], zt]);
        pts.push(foot([pp[0], pp[1]])?);
    } else {
        let size = port.connector.diameter.max(port.connector.width).max(port.connector.height);
        let a = [pp[0] + d[0] * (0.3f64).max(2.0 * size), pp[1] + d[1] * (0.3f64).max(2.0 * size), pp[2]];
        pts.push(a);
        pts.push([a[0], a[1], zt]);
        pts.push(foot([a[0], a[1]])?);
    }
    pts.dedup_by(|a, b| (0..3).all(|k| (a[k] - b[k]).abs() < 1e-9));
    // The level the element stands on, else the main's.
    let level = store
        .get(el)
        .and_then(mesh_objects::mesh_object_recipe)
        .and_then(|r| GlobalId::parse(&r.level))
        .or_else(|| GlobalId::parse(&ts.level))
        .ok_or_else(|| bad("connect_into.level", "no level to draw on"))?;
    let level = ElementRef::Guid(level);
    let (_, z0) = mep_runs::level_of(&store, &level)?;
    let c = &port.connector;
    let params = CreateParams {
        type_id: GlobalId::parse(&ts.segment_type).map(ElementRef::Guid),
        points: pts.iter().map(|q| vec![q[0], q[1], q[2] - z0]).collect(),
        level,
        middle_elevation: None,
        shape: Some(c.shape),
        diameter: (c.shape == Shape::Round).then_some(c.diameter),
        width: (c.shape != Shape::Round).then_some(c.width),
        height: (c.shape != Shape::Round).then_some(c.height),
        system_type_id: None,
        slope: 0.0,
        bend_radius: None,
    };
    let report = if domain == Domain::Duct { mep_runs::create_duct(txn, params)? } else { mep_runs::create_pipe(txn, params)? };
    if !mep::partners(txn.store()).contains_key(&(el, p.connector_index)) {
        mep_runs::warn(txn, "connect_into.not_joined", "the run starts at the connector but could not join it".into(), vec![el]);
    }
    Ok(report)
}
