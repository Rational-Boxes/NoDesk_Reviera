// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Native IFC read/write through IfcOpenShell (FFI). IFC is the source of truth (TECH_STACK §3.1).
//!
//! [`IfcModel`] is one IFC instance population. Entities are addressed by [`StepId`] while the
//! file is open; durable identity is the GlobalId (spec DM-ID-001), and STEP numbers are never
//! persisted. Attribute values cross the boundary generically as [`AttrValue`], so entities that
//! Reviera does not interpret are still kept and written back unchanged (DM-CORE-001 rule 8).

mod ffi;

use std::fmt;
use std::path::Path;

use cxx::UniquePtr;
use reviera_core::GlobalId;
use serde::{Deserialize, Serialize};

use ffi::bridge;

#[derive(Debug, thiserror::Error)]
pub enum IfcError {
    #[error("IfcOpenShell: {0}")]
    Native(String),
    #[error("{0}")]
    Unsupported(String),
}

impl From<cxx::Exception> for IfcError {
    fn from(e: cxx::Exception) -> Self {
        IfcError::Native(e.what().to_owned())
    }
}

pub type Result<T, E = IfcError> = std::result::Result<T, E>;

/// IFC schemas Reviera writes: IFC4 is the default, IFC4X3_ADD2 is offered in Local mode, and
/// IFC2X3 files are read (TECH_STACK §3.1, §3.7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Schema {
    Ifc2x3,
    Ifc4,
    Ifc4x3Add2,
}

impl Schema {
    pub fn identifier(self) -> &'static str {
        match self {
            Schema::Ifc2x3 => "IFC2X3",
            Schema::Ifc4 => "IFC4",
            Schema::Ifc4x3Add2 => "IFC4X3_ADD2",
        }
    }
}

/// STEP instance number (`#123`). Valid only while the file is open (DM-ID-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StepId(pub u32);

impl fmt::Display for StepId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// An IFC attribute value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "v", rename_all = "snake_case")]
pub enum AttrValue {
    Null,
    /// `*`: the value is derived from other attributes.
    Derived,
    Int(i64),
    Bool(bool),
    /// EXPRESS LOGICAL; `None` is UNKNOWN.
    Logical(Option<bool>),
    Real(f64),
    String(String),
    Binary(String),
    Enum(String),
    Ref(u32),
    /// A defined-type value inside a select, for example `IfcLabel('x')`.
    Typed {
        type_name: String,
        value: Box<AttrValue>,
    },
    List(Vec<AttrValue>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityInfo {
    pub id: StepId,
    pub class: String,
    pub global_id: Option<GlobalId>,
    pub name: Option<String>,
}

/// Triangulated geometry of one product, in world coordinates (metres).
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    pub id: StepId,
    pub global_id: Option<GlobalId>,
    pub class: String,
    /// x, y, z triples.
    pub verts: Vec<f64>,
    /// Vertex index triples.
    pub faces: Vec<u32>,
}

impl Mesh {
    pub fn triangle_count(&self) -> usize {
        self.faces.len() / 3
    }
}

/// The IfcOpenShell version linked into this build.
pub fn ifcopenshell_version() -> String {
    bridge::ifcopenshell_version()
}

pub struct IfcModel {
    inner: UniquePtr<bridge::Model>,
}

impl fmt::Debug for IfcModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IfcModel")
            .field("schema", &self.schema())
            .field("entities", &self.entity_count())
            .finish()
    }
}

impl IfcModel {
    pub fn open(path: &Path) -> Result<Self> {
        Ok(Self {
            inner: bridge::open_model(&path_str(path)?)?,
        })
    }

    pub fn new(schema: Schema) -> Result<Self> {
        Ok(Self {
            inner: bridge::new_model(schema.identifier())?,
        })
    }

    /// Schema identifier as written in the file header, for example `IFC4`.
    pub fn schema(&self) -> String {
        self.inner.schema_name()
    }

    pub fn entity_count(&self) -> usize {
        self.inner.entity_count()
    }

    /// Instances of `class` and its subtypes.
    pub fn entities_of_type(&self, class: &str) -> Result<Vec<EntityInfo>> {
        Ok(self
            .inner
            .entities_of_type(class)?
            .into_iter()
            .map(|e| EntityInfo {
                id: StepId(e.id),
                class: e.class_name,
                global_id: GlobalId::parse(&e.global_id),
                name: (!e.name.is_empty()).then_some(e.name),
            })
            .collect())
    }

    /// Every `IfcRoot`: the entities that carry a GlobalId.
    pub fn rooted(&self) -> Result<Vec<EntityInfo>> {
        self.entities_of_type("IfcRoot")
    }

    pub fn find_by_guid(&self, guid: &GlobalId) -> Option<StepId> {
        u32::try_from(self.inner.instance_by_guid(guid.as_str()))
            .ok()
            .map(StepId)
    }

    pub fn class_of(&self, id: StepId) -> Result<String> {
        Ok(self.inner.entity_class(id.0)?)
    }

    /// All attributes of an entity, in schema order, by name.
    pub fn attributes(&self, id: StepId) -> Result<Vec<(String, AttrValue)>> {
        let names = self.inner.attribute_names(id.0)?;
        let values: Vec<AttrValue> = serde_json::from_str(&self.inner.attributes_json(id.0)?)
            .map_err(|e| IfcError::Native(format!("undecodable attribute values on {id}: {e}")))?;
        Ok(names.into_iter().zip(values).collect())
    }

    pub fn attribute(&self, id: StepId, name: &str) -> Result<AttrValue> {
        self.attributes(id)?
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
            .ok_or_else(|| IfcError::Unsupported(format!("{id} has no attribute {name}")))
    }

    pub fn set_attribute(&mut self, id: StepId, name: &str, value: &AttrValue) -> Result<()> {
        let m = self.inner.pin_mut();
        let unsupported =
            || IfcError::Unsupported(format!("setting {value:?} on {name} is not supported yet"));
        match value {
            AttrValue::Null => m.set_null(id.0, name)?,
            AttrValue::String(s) => m.set_string(id.0, name, s)?,
            AttrValue::Real(r) => m.set_real(id.0, name, *r)?,
            AttrValue::Int(i) => m.set_int(id.0, name, to_i32(*i)?)?,
            AttrValue::Bool(b) => m.set_bool(id.0, name, *b)?,
            AttrValue::Enum(e) => m.set_enum(id.0, name, e)?,
            AttrValue::Ref(r) => m.set_ref(id.0, name, *r)?,
            AttrValue::Typed {
                type_name,
                value: inner,
            } => match inner.as_ref() {
                AttrValue::String(s) => m.set_typed_string(id.0, name, type_name, s)?,
                AttrValue::Real(r) => m.set_typed_real(id.0, name, type_name, *r)?,
                AttrValue::Int(i) => m.set_typed_int(id.0, name, type_name, to_i32(*i)?)?,
                AttrValue::Bool(b) => m.set_typed_bool(id.0, name, type_name, *b)?,
                _ => return Err(unsupported()),
            },
            AttrValue::List(items) => {
                if let Some(refs) = all(items, |v| {
                    if let AttrValue::Ref(r) = v {
                        Some(*r)
                    } else {
                        None
                    }
                }) {
                    m.set_ref_list(id.0, name, &refs)?
                } else if let Some(reals) = all(items, |v| {
                    if let AttrValue::Real(r) = v {
                        Some(*r)
                    } else {
                        None
                    }
                }) {
                    m.set_real_list(id.0, name, &reals)?
                } else {
                    return Err(unsupported());
                }
            }
            AttrValue::Derived | AttrValue::Logical(_) | AttrValue::Binary(_) => {
                return Err(unsupported());
            }
        }
        Ok(())
    }

    /// Instances of `class` (or its subtypes) that reference `id` in any attribute.
    pub fn referenced_by(&self, id: StepId, class: &str) -> Result<Vec<StepId>> {
        Ok(self
            .inner
            .referenced_by(id.0, class)?
            .into_iter()
            .map(StepId)
            .collect())
    }

    /// Creates an entity with every attribute unset.
    pub fn create(&mut self, class: &str) -> Result<StepId> {
        Ok(StepId(self.inner.pin_mut().create_entity(class)?))
    }

    /// Creates an `IfcRoot` subtype with the given GlobalId.
    pub fn create_rooted(&mut self, class: &str, guid: &GlobalId) -> Result<StepId> {
        let id = self.create(class)?;
        self.set_attribute(id, "GlobalId", &AttrValue::String(guid.as_str().to_owned()))?;
        Ok(id)
    }

    pub fn remove(&mut self, id: StepId) -> Result<()> {
        Ok(self.inner.pin_mut().remove_entity(id.0)?)
    }

    /// Writes the model as an IFC-SPF (`.ifc`) file.
    pub fn write(&self, path: &Path) -> Result<()> {
        Ok(self.inner.write(&path_str(path)?)?)
    }

    /// Triangulates every product with OpenCASCADE, in world coordinates.
    pub fn tessellate(&self, threads: u32) -> Result<Vec<Mesh>> {
        Ok(self
            .inner
            .tessellate(threads)?
            .into_iter()
            .map(|m| Mesh {
                id: StepId(m.id),
                global_id: GlobalId::parse(&m.guid),
                class: m.class_name,
                verts: m.verts,
                faces: m.faces,
            })
            .collect())
    }
}

fn to_i32(i: i64) -> Result<i32> {
    i32::try_from(i)
        .map_err(|_| IfcError::Unsupported(format!("{i} is out of range for an IFC INTEGER")))
}

/// `Some` if every item maps to `Some`.
fn all<T>(items: &[AttrValue], f: impl Fn(&AttrValue) -> Option<T>) -> Option<Vec<T>> {
    items.iter().map(f).collect()
}

fn path_str(path: &Path) -> Result<String> {
    path.to_str().map(str::to_owned).ok_or_else(|| {
        IfcError::Unsupported(format!("path is not valid UTF-8: {}", path.display()))
    })
}
