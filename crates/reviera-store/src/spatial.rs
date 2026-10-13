// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The model tree as the IFC file records it (UI-TREE-001): each object's parent through spatial
//! containment, aggregation, voiding (opening › element) and filling (door › opening).

use std::collections::HashMap;

use reviera_core::GlobalId;
use reviera_ifc::{IfcModel, StepId};

use crate::Result;

/// Relationship classes, with the attribute naming the parent and the one naming the children.
const RELATIONS: &[(&str, &str, &str)] = &[
    (
        "IfcRelContainedInSpatialStructure",
        "RelatingStructure",
        "RelatedElements",
    ),
    ("IfcRelAggregates", "RelatingObject", "RelatedObjects"),
    (
        "IfcRelVoidsElement",
        "RelatingBuildingElement",
        "RelatedOpeningElement",
    ),
    (
        "IfcRelFillsElement",
        "RelatingOpeningElement",
        "RelatedBuildingElement",
    ),
];

/// The parent of each object that has one, by GlobalId. When an object has several (contained
/// and also part of an aggregate), the aggregate wins: it is the closer parent.
pub(crate) fn parents(ifc: &IfcModel) -> Result<HashMap<GlobalId, GlobalId>> {
    let guid_of: HashMap<StepId, GlobalId> = ifc
        .entities_of_type("IfcObjectDefinition")?
        .into_iter()
        .filter_map(|e| Some((e.id, e.global_id?)))
        .collect();
    let mut out = HashMap::new();
    for (class, parent, children) in RELATIONS {
        let rels = ifc.entities_of_type(class)?;
        let ids: Vec<StepId> = rels.iter().map(|r| r.id).collect();
        for e in ifc.attributes_many(&ids)? {
            let Some(p) = e.refs(parent).first().and_then(|p| guid_of.get(p)) else {
                continue;
            };
            for c in e.refs(children) {
                if let Some(c) = guid_of.get(&c) {
                    // Containment is listed first; later relations are closer parents.
                    out.insert(c.clone(), p.clone());
                }
            }
        }
    }
    Ok(out)
}
