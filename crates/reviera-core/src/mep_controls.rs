// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Mechanical control devices (MEP-DUCT-012): thermostats, humidistats, sensors and controllers,
//! components of the Mechanical Control Device, Sensor and Controller categories.
//!
//! - **Placing**: on a wall's face (given, or the nearest within 300 mm of the point), at a
//!   mounting height above the level; the device then follows the face as the wall moves or
//!   changes, its back against it and facing out. Elsewhere it stands on the work plane.
//! - **Controlled elements**: a device controls zero or more elements of any discipline (kept as
//!   GlobalIds, `rxd:controls`); not itself nor another control device.
//! - **Host deleted**: the device stays where it was, unhosted, with a warning.
//!
//! In IFC each device is its category's class (`IfcUnitaryControlElement`, `IfcSensor`,
//! `IfcController`) with its type's predefined type, contained in its storey; hosted to its wall by
//! `IfcRelConnectsElements`, and related to each distribution flow element it controls by
//! `IfcRelFlowControlElements`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::mesh_objects::{self, FaceHost};
use crate::store::Store;
use crate::txn::{Transaction, Warning};

/// On a control device: the elements it controls (GlobalIds).
pub const CONTROLS_KEY: &str = "rxd:controls";

/// How far from a wall face a point may be for the device to go on it (metres).
const SNAP: f64 = 0.3;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// Whether an element is a placed control device.
pub fn is_device(store: &Store, e: &Element) -> bool {
    mesh_objects::mesh_object_recipe(e).is_some() && mesh_objects::type_of(store, e).is_some_and(|(_, t)| t.category.is_control_device())
}

/// The elements a device controls, as they are now.
pub fn controlled(store: &Store, e: &Element) -> Vec<ElementId> {
    e.param(CONTROLS_KEY)
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|g| g.as_str().and_then(GlobalId::parse))
        .filter_map(|g| store.find_by_guid(&g))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Exterior,
    Interior,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceParams {
    /// A control device type (Mechanical Control Device, Sensor or Controller).
    pub type_id: ElementRef,
    /// In plan (metres): on a wall, where along it and which side.
    pub point: [f64; 2],
    pub level: ElementRef,
    /// Mounting height above the level (metres; 1.5 by default).
    #[serde(default = "mounting")]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: f64,
    /// Off a wall: counter-clockwise turn.
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: f64,
    /// The wall whose face it goes on; omitted, the nearest wall face within 300 mm, if any
    /// (`work_plane` to place it free).
    pub host_wall: Option<ElementRef>,
    /// Which face of `host_wall`; the side the point is on when omitted.
    pub side: Option<Side>,
    /// Place it on the work plane, not on a wall.
    #[serde(default)]
    pub work_plane: bool,
    /// Elements it controls.
    #[serde(default)]
    pub controlled_ids: Vec<ElementRef>,
}

fn mounting() -> f64 {
    1.5
}

/// The face of `wall` nearest `p`, with its distance: (side, along, distance from the face).
fn face_of(store: &Store, wall: &Element, p: [f64; 2]) -> Option<(f64, f64, f64)> {
    let g = crate::walls::regenerate(store, wall).ok()?;
    let (d, n) = (g.direction(), g.normal());
    let rel = [p[0] - g.start[0], p[1] - g.start[1]];
    let along = rel[0] * d[0] + rel[1] * d[1];
    if along < -1e-9 || along > g.length() + 1e-9 {
        return None;
    }
    let off = rel[0] * n[0] + rel[1] * n[1];
    let side = if off >= 0.0 { 1.0 } else { -1.0 };
    Some((side, along, (off.abs() - g.width / 2.0).abs()))
}

/// `mech_control_device.place`.
pub(crate) fn place(txn: &mut Transaction<'_>, p: PlaceParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let type_id = store.resolve(&p.type_id).ok_or_else(|| bad("element.not_found", "no such type"))?;
    let ty = store.get(type_id).and_then(mesh_objects::mesh_object_type).ok_or_else(|| bad("mesh_object.not_a_type", format!("element {type_id} is not a component type")))?;
    if !ty.category.is_control_device() {
        return Err(bad("mech_control_device.not_a_device", format!("{} is a {}, not a control device", store.get(type_id).and_then(|e| e.name.clone()).unwrap_or_default(), ty.category.label())));
    }
    // The wall face: given, or the nearest within reach.
    let face = if p.work_plane {
        None
    } else {
        match &p.host_wall {
            Some(w) => {
                let wid = store.resolve(w).ok_or_else(|| bad("element.not_found", "no such wall"))?;
                let wall = store.get(wid).filter(|e| crate::walls::wall_recipe(e).is_some()).ok_or_else(|| bad("mech_control_device.host", format!("element {wid} is not a wall")))?;
                let (side, along, _) = face_of(&store, wall, p.point).ok_or_else(|| bad("mech_control_device.off_wall", "the point is beyond the wall's ends"))?;
                let side = match p.side {
                    Some(Side::Exterior) => 1.0,
                    Some(Side::Interior) => -1.0,
                    None => side,
                };
                Some(FaceHost { wall: wall.guid().to_string(), side, along })
            }
            None => store
                .iter()
                .filter(|e| crate::walls::wall_recipe(e).is_some())
                .filter_map(|w| face_of(&store, w, p.point).map(|(side, along, dist)| (w, side, along, dist)))
                .filter(|x| x.3 <= SNAP)
                .min_by(|a, b| a.3.total_cmp(&b.3))
                .map(|(w, side, along, _)| FaceHost { wall: w.guid().to_string(), side, along }),
        }
    };
    let args = serde_json::json!({ "type_id": type_id.0, "point": p.point, "level": p.level, "offset": p.offset, "rotation": p.rotation });
    let id = mesh_objects::place(txn, serde_json::from_value(args).map_err(|e| bad("mech_control_device.params", e.to_string()))?)?;
    if let Some(f) = face {
        let mut r = mesh_objects::mesh_object_recipe(txn.store().get(id).expect("placed")).expect("a copy");
        r.face = Some(f);
        let v = serde_json::to_value(&r).expect("serialises");
        txn.modify(id, |e| {
            e.params.insert(crate::walls::RECIPE_KEY.into(), v);
        })?;
    }
    if !p.controlled_ids.is_empty() {
        assign(txn, AssignParams { device_id: ElementRef::Id(id), controlled_ids: p.controlled_ids, add: false })?;
    }
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssignParams {
    pub device_id: ElementRef,
    /// The equipment, VAV boxes, dampers, terminals (any discipline) it controls; empty clears.
    pub controlled_ids: Vec<ElementRef>,
    /// Add to those it controls already, rather than replace them.
    #[serde(default)]
    pub add: bool,
}

/// `mech_control_device.assign`.
pub(crate) fn assign(txn: &mut Transaction<'_>, p: AssignParams) -> Result<()> {
    let store = txn.store().clone();
    let id = store.resolve(&p.device_id).ok_or_else(|| bad("element.not_found", "no such device"))?;
    let e = store.get(id).expect("resolved");
    if !is_device(&store, e) {
        return Err(bad("mech_control_device.not_a_device", format!("element {id} is not a control device")));
    }
    let mut guids: Vec<String> = if p.add { controlled(&store, e).iter().filter_map(|c| store.get(*c)).map(|c| c.guid().to_string()).collect() } else { Vec::new() };
    for r in &p.controlled_ids {
        let c = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such element to control"))?;
        if c == id {
            return Err(bad("mech_control_device.self", "a device cannot control itself"));
        }
        let ce = store.get(c).expect("resolved");
        if is_device(&store, ce) {
            return Err(bad("mech_control_device.device", format!("element {c} is a control device; devices control equipment, terminals and system zones")));
        }
        if crate::categories::is_type(ce) {
            return Err(bad("mech_control_device.type", format!("element {c} is a type; choose a placed element")));
        }
        let g = ce.guid().to_string();
        if !guids.contains(&g) {
            guids.push(g);
        }
    }
    txn.set_param(id, CONTROLS_KEY, serde_json::json!(guids))
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DeviceInfo {
    pub device: ElementId,
    pub name: String,
    pub ifc_class: String,
    pub predefined: String,
    /// The wall it is on, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<ElementId>,
    pub controlled: Vec<ElementId>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {}

/// `mech_control_device.list`.
pub(crate) fn list(store: &Store, _: ListParams) -> Result<Vec<DeviceInfo>> {
    Ok(store
        .iter()
        .filter(|e| is_device(store, e))
        .map(|e| {
            let r = mesh_objects::mesh_object_recipe(e).expect("a copy");
            let ty = mesh_objects::type_of(store, e).map(|(t, _)| t);
            DeviceInfo {
                device: e.id(),
                name: ty.and_then(|t| store.get(t)).and_then(|t| t.name.clone()).unwrap_or_default(),
                ifc_class: e.class.clone(),
                predefined: e.param("@PredefinedType").and_then(|v| v.as_str()).unwrap_or_default().to_owned(),
                host: r.face.as_ref().and_then(|f| GlobalId::parse(&f.wall)).and_then(|g| store.find_by_guid(&g)),
                controlled: controlled(store, e),
            }
        })
        .collect())
}

/// Before elements are deleted: devices on a deleted wall stay where they are, unhosted, with a
/// warning.
pub(crate) fn release(txn: &mut Transaction<'_>, deleted: &std::collections::BTreeSet<ElementId>) -> Result<()> {
    let store = txn.store().clone();
    let walls: Vec<String> = deleted.iter().filter_map(|d| store.get(*d)).filter(|e| crate::walls::wall_recipe(e).is_some()).map(|e| e.guid().to_string()).collect();
    if walls.is_empty() {
        return Ok(());
    }
    for e in store.iter() {
        if deleted.contains(&e.id()) {
            continue;
        }
        let Some(mut r) = mesh_objects::mesh_object_recipe(e) else { continue };
        if !r.face.as_ref().is_some_and(|f| walls.contains(&f.wall)) {
            continue;
        }
        let g = mesh_objects::regenerate(&store, e)?;
        r.point = [g.placement.origin[0], g.placement.origin[1]];
        r.rotation = g.placement.rotation;
        r.face = None;
        let v = serde_json::to_value(&r).expect("serialises");
        txn.modify(e.id(), |x| {
            x.params.insert(crate::walls::RECIPE_KEY.into(), v);
        })?;
        txn.warn(Warning {
            id: "mech_control_device.unhosted".into(),
            message: format!("{} lost its host wall; it stays where it was, unhosted", g.ty.category.label()),
            element_ids: vec![e.id()],
        });
    }
    Ok(())
}
