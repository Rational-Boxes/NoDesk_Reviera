// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Stacked walls in IFC (ARC-STKW-001): the stacked wall is an `IfcWall` (`ELEMENTEDWALL`) with
//! no body of its own, aggregating one `IfcWall` per sub-wall, typed by the row's wall type and
//! with a faceted B-rep body (cut by the stacked wall's doors and windows).
//!
//! The sub-walls are made from the stacked wall, with GlobalIds derived from its own. Their
//! `IfcRelAggregates` is marked `rxd:stacked_wall:<signature>` in its Description and each
//! sub-wall (and its `IfcRelDefinesByType`) `rxd:stacked_wall`, so an unchanged stacked wall is
//! left as it is and opening the file does not load the sub-walls as elements.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

use reviera_core::bodies::Mesh;
use reviera_core::walls::stacked::SubWall;
use reviera_core::{Document, ElementId, GlobalId};
use reviera_ifc::{AttrValue, IfcModel, StepId};

use crate::{Result, geometry};

pub(crate) const PART_TAG: &str = "rxd:stacked_wall";

fn r(id: StepId) -> AttrValue {
    AttrValue::Ref(id.0)
}

fn signature(subs: &[(SubWall, Mesh)], types: &[u32], scale: f64) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for (s, m) in subs {
        s.guid.as_str().hash(&mut h);
        for p in &m.0 {
            for v in p {
                v.to_bits().hash(&mut h);
            }
        }
        m.1.hash(&mut h);
    }
    types.hash(&mut h);
    scale.to_bits().hash(&mut h);
    format!("{:016x}", h.finish())
}

/// The aggregation of `whole`'s sub-walls made by Reviera, with its Description.
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

/// Removes a stacked wall's sub-walls, their typing and the aggregation.
pub(crate) fn remove_subs(ifc: &mut IfcModel, whole: StepId) -> Result<()> {
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
            geometry::remove_breps(ifc, p)?;
            ifc.remove(p)?;
        }
    }
    Ok(ifc.remove(rel)?)
}

/// Writes a stacked wall's sub-walls (its own body already removed), unless unchanged.
pub(crate) fn write_subs(
    ifc: &mut IfcModel,
    doc: &Document,
    steps: &HashMap<ElementId, StepId>,
    owner_history: Option<StepId>,
    scale: f64,
    wall: ElementId,
    subs: &[(SubWall, Mesh)],
) -> Result<()> {
    let store = doc.store();
    let Some(&step) = steps.get(&wall) else {
        return Ok(());
    };
    if ifc.schema() != "IFC2X3" {
        ifc.set_attribute(
            step,
            "PredefinedType",
            &AttrValue::Enum("ELEMENTEDWALL".into()),
        )?;
    }
    let type_steps: Vec<u32> = subs
        .iter()
        .map(|(s, _)| steps.get(&s.wall_type).map_or(0, |t| t.0))
        .collect();
    let tag = format!("{PART_TAG}:{}", signature(subs, &type_steps, scale));
    if our_aggregate(ifc, step)?.is_some_and(|(_, d)| d == tag) {
        return Ok(());
    }
    remove_subs(ifc, step)?;
    let contexts = geometry::Contexts::find(ifc)?;
    let ifc4 = ifc.schema() != "IFC2X3";
    let mut parts = Vec::new();
    let mut by_type: BTreeMap<ElementId, Vec<AttrValue>> = BTreeMap::new();
    for (s, mesh) in subs {
        let e = ifc.create_rooted("IfcWall", &s.guid)?;
        let name = store
            .get(s.wall_type)
            .and_then(|t| t.name.clone())
            .unwrap_or_else(|| "Wall".into());
        ifc.set_attribute(e, "Name", &AttrValue::String(name))?;
        ifc.set_attribute(e, "Description", &AttrValue::String(PART_TAG.into()))?;
        if let Some(oh) = owner_history {
            ifc.set_attribute(e, "OwnerHistory", &r(oh))?;
        }
        if ifc4 {
            ifc.set_attribute(e, "PredefinedType", &AttrValue::Enum("STANDARD".into()))?;
        }
        let solid = geometry::Polyhedron {
            points: mesh.0.clone(),
            faces: mesh
                .1
                .chunks(3)
                .map(|t| t.iter().map(|i| *i as usize).collect())
                .collect(),
        };
        geometry::write_breps(
            ifc,
            e,
            geometry::BrepFrame::WORLD,
            &[solid],
            scale,
            contexts,
        )?;
        parts.push(r(e));
        by_type.entry(s.wall_type).or_default().push(r(e));
    }
    for (ty, objects) in by_type {
        let Some(&ty) = steps.get(&ty) else { continue };
        let typing = ifc.create_rooted("IfcRelDefinesByType", &GlobalId::new())?;
        ifc.set_attribute(typing, "Description", &AttrValue::String(PART_TAG.into()))?;
        if let Some(oh) = owner_history {
            ifc.set_attribute(typing, "OwnerHistory", &r(oh))?;
        }
        ifc.set_attribute(typing, "RelatedObjects", &AttrValue::List(objects))?;
        ifc.set_attribute(typing, "RelatingType", &r(ty))?;
    }
    if parts.is_empty() {
        return Ok(());
    }
    let rel = ifc.create_rooted("IfcRelAggregates", &GlobalId::new())?;
    ifc.set_attribute(rel, "Description", &AttrValue::String(tag))?;
    if let Some(oh) = owner_history {
        ifc.set_attribute(rel, "OwnerHistory", &r(oh))?;
    }
    ifc.set_attribute(rel, "RelatingObject", &r(step))?;
    ifc.set_attribute(rel, "RelatedObjects", &AttrValue::List(parts))?;
    Ok(())
}
