// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Path arrays in IFC (ARC-MOD-033): the array is an `IfcElementAssembly` contained in its level,
//! aggregating its members. Each member is an element of its component's class (an `IfcColumn`
//! for a column component, …) typed by the component type, whose body is an `IfcMappedItem` of the
//! type's representation map.
//!
//! The members are made from the array: they are not elements of their own. Their
//! `IfcRelAggregates` is marked `rxd:path_array:<signature>` in its Description and each member
//! (and its `IfcRelDefinesByType`) `rxd:path_array`, so an unchanged array is left as it is, a
//! changed one gets new members, and opening the file does not load them as elements.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

use reviera_core::mesh_objects::MeshObjectGeometry;
use reviera_core::path_arrays::{self, Member};
use reviera_core::{Document, ElementId, GlobalId};
use reviera_ifc::{AttrValue, IfcModel, StepId};

use crate::{Result, geometry, mesh_objects};

pub(crate) const PART_TAG: &str = "rxd:path_array";

fn signature(members: &[(Member, MeshObjectGeometry)], maps: &[u32], scale: f64) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for (m, g) in members {
        m.index.hash(&mut h);
        for v in [
            m.point[0],
            m.point[1],
            m.z,
            m.rotation,
            m.scale,
            g.placement.scale,
        ] {
            v.to_bits().hash(&mut h);
        }
        m.r#type.hash(&mut h);
    }
    maps.hash(&mut h);
    scale.to_bits().hash(&mut h);
    format!("{:016x}", h.finish())
}

/// The aggregation of `whole`'s members made by Reviera, with its Description.
fn our_aggregate(ifc: &IfcModel, whole: StepId) -> Result<Option<(StepId, String)>> {
    for rel in ifc.referenced_by(whole, "IfcRelAggregates")? {
        if ifc.attribute(rel, "RelatingObject")? != AttrValue::Ref(whole.0) {
            continue;
        }
        if let AttrValue::String(d) = ifc.attribute(rel, "Description")?
            && d.starts_with(PART_TAG)
        {
            return Ok(Some((rel, d)));
        }
    }
    Ok(None)
}

/// Removes an array's members, their typing and the aggregation.
pub(crate) fn remove_members(ifc: &mut IfcModel, whole: StepId) -> Result<()> {
    let Some((rel, _)) = our_aggregate(ifc, whole)? else {
        return Ok(());
    };
    if let AttrValue::List(parts) = ifc.attribute(rel, "RelatedObjects")? {
        for p in parts {
            let AttrValue::Ref(p) = p else { continue };
            let p = StepId(p);
            for typing in ifc.referenced_by(p, "IfcRelDefinesByType")? {
                if matches!(ifc.attribute(typing, "Description")?, AttrValue::String(d) if d == PART_TAG)
                {
                    ifc.remove(typing)?;
                }
            }
            mesh_objects::remove_copy_geometry(ifc, p)?;
            ifc.remove(p)?;
        }
    }
    Ok(ifc.remove(rel)?)
}

fn r(id: StepId) -> AttrValue {
    AttrValue::Ref(id.0)
}

/// Writes every path array's members, leaving unchanged arrays as they are, and contains each
/// array in its level. Runs after the custom object types' maps are written.
pub(crate) fn write_path_arrays(
    ifc: &mut IfcModel,
    doc: &Document,
    steps: &HashMap<ElementId, StepId>,
    owner_history: Option<StepId>,
    scale: f64,
) -> Result<()> {
    let store = doc.store();
    let all = path_arrays::all(store);
    if all.is_empty() {
        return Ok(());
    }
    let contexts = geometry::Contexts::find(ifc)?;
    for (id, members) in all {
        let Some(&step) = steps.get(&id) else {
            continue;
        };
        if let Some(storey) = path_arrays::path_array_recipe(store.get(id).expect("listed"))
            .and_then(|rec| GlobalId::parse(&rec.level))
            .and_then(|l| store.find_by_guid(&l))
            .and_then(|l| steps.get(&l).copied())
        {
            geometry::contain(ifc, step, storey, owner_history)?;
        }
        // Each member's type and its map.
        let mut maps: BTreeMap<ElementId, (StepId, StepId)> = BTreeMap::new();
        for (_, g) in &members {
            if let Some(&ty) = steps.get(&g.type_id)
                && let Some(&map) = mesh_objects::maps_of(ifc, ty)?.first()
            {
                maps.insert(g.type_id, (ty, map));
            }
        }
        let map_ids: Vec<u32> = maps.values().map(|(_, m)| m.0).collect();
        let tag = format!("{PART_TAG}:{}", signature(&members, &map_ids, scale));
        if our_aggregate(ifc, step)?.is_some_and(|(_, d)| d == tag) {
            continue;
        }
        remove_members(ifc, step)?;
        // The assembly sits at the origin; its members are placed absolutely.
        if matches!(ifc.attribute(step, "ObjectPlacement")?, AttrValue::Null) {
            let origin = ifc.create("IfcCartesianPoint")?;
            ifc.set_attribute(
                origin,
                "Coordinates",
                &AttrValue::List(vec![AttrValue::Real(0.0); 3]),
            )?;
            let axes = ifc.create("IfcAxis2Placement3D")?;
            ifc.set_attribute(axes, "Location", &r(origin))?;
            let placement = ifc.create("IfcLocalPlacement")?;
            ifc.set_attribute(placement, "RelativePlacement", &r(axes))?;
            ifc.set_attribute(step, "ObjectPlacement", &r(placement))?;
        }
        let mut parts = Vec::new();
        let mut by_type: BTreeMap<ElementId, Vec<AttrValue>> = BTreeMap::new();
        for (m, g) in &members {
            let Some(&(_, map)) = maps.get(&g.type_id) else {
                continue;
            };
            let class = g.ty.category.class();
            let e = ifc.create_rooted(class, &GlobalId::new())?;
            let name = store
                .get(g.type_id)
                .and_then(|t| t.name.clone())
                .unwrap_or_default();
            ifc.set_attribute(
                e,
                "Name",
                &AttrValue::String(format!("{name} {}", m.index + 1)),
            )?;
            ifc.set_attribute(e, "Description", &AttrValue::String(PART_TAG.into()))?;
            if let Some(oh) = owner_history {
                ifc.set_attribute(e, "OwnerHistory", &r(oh))?;
            }
            mesh_objects::write_mapped(ifc, e, g.placement, map, PART_TAG, scale, contexts)?;
            parts.push(r(e));
            by_type.entry(g.type_id).or_default().push(r(e));
        }
        for (ty, objects) in by_type {
            let typing = ifc.create_rooted("IfcRelDefinesByType", &GlobalId::new())?;
            ifc.set_attribute(typing, "Description", &AttrValue::String(PART_TAG.into()))?;
            if let Some(oh) = owner_history {
                ifc.set_attribute(typing, "OwnerHistory", &r(oh))?;
            }
            ifc.set_attribute(typing, "RelatedObjects", &AttrValue::List(objects))?;
            ifc.set_attribute(typing, "RelatingType", &r(maps[&ty].0))?;
        }
        if parts.is_empty() {
            continue;
        }
        let rel = ifc.create_rooted("IfcRelAggregates", &GlobalId::new())?;
        ifc.set_attribute(rel, "Description", &AttrValue::String(tag))?;
        if let Some(oh) = owner_history {
            ifc.set_attribute(rel, "OwnerHistory", &r(oh))?;
        }
        ifc.set_attribute(rel, "RelatingObject", &r(step))?;
        ifc.set_attribute(rel, "RelatedObjects", &AttrValue::List(parts))?;
    }
    Ok(())
}
