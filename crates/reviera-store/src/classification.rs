// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Classification in IFC (DM-CLS-001): each system with codes in use is an `IfcClassification`
//! (Source, Edition, Name, Location); each code used an `IfcClassificationReference`
//! (Identification, Name, ReferencedSource: its parent's reference, or the classification),
//! associated with its elements by one `IfcRelAssociatesClassification`. All are marked
//! `rxd:classification` in their Description and rewritten on every save (the tables live in
//! RXD). IFC2X3 files get none of it.

use std::collections::{BTreeMap, HashMap};

use reviera_core::{Document, ElementId, GlobalId};
use reviera_ifc::{AttrValue, IfcModel, StepId};

use crate::Result;

const TAG: &str = "rxd:classification";

fn ours(ifc: &IfcModel, id: StepId) -> bool {
    matches!(ifc.attribute(id, "Description"), Ok(AttrValue::String(d)) if d == TAG)
}

pub(crate) fn write_classifications(ifc: &mut IfcModel, doc: &Document, steps: &HashMap<ElementId, StepId>, owner_history: Option<StepId>) -> Result<()> {
    if ifc.schema() == "IFC2X3" {
        return Ok(());
    }
    for class in ["IfcRelAssociatesClassification", "IfcClassificationReference", "IfcClassification"] {
        for e in ifc.entities_of_type(class)? {
            if ours(ifc, e.id) {
                ifc.remove(e.id)?;
            }
        }
    }
    let s = |v: &str| AttrValue::String(v.into());
    for (system, used) in reviera_core::classification::used(doc.store()) {
        if used.is_empty() {
            continue;
        }
        let c = ifc.create("IfcClassification")?;
        ifc.set_attribute(c, "Name", &s(&system.name))?;
        ifc.set_attribute(c, "Description", &s(TAG))?;
        if !system.source.is_empty() {
            ifc.set_attribute(c, "Source", &s(&system.source))?;
        }
        if !system.edition.is_empty() {
            ifc.set_attribute(c, "Edition", &s(&system.edition))?;
        }
        if !system.location.is_empty() {
            ifc.set_attribute(c, "Location", &s(&system.location))?;
        }
        // The references: the codes used and their ancestors, parents first.
        let mut refs: BTreeMap<String, StepId> = BTreeMap::new();
        fn reference(ifc: &mut IfcModel, system: &reviera_core::classification::System, code: &str, root: StepId, refs: &mut BTreeMap<String, StepId>) -> Result<StepId> {
            if let Some(r) = refs.get(code) {
                return Ok(*r);
            }
            let item = system.item(code).expect("used codes are in the table").clone();
            let parent = match &item.parent {
                Some(p) => reference(ifc, system, p, root, refs)?,
                None => root,
            };
            let r = ifc.create("IfcClassificationReference")?;
            ifc.set_attribute(r, "Identification", &AttrValue::String(item.code.clone()))?;
            if !item.title.is_empty() {
                ifc.set_attribute(r, "Name", &AttrValue::String(item.title.clone()))?;
            }
            ifc.set_attribute(r, "Description", &AttrValue::String(TAG.into()))?;
            ifc.set_attribute(r, "ReferencedSource", &AttrValue::Ref(parent.0))?;
            refs.insert(code.to_owned(), r);
            Ok(r)
        }
        for (code, elements) in &used {
            let r = reference(ifc, &system, code, c, &mut refs)?;
            let objects: Vec<AttrValue> = elements.iter().filter_map(|e| steps.get(e)).map(|st| AttrValue::Ref(st.0)).collect();
            if objects.is_empty() {
                continue;
            }
            let rel = ifc.create_rooted("IfcRelAssociatesClassification", &GlobalId::new())?;
            if let Some(oh) = owner_history {
                ifc.set_attribute(rel, "OwnerHistory", &AttrValue::Ref(oh.0))?;
            }
            ifc.set_attribute(rel, "Description", &s(TAG))?;
            ifc.set_attribute(rel, "RelatedObjects", &AttrValue::List(objects))?;
            ifc.set_attribute(rel, "RelatingClassification", &AttrValue::Ref(r.0))?;
        }
    }
    Ok(())
}
