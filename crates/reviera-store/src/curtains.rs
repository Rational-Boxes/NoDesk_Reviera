// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Curtain walls in IFC (ARC-CW-001): an `IfcCurtainWall` contained in its base level and
//! aggregating its panels (`IfcPlate`, `CURTAIN_PANEL`) and mullions (`IfcMember`, `MULLION`),
//! each with a faceted B-rep body.
//!
//! The parts are made from the curtain wall: they are not elements of their own. Their
//! `IfcRelAggregates` is marked `rxd:curtain:<signature>` in its Description and each part
//! `rxd:curtain`, so an unchanged curtain wall is left as it is, a changed one gets new parts,
//! and opening the file does not load the parts as elements.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use reviera_core::curtain::{self, CurtainGeometry};
use reviera_core::{Document, ElementId, GlobalId};
use reviera_ifc::{AttrValue, IfcModel, StepId};

use crate::{Result, geometry};

pub(crate) const PART_TAG: &str = "rxd:curtain";

fn signature(g: &CurtainGeometry, vents: Option<&reviera_core::vents::VentSettings>, scale: f64) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    if let Some(v) = vents {
        for x in [v.opening_angle, v.discharge] {
            x.to_bits().hash(&mut h);
        }
        (v.actuator as u8).hash(&mut h);
    }
    for p in &g.parts {
        p.mullion.hash(&mut h);
        (p.panel as u8).hash(&mut h);
        for v in p.x.iter().chain(&p.y).chain(&p.z).chain(p.outline.iter().flatten().flatten()) {
            v.to_bits().hash(&mut h);
        }
    }
    for v in g.line.start.iter().chain(&g.line.end) {
        v.to_bits().hash(&mut h);
    }
    (g.ty.panel as u8).hash(&mut h);
    scale.to_bits().hash(&mut h);
    format!("{:016x}", h.finish())
}

/// The aggregation of `whole`'s parts made by Reviera, with its Description.
fn our_aggregate(ifc: &IfcModel, whole: StepId) -> Result<Option<(StepId, String)>> {
    for rel in ifc.referenced_by(whole, "IfcRelAggregates")? {
        let relating = ifc.attribute(rel, "RelatingObject")?;
        if relating != AttrValue::Ref(whole.0) {
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

/// Removes the parts Reviera made for `whole`, and their aggregation.
pub(crate) fn remove_parts(ifc: &mut IfcModel, whole: StepId) -> Result<()> {
    let Some((rel, _)) = our_aggregate(ifc, whole)? else {
        return Ok(());
    };
    if let AttrValue::List(parts) = ifc.attribute(rel, "RelatedObjects")? {
        for p in parts {
            if let AttrValue::Ref(p) = p {
                // A vent's Reviera_Vent (ARC-GLZ-004).
                for rel in ifc.referenced_by(StepId(p), "IfcRelDefinesByProperties")? {
                    if let AttrValue::Ref(set) = ifc.attribute(rel, "RelatingPropertyDefinition")? {
                        if let AttrValue::List(props) = ifc.attribute(StepId(set), "HasProperties")? {
                            for q in props {
                                if let AttrValue::Ref(q) = q {
                                    ifc.remove(StepId(q))?;
                                }
                            }
                        }
                        ifc.remove(StepId(set))?;
                    }
                    ifc.remove(rel)?;
                }
                geometry::remove_breps(ifc, StepId(p))?;
                ifc.remove(StepId(p))?;
            }
        }
    }
    Ok(ifc.remove(rel)?)
}

/// A vent window's `Reviera_Vent` (ARC-GLZ-004).
fn vent_pset(ifc: &mut IfcModel, target: StepId, owner_history: Option<StepId>, values: &[(String, serde_json::Value)]) -> Result<()> {
    let mut items = Vec::new();
    let mut name = String::new();
    for (key, v) in values {
        let Some((set, prop)) = key.split_once('.') else { continue };
        name = set.to_owned();
        let ty = v["type"].as_str().unwrap_or("IfcLabel");
        let p = ifc.create("IfcPropertySingleValue")?;
        ifc.set_attribute(p, "Name", &AttrValue::String(prop.to_owned()))?;
        ifc.set_attribute(p, "NominalValue", &crate::params::to_attr(ty, &v["value"])?)?;
        items.push(AttrValue::Ref(p.0));
    }
    let set = ifc.create_rooted("IfcPropertySet", &GlobalId::new())?;
    ifc.set_attribute(set, "Name", &AttrValue::String(name))?;
    ifc.set_attribute(set, "HasProperties", &AttrValue::List(items))?;
    let rel = ifc.create_rooted("IfcRelDefinesByProperties", &GlobalId::new())?;
    if let Some(oh) = owner_history {
        ifc.set_attribute(set, "OwnerHistory", &AttrValue::Ref(oh.0))?;
        ifc.set_attribute(rel, "OwnerHistory", &AttrValue::Ref(oh.0))?;
    }
    ifc.set_attribute(rel, "RelatedObjects", &AttrValue::List(vec![AttrValue::Ref(target.0)]))?;
    ifc.set_attribute(rel, "RelatingPropertyDefinition", &AttrValue::Ref(set.0))?;
    Ok(())
}

/// Writes every curtain wall's parts.
pub(crate) fn write_curtains(
    ifc: &mut IfcModel,
    doc: &Document,
    steps: &HashMap<ElementId, StepId>,
    owner_history: Option<StepId>,
    scale: f64,
) -> Result<()> {
    let store = doc.store();
    let all = curtain::all(store);
    if all.is_empty() {
        return Ok(());
    }
    let contexts = geometry::Contexts::find(ifc)?;
    let ifc4 = ifc.schema() != "IFC2X3";
    for (id, g) in all {
        let Some(&step) = steps.get(&id) else {
            continue;
        };
        if let Some(storey) = curtain::curtain_recipe(store.get(id).expect("listed"))
            .and_then(|r| GlobalId::parse(&r.extent.base_level))
            .and_then(|l| store.find_by_guid(&l))
            .and_then(|l| steps.get(&l).copied())
        {
            geometry::contain(ifc, step, storey, owner_history)?;
        }
        let vents = reviera_core::vents::settings_of(store.get(id).expect("listed"));
        let tag = format!("{PART_TAG}:{}", signature(&g, vents.as_ref(), scale));
        if our_aggregate(ifc, step)?.is_some_and(|(_, d)| d == tag) {
            continue;
        }
        remove_parts(ifc, step)?;
        let mut parts = Vec::new();
        for (k, p) in g.parts.iter().enumerate() {
            let (class, name, predefined) = if p.mullion {
                ("IfcMember", "Mullion", "MULLION")
            } else if p.panel == curtain::Panel::Door {
                // A curtain-wall door (ARC-CW-006).
                ("IfcDoor", "Curtain Wall Door", "DOOR")
            } else if p.panel == curtain::Panel::Vent {
                // An opening pane (ARC-GLZ-004).
                ("IfcWindow", "Vent", "WINDOW")
            } else if p.panel == curtain::Panel::RollUp {
                ("IfcWindow", "Roll-up Side", "WINDOW")
            } else {
                ("IfcPlate", "Panel", "CURTAIN_PANEL")
            };
            let e = ifc.create_rooted(class, &GlobalId::new())?;
            if let Some(h) = owner_history {
                ifc.set_attribute(e, "OwnerHistory", &AttrValue::Ref(h.0))?;
            }
            ifc.set_attribute(e, "Name", &AttrValue::String(format!("{name} {}", k + 1)))?;
            ifc.set_attribute(e, "Description", &AttrValue::String(PART_TAG.to_owned()))?;
            if ifc4 {
                ifc.set_attribute(e, "PredefinedType", &AttrValue::Enum(predefined.into()))?;
            }
            let (points, triangles) = g.mesh(p);
            // A box's eight corners: bottom ring then top ring; a part clipped by an attached
            // top (ARC-GLZ-001) by its mesh's triangles.
            let faces = if p.outline.is_some() {
                triangles.chunks(3).map(|t| t.iter().map(|i| *i as usize).collect()).collect()
            } else {
                vec![
                    vec![3, 2, 1, 0],
                    vec![4, 5, 6, 7],
                    vec![0, 1, 5, 4],
                    vec![1, 2, 6, 5],
                    vec![2, 3, 7, 6],
                    vec![3, 0, 4, 7],
                ]
            };
            geometry::write_breps(
                ifc,
                e,
                geometry::BrepFrame::WORLD,
                &[geometry::Polyhedron { points, faces }],
                scale,
                contexts,
            )?;
            if matches!(p.panel, curtain::Panel::Vent | curtain::Panel::RollUp) && !p.mullion
                && let Some(s) = &vents
            {
                let values = reviera_core::vents::vent_params(p.panel, p.x[1] - p.x[0], p.z[1] - p.z[0], s);
                vent_pset(ifc, e, owner_history, &values)?;
            }
            parts.push(AttrValue::Ref(e.0));
        }
        let rel = ifc.create_rooted("IfcRelAggregates", &GlobalId::new())?;
        if let Some(h) = owner_history {
            ifc.set_attribute(rel, "OwnerHistory", &AttrValue::Ref(h.0))?;
        }
        ifc.set_attribute(rel, "Description", &AttrValue::String(tag))?;
        ifc.set_attribute(rel, "RelatingObject", &AttrValue::Ref(step.0))?;
        ifc.set_attribute(rel, "RelatedObjects", &AttrValue::List(parts))?;
    }
    Ok(())
}
