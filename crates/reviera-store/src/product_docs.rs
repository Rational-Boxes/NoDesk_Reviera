// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Product documentation in IFC (COL-FE-006): each documented material's folder is an
//! `IfcDocumentReference` (Location: the folder's address, Identification: its uid, Name: its
//! name) tied to the `IfcMaterial` by an `IfcExternalReferenceRelationship`, and each field set's
//! values are an `IfcMaterialProperties` of `IfcPropertySingleValue`s, the printed unit in each
//! one's Description and the documents they came from in `SourceDocuments`. All are marked
//! `rxd:product_docs` in their Description and rewritten on every save (the data lives in RXD).
//! A material no layer names gets an `IfcMaterial` of its own, marked the same way. IFC2X3 files
//! get none of it.

use std::collections::{BTreeSet, HashMap};

use reviera_core::Document;
use reviera_ifc::{AttrValue, IfcModel, StepId};
use serde_json::Value;

use crate::Result;

const TAG: &str = "rxd:product_docs";

fn ours(ifc: &IfcModel, id: StepId) -> bool {
    matches!(ifc.attribute(id, "Description"), Ok(AttrValue::String(d)) if d == TAG || d.starts_with("rxd:product_docs "))
}

fn label(v: &str) -> AttrValue {
    let t = if v.chars().count() > 255 { "IfcText" } else { "IfcLabel" };
    AttrValue::Typed { type_name: t.into(), value: Box::new(AttrValue::String(v.into())) }
}

fn nominal(v: &Value) -> Option<AttrValue> {
    Some(match v {
        Value::Bool(b) => AttrValue::Typed { type_name: "IfcBoolean".into(), value: Box::new(AttrValue::Bool(*b)) },
        Value::Number(n) => AttrValue::Typed { type_name: "IfcReal".into(), value: Box::new(AttrValue::Real(n.as_f64()?)) },
        Value::String(s) if !s.trim().is_empty() => label(s.trim()),
        _ => return None,
    })
}

pub(crate) fn write_product_docs(ifc: &mut IfcModel, doc: &Document) -> Result<()> {
    if ifc.schema() == "IFC2X3" {
        return Ok(());
    }
    for class in ["IfcExternalReferenceRelationship", "IfcDocumentReference", "IfcMaterialProperties", "IfcPropertySingleValue", "IfcMaterial"] {
        for e in ifc.entities_of_type(class)? {
            if ours(ifc, e.id) {
                ifc.remove(e.id)?;
            }
        }
    }
    let data = reviera_core::product_docs::data(doc.store());
    if data.materials.is_empty() {
        return Ok(());
    }
    let mut materials: HashMap<String, StepId> = HashMap::new();
    for e in ifc.entities_of_type("IfcMaterial")? {
        if let Ok(AttrValue::String(n)) = ifc.attribute(e.id, "Name") {
            materials.entry(n).or_insert(e.id);
        }
    }
    let s = |v: &str| AttrValue::String(v.into());
    for (name, md) in &data.materials {
        let material = match materials.get(name) {
            Some(m) => *m,
            None => {
                let m = ifc.create("IfcMaterial")?;
                ifc.set_attribute(m, "Name", &s(name))?;
                ifc.set_attribute(m, "Description", &s(TAG))?;
                m
            }
        };
        if let Some(l) = &md.link {
            let r = ifc.create("IfcDocumentReference")?;
            let location = if l.url.is_empty() { format!("fileengine://{}/{}", l.connection_id, l.folder_uid) } else { format!("{}#folder={}", l.url.trim_end_matches('/'), l.folder_uid) };
            ifc.set_attribute(r, "Location", &s(&location))?;
            ifc.set_attribute(r, "Identification", &s(&l.folder_uid))?;
            if !l.folder_name.is_empty() {
                ifc.set_attribute(r, "Name", &s(&l.folder_name))?;
            }
            ifc.set_attribute(r, "Description", &s(TAG))?;
            let rel = ifc.create("IfcExternalReferenceRelationship")?;
            ifc.set_attribute(rel, "Name", &s("Product documentation"))?;
            ifc.set_attribute(rel, "Description", &s(TAG))?;
            ifc.set_attribute(rel, "RelatingReference", &AttrValue::Ref(r.0))?;
            ifc.set_attribute(rel, "RelatedResourceObjects", &AttrValue::List(vec![AttrValue::Ref(material.0)]))?;
        }
        for (set, values) in &md.values {
            let mut props = Vec::new();
            let mut sources = BTreeSet::new();
            for (field, t) in values {
                let Some(v) = nominal(&t.value) else { continue };
                let p = ifc.create("IfcPropertySingleValue")?;
                ifc.set_attribute(p, "Name", &s(if t.property.is_empty() { field } else { &t.property }))?;
                let note = match &t.unit {
                    Some(u) if !u.trim().is_empty() => format!("{TAG} {}", u.trim()),
                    _ => TAG.to_owned(),
                };
                ifc.set_attribute(p, "Description", &s(&note))?;
                ifc.set_attribute(p, "NominalValue", &v)?;
                props.push(AttrValue::Ref(p.0));
                let doc_name = if t.source.file_name.is_empty() { &t.source.file_uid } else { &t.source.file_name };
                sources.insert(format!("{doc_name} (version {})", t.source.version));
            }
            if props.is_empty() {
                continue;
            }
            let p = ifc.create("IfcPropertySingleValue")?;
            ifc.set_attribute(p, "Name", &s("SourceDocuments"))?;
            ifc.set_attribute(p, "Description", &s(TAG))?;
            ifc.set_attribute(p, "NominalValue", &label(&sources.into_iter().collect::<Vec<_>>().join("; ")))?;
            props.push(AttrValue::Ref(p.0));
            let pset = md.psets.get(set).cloned().unwrap_or_else(|| format!("Reviera_{}", set.to_uppercase()));
            let mp = ifc.create("IfcMaterialProperties")?;
            ifc.set_attribute(mp, "Name", &s(&pset))?;
            ifc.set_attribute(mp, "Description", &s(TAG))?;
            ifc.set_attribute(mp, "Properties", &AttrValue::List(props))?;
            ifc.set_attribute(mp, "Material", &AttrValue::Ref(material.0))?;
        }
    }
    Ok(())
}
