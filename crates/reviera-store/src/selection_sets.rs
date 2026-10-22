// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Selection sets in IFC (ARC-SEL-002): with the project option *Selection sets as IfcGroup*,
//! each saved selection is an `IfcGroup` (ObjectType "SelectionSet") whose members are assigned
//! by one `IfcRelAssignsToGroup`. Both are marked `rxd:selection_set` in their Description and
//! rewritten on every save, so they are not read back as elements (the sets live in RXD).

use std::collections::HashMap;

use reviera_core::{Document, ElementId, GlobalId};
use reviera_ifc::{AttrValue, IfcModel, StepId};

use crate::Result;

pub(crate) const TAG: &str = "rxd:selection_set";

fn ours(ifc: &IfcModel, id: StepId) -> bool {
    matches!(ifc.attribute(id, "Description"), Ok(AttrValue::String(d)) if d == TAG)
}

pub(crate) fn write_selection_sets(ifc: &mut IfcModel, doc: &Document, steps: &HashMap<ElementId, StepId>, owner_history: Option<StepId>) -> Result<()> {
    for class in ["IfcRelAssignsToGroup", "IfcGroup"] {
        for e in ifc.entities_of_type(class)? {
            if ours(ifc, e.id) {
                ifc.remove(e.id)?;
            }
        }
    }
    let store = doc.store();
    if !reviera_core::selection_sets::exported(store) {
        return Ok(());
    }
    let s = |v: &str| AttrValue::String(v.into());
    for (set, members) in reviera_core::selection_sets::resolved(store) {
        let group = ifc.create_rooted("IfcGroup", &GlobalId::parse(&set.id).unwrap_or_default())?;
        if let Some(oh) = owner_history {
            ifc.set_attribute(group, "OwnerHistory", &AttrValue::Ref(oh.0))?;
        }
        ifc.set_attribute(group, "Name", &s(&set.name))?;
        ifc.set_attribute(group, "Description", &s(TAG))?;
        ifc.set_attribute(group, "ObjectType", &s("SelectionSet"))?;
        let refs: Vec<AttrValue> = members.iter().filter_map(|m| steps.get(m)).map(|st| AttrValue::Ref(st.0)).collect();
        if refs.is_empty() {
            continue;
        }
        let rel = ifc.create_rooted("IfcRelAssignsToGroup", &GlobalId::new())?;
        if let Some(oh) = owner_history {
            ifc.set_attribute(rel, "OwnerHistory", &AttrValue::Ref(oh.0))?;
        }
        ifc.set_attribute(rel, "Description", &s(TAG))?;
        ifc.set_attribute(rel, "RelatedObjects", &AttrValue::List(refs))?;
        ifc.set_attribute(rel, "RelatingGroup", &AttrValue::Ref(group.0))?;
    }
    Ok(())
}
