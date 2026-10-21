// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! MEP ports, connections and systems in IFC (MEP-CONN-001, MEP-CONN-002, MEP-SYS-001).
//!
//! - Each port is an `IfcDistributionPort` nested in its element by one `IfcRelNests` per
//!   element (in connector order), placed at the port with its Z axis the outward direction, with
//!   its `FlowDirection`, `PredefinedType` (DUCT, PIPE, CABLE, CABLECARRIER) and `SystemType`
//!   (from its system's type or its classification), and `Pset_DistributionPortCommon`
//!   (`PortNumber`) with its size in `Pset_DistributionPortTypeDuct` or `…Pipe`.
//! - Each connection is an `IfcRelConnectsPorts`; an analytical pipe connection (MEP-PIPE-009)
//!   connects the equipment's port to a virtual port nested in the pipe, with no realizing element.
//! - Each system gets its members by `IfcRelAssignsToGroup` and the building it serves by
//!   `IfcRelServicesBuildings`.
//!
//! Everything is rewritten on each save and marked with Reviera's tag in its Description, so it
//! is not read back as elements (the definitions live in the RXD recipes). Ports keep GlobalIds
//! made from their element's and their index, so unchanged ports keep their identity. IFC2X3
//! files (read-only for new content) get none of it.

use std::collections::HashMap;

use reviera_core::mep::{self, Classification, Domain, FlowDirection};
use reviera_core::mep_systems;
use reviera_core::{Document, ElementId, GlobalId};
use reviera_ifc::{AttrValue, IfcModel, StepId};

use crate::Result;

pub(crate) const TAG: &str = "rxd:mep";

fn r(id: StepId) -> AttrValue {
    AttrValue::Ref(id.0)
}

fn s(v: &str) -> AttrValue {
    AttrValue::String(v.into())
}

fn e(v: &str) -> AttrValue {
    AttrValue::Enum(v.into())
}

fn create(ifc: &mut IfcModel, class: &str, attrs: &[(&str, AttrValue)]) -> Result<StepId> {
    let id = ifc.create(class)?;
    for (name, value) in attrs {
        ifc.set_attribute(id, name, value)?;
    }
    Ok(id)
}

/// A GlobalId from an element's and a port's number (FNV-1a, 128 bits, a version 8 UUID).
fn port_guid(element: &GlobalId, index: usize) -> GlobalId {
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013B;
    let mut h: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    for b in element.as_str().bytes().chain([0]).chain(index.to_string().bytes()).chain(*b"port") {
        h ^= u128::from(b);
        h = h.wrapping_mul(PRIME);
    }
    let h = (h & !(0xF << 76)) | (0x8 << 76);
    let h = (h & !(0x3 << 62)) | (0x2 << 62);
    GlobalId::from_uuid(uuid::Uuid::from_u128(h))
}

/// `IfcDistributionSystemEnum` of a classification (no system to say otherwise).
fn system_enum(c: Option<Classification>, domain: Domain) -> &'static str {
    match (c, domain) {
        (Some(c), _) => {
            let t = mep_systems::builtin(c);
            mep_systems::ifc_predefined(&t)
        }
        (None, Domain::Electrical) => "ELECTRICAL",
        (None, _) => "NOTDEFINED",
    }
}

fn is_ours(ifc: &IfcModel, id: StepId) -> bool {
    matches!(ifc.attribute(id, "Description"), Ok(AttrValue::String(d)) if d == TAG)
}

/// Removes what an earlier save wrote: the tagged relationships and the nested ports with their
/// placements and property sets.
fn remove_earlier(ifc: &mut IfcModel) -> Result<()> {
    for class in ["IfcRelConnectsPorts", "IfcRelServicesBuildings", "IfcRelFlowControlElements", "IfcRelConnectsElements"] {
        for rel in ifc.entities_of_type(class)? {
            if is_ours(ifc, rel.id) {
                ifc.remove(rel.id)?;
            }
        }
    }
    for rel in ifc.entities_of_type("IfcRelAssignsToGroup")? {
        if is_ours(ifc, rel.id) {
            ifc.remove(rel.id)?;
        }
    }
    for rel in ifc.entities_of_type("IfcRelNests")? {
        if !is_ours(ifc, rel.id) {
            continue;
        }
        let ports = match ifc.attribute(rel.id, "RelatedObjects")? {
            AttrValue::List(v) => v
                .into_iter()
                .filter_map(|a| match a {
                    AttrValue::Ref(id) => Some(StepId(id)),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        ifc.remove(rel.id)?;
        for port in ports {
            for pset_rel in ifc.referenced_by(port, "IfcRelDefinesByProperties")? {
                if let AttrValue::Ref(pset) = ifc.attribute(pset_rel, "RelatingPropertyDefinition")? {
                    if let AttrValue::List(props) = ifc.attribute(StepId(pset), "HasProperties")? {
                        for p in props {
                            if let AttrValue::Ref(p) = p {
                                let _ = ifc.remove(StepId(p));
                            }
                        }
                    }
                    let _ = ifc.remove(StepId(pset));
                }
                let _ = ifc.remove(pset_rel);
            }
            if let AttrValue::Ref(pl) = ifc.attribute(port, "ObjectPlacement")? {
                if let AttrValue::Ref(ax) = ifc.attribute(StepId(pl), "RelativePlacement")? {
                    for a in ["Location", "Axis", "RefDirection"] {
                        if let AttrValue::Ref(x) = ifc.attribute(StepId(ax), a)? {
                            let _ = ifc.remove(StepId(x));
                        }
                    }
                    let _ = ifc.remove(StepId(ax));
                }
                let _ = ifc.remove(StepId(pl));
            }
            ifc.remove(port)?;
        }
    }
    Ok(())
}

fn placement(ifc: &mut IfcModel, at: [f64; 3], z: [f64; 3], scale: f64) -> Result<StepId> {
    let reals = |v: &[f64]| AttrValue::List(v.iter().copied().map(AttrValue::Real).collect());
    let location = create(ifc, "IfcCartesianPoint", &[("Coordinates", reals(&at.map(|v| v / scale)))])?;
    // A reference direction square to the axis.
    let x = if z[2].abs() < 0.9 { [-z[1], z[0], 0.0] } else { [1.0, 0.0, 0.0] };
    let l = (x[0] * x[0] + x[1] * x[1] + x[2] * x[2]).sqrt();
    let x = x.map(|v| v / l);
    let axis = create(ifc, "IfcDirection", &[("DirectionRatios", reals(&z))])?;
    let refd = create(ifc, "IfcDirection", &[("DirectionRatios", reals(&x))])?;
    let a2 = create(
        ifc,
        "IfcAxis2Placement3D",
        &[("Location", r(location)), ("Axis", r(axis)), ("RefDirection", r(refd))],
    )?;
    create(ifc, "IfcLocalPlacement", &[("RelativePlacement", r(a2))])
}

fn pset(
    ifc: &mut IfcModel,
    owner_history: Option<StepId>,
    target: StepId,
    name: &str,
    props: &[(&str, AttrValue)],
) -> Result<()> {
    let mut items = Vec::new();
    for (n, v) in props {
        let p = create(ifc, "IfcPropertySingleValue", &[("Name", s(n)), ("NominalValue", v.clone())])?;
        items.push(r(p));
    }
    let set = ifc.create_rooted("IfcPropertySet", &GlobalId::new())?;
    ifc.set_attribute(set, "Name", &s(name))?;
    ifc.set_attribute(set, "HasProperties", &AttrValue::List(items))?;
    let rel = ifc.create_rooted("IfcRelDefinesByProperties", &GlobalId::new())?;
    if let Some(oh) = owner_history {
        ifc.set_attribute(set, "OwnerHistory", &r(oh))?;
        ifc.set_attribute(rel, "OwnerHistory", &r(oh))?;
    }
    ifc.set_attribute(rel, "RelatedObjects", &AttrValue::List(vec![r(target)]))?;
    ifc.set_attribute(rel, "RelatingPropertyDefinition", &r(set))?;
    Ok(())
}

fn typed(class: &str, v: AttrValue) -> AttrValue {
    AttrValue::Typed {
        type_name: class.into(),
        value: Box::new(v),
    }
}

/// Writes the ports, connections and systems.
pub(crate) fn write_mep(
    ifc: &mut IfcModel,
    doc: &Document,
    steps: &HashMap<ElementId, StepId>,
    owner_history: Option<StepId>,
    scale: f64,
) -> Result<()> {
    if ifc.schema() == "IFC2X3" {
        return Ok(());
    }
    remove_earlier(ifc)?;
    let store = doc.store();
    let ports = mep::all_ports(store);
    let in_systems = mep_systems::port_systems(store);
    let systems = mep_systems::systems(store);
    let types: HashMap<String, mep_systems::SystemType> =
        mep_systems::system_types(store).into_iter().map(|t| (t.id.clone(), t)).collect();
    let system_of: HashMap<ElementId, &mep_systems::SystemInfo> = systems.iter().map(|s| (s.id, s)).collect();
    let mut port_steps: HashMap<(ElementId, usize), StepId> = HashMap::new();
    let mut by_element: Vec<(ElementId, Vec<StepId>)> = Vec::new();
    for p in &ports {
        if !steps.contains_key(&p.element) {
            continue;
        }
        let guid = store.get(p.element).expect("exists").guid().clone();
        let port = ifc.create_rooted("IfcDistributionPort", &port_guid(&guid, p.index))?;
        if let Some(oh) = owner_history {
            ifc.set_attribute(port, "OwnerHistory", &r(oh))?;
        }
        let name = if p.connector.name.is_empty() { format!("Port {}", p.index + 1) } else { p.connector.name.clone() };
        ifc.set_attribute(port, "Name", &s(&name))?;
        ifc.set_attribute(port, "Description", &s(TAG))?;
        let pl = placement(ifc, p.position, p.direction, scale)?;
        ifc.set_attribute(port, "ObjectPlacement", &r(pl))?;
        let dir = match p.connector.flow_direction {
            FlowDirection::In => "SINK",
            FlowDirection::Out => "SOURCE",
            FlowDirection::Bidirectional => "SOURCEANDSINK",
        };
        ifc.set_attribute(port, "FlowDirection", &e(dir))?;
        ifc.set_attribute(port, "PredefinedType", &e(p.connector.domain.ifc_port_type()))?;
        let system = in_systems
            .get(&(p.element, p.index))
            .and_then(|ss| ss.first())
            .and_then(|s| system_of.get(s))
            .and_then(|s| types.get(&s.system_type))
            .map(mep_systems::ifc_predefined)
            .unwrap_or_else(|| system_enum(p.connector.classification, p.connector.domain));
        ifc.set_attribute(port, "SystemType", &e(system))?;
        let mut common = vec![("PortNumber", typed("IfcInteger", AttrValue::Int(p.index as i64 + 1)))];
        if !p.connector.connection_type.is_empty() {
            common.push(("ConnectionType", typed("IfcLabel", s(&p.connector.connection_type))));
        }
        pset(ifc, owner_history, port, "Pset_DistributionPortCommon", &common)?;
        let len = |v: f64| typed("IfcPositiveLengthMeasure", AttrValue::Real(v / scale));
        // The kept network results of segments and fittings (MEP-CALC-002), in SI units.
        let kept = store.get(p.element).map(|el| {
            let v = |f: &str| reviera_core::mep_network::kept_value(el, f).and_then(serde_json::Value::as_f64);
            (v("flow"), v("velocity"))
        });
        let mut results = Vec::new();
        if let Some((flow, velocity)) = kept {
            if let Some(q) = flow {
                results.push(("VolumetricFlowRate", typed("IfcVolumetricFlowRateMeasure", AttrValue::Real(q))));
            }
            if let Some(v) = velocity {
                results.push(("Velocity", typed("IfcLinearVelocityMeasure", AttrValue::Real(v))));
            }
        }
        match (p.connector.domain, p.connector.shape) {
            (Domain::Duct, mep::Shape::Round) => {
                let mut props = vec![("NominalDiameterOrWidth", len(p.connector.diameter))];
                props.extend(results);
                pset(ifc, owner_history, port, "Pset_DistributionPortTypeDuct", &props)?;
            }
            (Domain::Duct, _) => {
                let mut props = vec![("NominalDiameterOrWidth", len(p.connector.width)), ("NominalHeight", len(p.connector.height))];
                props.extend(results);
                pset(ifc, owner_history, port, "Pset_DistributionPortTypeDuct", &props)?;
            }
            (Domain::Pipe, _) => {
                let mut props = vec![("NominalDiameter", len(p.connector.diameter))];
                props.extend(results);
                pset(ifc, owner_history, port, "Pset_DistributionPortTypePipe", &props)?;
            }
            _ => {}
        }
        port_steps.insert((p.element, p.index), port);
        match by_element.last_mut() {
            Some((el, v)) if *el == p.element => v.push(port),
            _ => by_element.push((p.element, vec![port])),
        }
    }
    // Analytical pipe connections (MEP-PIPE-009): a virtual port nested in the pipe where the
    // connection attaches, no realizing element, connected to the equipment's port.
    let mut analytical: Vec<(StepId, StepId)> = Vec::new();
    for ac in reviera_core::mep_analytical::all(store) {
        let (Some(_), Some(&eq_port), Some(conn)) = (steps.get(&ac.pipe), port_steps.get(&(ac.equipment, ac.connector)), store.get(ac.id)) else { continue };
        let Some(p) = ports.iter().find(|p| p.element == ac.equipment && p.index == ac.connector) else { continue };
        let port = ifc.create_rooted("IfcDistributionPort", &port_guid(conn.guid(), 0))?;
        if let Some(oh) = owner_history {
            ifc.set_attribute(port, "OwnerHistory", &r(oh))?;
        }
        ifc.set_attribute(port, "Name", &s("Analytical connection"))?;
        ifc.set_attribute(port, "Description", &s(TAG))?;
        let toward = [ac.from[0] - ac.point[0], ac.from[1] - ac.point[1], ac.from[2] - ac.point[2]];
        let n = (toward[0].powi(2) + toward[1].powi(2) + toward[2].powi(2)).sqrt();
        let z = if n > 1e-9 { [toward[0] / n, toward[1] / n, toward[2] / n] } else { [-p.direction[0], -p.direction[1], -p.direction[2]] };
        let pl = placement(ifc, ac.point, z, scale)?;
        ifc.set_attribute(port, "ObjectPlacement", &r(pl))?;
        let dir = match p.connector.flow_direction {
            FlowDirection::In => "SOURCE",
            FlowDirection::Out => "SINK",
            FlowDirection::Bidirectional => "SOURCEANDSINK",
        };
        ifc.set_attribute(port, "FlowDirection", &e(dir))?;
        ifc.set_attribute(port, "PredefinedType", &e("PIPE"))?;
        ifc.set_attribute(port, "SystemType", &e(system_enum(p.connector.classification, Domain::Pipe)))?;
        match by_element.iter_mut().find(|(el, _)| *el == ac.pipe) {
            Some((_, v)) => v.push(port),
            None => by_element.push((ac.pipe, vec![port])),
        }
        analytical.push((eq_port, port));
    }
    for (el, nested) in by_element {
        let Some(&host) = steps.get(&el) else { continue };
        let rel = ifc.create_rooted("IfcRelNests", &GlobalId::new())?;
        if let Some(oh) = owner_history {
            ifc.set_attribute(rel, "OwnerHistory", &r(oh))?;
        }
        ifc.set_attribute(rel, "Description", &s(TAG))?;
        ifc.set_attribute(rel, "RelatingObject", &r(host))?;
        ifc.set_attribute(rel, "RelatedObjects", &AttrValue::List(nested.into_iter().map(r).collect()))?;
    }
    for c in mep::connections(store) {
        let (Some(&a), Some(&b)) = (port_steps.get(&c.a), port_steps.get(&c.b)) else { continue };
        let rel = ifc.create_rooted("IfcRelConnectsPorts", &GlobalId::new())?;
        if let Some(oh) = owner_history {
            ifc.set_attribute(rel, "OwnerHistory", &r(oh))?;
        }
        ifc.set_attribute(rel, "Description", &s(TAG))?;
        ifc.set_attribute(rel, "RelatingPort", &r(a))?;
        ifc.set_attribute(rel, "RelatedPort", &r(b))?;
    }
    for (a, b) in analytical {
        let rel = ifc.create_rooted("IfcRelConnectsPorts", &GlobalId::new())?;
        if let Some(oh) = owner_history {
            ifc.set_attribute(rel, "OwnerHistory", &r(oh))?;
        }
        ifc.set_attribute(rel, "Description", &s(TAG))?;
        ifc.set_attribute(rel, "RelatingPort", &r(a))?;
        ifc.set_attribute(rel, "RelatedPort", &r(b))?;
    }
    // Control devices (MEP-DUCT-012): hosted to their wall, and related to what they control.
    let mut controlling: Vec<(ElementId, Vec<StepId>)> = Vec::new();
    for e in store.iter().filter(|e| reviera_core::mep_controls::is_device(store, e)) {
        let Some(&dev) = steps.get(&e.id()) else { continue };
        let wall = reviera_core::mesh_objects::mesh_object_recipe(e)
            .and_then(|r| r.face)
            .and_then(|f| GlobalId::parse(&f.wall))
            .and_then(|g| store.find_by_guid(&g))
            .and_then(|w| steps.get(&w).copied());
        if let Some(wall) = wall {
            let rel = ifc.create_rooted("IfcRelConnectsElements", &GlobalId::new())?;
            if let Some(oh) = owner_history {
                ifc.set_attribute(rel, "OwnerHistory", &r(oh))?;
            }
            ifc.set_attribute(rel, "Description", &s(TAG))?;
            ifc.set_attribute(rel, "RelatingElement", &r(wall))?;
            ifc.set_attribute(rel, "RelatedElement", &r(dev))?;
        }
        for c in reviera_core::mep_controls::controlled(store, e) {
            // Only distribution flow elements take the relationship; other controlled elements
            // stay in the model's data.
            if !store.get(c).is_some_and(|x| is_flow_element(&x.class)) {
                continue;
            }
            match controlling.iter_mut().find(|(x, _)| *x == c) {
                Some((_, v)) => v.push(dev),
                None => controlling.push((c, vec![dev])),
            }
        }
    }
    for (c, devices) in controlling {
        let Some(&flow) = steps.get(&c) else { continue };
        let rel = ifc.create_rooted("IfcRelFlowControlElements", &GlobalId::new())?;
        if let Some(oh) = owner_history {
            ifc.set_attribute(rel, "OwnerHistory", &r(oh))?;
        }
        ifc.set_attribute(rel, "Description", &s(TAG))?;
        ifc.set_attribute(rel, "RelatedControlElements", &AttrValue::List(devices.into_iter().map(r).collect()))?;
        ifc.set_attribute(rel, "RelatingFlowElement", &r(flow))?;
    }
    // Circuits: their members by IfcRelAssignsToGroup (MEP-ELEC-002).
    for c in reviera_core::mep_electrical::circuits(store) {
        let Some(&step) = steps.get(&c.id) else { continue };
        let members: Vec<AttrValue> = c.elements.iter().filter_map(|e| steps.get(e)).map(|s| r(*s)).collect();
        if members.is_empty() {
            continue;
        }
        let rel = ifc.create_rooted("IfcRelAssignsToGroup", &GlobalId::new())?;
        if let Some(oh) = owner_history {
            ifc.set_attribute(rel, "OwnerHistory", &r(oh))?;
        }
        ifc.set_attribute(rel, "Description", &s(TAG))?;
        ifc.set_attribute(rel, "RelatedObjects", &AttrValue::List(members))?;
        ifc.set_attribute(rel, "RelatingGroup", &r(step))?;
    }
    let building = store
        .iter()
        .find(|e| e.class == "IfcBuilding")
        .and_then(|b| steps.get(&b.id()).copied());
    for sys in &systems {
        let Some(&step) = steps.get(&sys.id) else { continue };
        let members: Vec<AttrValue> = sys.elements.iter().filter_map(|e| steps.get(e)).map(|s| r(*s)).collect();
        if !members.is_empty() {
            let rel = ifc.create_rooted("IfcRelAssignsToGroup", &GlobalId::new())?;
            if let Some(oh) = owner_history {
                ifc.set_attribute(rel, "OwnerHistory", &r(oh))?;
            }
            ifc.set_attribute(rel, "Description", &s(TAG))?;
            ifc.set_attribute(rel, "RelatedObjects", &AttrValue::List(members))?;
            ifc.set_attribute(rel, "RelatingGroup", &r(step))?;
        }
        if let Some(b) = building {
            let rel = ifc.create_rooted("IfcRelServicesBuildings", &GlobalId::new())?;
            if let Some(oh) = owner_history {
                ifc.set_attribute(rel, "OwnerHistory", &r(oh))?;
            }
            ifc.set_attribute(rel, "Description", &s(TAG))?;
            ifc.set_attribute(rel, "RelatingSystem", &r(step))?;
            ifc.set_attribute(rel, "RelatedBuildings", &AttrValue::List(vec![r(b)]))?;
        }
    }
    Ok(())
}

/// Whether an IFC class is an `IfcDistributionFlowElement` Reviera writes.
fn is_flow_element(class: &str) -> bool {
    matches!(
        class,
        "IfcUnitaryEquipment"
            | "IfcAirTerminal"
            | "IfcAirTerminalBox"
            | "IfcDamper"
            | "IfcFan"
            | "IfcPump"
            | "IfcValve"
            | "IfcDuctSegment"
            | "IfcDuctFitting"
            | "IfcPipeSegment"
            | "IfcPipeFitting"
            | "IfcSanitaryTerminal"
            | "IfcFireSuppressionTerminal"
            | "IfcLightFixture"
            | "IfcOutlet"
            | "IfcElectricDistributionBoard"
            | "IfcTransformer"
            | "IfcCableCarrierSegment"
            | "IfcCableCarrierFitting"
            | "IfcJunctionBox"
    )
}
