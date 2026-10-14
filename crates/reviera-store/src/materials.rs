// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Material layer sets of layered wall types (ARC-CMPD-001).
//!
//! Each Reviera wall type with layers of its own gets an `IfcMaterialLayerSet` of
//! `IfcMaterialLayer`s (exterior first), associated with the type; its walls share one
//! `IfcMaterialLayerSetUsage` along their axis, centred on the location line (the walls'
//! centreline). The two `IfcRelAssociatesMaterial`s are marked in their Description
//! (`rxd:layers:<type>:<signature>`, `rxd:usage:<type>`), so an unchanged structure is left as it
//! is and one that changed or went is replaced or removed. Materials are shared by name.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

use reviera_core::compound::{self, Compound, Function};
use reviera_core::{Document, ElementId, GlobalId};
use reviera_ifc::{AttrValue, IfcModel, StepId};
use serde_json::Value;

use crate::Result;

const LAYERS_TAG: &str = "rxd:layers:";
const USAGE_TAG: &str = "rxd:usage:";

fn create(ifc: &mut IfcModel, class: &str, attrs: &[(&str, AttrValue)]) -> Result<StepId> {
    let id = ifc.create(class)?;
    for (name, value) in attrs {
        ifc.set_attribute(id, name, value)?;
    }
    Ok(id)
}

fn signature(c: &Compound, name: &str, scale: f64) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(c).unwrap_or_default().hash(&mut h);
    name.hash(&mut h);
    scale.to_bits().hash(&mut h);
    format!("{:016x}", h.finish())
}

/// The material named `name`, made when there is none yet.
fn material(ifc: &mut IfcModel, cache: &mut HashMap<String, StepId>, name: &str) -> Result<StepId> {
    if let Some(m) = cache.get(name) {
        return Ok(*m);
    }
    let m = create(
        ifc,
        "IfcMaterial",
        &[("Name", AttrValue::String(name.to_owned()))],
    )?;
    cache.insert(name.to_owned(), m);
    Ok(m)
}

/// Removes a relation and the layer set or usage it relates (the set's layers too).
fn remove(ifc: &mut IfcModel, rel: StepId) -> Result<()> {
    if let AttrValue::Ref(m) = ifc.attribute(rel, "RelatingMaterial")? {
        let m = StepId(m);
        if ifc.class_of(m)? == "IfcMaterialLayerSet"
            && let AttrValue::List(layers) = ifc.attribute(m, "MaterialLayers")?
        {
            for l in layers {
                if let AttrValue::Ref(l) = l {
                    ifc.remove(StepId(l))?;
                }
            }
        }
        // Usages of a removed set go with it.
        if ifc.class_of(m)? == "IfcMaterialLayerSet" {
            for u in ifc.referenced_by(m, "IfcMaterialLayerSetUsage")? {
                ifc.remove(u)?;
            }
        }
        ifc.remove(m)?;
    }
    Ok(ifc.remove(rel)?)
}

/// Writes the layer sets of every layered wall type, and their walls' usages.
pub(crate) fn write_layer_sets(
    ifc: &mut IfcModel,
    doc: &Document,
    steps: &HashMap<ElementId, StepId>,
    owner_history: Option<StepId>,
    scale: f64,
) -> Result<()> {
    let store = doc.store();
    let ifc4 = ifc.schema() != "IFC2X3";
    // Ours, by type GlobalId: (relation, description).
    let mut layers_rels: BTreeMap<String, (StepId, String)> = BTreeMap::new();
    let mut usage_rels: BTreeMap<String, StepId> = BTreeMap::new();
    for r in ifc.entities_of_type("IfcRelAssociatesMaterial")? {
        if let AttrValue::String(d) = ifc.attribute(r.id, "Description")? {
            if let Some(rest) = d.strip_prefix(LAYERS_TAG) {
                let guid = rest.split(':').next().unwrap_or_default().to_owned();
                layers_rels.insert(guid, (r.id, d.clone()));
            } else if let Some(guid) = d.strip_prefix(USAGE_TAG) {
                usage_rels.insert(guid.to_owned(), r.id);
            }
        }
    }
    let types: Vec<(ElementId, String, String, Compound)> = store
        .iter()
        .filter(|e| compound::is_layered(e))
        .filter_map(|e| {
            Some((
                e.id(),
                e.guid().to_string(),
                e.name.clone().unwrap_or_default(),
                compound::of_wall_type(e)?,
            ))
        })
        .collect();
    if types.is_empty() && layers_rels.is_empty() && usage_rels.is_empty() {
        return Ok(());
    }
    let mut cache: HashMap<String, StepId> = HashMap::new();
    for m in ifc.entities_of_type("IfcMaterial")? {
        if let Some(n) = m.name.clone() {
            cache.entry(n).or_insert(m.id);
        }
    }
    let rel = |ifc: &mut IfcModel, desc: String, objects: Vec<StepId>, material: StepId| {
        let r = ifc.create_rooted("IfcRelAssociatesMaterial", &GlobalId::new())?;
        if let Some(h) = owner_history {
            ifc.set_attribute(r, "OwnerHistory", &AttrValue::Ref(h.0))?;
        }
        ifc.set_attribute(r, "Description", &AttrValue::String(desc))?;
        ifc.set_attribute(
            r,
            "RelatedObjects",
            &AttrValue::List(objects.into_iter().map(|o| AttrValue::Ref(o.0)).collect()),
        )?;
        ifc.set_attribute(r, "RelatingMaterial", &AttrValue::Ref(material.0))?;
        Ok::<StepId, crate::StoreError>(r)
    };
    for (id, guid, name, c) in types {
        let Some(&type_step) = steps.get(&id) else {
            continue;
        };
        let desc = format!("{LAYERS_TAG}{guid}:{}", signature(&c, &name, scale));
        let set = match layers_rels.remove(&guid) {
            Some((r, d)) if d == desc => match ifc.attribute(r, "RelatingMaterial")? {
                AttrValue::Ref(s) => StepId(s),
                _ => continue,
            },
            old => {
                // The usage first: removing the set removes its usages.
                if let Some(u) = usage_rels.remove(&guid) {
                    remove(ifc, u)?;
                }
                if let Some((r, _)) = old {
                    remove(ifc, r)?;
                }
                let mut layers = Vec::new();
                for (i, l) in c.layers.iter().enumerate() {
                    let mname = if l.material.is_empty() {
                        l.function.name().to_owned()
                    } else {
                        l.material.clone()
                    };
                    let m = material(ifc, &mut cache, &mname)?;
                    // IsVentilated is optional; a membrane's is unknown (left unset). Only an air
                    // layer is ventilated, and Reviera has one function for thermal and air
                    // layers, so they are written as not ventilated.
                    let ventilated = match l.function {
                        Function::Membrane => AttrValue::Null,
                        _ => AttrValue::Bool(false),
                    };
                    let mut attrs = vec![
                        ("Material", AttrValue::Ref(m.0)),
                        ("LayerThickness", AttrValue::Real(l.thickness / scale)),
                        ("IsVentilated", ventilated),
                    ];
                    if ifc4 {
                        let p = l.priority();
                        let priority = if p == 0 {
                            0
                        } else {
                            (100 - 10 * (i64::from(p) - 1)).clamp(0, 100)
                        };
                        attrs.push(("Name", AttrValue::String(l.function.name().to_owned())));
                        attrs.push((
                            "Category",
                            AttrValue::String(l.function.category().to_owned()),
                        ));
                        attrs.push(("Priority", AttrValue::Int(priority)));
                        // The core's layers, so other tools can tell.
                        if (c.core[0]..c.core[1]).contains(&i) {
                            attrs.push(("Description", AttrValue::String("Core".to_owned())));
                        }
                    }
                    layers.push(AttrValue::Ref(create(ifc, "IfcMaterialLayer", &attrs)?.0));
                }
                let set = create(
                    ifc,
                    "IfcMaterialLayerSet",
                    &[
                        ("MaterialLayers", AttrValue::List(layers)),
                        ("LayerSetName", AttrValue::String(name.clone())),
                    ],
                )?;
                rel(ifc, desc, vec![type_step], set)?;
                set
            }
        };
        // The walls of the type share one usage, centred on their location line.
        let walls: Vec<StepId> = store
            .iter()
            .filter(|w| {
                reviera_core::walls::wall_recipe(w).is_some()
                    && w.param("@type").and_then(Value::as_str) == Some(guid.as_str())
            })
            .filter_map(|w| steps.get(&w.id()).copied())
            .collect();
        let objects = AttrValue::List(
            walls
                .iter()
                .map(|w| AttrValue::Ref(w.0))
                .collect::<Vec<_>>(),
        );
        match usage_rels.remove(&guid) {
            Some(u) if walls.is_empty() => remove(ifc, u)?,
            Some(u) => {
                if ifc.attribute(u, "RelatedObjects")? != objects {
                    ifc.set_attribute(u, "RelatedObjects", &objects)?;
                }
            }
            None if walls.is_empty() => {}
            None => {
                let usage = create(
                    ifc,
                    "IfcMaterialLayerSetUsage",
                    &[
                        ("ForLayerSet", AttrValue::Ref(set.0)),
                        ("LayerSetDirection", AttrValue::Enum("AXIS2".into())),
                        ("DirectionSense", AttrValue::Enum("POSITIVE".into())),
                        (
                            "OffsetFromReferenceLine",
                            AttrValue::Real(-c.thickness() / 2.0 / scale),
                        ),
                    ],
                )?;
                rel(ifc, format!("{USAGE_TAG}{guid}"), walls, usage)?;
            }
        }
    }
    // Types no longer layered, or gone: usages first.
    for (_, u) in usage_rels {
        remove(ifc, u)?;
    }
    for (_, (r, _)) in layers_rels {
        remove(ifc, r)?;
    }
    Ok(())
}
