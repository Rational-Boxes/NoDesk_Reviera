// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Groups in IFC (ARC-GRP-001): each instance is an `IfcGroup` whose members are assigned to it
//! with an `IfcRelAssignsToGroup`, marked `rxd:group` in its Description so it is rewritten when
//! the members change.

use std::collections::HashMap;

use reviera_core::{Document, ElementId, GlobalId};
use reviera_ifc::{AttrValue, IfcModel, StepId};

use crate::Result;

const TAG: &str = "rxd:group";

/// Writes every group instance's member assignment.
pub(crate) fn write_groups(
    ifc: &mut IfcModel,
    doc: &Document,
    steps: &HashMap<ElementId, StepId>,
    owner_history: Option<StepId>,
) -> Result<()> {
    let store = doc.store();
    for e in store.iter() {
        let Some(g) = reviera_core::groups::recipe(e) else {
            continue;
        };
        let Some(&step) = steps.get(&e.id()) else {
            continue;
        };
        let members: Vec<AttrValue> = g
            .members
            .iter()
            .filter_map(|m| GlobalId::parse(&m.guid))
            .filter_map(|guid| store.find_by_guid(&guid))
            .filter_map(|id| steps.get(&id))
            .map(|s| AttrValue::Ref(s.0))
            .collect();
        let ours: Vec<StepId> = ifc
            .referenced_by(step, "IfcRelAssignsToGroup")?
            .into_iter()
            .filter(|rel| {
                matches!(ifc.attribute(*rel, "Description"), Ok(AttrValue::String(d)) if d == TAG)
            })
            .collect();
        if let [rel] = ours.as_slice()
            && ifc.attribute(*rel, "RelatedObjects")? == AttrValue::List(members.clone())
        {
            continue;
        }
        for rel in ours {
            ifc.remove(rel)?;
        }
        if members.is_empty() {
            continue;
        }
        let rel = ifc.create_rooted("IfcRelAssignsToGroup", &GlobalId::new())?;
        ifc.set_attribute(rel, "Description", &AttrValue::String(TAG.into()))?;
        if let Some(oh) = owner_history {
            ifc.set_attribute(rel, "OwnerHistory", &AttrValue::Ref(oh.0))?;
        }
        ifc.set_attribute(rel, "RelatedObjects", &AttrValue::List(members))?;
        ifc.set_attribute(rel, "RelatingGroup", &AttrValue::Ref(step.0))?;
    }
    Ok(())
}
