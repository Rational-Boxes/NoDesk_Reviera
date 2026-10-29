// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Parameter values stored in IFC (spec Ch 2 §2.3.3, DM-IFC-003).
//!
//! Element parameters use these keys:
//!
//! | Key | Value | Source |
//! |---|---|---|
//! | `<PsetName>.<Property>` | `{"type": "IfcLabel", "value": "EI 60"}` | `IfcPropertySingleValue` in a property set |
//! | `<QtoName>.<Quantity>` | `{"type": "IfcQuantityLength", "value": 5.0}` | `IfcElementQuantity` (read-only) |
//! | `<PsetName>.<Property>` | `{"kind": "IfcPropertyEnumeratedValue", …}` | other property kinds (read-only for now) |
//! | `@Description`, `@Tag`, `@ObjectType`, `@PredefinedType`, `@LongName` | string | direct IFC attributes (`@Tag` is Mark; `@LongName` a space's or storey's long name) |
//! | `@Elevation` | number, metres | storey elevation (`IfcBuildingStorey.Elevation`) |
//! | `@type` | GlobalId of the type | `IfcRelDefinesByType` |
//! | `rxd:<section>` | JSON | a section of the element's RXD data (`Reviera_Element`), not an IFC value |
//!
//! Instance values come from property sets on the occurrence; a type's own values are parameters
//! of the type element. The value's IFC measure type is kept, so values round-trip unchanged.

use std::collections::{BTreeMap, HashMap, HashSet};

use reviera_core::GlobalId;
use reviera_ifc::{AttrValue, Entity, EntityInfo, IfcModel, StepId};
use serde_json::{Value, json};

use crate::Result;
use crate::psets::unsupported;

/// Parameter values of one element, by key.
pub type Params = BTreeMap<String, Value>;

/// Direct attributes exposed as parameters.
pub const ATTRIBUTES: &[&str] = &[
    "Description",
    "Tag",
    "ObjectType",
    "PredefinedType",
    "LongName",
    // Structural load groups and actions (STR-LC-001, STR-LOAD-001).
    "ActionType",
    "ActionSource",
    "Purpose",
    "GlobalOrLocal",
    "ProjectedOrTrue",
    "DestabilizingLoad",
];
/// Direct attributes whose values are enumerations.
const ENUM_ATTRIBUTES: &[&str] = &[
    "PredefinedType",
    "ActionType",
    "ActionSource",
    "GlobalOrLocal",
    "ProjectedOrTrue",
];
/// Direct attributes that are lengths, exposed in metres.
pub const LENGTH_ATTRIBUTES: &[&str] = &["Elevation"];
pub const TYPE_KEY: &str = "@type";
/// Prefix of keys that hold RXD element-data sections.
pub const RXD_PREFIX: &str = "rxd:";

/// One property or quantity set on one owner.
#[derive(Debug, Clone)]
pub(crate) struct SetEntry {
    pub id: StepId,
    pub quantity: bool,
    pub props: BTreeMap<String, StepId>,
    /// Attached to more than one object: copy before writing.
    pub shared: bool,
    pub link: SetLink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SetLink {
    Rel(StepId),
    TypeObject,
}

/// Property and quantity sets by owner and set name.
#[derive(Debug, Clone, Default)]
pub(crate) struct SetIndex {
    pub by_owner: HashMap<StepId, BTreeMap<String, SetEntry>>,
}

/// Reads the parameter values of `objects`. Reviera's own `Reviera_*` sets are skipped.
pub(crate) fn load(
    ifc: &IfcModel,
    objects: &[EntityInfo],
    length_scale: f64,
) -> Result<(HashMap<StepId, Params>, SetIndex)> {
    let mut params: HashMap<StepId, BTreeMap<String, Value>> = HashMap::new();
    let ids: Vec<StepId> = objects.iter().map(|e| e.id).collect();
    let guid_of: HashMap<StepId, &GlobalId> = objects
        .iter()
        .filter_map(|e| Some((e.id, e.global_id.as_ref()?)))
        .collect();

    // Direct attributes, and the sets type objects carry directly.
    let mut owners: HashMap<StepId, Vec<(StepId, SetLink)>> = HashMap::new();
    for (id, entity) in ids.iter().zip(ifc.attributes_many(&ids)?) {
        let p = params.entry(*id).or_default();
        for name in ATTRIBUTES {
            match entity.get(name) {
                AttrValue::String(s) => {
                    p.insert(format!("@{name}"), json!(s));
                }
                AttrValue::Enum(e) => {
                    p.insert(format!("@{name}"), json!(e));
                }
                _ => {}
            }
        }
        for name in LENGTH_ATTRIBUTES {
            if let AttrValue::Real(r) = entity.get(name) {
                p.insert(format!("@{name}"), json!(r * length_scale));
            }
        }
        for set in entity.refs("HasPropertySets") {
            owners
                .entry(set)
                .or_default()
                .push((*id, SetLink::TypeObject));
        }
    }

    // Owners of each set through IfcRelDefinesByProperties.
    let rels = ifc.entities_of_type("IfcRelDefinesByProperties")?;
    let rel_ids: Vec<StepId> = rels.iter().map(|r| r.id).collect();
    for (rel, e) in rel_ids.iter().zip(ifc.attributes_many(&rel_ids)?) {
        for set in e.refs("RelatingPropertyDefinition") {
            for obj in e.refs("RelatedObjects") {
                owners
                    .entry(set)
                    .or_default()
                    .push((obj, SetLink::Rel(*rel)));
            }
        }
    }
    // The sets and their properties, in two bulk reads.
    let set_ids: Vec<StepId> = owners.keys().copied().collect();
    let sets = ifc.attributes_many(&set_ids)?;
    let mut prop_ids = Vec::new();
    for e in &sets {
        prop_ids.extend(e.refs("HasProperties"));
        prop_ids.extend(e.refs("Quantities"));
    }
    let props: HashMap<StepId, Entity> = prop_ids
        .iter()
        .copied()
        .zip(ifc.attributes_many(&prop_ids)?)
        .collect();

    let wanted: HashSet<StepId> = ids.iter().copied().collect();
    let mut index = SetIndex::default();
    for (set_id, set) in set_ids.iter().zip(&sets) {
        let Some(name) = set.string("Name").map(str::to_owned) else {
            continue;
        };
        // Reviera's RXD carriers, and sets derived on save (marked `rxd:…`), are not parameters;
        // its published sets (ARC-GLZ-007) are.
        if reviera_rxd::is_carrier(&name) || set.string("Description").is_some_and(|d| d.starts_with("rxd:")) {
            continue;
        }
        let quantity = set.class.eq_ignore_ascii_case("IfcElementQuantity");
        let members = if quantity {
            set.refs("Quantities")
        } else {
            set.refs("HasProperties")
        };
        let mut values = BTreeMap::new();
        let mut prop_index = BTreeMap::new();
        for pid in members {
            let Some(p) = props.get(&pid) else { continue };
            let Some(pname) = p.string("Name") else {
                continue;
            };
            prop_index.insert(pname.to_owned(), pid);
            values.insert(pname.to_owned(), property_value(p));
        }
        let set_owners = &owners[set_id];
        let shared = set_owners.len() > 1;
        for (owner, link) in set_owners {
            if !wanted.contains(owner) {
                continue;
            }
            let p = params.entry(*owner).or_default();
            for (pname, v) in &values {
                p.insert(format!("{name}.{pname}"), v.clone());
            }
            index.by_owner.entry(*owner).or_default().insert(
                name.clone(),
                SetEntry {
                    id: *set_id,
                    quantity,
                    props: prop_index.clone(),
                    shared,
                    link: *link,
                },
            );
        }
    }

    // Type links.
    let type_rels = ifc.entities_of_type("IfcRelDefinesByType")?;
    let type_rel_ids: Vec<StepId> = type_rels.iter().map(|r| r.id).collect();
    for e in ifc.attributes_many(&type_rel_ids)? {
        let Some(t) = e.refs("RelatingType").first().and_then(|t| guid_of.get(t)) else {
            continue;
        };
        for obj in e.refs("RelatedObjects") {
            if wanted.contains(&obj) {
                params
                    .entry(obj)
                    .or_default()
                    .insert(TYPE_KEY.into(), json!(t.as_str()));
            }
        }
    }
    Ok((params, index))
}

/// The parameter value of one property or quantity.
fn property_value(p: &Entity) -> Value {
    let class = p.class.as_str();
    if let Some(q) = class.strip_prefix("IfcQuantity") {
        let value = p.get(&format!("{q}Value"));
        return json!({ "type": class, "value": scalar(value) });
    }
    match class {
        "IfcPropertySingleValue" => match p.get("NominalValue") {
            AttrValue::Typed { type_name, value } => {
                json!({ "type": type_name, "value": scalar(value) })
            }
            _ => json!({ "type": Value::Null, "value": Value::Null }),
        },
        "IfcPropertyEnumeratedValue" => {
            let values: Vec<Value> = match p.get("EnumerationValues") {
                AttrValue::List(items) => items
                    .iter()
                    .map(|i| match i {
                        AttrValue::Typed { value, .. } => scalar(value),
                        other => scalar(other),
                    })
                    .collect(),
                _ => Vec::new(),
            };
            json!({ "kind": class, "values": values })
        }
        other => json!({ "kind": other }),
    }
}

fn scalar(v: &AttrValue) -> Value {
    match v {
        AttrValue::String(s) | AttrValue::Enum(s) | AttrValue::Binary(s) => json!(s),
        AttrValue::Real(r) => json!(r),
        AttrValue::Int(i) => json!(i),
        AttrValue::Bool(b) => json!(b),
        AttrValue::Logical(l) => json!(l),
        AttrValue::Typed { value, .. } => scalar(value),
        _ => Value::Null,
    }
}

/// Whether a parameter value can be written back (single values only; quantities are computed).
pub fn is_writable(v: &Value) -> bool {
    v.get("kind").is_none()
        && !v
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|t| t.starts_with("IfcQuantity"))
}

/// The JSON category of an IFC defined type: "string", "integer", "boolean" or "number".
pub fn category(ifc_type: &str) -> &'static str {
    match ifc_type {
        "IfcLabel"
        | "IfcText"
        | "IfcIdentifier"
        | "IfcDescriptiveMeasure"
        | "IfcDate"
        | "IfcDateTime"
        | "IfcTime"
        | "IfcDuration"
        | "IfcURIReference" => "string",
        "IfcInteger" | "IfcCountMeasure" | "IfcPositiveInteger" | "IfcTimeStamp" => "integer",
        "IfcBoolean" | "IfcLogical" => "boolean",
        _ => "number",
    }
}

/// The IFC type inferred for a plain JSON value.
pub fn infer_type(v: &Value) -> Option<&'static str> {
    match v {
        Value::String(_) => Some("IfcLabel"),
        Value::Bool(_) => Some("IfcBoolean"),
        Value::Number(n) if n.is_i64() => Some("IfcInteger"),
        Value::Number(_) => Some("IfcReal"),
        _ => None,
    }
}

/// Splits a parameter key into (set, property); `None` for attribute keys.
pub fn split_key(key: &str) -> Option<(&str, &str)> {
    if key.starts_with('@') {
        return None;
    }
    key.split_once('.')
}

pub(crate) fn to_attr(ifc_type: &str, v: &Value) -> Result<AttrValue> {
    let inner = match v {
        Value::Null => return Ok(AttrValue::Null),
        Value::String(s) => AttrValue::String(s.clone()),
        Value::Bool(b) => AttrValue::Bool(*b),
        Value::Number(n) if category(ifc_type) == "integer" => AttrValue::Int(
            n.as_i64()
                .ok_or_else(|| unsupported(format!("{ifc_type} needs an integer")))?,
        ),
        Value::Number(n) => AttrValue::Real(n.as_f64().unwrap_or_default()),
        other => return Err(unsupported(format!("{other} is not a property value"))),
    };
    Ok(AttrValue::Typed {
        type_name: ifc_type.into(),
        value: Box::new(inner),
    })
}

/// Writes changed parameter values of one owner back to IFC.
pub(crate) struct ValueWriter<'a> {
    pub ifc: &'a mut IfcModel,
    pub index: &'a mut SetIndex,
    pub owner_history: Option<StepId>,
    /// Metres per file length unit.
    pub length_scale: f64,
}

impl ValueWriter<'_> {
    /// Applies the differences between `before` and `after` for the entity `owner`.
    pub fn apply(
        &mut self,
        owner: StepId,
        is_type: bool,
        before: &BTreeMap<String, Value>,
        after: &BTreeMap<String, Value>,
    ) -> Result<()> {
        let mut by_set: BTreeMap<String, Vec<(String, Option<Value>)>> = BTreeMap::new();
        let keys: std::collections::BTreeSet<&String> = before.keys().chain(after.keys()).collect();
        for key in keys {
            let (old, new) = (before.get(key), after.get(key));
            if old == new {
                continue;
            }
            if key.starts_with(RXD_PREFIX) {
                continue;
            }
            if key == TYPE_KEY {
                self.set_type(owner, new)?;
                continue;
            }
            if let Some(attr) = key.strip_prefix('@') {
                self.set_attribute(owner, attr, new)?;
                continue;
            }
            let Some((set, prop)) = self.locate(owner, key) else {
                return Err(unsupported(format!(
                    "`{key}` is not a property-set parameter"
                )));
            };
            if [old, new].into_iter().flatten().any(|v| !is_writable(v)) {
                return Err(unsupported(format!("`{key}` is computed or read-only")));
            }
            by_set.entry(set).or_default().push((prop, new.cloned()));
        }
        for (set, changes) in by_set {
            self.write_set(owner, is_type, &set, changes)?;
        }
        Ok(())
    }

    /// The (set, property) of a key: from the index when the property exists, else split at the
    /// first `.`.
    fn locate(&self, owner: StepId, key: &str) -> Option<(String, String)> {
        if let Some(sets) = self.index.by_owner.get(&owner) {
            for (name, entry) in sets {
                if let Some(prop) = key
                    .strip_prefix(name.as_str())
                    .and_then(|r| r.strip_prefix('.'))
                    && entry.props.contains_key(prop)
                {
                    return Some((name.clone(), prop.to_owned()));
                }
            }
        }
        split_key(key).map(|(s, p)| (s.to_owned(), p.to_owned()))
    }

    /// Moves `owner` into the `IfcRelDefinesByType` of the type with GlobalId `value` (or out of
    /// any type with `None`).
    fn set_type(&mut self, owner: StepId, value: Option<&Value>) -> Result<()> {
        let new_type = match value {
            None | Some(Value::Null) => None,
            Some(Value::String(g)) => Some(
                GlobalId::parse(g)
                    .and_then(|g| self.ifc.find_by_guid(&g))
                    .ok_or_else(|| unsupported(format!("type {g} is not in the file")))?,
            ),
            Some(other) => {
                return Err(unsupported(format!(
                    "`@type` must be a GlobalId, not {other}"
                )));
            }
        };
        for rel in self.ifc.referenced_by(owner, "IfcRelDefinesByType")? {
            let e = &self.ifc.attributes_many(&[rel])?[0];
            if !e.refs("RelatedObjects").contains(&owner) {
                continue;
            }
            if new_type.is_some_and(|t| e.refs("RelatingType") == [t]) {
                return Ok(());
            }
            let kept: Vec<_> = e
                .refs("RelatedObjects")
                .into_iter()
                .filter(|o| *o != owner)
                .map(|o| AttrValue::Ref(o.0))
                .collect();
            if kept.is_empty() {
                self.ifc.remove(rel)?;
            } else {
                self.ifc
                    .set_attribute(rel, "RelatedObjects", &AttrValue::List(kept))?;
            }
        }
        let Some(t) = new_type else { return Ok(()) };
        let existing = self
            .ifc
            .referenced_by(t, "IfcRelDefinesByType")?
            .into_iter()
            .find(|r| {
                self.ifc
                    .attribute(*r, "RelatingType")
                    .is_ok_and(|v| v == AttrValue::Ref(t.0))
            });
        let (rel, mut objects) = match existing {
            Some(rel) => (
                rel,
                self.ifc.attributes_many(&[rel])?[0].refs("RelatedObjects"),
            ),
            None => {
                let rel = self
                    .ifc
                    .create_rooted("IfcRelDefinesByType", &GlobalId::new())?;
                if let Some(oh) = self.owner_history {
                    self.ifc
                        .set_attribute(rel, "OwnerHistory", &AttrValue::Ref(oh.0))?;
                }
                self.ifc
                    .set_attribute(rel, "RelatingType", &AttrValue::Ref(t.0))?;
                (rel, Vec::new())
            }
        };
        objects.push(owner);
        let list = AttrValue::List(objects.into_iter().map(|o| AttrValue::Ref(o.0)).collect());
        Ok(self.ifc.set_attribute(rel, "RelatedObjects", &list)?)
    }

    fn set_attribute(&mut self, owner: StepId, attr: &str, value: Option<&Value>) -> Result<()> {
        if LENGTH_ATTRIBUTES.contains(&attr) {
            let v = match value {
                None | Some(Value::Null) => AttrValue::Null,
                Some(Value::Number(n)) => {
                    AttrValue::Real(n.as_f64().unwrap_or_default() / self.length_scale)
                }
                Some(other) => {
                    return Err(unsupported(format!(
                        "`@{attr}` must be a length, not {other}"
                    )));
                }
            };
            return Ok(self.ifc.set_attribute(owner, attr, &v)?);
        }
        if !ATTRIBUTES.contains(&attr) {
            return Err(unsupported(format!(
                "`@{attr}` is not a writable attribute"
            )));
        }
        let v = match value {
            None | Some(Value::Null) => AttrValue::Null,
            Some(Value::String(s)) if ENUM_ATTRIBUTES.contains(&attr) => AttrValue::Enum(s.clone()),
            Some(Value::String(s)) => AttrValue::String(s.clone()),
            Some(Value::Bool(b)) => AttrValue::Bool(*b),
            Some(other) => return Err(unsupported(format!("`@{attr}` must be text, not {other}"))),
        };
        Ok(self.ifc.set_attribute(owner, attr, &v)?)
    }

    fn create_prop(&mut self, name: &str, value: &Value) -> Result<StepId> {
        let ifc_type = value
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| unsupported(format!("`{name}` has no IFC type")))?;
        let p = self.ifc.create("IfcPropertySingleValue")?;
        self.ifc
            .set_attribute(p, "Name", &AttrValue::String(name.into()))?;
        let v = to_attr(ifc_type, value.get("value").unwrap_or(&Value::Null))?;
        self.ifc.set_attribute(p, "NominalValue", &v)?;
        Ok(p)
    }

    fn write_set(
        &mut self,
        owner: StepId,
        is_type: bool,
        set: &str,
        changes: Vec<(String, Option<Value>)>,
    ) -> Result<()> {
        let existing = self
            .index
            .by_owner
            .get(&owner)
            .and_then(|s| s.get(set))
            .cloned();
        if existing.as_ref().is_some_and(|e| e.quantity) {
            return Err(unsupported(format!(
                "`{set}` is a quantity set; quantities are computed"
            )));
        }
        let shared = existing.as_ref().is_some_and(|e| e.shared);
        let mut props: BTreeMap<String, StepId> = existing
            .as_ref()
            .map(|e| e.props.clone())
            .unwrap_or_default();
        let mut dropped = Vec::new();
        for (name, value) in &changes {
            let old = props.get(name).copied();
            match (value, old) {
                (None, Some(old)) => {
                    props.remove(name);
                    dropped.push(old);
                }
                (None, None) => {}
                // Update in place when only this set uses the property.
                (Some(v), Some(old))
                    if !shared && self.ifc.referenced_by(old, "IfcPropertySet")?.len() == 1 =>
                {
                    let ifc_type = v.get("type").and_then(Value::as_str).unwrap_or("IfcLabel");
                    let value = to_attr(ifc_type, v.get("value").unwrap_or(&Value::Null))?;
                    self.ifc.set_attribute(old, "NominalValue", &value)?;
                }
                (Some(v), _) => {
                    let id = self.create_prop(name, v)?;
                    props.insert(name.clone(), id);
                }
            }
        }
        let list = AttrValue::List(props.values().map(|i| AttrValue::Ref(i.0)).collect());
        let entry = match existing {
            // A set with no values is not written.
            Some(e) if props.is_empty() => {
                detach(self.ifc, &e, owner)?;
                if !e.shared {
                    self.ifc.remove(e.id)?;
                }
                self.index.by_owner.get_mut(&owner).map(|s| s.remove(set));
                None
            }
            None if props.is_empty() => None,
            Some(e) if !e.shared => {
                self.ifc.set_attribute(e.id, "HasProperties", &list)?;
                Some(SetEntry { props, ..e })
            }
            // Shared with other objects: give this owner its own copy.
            other => {
                if let Some(e) = &other {
                    detach(self.ifc, e, owner)?;
                }
                let mut w = crate::psets::PsetWriter {
                    ifc: self.ifc,
                    owner_history: self.owner_history,
                };
                let id = w.create_with(set, owner, is_type, &list)?;
                Some(SetEntry {
                    id,
                    quantity: false,
                    props,
                    shared: false,
                    link: link_of(self.ifc, id, is_type)?,
                })
            }
        };
        if let Some(entry) = entry {
            self.index
                .by_owner
                .entry(owner)
                .or_default()
                .insert(set.to_owned(), entry);
        }
        for old in dropped {
            if self.ifc.referenced_by(old, "IfcPropertySet")?.is_empty() {
                self.ifc.remove(old)?;
            }
        }
        Ok(())
    }
}

/// Removes every property and quantity set of `owner` (when the element is deleted). Shared sets
/// only lose the owner.
pub(crate) fn remove_all(ifc: &mut IfcModel, index: &mut SetIndex, owner: StepId) -> Result<()> {
    for entry in index
        .by_owner
        .remove(&owner)
        .unwrap_or_default()
        .into_values()
    {
        detach(ifc, &entry, owner)?;
        if entry.shared {
            continue;
        }
        ifc.remove(entry.id)?;
        let holder = if entry.quantity {
            "IfcElementQuantity"
        } else {
            "IfcPropertySet"
        };
        for prop in entry.props.into_values() {
            if ifc.referenced_by(prop, holder)?.is_empty() {
                ifc.remove(prop)?;
            }
        }
    }
    Ok(())
}

fn link_of(ifc: &IfcModel, set: StepId, is_type: bool) -> Result<SetLink> {
    if is_type {
        return Ok(SetLink::TypeObject);
    }
    let rel = ifc
        .referenced_by(set, "IfcRelDefinesByProperties")?
        .first()
        .copied();
    Ok(rel.map_or(SetLink::TypeObject, SetLink::Rel))
}

/// Removes `owner` from a set's relationship (or a type's set list); deletes a relationship left
/// with no objects.
fn detach(ifc: &mut IfcModel, e: &SetEntry, owner: StepId) -> Result<()> {
    let (holder, attr, remove) = match e.link {
        SetLink::Rel(rel) => (rel, "RelatedObjects", owner.0),
        SetLink::TypeObject => (owner, "HasPropertySets", e.id.0),
    };
    if let AttrValue::List(items) = ifc.attribute(holder, attr)? {
        let kept: Vec<_> = items
            .into_iter()
            .filter(|i| *i != AttrValue::Ref(remove))
            .collect();
        if kept.is_empty() && matches!(e.link, SetLink::Rel(_)) {
            ifc.remove(holder)?;
        } else {
            let v = if kept.is_empty() {
                AttrValue::Null
            } else {
                AttrValue::List(kept)
            };
            ifc.set_attribute(holder, attr, &v)?;
        }
    }
    Ok(())
}
