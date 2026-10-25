// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! System-zones in IFC (MEP-ZONE-001). The system-zone itself is an `IfcSpatialZone` (THERMAL)
//! element with a body, referenced in its storey by `IfcRelReferencedInSpatialStructure`. Derived
//! from it on every save, marked `rxd:system_zone` in their Description so they are rewritten and
//! not read back as elements:
//!
//! - an `IfcZone` (ObjectType "SystemZone") grouping its spaces, and the equipment that serves
//!   it, by one `IfcRelAssignsToGroup`, with `Pset_ZoneCommon` (NetPlannedArea);
//! - the system that serves it by `IfcRelServicesBuildings` (system → spatial zone);
//! - its set points as `Pset_SpaceThermalRequirements` on each of its spaces.
//!
//! On load, an `IfcZone` from another tool keeps its members' GlobalIds as `@GroupMembers`, so
//! it can be converted into a system-zone.

use std::collections::HashMap;

use reviera_core::system_zones as zones;
use reviera_core::{Document, ElementId, GlobalId, Store};
use reviera_ifc::{AttrValue, IfcModel, StepId};

use crate::{Result, geometry};

pub(crate) const TAG: &str = "rxd:system_zone";

fn r(id: StepId) -> AttrValue {
    AttrValue::Ref(id.0)
}

fn s(v: &str) -> AttrValue {
    AttrValue::String(v.into())
}

fn typed(t: &str, v: f64) -> AttrValue {
    AttrValue::Typed { type_name: t.into(), value: Box::new(AttrValue::Real(v)) }
}

fn ours(ifc: &IfcModel, id: StepId) -> bool {
    matches!(ifc.attribute(id, "Description"), Ok(AttrValue::String(d)) if d == TAG)
}

fn refs(v: AttrValue) -> Vec<StepId> {
    match v {
        AttrValue::List(l) => l.into_iter().filter_map(|a| if let AttrValue::Ref(i) = a { Some(StepId(i)) } else { None }).collect(),
        _ => Vec::new(),
    }
}

/// A GlobalId from a zone's and a salt (FNV-1a, 128 bits, a version 8 UUID), so the derived
/// `IfcZone` keeps its identity from save to save.
fn derived_guid(zone: &GlobalId, salt: &str) -> GlobalId {
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013B;
    let mut h: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    for b in zone.as_str().bytes().chain([0]).chain(salt.bytes()) {
        h ^= u128::from(b);
        h = h.wrapping_mul(PRIME);
    }
    let h = (h & !(0xF << 76)) | (0x8 << 76);
    let h = (h & !(0x3 << 62)) | (0x2 << 62);
    GlobalId::from_uuid(uuid::Uuid::from_u128(h))
}

/// Removes what an earlier save derived: tagged relationships, zones and property sets (with
/// their properties).
fn remove_earlier(ifc: &mut IfcModel) -> Result<()> {
    for class in ["IfcRelAssignsToGroup", "IfcRelServicesBuildings", "IfcRelReferencedInSpatialStructure"] {
        for e in ifc.entities_of_type(class)? {
            if ours(ifc, e.id) {
                ifc.remove(e.id)?;
            }
        }
    }
    for rel in ifc.entities_of_type("IfcRelDefinesByProperties")? {
        if !ours(ifc, rel.id) {
            continue;
        }
        if let AttrValue::Ref(set) = ifc.attribute(rel.id, "RelatingPropertyDefinition")? {
            for p in refs(ifc.attribute(StepId(set), "HasProperties")?) {
                let _ = ifc.remove(p);
            }
            let _ = ifc.remove(StepId(set));
        }
        ifc.remove(rel.id)?;
    }
    for e in ifc.entities_of_type("IfcZone")? {
        if ours(ifc, e.id) {
            ifc.remove(e.id)?;
        }
    }
    Ok(())
}

fn create_rooted(ifc: &mut IfcModel, class: &str, guid: &GlobalId, owner_history: Option<StepId>) -> Result<StepId> {
    let id = ifc.create_rooted(class, guid)?;
    if let Some(oh) = owner_history {
        ifc.set_attribute(id, "OwnerHistory", &r(oh))?;
    }
    ifc.set_attribute(id, "Description", &s(TAG))?;
    Ok(id)
}

fn pset(ifc: &mut IfcModel, owner_history: Option<StepId>, targets: Vec<AttrValue>, name: &str, props: &[(&str, AttrValue)]) -> Result<()> {
    let mut items = Vec::new();
    for (n, v) in props {
        let p = ifc.create("IfcPropertySingleValue")?;
        ifc.set_attribute(p, "Name", &s(n))?;
        ifc.set_attribute(p, "NominalValue", v)?;
        items.push(r(p));
    }
    let set = create_rooted(ifc, "IfcPropertySet", &GlobalId::new(), owner_history)?;
    ifc.set_attribute(set, "Name", &s(name))?;
    ifc.set_attribute(set, "HasProperties", &AttrValue::List(items))?;
    let rel = create_rooted(ifc, "IfcRelDefinesByProperties", &GlobalId::new(), owner_history)?;
    ifc.set_attribute(rel, "RelatedObjects", &AttrValue::List(targets))?;
    ifc.set_attribute(rel, "RelatingPropertyDefinition", &r(set))?;
    Ok(())
}

pub(crate) fn write_system_zones(ifc: &mut IfcModel, doc: &Document, steps: &HashMap<ElementId, StepId>, owner_history: Option<StepId>, scale: f64) -> Result<()> {
    remove_earlier(ifc)?;
    let store = doc.store();
    let all = zones::all(store);
    if all.is_empty() {
        return Ok(());
    }
    let contexts = geometry::Contexts::find(ifc)?;
    for id in all {
        let Some(&step) = steps.get(&id) else { continue };
        let e = store.get(id).expect("listed");
        let Some(g) = zones::geometry(store, e) else {
            geometry::remove_slab(ifc, step)?;
            continue;
        };
        let body = geometry::SlabBody { rings: vec![(g.outline.as_slice(), Vec::new())], top: g.base + g.height, depth: g.height };
        geometry::write_slab(ifc, step, &body, scale, contexts)?;
        // A spatial zone is referenced in its storey, not contained.
        if let Some(&storey) = steps.get(&g.level) {
            let rel = create_rooted(ifc, "IfcRelReferencedInSpatialStructure", &GlobalId::new(), owner_history)?;
            ifc.set_attribute(rel, "RelatedElements", &AttrValue::List(vec![r(step)]))?;
            ifc.set_attribute(rel, "RelatingStructure", &r(storey))?;
        }
        write_zone(ifc, store, steps, id, step, &g, owner_history, scale)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_zone(ifc: &mut IfcModel, store: &Store, steps: &HashMap<ElementId, StepId>, id: ElementId, step: StepId, g: &zones::ZoneGeometry, owner_history: Option<StepId>, scale: f64) -> Result<()> {
    let e = store.get(id).expect("a zone");
    let props = zones::props(e);
    let spaces: Vec<StepId> = zones::spaces(store, id).iter().filter_map(|s| steps.get(s).copied()).collect();
    let served = zones::served_by(store, e);
    let group = create_rooted(ifc, "IfcZone", &derived_guid(e.guid(), "zone"), owner_history)?;
    ifc.set_attribute(group, "Name", &s(e.name.as_deref().unwrap_or("System-Zone")))?;
    ifc.set_attribute(group, "ObjectType", &s("SystemZone"))?;
    let mut members: Vec<AttrValue> = spaces.iter().map(|st| r(*st)).collect();
    if let Some((eq, false)) = served
        && let Some(&st) = steps.get(&eq)
    {
        members.push(r(st));
    }
    if !members.is_empty() {
        let rel = create_rooted(ifc, "IfcRelAssignsToGroup", &GlobalId::new(), owner_history)?;
        ifc.set_attribute(rel, "RelatedObjects", &AttrValue::List(members))?;
        ifc.set_attribute(rel, "RelatingGroup", &r(group))?;
    }
    let space_area: f64 = zones::spaces(store, id).iter().filter_map(|s| reviera_core::mep_spaces::info(store, *s)).map(|i| i.area).sum();
    let area = if spaces.is_empty() { g.area() } else { space_area };
    pset(ifc, owner_history, vec![r(group)], "Pset_ZoneCommon", &[("NetPlannedArea", typed("IfcAreaMeasure", area / (scale * scale)))])?;
    if let Some((sys, true)) = served
        && let Some(&st) = steps.get(&sys)
    {
        let rel = create_rooted(ifc, "IfcRelServicesBuildings", &GlobalId::new(), owner_history)?;
        ifc.set_attribute(rel, "RelatingSystem", &r(st))?;
        ifc.set_attribute(rel, "RelatedBuildings", &AttrValue::List(vec![r(step)]))?;
    }
    // Set points on each space (kelvin, ratios), when it has any.
    let mut req: Vec<(&str, AttrValue)> = Vec::new();
    if let Some(t) = props.heating_set_point {
        req.push(("SpaceTemperatureMin", typed("IfcThermodynamicTemperatureMeasure", t + 273.15)));
    }
    if let Some(t) = props.cooling_set_point {
        req.push(("SpaceTemperatureMax", typed("IfcThermodynamicTemperatureMeasure", t + 273.15)));
    }
    if let Some(h) = props.humidity_min {
        req.push(("SpaceHumidityMin", typed("IfcRatioMeasure", h / 100.0)));
    }
    if let Some(h) = props.humidity_max {
        req.push(("SpaceHumidityMax", typed("IfcRatioMeasure", h / 100.0)));
    }
    if !req.is_empty() && !spaces.is_empty() {
        pset(ifc, owner_history, spaces.iter().map(|st| r(*st)).collect(), "Pset_SpaceThermalRequirements", &req)?;
    }
    Ok(())
}

/// The GlobalIds an untagged `IfcZone` groups, for `@GroupMembers`.
pub(crate) fn legacy_members(ifc: &IfcModel, zone: StepId) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for rel in ifc.referenced_by(zone, "IfcRelAssignsToGroup")? {
        if ifc.attribute(rel, "RelatingGroup")? != AttrValue::Ref(zone.0) {
            continue;
        }
        for m in refs(ifc.attribute(rel, "RelatedObjects")?) {
            if let AttrValue::String(g) = ifc.attribute(m, "GlobalId")? {
                out.push(g);
            }
        }
    }
    Ok(out)
}
