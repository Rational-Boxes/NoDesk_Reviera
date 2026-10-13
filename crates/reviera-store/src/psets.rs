// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Reading and writing the `Reviera_*` property sets that carry RXD (Ch 2a §3). Elements are linked
//! through `IfcRelDefinesByProperties`; type objects through `HasPropertySets`.

use std::collections::BTreeMap;

use reviera_core::GlobalId;
use reviera_ifc::{AttrValue, IfcModel, StepId};
use reviera_rxd::{Encoded, Encoding, prop};

use crate::{Result, StoreError};

/// A `Reviera_*` property set as found in the file.
#[derive(Debug, Clone)]
pub(crate) struct Pset {
    pub id: StepId,
    pub name: String,
    /// Property name → (property entity, value inside its defined type).
    pub props: BTreeMap<String, (StepId, AttrValue)>,
    /// Objects it is attached to, with the relationship (or type object) that attaches it.
    pub targets: Vec<(StepId, Link)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Link {
    Rel(StepId),
    TypeObject,
}

impl Pset {
    pub fn string(&self, name: &str) -> Option<&str> {
        match self.props.get(name) {
            Some((_, AttrValue::String(s))) => Some(s),
            _ => None,
        }
    }

    pub fn int(&self, name: &str) -> Option<i64> {
        match self.props.get(name) {
            Some((_, AttrValue::Int(i))) => Some(*i),
            _ => None,
        }
    }

    /// The payload chunks: `Data`, or `Data_0001` … in numeric order.
    pub fn chunks(&self) -> Vec<String> {
        if let Some(d) = self.string(prop::DATA) {
            return vec![d.to_owned()];
        }
        // BTreeMap order sorts the zero-padded names numerically.
        self.props
            .iter()
            .filter(|(n, _)| n.starts_with("Data_"))
            .filter_map(|(_, (_, v))| {
                if let AttrValue::String(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn decode(&self, limits: reviera_rxd::DecodeLimits) -> Result<serde_json::Value> {
        let encoding = Encoding::parse(self.string(prop::ENCODING).unwrap_or("json"))?;
        Ok(reviera_rxd::decode(
            encoding,
            &self.chunks(),
            self.string(prop::SHA256),
            limits,
        )?)
    }
}

pub(crate) fn read_pset(ifc: &IfcModel, id: StepId) -> Result<Pset> {
    let attrs: BTreeMap<String, AttrValue> = ifc.attributes(id)?.into_iter().collect();
    let name = match attrs.get("Name") {
        Some(AttrValue::String(s)) => s.clone(),
        _ => String::new(),
    };
    let mut props = BTreeMap::new();
    if let Some(AttrValue::List(items)) = attrs.get("HasProperties") {
        for item in items {
            let AttrValue::Ref(p) = item else { continue };
            let p = StepId(*p);
            let pa: BTreeMap<String, AttrValue> = ifc.attributes(p)?.into_iter().collect();
            if let (Some(AttrValue::String(n)), Some(v)) = (pa.get("Name"), pa.get("NominalValue"))
            {
                let v = match v {
                    AttrValue::Typed { value, .. } => value.as_ref().clone(),
                    other => other.clone(),
                };
                props.insert(n.clone(), (p, v));
            }
        }
    }
    let mut targets = Vec::new();
    for rel in ifc.referenced_by(id, "IfcRelDefinesByProperties")? {
        if let AttrValue::List(objs) = ifc.attribute(rel, "RelatedObjects")? {
            targets.extend(objs.iter().filter_map(|o| {
                if let AttrValue::Ref(r) = o {
                    Some((StepId(*r), Link::Rel(rel)))
                } else {
                    None
                }
            }));
        }
    }
    targets.extend(
        ifc.referenced_by(id, "IfcTypeObject")?
            .into_iter()
            .map(|t| (t, Link::TypeObject)),
    );
    Ok(Pset {
        id,
        name,
        props,
        targets,
    })
}

/// Every property set whose name starts with `Reviera_`.
pub(crate) fn reviera_psets(ifc: &IfcModel) -> Result<Vec<Pset>> {
    ifc.entities_of_type("IfcPropertySet")?
        .into_iter()
        .filter(|e| e.name.as_deref().is_some_and(|n| n.starts_with("Reviera_")))
        .map(|e| read_pset(ifc, e.id))
        .collect()
}

pub(crate) fn label(s: &str) -> AttrValue {
    typed("IfcLabel", AttrValue::String(s.into()))
}

pub(crate) fn identifier(s: &str) -> AttrValue {
    typed("IfcIdentifier", AttrValue::String(s.into()))
}

pub(crate) fn text(s: &str) -> AttrValue {
    typed("IfcText", AttrValue::String(s.into()))
}

pub(crate) fn integer(i: i64) -> AttrValue {
    typed("IfcInteger", AttrValue::Int(i))
}

fn typed(type_name: &str, value: AttrValue) -> AttrValue {
    AttrValue::Typed {
        type_name: type_name.into(),
        value: Box::new(value),
    }
}

/// The properties describing one encoded payload (§3.2).
pub(crate) fn payload_props(kind: &str, rev: i64, encoded: &Encoded) -> Vec<(String, AttrValue)> {
    let mut props = vec![
        (prop::KIND.to_owned(), label(kind)),
        (
            prop::SCHEMA_VERSION.to_owned(),
            label(reviera_rxd::SCHEMA_VERSION),
        ),
        (prop::ENCODING.to_owned(), label(encoded.encoding.as_str())),
        (prop::SHA256.to_owned(), identifier(&encoded.sha256)),
        (prop::REV.to_owned(), integer(rev)),
    ];
    if let [single] = encoded.chunks.as_slice() {
        props.push((prop::DATA.to_owned(), text(single)));
    } else {
        props.extend(
            encoded
                .chunks
                .iter()
                .enumerate()
                .map(|(i, c)| (prop::chunk(i + 1), text(c))),
        );
    }
    props
}

/// Writes property sets, attaching them the way the target needs.
pub(crate) struct PsetWriter<'a> {
    pub ifc: &'a mut IfcModel,
    /// Required on new `IfcRoot` entities in IFC2X3, optional (and left unset) in IFC4.
    pub owner_history: Option<StepId>,
}

impl PsetWriter<'_> {
    fn create_rooted(&mut self, class: &str) -> Result<StepId> {
        let id = self.ifc.create_rooted(class, &GlobalId::new())?;
        if let Some(oh) = self.owner_history {
            self.ifc
                .set_attribute(id, "OwnerHistory", &AttrValue::Ref(oh.0))?;
        }
        Ok(id)
    }

    fn create_prop(&mut self, name: &str, value: &AttrValue) -> Result<StepId> {
        let p = self.ifc.create("IfcPropertySingleValue")?;
        self.ifc
            .set_attribute(p, "Name", &AttrValue::String(name.into()))?;
        self.ifc.set_attribute(p, "NominalValue", value)?;
        Ok(p)
    }

    /// Creates a property set and attaches it to `target` (a type object when `target_is_type`).
    pub fn create(
        &mut self,
        name: &str,
        target: StepId,
        target_is_type: bool,
        props: &[(String, AttrValue)],
    ) -> Result<StepId> {
        let ids = props
            .iter()
            .map(|(n, v)| self.create_prop(n, v))
            .collect::<Result<Vec<_>>>()?;
        let pset = self.create_rooted("IfcPropertySet")?;
        self.ifc
            .set_attribute(pset, "Name", &AttrValue::String(name.into()))?;
        self.ifc.set_attribute(pset, "HasProperties", &refs(&ids))?;
        if target_is_type {
            let mut list = match self.ifc.attribute(target, "HasPropertySets")? {
                AttrValue::List(items) => items,
                _ => Vec::new(),
            };
            list.push(AttrValue::Ref(pset.0));
            self.ifc
                .set_attribute(target, "HasPropertySets", &AttrValue::List(list))?;
        } else {
            let rel = self.create_rooted("IfcRelDefinesByProperties")?;
            self.ifc
                .set_attribute(rel, "RelatedObjects", &refs(&[target]))?;
            self.ifc
                .set_attribute(rel, "RelatingPropertyDefinition", &AttrValue::Ref(pset.0))?;
        }
        Ok(pset)
    }

    /// Replaces the properties of an existing set: values are updated in place, missing ones are
    /// created, and payload chunks that are no longer used are removed. Other properties are kept.
    pub fn update(&mut self, pset: &Pset, props: &[(String, AttrValue)]) -> Result<()> {
        let wanted: BTreeMap<&str, &AttrValue> =
            props.iter().map(|(n, v)| (n.as_str(), v)).collect();
        let mut ids = Vec::new();
        for (name, (id, _)) in &pset.props {
            let is_payload = name == prop::DATA || name.starts_with("Data_");
            match wanted.get(name.as_str()) {
                Some(v) => {
                    self.ifc.set_attribute(*id, "NominalValue", v)?;
                    ids.push(*id);
                }
                None if is_payload => self.ifc.remove(*id)?,
                None => ids.push(*id),
            }
        }
        for (name, v) in props {
            if !pset.props.contains_key(name) {
                ids.push(self.create_prop(name, v)?);
            }
        }
        self.ifc
            .set_attribute(pset.id, "HasProperties", &refs(&ids))?;
        Ok(())
    }

    /// Stops a shared property set from applying to `target`, so `target` can get its own.
    pub fn detach(&mut self, pset: &Pset, target: StepId) -> Result<()> {
        for (t, link) in &pset.targets {
            if *t != target {
                continue;
            }
            let (owner, attr) = match link {
                Link::Rel(rel) => (*rel, "RelatedObjects"),
                Link::TypeObject => (target, "HasPropertySets"),
            };
            let remove = if attr == "RelatedObjects" {
                target.0
            } else {
                pset.id.0
            };
            if let AttrValue::List(items) = self.ifc.attribute(owner, attr)? {
                let kept: Vec<_> = items
                    .into_iter()
                    .filter(|i| *i != AttrValue::Ref(remove))
                    .collect();
                let value = if kept.is_empty() {
                    AttrValue::Null
                } else {
                    AttrValue::List(kept)
                };
                self.ifc.set_attribute(owner, attr, &value)?;
            }
        }
        Ok(())
    }
}

fn refs(ids: &[StepId]) -> AttrValue {
    AttrValue::List(ids.iter().map(|i| AttrValue::Ref(i.0)).collect())
}

pub(crate) fn unsupported(msg: impl Into<String>) -> StoreError {
    StoreError::Unsupported(msg.into())
}
