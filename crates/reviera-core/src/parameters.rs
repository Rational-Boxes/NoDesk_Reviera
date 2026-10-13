// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Parameter definitions, bindings and values (spec Ch 2 §2.9.1–§2.9.3, DM-PARAM-001…003).
//!
//! Values live in element parameters under their IFC home (§2.3.3): `@Tag` for an attribute,
//! `<Pset>.<Property>` for a property, quantities for computed values. Definitions say what a value
//! means: built-in (this registry), project or shared (stored as the RXD document
//! `standards/parameters`), or IFC-native (a property no definition covers). A parameter is either
//! a type value, stored on the type, or an instance value, never both; when a third-party file has
//! the same property on an occurrence, the occurrence value wins and is reported as an override.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::categories::{self, Category, category_of, is_type};
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;
use crate::units::{self, Dim};

pub const PARAMETERS_CLASS: &str = "rxd:standards/parameters";
const PARAMETERS_KEY: &str = "parameters";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    BuiltIn,
    Project,
    Shared,
    IfcNative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Binding {
    Type,
    Instance,
}

/// Where a value is stored in IFC (§2.3.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IfcHome {
    Attribute {
        name: String,
    },
    Property {
        pset: String,
        property: String,
    },
    /// Read from the element's quantity sets (computed, read-only).
    Quantity {
        quantity: String,
    },
}

impl IfcHome {
    fn key(&self) -> Option<String> {
        match self {
            IfcHome::Attribute { name } => Some(format!("@{name}")),
            IfcHome::Property { pset, property } => Some(format!("{pset}.{property}")),
            IfcHome::Quantity { .. } => None,
        }
    }
}

/// A definition as reported by `param.list` and `param.definition`.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Definition {
    /// Built-in key, project/shared parameter GUID, or `<Pset>.<Property>` for IFC-native ones.
    #[serde(rename = "ref")]
    pub reference: String,
    pub name: String,
    pub kind: Kind,
    pub data_type: String,
    pub group: String,
    pub binding: Binding,
    pub read_only: bool,
    /// For the element's (or the first bound) category.
    pub ifc_home: Option<IfcHome>,
    pub categories: Vec<String>,
    pub tooltip: Option<String>,
}

// ---- Built-in registry (§2.9.2) ----

#[derive(Debug, Clone, Copy)]
enum Home {
    Attr(&'static str),
    /// A property of the category's `Pset_<Class>Common`.
    Common(&'static str),
    Pset(&'static str, &'static str),
    /// A quantity, chosen per category.
    Quantity(fn(&str) -> Option<&'static str>),
}

#[derive(Debug, Clone, Copy)]
enum Applies {
    Model,
    WithCommonPset,
    Cats(&'static [&'static str]),
}

#[derive(Debug)]
struct BuiltIn {
    key: &'static str,
    name: &'static str,
    data_type: &'static str,
    group: &'static str,
    binding: Binding,
    read_only: bool,
    home: Home,
    applies: Applies,
}

fn area_quantity(cat: &str) -> Option<&'static str> {
    Some(match cat {
        "cat.walls" | "cat.curtain_walls" => "NetSideArea",
        "cat.floors" | "cat.roofs" | "cat.ceilings" => "NetArea",
        "cat.doors" | "cat.windows" => "Area",
        "cat.rooms" | "cat.areas" => "NetFloorArea",
        _ => return None,
    })
}

const AREA_CATS: &[&str] = &[
    "cat.walls",
    "cat.curtain_walls",
    "cat.floors",
    "cat.roofs",
    "cat.ceilings",
    "cat.doors",
    "cat.windows",
    "cat.rooms",
    "cat.areas",
];
const VOLUME_CATS: &[&str] = &[
    "cat.walls",
    "cat.floors",
    "cat.roofs",
    "cat.columns",
    "cat.structural_columns",
    "cat.structural_framing",
    "cat.rooms",
    "cat.structural_foundations",
];
const LENGTH_CATS: &[&str] = &[
    "cat.walls",
    "cat.structural_framing",
    "cat.columns",
    "cat.structural_columns",
    "cat.curtain_mullions",
    "cat.railings",
];

// One argument per column of the built-in table below.
#[allow(clippy::too_many_arguments)]
const fn bi(
    key: &'static str,
    name: &'static str,
    data_type: &'static str,
    group: &'static str,
    binding: Binding,
    read_only: bool,
    home: Home,
    applies: Applies,
) -> BuiltIn {
    BuiltIn {
        key,
        name,
        data_type,
        group,
        binding,
        read_only,
        home,
        applies,
    }
}

use Binding::{Instance, Type};

static BUILT_INS: &[BuiltIn] = &[
    bi(
        "elem.mark",
        "Mark",
        "text",
        "identity_data",
        Instance,
        false,
        Home::Attr("Tag"),
        Applies::Model,
    ),
    bi(
        "elem.comments",
        "Comments",
        "text",
        "identity_data",
        Instance,
        false,
        Home::Pset("Identity Data", "Comments"),
        Applies::Model,
    ),
    bi(
        "type.type_mark",
        "Type Mark",
        "text",
        "identity_data",
        Type,
        false,
        Home::Attr("Tag"),
        Applies::Model,
    ),
    bi(
        "type.description",
        "Description",
        "text",
        "identity_data",
        Type,
        false,
        Home::Attr("Description"),
        Applies::Model,
    ),
    bi(
        "type.fire_rating",
        "Fire Rating",
        "text",
        "identity_data",
        Type,
        false,
        Home::Common("FireRating"),
        Applies::WithCommonPset,
    ),
    bi(
        "type.acoustic_rating",
        "Acoustic Rating",
        "text",
        "identity_data",
        Type,
        false,
        Home::Common("AcousticRating"),
        Applies::WithCommonPset,
    ),
    bi(
        "type.manufacturer",
        "Manufacturer",
        "text",
        "identity_data",
        Type,
        false,
        Home::Pset("Pset_ManufacturerTypeInformation", "Manufacturer"),
        Applies::Model,
    ),
    bi(
        "type.model",
        "Model",
        "text",
        "identity_data",
        Type,
        false,
        Home::Pset("Pset_ManufacturerTypeInformation", "ModelLabel"),
        Applies::Model,
    ),
    bi(
        "type.cost",
        "Cost",
        "currency",
        "identity_data",
        Type,
        false,
        Home::Pset("Identity Data", "Cost"),
        Applies::Model,
    ),
    bi(
        "type.keynote",
        "Keynote",
        "text",
        "identity_data",
        Type,
        false,
        Home::Pset("Identity Data", "Keynote"),
        Applies::Model,
    ),
    bi(
        "type.type_comments",
        "Type Comments",
        "text",
        "identity_data",
        Type,
        false,
        Home::Pset("Identity Data", "Type Comments"),
        Applies::Model,
    ),
    bi(
        "type.url",
        "URL",
        "url",
        "identity_data",
        Type,
        false,
        Home::Pset("Identity Data", "URL"),
        Applies::Model,
    ),
    bi(
        "elem.area",
        "Area",
        "area",
        "dimensions",
        Instance,
        true,
        Home::Quantity(area_quantity),
        Applies::Cats(AREA_CATS),
    ),
    bi(
        "elem.volume",
        "Volume",
        "volume",
        "dimensions",
        Instance,
        true,
        Home::Quantity(|_| Some("NetVolume")),
        Applies::Cats(VOLUME_CATS),
    ),
    bi(
        "elem.length",
        "Length",
        "length",
        "dimensions",
        Instance,
        true,
        Home::Quantity(|_| Some("Length")),
        Applies::Cats(LENGTH_CATS),
    ),
];

impl BuiltIn {
    fn applies_to(&self, cat: &Category) -> bool {
        match self.applies {
            Applies::Model => cat.is_model,
            Applies::WithCommonPset => cat.common_pset.is_some(),
            Applies::Cats(list) => list.contains(&cat.key),
        }
    }

    fn home(&self, cat: Option<&Category>) -> Option<IfcHome> {
        Some(match self.home {
            Home::Attr(n) => IfcHome::Attribute { name: n.into() },
            Home::Common(p) => IfcHome::Property {
                pset: cat?.common_pset?.into(),
                property: p.into(),
            },
            Home::Pset(s, p) => IfcHome::Property {
                pset: s.into(),
                property: p.into(),
            },
            Home::Quantity(f) => IfcHome::Quantity {
                quantity: f(cat?.key)?.into(),
            },
        })
    }

    fn definition(&self, cat: Option<&Category>) -> Definition {
        let cats = categories::CATEGORIES
            .iter()
            .filter(|c| self.applies_to(c))
            .map(|c| c.key.to_owned())
            .collect();
        let cat = cat.or_else(|| categories::CATEGORIES.iter().find(|c| self.applies_to(c)));
        Definition {
            reference: self.key.into(),
            name: self.name.into(),
            kind: Kind::BuiltIn,
            data_type: self.data_type.into(),
            group: self.group.into(),
            binding: self.binding,
            read_only: self.read_only,
            ifc_home: self.home(cat),
            categories: cats,
            tooltip: None,
        }
    }
}

// ---- Project and shared parameters (§2.9.3) ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GroupBehavior {
    #[default]
    Aligned,
    Vary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PropertyHome {
    pub pset: String,
    pub property: String,
}

/// A project or shared parameter with its binding (one binding per definition).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProjectParameter {
    pub guid: String,
    pub name: String,
    pub data_type: String,
    pub group: String,
    pub tooltip: Option<String>,
    /// From a parameter library (identity is the GUID).
    pub shared: bool,
    pub categories: Vec<String>,
    pub instance: bool,
    #[serde(default)]
    pub group_behavior: GroupBehavior,
    /// Property set for the value; the property is named after the parameter.
    pub pset_name: String,
    /// Per-category IFC homes that override `pset_name` (for example `Pset_WallCommon.FireRating`).
    #[serde(default)]
    pub homes: BTreeMap<String, PropertyHome>,
}

impl ProjectParameter {
    fn home(&self, cat: &str) -> PropertyHome {
        self.homes
            .get(cat)
            .cloned()
            .unwrap_or_else(|| PropertyHome {
                pset: self.pset_name.clone(),
                property: self.name.clone(),
            })
    }

    fn key(&self, cat: &str) -> String {
        let h = self.home(cat);
        format!("{}.{}", h.pset, h.property)
    }

    fn definition(&self, cat: Option<&str>) -> Definition {
        let cat = cat.or_else(|| self.categories.first().map(String::as_str));
        Definition {
            reference: self.guid.clone(),
            name: self.name.clone(),
            kind: if self.shared {
                Kind::Shared
            } else {
                Kind::Project
            },
            data_type: self.data_type.clone(),
            group: self.group.clone(),
            binding: if self.instance {
                Binding::Instance
            } else {
                Binding::Type
            },
            read_only: false,
            ifc_home: cat.map(|c| {
                let h = self.home(c);
                IfcHome::Property {
                    pset: h.pset,
                    property: h.property,
                }
            }),
            categories: self.categories.clone(),
            tooltip: self.tooltip.clone(),
        }
    }
}

pub fn project_parameters(store: &Store) -> Vec<ProjectParameter> {
    store
        .get(ElementId::PARAMETERS)
        .and_then(|e| e.param(PARAMETERS_KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

fn write_project_parameters(txn: &mut Transaction<'_>, list: &[ProjectParameter]) -> Result<()> {
    let value = serde_json::to_value(list).expect("parameters serialise");
    if txn.store().contains(ElementId::PARAMETERS) {
        txn.modify(ElementId::PARAMETERS, |e| {
            e.params.insert(PARAMETERS_KEY.into(), value);
        })
    } else {
        let init = NewElement::new(PARAMETERS_CLASS)
            .name("Project Parameters")
            .param(PARAMETERS_KEY, value);
        txn.create_with_id(ElementId::PARAMETERS, init)
    }
}

// ---- Resolution ----

#[derive(Debug, Clone)]
enum Def {
    BuiltIn(&'static BuiltIn),
    Project(ProjectParameter),
    Native { pset: String, property: String },
}

impl Def {
    fn binding(&self) -> Binding {
        match self {
            Def::BuiltIn(b) => b.binding,
            Def::Project(p) if p.instance => Binding::Instance,
            Def::Project(_) => Binding::Type,
            Def::Native { .. } => Binding::Instance,
        }
    }

    fn read_only(&self) -> bool {
        matches!(self, Def::BuiltIn(b) if b.read_only)
    }

    fn applies_to(&self, cat: Option<&Category>) -> bool {
        match (self, cat) {
            (Def::BuiltIn(b), Some(c)) => b.applies_to(c),
            (Def::BuiltIn(_), None) => false,
            (Def::Project(p), Some(c)) => p.categories.iter().any(|k| k == c.key),
            (Def::Project(_), None) => false,
            (Def::Native { .. }, _) => true,
        }
    }

    fn home(&self, cat: Option<&Category>) -> Option<IfcHome> {
        match self {
            Def::BuiltIn(b) => b.home(cat),
            Def::Project(p) => cat.map(|c| {
                let h = p.home(c.key);
                IfcHome::Property {
                    pset: h.pset,
                    property: h.property,
                }
            }),
            Def::Native { pset, property } => Some(IfcHome::Property {
                pset: pset.clone(),
                property: property.clone(),
            }),
        }
    }

    fn data_type(&self, existing: Option<&Value>) -> String {
        match self {
            Def::BuiltIn(b) => b.data_type.into(),
            Def::Project(p) => p.data_type.clone(),
            Def::Native { .. } => existing
                .and_then(|v| v.get("type"))
                .and_then(Value::as_str)
                .map(|t| match units::data_type_for_ifc(t) {
                    Some(dt) => dt.key.to_owned(),
                    None => match t {
                        "IfcBoolean" | "IfcLogical" => "yes_no".into(),
                        "IfcInteger" | "IfcCountMeasure" => "integer".into(),
                        "IfcReal" => "number".into(),
                        "IfcText" => "multiline_text".into(),
                        _ => "text".into(),
                    },
                })
                .unwrap_or_else(|| "text".into()),
        }
    }

    fn definition(&self, cat: Option<&Category>) -> Definition {
        match self {
            Def::BuiltIn(b) => b.definition(cat),
            Def::Project(p) => p.definition(cat.map(|c| c.key)),
            Def::Native { pset, property } => Definition {
                reference: format!("{pset}.{property}"),
                name: property.clone(),
                kind: Kind::IfcNative,
                data_type: "text".into(),
                group: pset.clone(),
                binding: Binding::Instance,
                read_only: false,
                ifc_home: Some(IfcHome::Property {
                    pset: pset.clone(),
                    property: property.clone(),
                }),
                categories: Vec::new(),
                tooltip: None,
            },
        }
    }
}

/// Finds a definition: a built-in key, a project/shared GUID or name (preferring one bound to
/// `cat`), or `<Pset>.<Property>` for an IFC-native parameter.
fn find(store: &Store, reference: &str, cat: Option<&Category>) -> Result<Def> {
    if let Some(b) = BUILT_INS.iter().find(|b| b.key == reference) {
        return Ok(Def::BuiltIn(b));
    }
    let project = project_parameters(store);
    if let Some(p) = project.iter().find(|p| p.guid == reference) {
        return Ok(Def::Project(p.clone()));
    }
    let named: Vec<_> = project.iter().filter(|p| p.name == reference).collect();
    if let Some(p) = named
        .iter()
        .find(|p| cat.is_some_and(|c| p.categories.iter().any(|k| k == c.key)))
        .or(named.first())
    {
        return Ok(Def::Project((*p).clone()));
    }
    if let Some(b) = BUILT_INS
        .iter()
        .find(|b| b.name == reference && cat.is_none_or(|c| b.applies_to(c)))
    {
        return Ok(Def::BuiltIn(b));
    }
    if let Some((pset, property)) = reference.split_once('.')
        && !reference.starts_with('@')
    {
        return Ok(Def::Native {
            pset: pset.into(),
            property: property.into(),
        });
    }
    Err(Error::new(
        "param.not_found",
        format!("no parameter `{reference}`"),
    ))
}

fn type_of(store: &Store, e: &Element) -> Option<ElementId> {
    let guid = GlobalId::parse(e.param("@type")?.as_str()?)?;
    store.find_by_guid(&guid)
}

/// Where a parameter's value lives for one element.
struct Target {
    element: ElementId,
    key: Option<String>,
    home: Option<IfcHome>,
    source: &'static str,
}

fn target(store: &Store, def: &Def, e: &Element) -> Result<Target> {
    let cat = category_of(e);
    if !def.applies_to(cat) {
        return Err(Error::new(
            "param.not_applicable",
            format!(
                "this parameter does not apply to {}",
                cat.map_or("uncategorised elements", |c| c.name)
            ),
        ));
    }
    let home = def.home(cat);
    let key = home.as_ref().and_then(IfcHome::key);
    if home.is_none() {
        return Err(Error::new(
            "param.no_ifc_home",
            "this parameter has no storage for the element's category",
        ));
    }
    let (element, source) = match def.binding() {
        Binding::Instance if is_type(e) => {
            return Err(Error::new(
                "param.instance_on_type",
                "an instance parameter cannot be set on a type",
            ));
        }
        Binding::Instance => (e.id(), "instance"),
        Binding::Type if is_type(e) => (e.id(), "type"),
        // An occurrence that already carries the property overrides its type (third-party files).
        Binding::Type if key.as_ref().is_some_and(|k| e.params.contains_key(k)) => {
            (e.id(), "override")
        }
        Binding::Type => match type_of(store, e) {
            Some(t) => (t, "type"),
            None => {
                return Err(Error::new(
                    "param.no_type",
                    "the element has no type to hold a type parameter",
                ));
            }
        },
    };
    Ok(Target {
        element,
        key,
        home,
        source,
    })
}

/// A parameter value as returned by `param.get`.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ParamValue {
    #[serde(rename = "ref")]
    pub reference: String,
    pub name: String,
    /// Plain value (SI for measures), or null.
    pub value: Value,
    /// Formatted in project units.
    pub display: Option<String>,
    /// `instance`, `type`, `override` (an occurrence value masking its type) or `computed`.
    pub source: String,
    pub read_only: bool,
}

pub fn get(store: &Store, element: ElementId, reference: &str) -> Result<ParamValue> {
    let e = store.get(element).ok_or_else(|| {
        Error::new(
            "element.not_found",
            format!("element {element} does not exist"),
        )
    })?;
    let def = find(store, reference, category_of(e))?;
    let t = target(store, &def, e)?;
    let holder = store.get(t.element).expect("targets exist");
    let (raw, source) = match &t.home {
        Some(IfcHome::Quantity { quantity }) => {
            let suffix = format!(".{quantity}");
            let q = holder.params.iter().find(|(k, v)| {
                k.ends_with(&suffix)
                    && v.get("type")
                        .and_then(Value::as_str)
                        .is_some_and(|t| t.starts_with("IfcQuantity"))
            });
            (q.map(|(_, v)| v.clone()), "computed")
        }
        _ => (
            t.key.as_ref().and_then(|k| holder.param(k)).cloned(),
            t.source,
        ),
    };
    let value = match &raw {
        Some(Value::Object(o)) => o.get("value").cloned().unwrap_or(Value::Null),
        Some(other) => other.clone(),
        None => Value::Null,
    };
    let data_type = def.data_type(raw.as_ref());
    let display = match (&value, units::data_type(&data_type)) {
        (Value::Number(n), Some(dt)) if dt.dim != Dim::None => units::format(
            n.as_f64().unwrap_or_default(),
            dt.key,
            &units::settings_of(store),
            None,
        )
        .ok(),
        (Value::Null, _) => None,
        (Value::String(s), _) => Some(s.clone()),
        (other, _) => Some(other.to_string()),
    };
    let d = def.definition(category_of(e));
    Ok(ParamValue {
        reference: d.reference,
        name: d.name,
        value,
        display,
        source: source.into(),
        read_only: def.read_only(),
    })
}

/// Converts an input value to the stored form for a data type; measures accept unit strings.
fn convert(store: &Store, data_type: &str, value: &Value) -> Result<Value> {
    let bad = || {
        Error::new(
            "param.invalid_value",
            format!("{value} is not a valid `{data_type}` value"),
        )
    };
    let dt = units::data_type(data_type).ok_or_else(|| {
        Error::new(
            "param.unsupported_type",
            format!("`{data_type}` values cannot be set yet"),
        )
    })?;
    if value.is_null() {
        return Ok(Value::Null);
    }
    Ok(match (dt.dim, data_type) {
        (Dim::None, "integer") => json!(value.as_i64().ok_or_else(bad)?),
        (Dim::None, "number") => json!(value.as_f64().ok_or_else(bad)?),
        (Dim::None, "yes_no") => json!(value.as_bool().ok_or_else(bad)?),
        (Dim::None, _) => json!(value.as_str().ok_or_else(bad)?),
        (_, key) => match value {
            Value::Number(n) => json!(n.as_f64().ok_or_else(bad)?),
            Value::String(s) => json!(units::parse(s, key, &units::settings_of(store))?),
            _ => return Err(bad()),
        },
    })
}

/// Sets (or, with `None`, clears) a parameter on elements; type values go to the type once.
/// Returns the elements written.
pub fn set(
    txn: &mut Transaction<'_>,
    elements: &[ElementId],
    reference: &str,
    value: Option<&Value>,
) -> Result<Vec<ElementId>> {
    let mut written = BTreeSet::new();
    for id in elements {
        let store = txn.store().clone();
        let e = store.get(*id).ok_or_else(|| {
            Error::new("element.not_found", format!("element {id} does not exist"))
        })?;
        let def = find(&store, reference, category_of(e))?;
        if def.read_only() {
            return Err(Error::new(
                "param.read_only",
                format!("`{reference}` is computed and cannot be set"),
            ));
        }
        let t = target(&store, &def, e)?;
        if !written.insert(t.element) {
            continue;
        }
        let key = t.key.expect("writable parameters have a key");
        let holder = store.get(t.element).expect("targets exist");
        let stored = match value {
            None => None,
            Some(v) => {
                let data_type = def.data_type(holder.param(&key));
                let plain = convert(&store, &data_type, v)?;
                Some(if key.starts_with('@') {
                    if !plain.is_string() && !plain.is_null() {
                        return Err(Error::new("param.invalid_value", "this parameter is text"));
                    }
                    plain
                } else {
                    // Keep the property's IFC type when it exists, else the data type's.
                    let ifc_type = holder
                        .param(&key)
                        .and_then(|v| v.get("type"))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .unwrap_or_else(|| {
                            units::data_type(&data_type)
                                .map_or("IfcLabel", |d| d.ifc_type)
                                .to_owned()
                        });
                    json!({ "type": ifc_type, "value": plain })
                })
            }
        };
        txn.modify(t.element, |el| match stored {
            Some(v) => {
                el.params.insert(key, v);
            }
            None => {
                el.params.remove(&key);
            }
        })?;
    }
    Ok(written.into_iter().collect())
}

/// Definitions for a category or an element (`param.list`). For an element, IFC-native
/// properties that no definition covers are listed too.
pub fn list(
    store: &Store,
    category: Option<&str>,
    element: Option<ElementId>,
) -> Result<Vec<Definition>> {
    let e = match element {
        Some(id) => Some(store.get(id).ok_or_else(|| {
            Error::new("element.not_found", format!("element {id} does not exist"))
        })?),
        None => None,
    };
    let cat =
        match (category, e) {
            (Some(k), _) => Some(categories::category(k).ok_or_else(|| {
                Error::new("param.unknown_category", format!("no category `{k}`"))
            })?),
            (None, Some(e)) => category_of(e),
            (None, None) => None,
        };
    let mut defs: Vec<Def> = BUILT_INS.iter().map(Def::BuiltIn).collect();
    defs.extend(project_parameters(store).into_iter().map(Def::Project));
    let mut out: Vec<Definition> = defs
        .iter()
        .filter(|d| cat.is_none() || d.applies_to(cat))
        .map(|d| d.definition(cat))
        .collect();
    if let Some(e) = e {
        let covered: BTreeSet<String> = out
            .iter()
            .filter_map(|d| d.ifc_home.as_ref().and_then(IfcHome::key))
            .collect();
        for (k, v) in &e.params {
            let quantity = v
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|t| t.starts_with("IfcQuantity"));
            if k.starts_with('@') || covered.contains(k) || quantity {
                continue;
            }
            if let Some((pset, property)) = k.split_once('.') {
                let def = Def::Native {
                    pset: pset.into(),
                    property: property.into(),
                };
                let mut d = def.definition(cat);
                d.data_type = def.data_type(Some(v));
                out.push(d);
            }
        }
    }
    Ok(out)
}

pub fn definition(store: &Store, reference: &str) -> Result<Definition> {
    Ok(find(store, reference, None)?.definition(None))
}

// ---- Managing project parameters ----

/// The `def` argument of `param.project.add`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NewDefinition {
    pub name: String,
    pub data_type: String,
    #[serde(default = "default_group")]
    pub group: String,
    pub tooltip: Option<String>,
    /// A shared parameter's GUID from its parameter library.
    pub guid: Option<String>,
}

fn default_group() -> String {
    "other".into()
}

const RESERVED_CHARS: &[char] = &['[', ']', '{', '}', '|', ';', '<', '>'];

fn validate_name(store: &Store, name: &str, cats: &[String], except: Option<&str>) -> Result<()> {
    if name.trim().is_empty() || name.len() > 255 || name.contains(RESERVED_CHARS) {
        return Err(Error::new(
            "param.invalid_name",
            "a name is 1–255 characters without [ ] { } | ; < >",
        ));
    }
    for c in cats {
        let cat = categories::category(c)
            .ok_or_else(|| Error::new("param.unknown_category", format!("no category `{c}`")))?;
        if let Some(b) = BUILT_INS
            .iter()
            .find(|b| b.name.eq_ignore_ascii_case(name) && b.applies_to(cat))
        {
            return Err(Error::new(
                "param.name_clash",
                format!(
                    "`{name}` clashes with the built-in parameter {} ({}) of {}",
                    b.name, b.key, cat.name
                ),
            ));
        }
        if let Some(p) = project_parameters(store).iter().find(|p| {
            Some(p.guid.as_str()) != except
                && p.name.eq_ignore_ascii_case(name)
                && p.categories.contains(c)
        }) {
            return Err(Error::new(
                "param.name_clash",
                format!(
                    "`{name}` is already a parameter of {} ({})",
                    cat.name, p.guid
                ),
            ));
        }
    }
    Ok(())
}

fn validate_pset_name(name: &str) -> Result<()> {
    if name.starts_with("Pset_") || name.starts_with("Qto_") || name.starts_with("Reviera_") {
        return Err(Error::new(
            "param.reserved_pset",
            "a parameter's own property set cannot be a standard (Pset_, Qto_) or Reviera_ set; use param.set_ifc_home to map onto a standard property",
        ));
    }
    if name.is_empty() || name.contains('.') {
        return Err(Error::new(
            "param.invalid_pset",
            "property set names must be non-empty and without `.`",
        ));
    }
    Ok(())
}

pub fn project_add(
    txn: &mut Transaction<'_>,
    def: NewDefinition,
    categories: Vec<String>,
    is_instance: bool,
    group_behavior: GroupBehavior,
    pset_name: Option<String>,
) -> Result<String> {
    let store = txn.store().clone();
    if categories.is_empty() {
        return Err(Error::new(
            "param.no_categories",
            "choose at least one category",
        ));
    }
    validate_name(&store, &def.name, &categories, None)?;
    let dt = units::data_type(&def.data_type).ok_or_else(|| {
        Error::new(
            "param.unsupported_type",
            format!("no data type `{}`", def.data_type),
        )
    })?;
    let pset_name = pset_name.unwrap_or_else(|| {
        let project = store
            .get(ElementId::PROJECT)
            .and_then(|p| p.name.clone())
            .unwrap_or_else(|| "Project".into());
        format!("{project} Parameters")
    });
    validate_pset_name(&pset_name)?;
    let shared = def.guid.is_some();
    let guid = match def.guid {
        Some(g) => uuid::Uuid::parse_str(&g)
            .map_err(|_| Error::new("param.invalid_guid", "a shared parameter GUID is a UUID"))?
            .to_string(),
        None => uuid::Uuid::new_v4().to_string(),
    };
    let mut list = project_parameters(&store);
    if list.iter().any(|p| p.guid == guid) {
        return Err(Error::new(
            "param.duplicate",
            "this shared parameter is already in the project",
        ));
    }
    list.push(ProjectParameter {
        guid: guid.clone(),
        name: def.name,
        data_type: dt.key.into(),
        group: def.group,
        tooltip: def.tooltip,
        shared,
        categories,
        instance: is_instance,
        group_behavior,
        pset_name,
        homes: BTreeMap::new(),
    });
    write_project_parameters(txn, &list)?;
    Ok(guid)
}

/// The elements of the given categories, instances and types alike.
fn elements_in(store: &Store, cats: &[String]) -> Vec<ElementId> {
    store
        .iter()
        .filter(|e| category_of(e).is_some_and(|c| cats.iter().any(|k| k == c.key)))
        .map(Element::id)
        .collect()
}

/// Moves values from each element's old key to its new key.
fn move_values(
    txn: &mut Transaction<'_>,
    old: &ProjectParameter,
    new: &ProjectParameter,
) -> Result<usize> {
    let store = txn.store().clone();
    let mut moved = 0;
    for id in elements_in(&store, &old.categories) {
        let e = store.get(id).expect("listed");
        let Some(cat) = category_of(e) else { continue };
        let (from, to) = (old.key(cat.key), new.key(cat.key));
        if from == to {
            continue;
        }
        if let Some(v) = e.param(&from).cloned() {
            txn.modify(id, |el| {
                el.params.remove(&from);
                el.params.insert(to, v);
            })?;
            moved += 1;
        }
    }
    Ok(moved)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    MostCommon,
    First,
    Clear,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Changes {
    pub name: Option<String>,
    pub group: Option<String>,
    pub tooltip: Option<String>,
    pub categories: Option<Vec<String>>,
    pub is_instance: Option<bool>,
    pub pset_name: Option<String>,
    /// How to resolve differing instance values when changing to a type parameter.
    pub resolution: Option<Resolution>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ModifyReport {
    pub values_moved: usize,
    /// Types whose instances had different values (instance → type).
    pub conflicts: usize,
    pub values_cleared: usize,
}

pub fn project_modify(
    txn: &mut Transaction<'_>,
    guid: &str,
    changes: Changes,
) -> Result<ModifyReport> {
    let store = txn.store().clone();
    let mut list = project_parameters(&store);
    let pos = list
        .iter()
        .position(|p| p.guid == guid)
        .ok_or_else(|| Error::new("param.not_found", format!("no project parameter {guid}")))?;
    let old = list[pos].clone();
    let mut new = old.clone();
    if let Some(n) = changes.name {
        new.name = n;
    }
    if let Some(g) = changes.group {
        new.group = g;
    }
    if changes.tooltip.is_some() {
        new.tooltip = changes.tooltip;
    }
    if let Some(c) = changes.categories {
        if c.is_empty() {
            return Err(Error::new(
                "param.no_categories",
                "choose at least one category",
            ));
        }
        new.categories = c;
    }
    if let Some(p) = changes.pset_name {
        validate_pset_name(&p)?;
        new.pset_name = p;
    }
    validate_name(&store, &new.name, &new.categories, Some(guid))?;
    let mut report = ModifyReport {
        values_moved: move_values(txn, &old, &new)?,
        ..Default::default()
    };

    if let Some(instance) = changes.is_instance
        && instance != old.instance
    {
        let store = txn.store().clone();
        let mut types: BTreeMap<ElementId, Vec<(ElementId, Value)>> = BTreeMap::new();
        for id in elements_in(&store, &new.categories) {
            let e = store.get(id).expect("listed");
            if is_type(e) {
                types.entry(id).or_default();
            } else if let Some(t) = type_of(&store, e) {
                let cat = category_of(e).expect("listed by category");
                let v = e.param(&new.key(cat.key)).cloned().unwrap_or(Value::Null);
                types.entry(t).or_default().push((id, v));
            }
        }
        for (t, instances) in types {
            let Some(cat) = store.get(t).and_then(category_of) else {
                continue;
            };
            let key = new.key(cat.key);
            if instance {
                // Type → instance: each instance gets its type's value.
                let Some(v) = store.get(t).and_then(|e| e.param(&key)).cloned() else {
                    continue;
                };
                for (i, _) in &instances {
                    let (k, v) = (key.clone(), v.clone());
                    txn.modify(*i, |el| {
                        el.params.insert(k, v);
                    })?;
                }
                let k = key.clone();
                txn.modify(t, |el| {
                    el.params.remove(&k);
                })?;
                report.values_moved += instances.len();
            } else {
                // Instance → type: one value per type, by the chosen resolution.
                let values: Vec<&Value> = instances
                    .iter()
                    .map(|(_, v)| v)
                    .filter(|v| !v.is_null())
                    .collect();
                let distinct: BTreeSet<String> = values.iter().map(|v| v.to_string()).collect();
                if distinct.len() > 1 {
                    report.conflicts += 1;
                }
                let resolution = match (changes.resolution, distinct.len() > 1) {
                    (Some(r), _) => r,
                    (None, false) => Resolution::First,
                    (None, true) => {
                        return Err(Error::new(
                            "param.resolution_required",
                            "instances of the same type have different values; choose most_common, first or clear",
                        ));
                    }
                };
                let chosen = match resolution {
                    Resolution::Clear => None,
                    Resolution::First => values.first().map(|v| (*v).clone()),
                    Resolution::MostCommon => {
                        let mut counts: Vec<(usize, String, &Value)> = Vec::new();
                        for v in &values {
                            let s = v.to_string();
                            match counts.iter_mut().find(|c| c.1 == s) {
                                Some(c) => c.0 += 1,
                                None => counts.push((1, s, v)),
                            }
                        }
                        // Ties go to the value seen first.
                        counts
                            .iter()
                            .fold(None::<&(usize, String, &Value)>, |best, c| match best {
                                Some(b) if b.0 >= c.0 => Some(b),
                                _ => Some(c),
                            })
                            .map(|c| c.2.clone())
                    }
                };
                for (i, v) in &instances {
                    if !v.is_null() {
                        let k = key.clone();
                        txn.modify(*i, |el| {
                            el.params.remove(&k);
                        })?;
                        report.values_cleared += usize::from(chosen.is_none());
                    }
                }
                if let Some(v) = chosen {
                    let k = key.clone();
                    txn.modify(t, |el| {
                        el.params.insert(k, v);
                    })?;
                    report.values_moved += 1;
                }
            }
        }
        new.instance = instance;
    }
    list[pos] = new;
    write_project_parameters(txn, &list)?;
    Ok(report)
}

/// Removes a project parameter and all its values; returns how many values were removed.
pub fn project_remove(txn: &mut Transaction<'_>, guid: &str) -> Result<usize> {
    let store = txn.store().clone();
    let mut list = project_parameters(&store);
    let pos = list
        .iter()
        .position(|p| p.guid == guid)
        .ok_or_else(|| Error::new("param.not_found", format!("no project parameter {guid}")))?;
    let p = list.remove(pos);
    let mut removed = 0;
    for id in elements_in(&store, &p.categories) {
        let e = store.get(id).expect("listed");
        let Some(cat) = category_of(e) else { continue };
        let key = p.key(cat.key);
        if e.params.contains_key(&key) {
            txn.modify(id, |el| {
                el.params.remove(&key);
            })?;
            removed += 1;
        }
    }
    write_project_parameters(txn, &list)?;
    Ok(removed)
}

/// Maps a project or shared parameter onto a property set and property, for one category or all
/// bound categories, moving existing values (`param.set_ifc_home`).
pub fn set_ifc_home(
    txn: &mut Transaction<'_>,
    reference: &str,
    category: Option<&str>,
    pset: &str,
    property: &str,
) -> Result<usize> {
    let store = txn.store().clone();
    let Def::Project(old) = find(&store, reference, category.and_then(categories::category))?
    else {
        return Err(Error::new(
            "param.builtin_home",
            "only project and shared parameters can be mapped",
        ));
    };
    if pset.starts_with("Reviera_")
        || pset.starts_with("Qto_")
        || pset.contains('.')
        || property.is_empty()
    {
        return Err(Error::new(
            "param.invalid_pset",
            "choose a property set that is not Reviera_ or Qto_, without `.`",
        ));
    }
    let mut new = old.clone();
    let cats: Vec<String> = match category {
        Some(c) if old.categories.iter().any(|k| k == c) => vec![c.to_owned()],
        Some(c) => {
            return Err(Error::new(
                "param.not_applicable",
                format!("the parameter is not bound to {c}"),
            ));
        }
        None => old.categories.clone(),
    };
    for c in cats {
        new.homes.insert(
            c,
            PropertyHome {
                pset: pset.into(),
                property: property.into(),
            },
        );
    }
    let moved = move_values(txn, &old, &new)?;
    let mut list = project_parameters(&store);
    if let Some(p) = list.iter_mut().find(|p| p.guid == old.guid) {
        *p = new;
    }
    write_project_parameters(txn, &list)?;
    Ok(moved)
}
