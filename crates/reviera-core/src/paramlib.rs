// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Parameter libraries (spec Ch 2 §2.9.7, DM-SPAR-001): GUID-identified parameter definitions
//! shared across projects, in Reviera's own open format.
//!
//! - A library is a UTF-8 JSON file (`.rplib.json`), checked against the published JSON Schema
//!   `schemas/paramlib/reviera.paramlib-1.schema.json` (MIT) and then against the rules: unique
//!   canonical GUIDs, known data types, names free of `[ ] { } | ; < >` and unique within their
//!   group, groups that exist, unique enum values, `min` not above `max`, no `Reviera_` property
//!   sets, and `superseded_by` naming a definition of the same data type that is not deprecated.
//! - It is written with sorted keys and 2-space indentation, so it diffs well.
//! - Definitions are never removed, only deprecated; a definition's data type never changes
//!   ([`check_against`] compares a library with an earlier copy).
//! - The legacy shared-parameter text format (tab-separated `*META`, `*GROUP`, `*PARAM` sections)
//!   is imported for migration, its GUIDs kept, its data-type tokens mapped to Reviera's.
//!
//! Opening, editing and saving library files are application commands (`paramlib.*`); this
//! module holds the format.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};

pub const SCHEMA: &str = "reviera.paramlib";
pub const SCHEMA_VERSION: &str = "1.0.0";
/// The published JSON Schema.
pub const JSON_SCHEMA: &str = include_str!("../../../schemas/paramlib/reviera.paramlib-1.schema.json");

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LibraryInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub publisher: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub license: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub default_pset: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Group {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct IfcHome {
    pub pset: String,
    pub property: String,
}

fn yes() -> bool {
    true
}

/// A shared parameter definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Definition {
    pub guid: String,
    pub name: String,
    /// A data type key (§2.9.4).
    pub data_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "enum")]
    pub enum_values: Option<Vec<Value>>,
    #[serde(default)]
    pub group: String,
    #[serde(default = "other")]
    pub param_group: String,
    #[serde(default = "common")]
    pub discipline: String,
    #[serde(default)]
    pub tooltip: String,
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default = "yes")]
    pub user_modifiable: bool,
    #[serde(default)]
    pub hide_when_no_value: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ifc: Option<IfcHome>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub deprecated: bool,
    #[serde(default)]
    pub superseded_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

fn other() -> String {
    "other".into()
}
fn common() -> String {
    "common".into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Library {
    pub schema: String,
    pub schema_version: String,
    pub library: LibraryInfo,
    #[serde(default)]
    pub groups: Vec<Group>,
    pub definitions: Vec<Definition>,
}

impl Library {
    /// An empty library.
    pub fn new(name: &str) -> Self {
        Library {
            schema: SCHEMA.into(),
            schema_version: SCHEMA_VERSION.into(),
            library: LibraryInfo { id: uuid::Uuid::new_v4().to_string(), name: name.into(), version: "1.0.0".into(), publisher: String::new(), license: String::new(), default_pset: String::new() },
            groups: Vec::new(),
            definitions: Vec::new(),
        }
    }

    pub fn definition(&self, guid: &str) -> Option<&Definition> {
        let g = canonical(guid)?;
        self.definitions.iter().find(|d| d.guid == g)
    }
}

/// A GUID in canonical lowercase hyphenated form.
pub fn canonical(guid: &str) -> Option<String> {
    uuid::Uuid::parse_str(guid.trim()).ok().map(|u| u.to_string())
}

fn validator() -> &'static jsonschema::Validator {
    static V: OnceLock<jsonschema::Validator> = OnceLock::new();
    V.get_or_init(|| {
        let schema: Value = serde_json::from_str(JSON_SCHEMA).expect("the published schema is JSON");
        jsonschema::validator_for(&schema).expect("the published schema compiles")
    })
}

/// Reads and checks a library file's text.
pub fn parse(text: &str) -> Result<Library> {
    let v: Value = serde_json::from_str(text).map_err(|e| bad("paramlib.json", format!("not JSON: {e}")))?;
    let errors: Vec<String> = validator().iter_errors(&v).take(5).map(|e| format!("{} at {}", e, e.instance_path())).collect();
    if !errors.is_empty() {
        return Err(bad("paramlib.schema", format!("the library does not follow reviera.paramlib: {}", errors.join("; "))));
    }
    let lib: Library = serde_json::from_value(v).map_err(|e| bad("paramlib.schema", e.to_string()))?;
    check(&lib)?;
    Ok(lib)
}

/// The rules beyond the JSON Schema.
pub fn check(lib: &Library) -> Result<()> {
    if canonical(&lib.library.id).is_none() {
        return Err(bad("paramlib.id", "the library id is a UUID"));
    }
    let groups: BTreeSet<&str> = lib.groups.iter().map(|g| g.id.as_str()).collect();
    if groups.len() != lib.groups.len() {
        return Err(bad("paramlib.group", "two groups share an id"));
    }
    let mut seen = BTreeSet::new();
    let mut names = BTreeSet::new();
    for d in &lib.definitions {
        let what = format!("{} ({})", d.name, d.guid);
        let g = canonical(&d.guid).ok_or_else(|| bad("paramlib.guid", format!("{what}: the GUID is not a UUID")))?;
        if g != d.guid {
            return Err(bad("paramlib.guid", format!("{what}: write the GUID in canonical lowercase form ({g})")));
        }
        if !seen.insert(g) {
            return Err(bad("paramlib.guid", format!("{what}: the GUID is used twice")));
        }
        check_definition(lib, d, &groups)?;
        if !names.insert((d.group.clone(), d.name.to_lowercase())) {
            return Err(bad("paramlib.name", format!("{what}: another definition in its group has that name")));
        }
    }
    Ok(())
}

fn check_definition(lib: &Library, d: &Definition, groups: &BTreeSet<&str>) -> Result<()> {
    let what = format!("{} ({})", d.name, d.guid);
    if d.name.trim().is_empty() || d.name.chars().count() > 255 || d.name.contains(['[', ']', '{', '}', '|', ';', '<', '>']) {
        return Err(bad("paramlib.name", format!("{what}: a name is 1–255 characters without [ ] {{ }} | ; < >")));
    }
    if crate::units::data_type(&d.data_type).is_none() {
        return Err(bad("paramlib.data_type", format!("{what}: no data type {:?}", d.data_type)));
    }
    if !d.group.is_empty() && !groups.contains(d.group.as_str()) {
        return Err(bad("paramlib.group", format!("{what}: no group {:?}", d.group)));
    }
    if let Some(e) = &d.enum_values {
        let mut set = BTreeSet::new();
        for v in e {
            let numeric = crate::units::data_type(&d.data_type).is_some_and(|t| t.dim != crate::units::Dim::None) || matches!(d.data_type.as_str(), "integer" | "number");
            if numeric != v.is_number() {
                return Err(bad("paramlib.enum", format!("{what}: {v} does not suit {}", d.data_type)));
            }
            if !set.insert(v.to_string()) {
                return Err(bad("paramlib.enum", format!("{what}: {v} is listed twice")));
            }
        }
    }
    if let (Some(lo), Some(hi)) = (d.min, d.max)
        && lo > hi
    {
        return Err(bad("paramlib.range", format!("{what}: min is above max")));
    }
    if let Some(h) = &d.ifc
        && h.pset.starts_with("Reviera_")
    {
        return Err(bad("paramlib.ifc", format!("{what}: Reviera_ property sets are Reviera's own")));
    }
    if let Some(s) = &d.superseded_by {
        let t = lib.definition(s).ok_or_else(|| bad("paramlib.superseded_by", format!("{what}: superseded by {s}, which the library does not have")))?;
        if t.deprecated || t.data_type != d.data_type {
            return Err(bad("paramlib.superseded_by", format!("{what}: its successor must be current and of the same data type")));
        }
    }
    Ok(())
}

/// Checks a library against an earlier copy of it: no definition removed, no data type changed.
pub fn check_against(new: &Library, old: &Library) -> Result<()> {
    for o in &old.definitions {
        match new.definitions.iter().find(|d| d.guid == o.guid) {
            None => return Err(bad("paramlib.removed", format!("{} ({}) was removed; deprecate definitions instead", o.name, o.guid))),
            Some(d) if d.data_type != o.data_type => {
                return Err(bad("paramlib.data_type_changed", format!("{} ({}) changed its data type from {} to {}", d.name, d.guid, o.data_type, d.data_type)));
            }
            _ => {}
        }
    }
    Ok(())
}

/// The library as written: sorted keys, 2-space indentation, a final newline.
pub fn write(lib: &Library) -> String {
    fn sort(v: Value) -> Value {
        match v {
            Value::Object(m) => Value::Object(m.into_iter().map(|(k, v)| (k, sort(v))).collect::<BTreeMap<_, _>>().into_iter().collect()),
            Value::Array(a) => Value::Array(a.into_iter().map(sort).collect()),
            v => v,
        }
    }
    let v = sort(serde_json::to_value(lib).expect("serialises"));
    let mut s = serde_json::to_string_pretty(&v).expect("serialises");
    s.push('\n');
    s
}

// ---- The legacy shared-parameter text format ----

/// A legacy data-type token's data type, if Reviera knows it.
pub fn legacy_type(token: &str) -> Option<&'static str> {
    let t = token.trim().to_uppercase();
    // Newer files name specs, "autodesk.spec.aec:length-2.0.0" and the like.
    let t = t.rsplit(':').next().unwrap_or(&t).split('-').next().unwrap_or(&t).replace('.', "").to_uppercase();
    Some(match t.as_str() {
        "TEXT" | "STRING" => "text",
        "MULTILINETEXT" => "multiline_text",
        "INTEGER" | "INT64" => "integer",
        "NUMBER" | "DOUBLE" => "number",
        "YESNO" | "BOOLEAN" => "yes_no",
        "URL" => "url",
        "LENGTH" => "length",
        "AREA" => "area",
        "VOLUME" => "volume",
        "ANGLE" => "angle",
        "SLOPE" => "slope",
        "CURRENCY" => "currency",
        "MASS" => "mass",
        "FORCE" => "force",
        "MOMENT" => "moment",
        "TEMPERATURE" => "temperature",
        "AIRFLOW" | "HVAC_AIRFLOW" | "HVACAIRFLOW" => "airflow",
        "POWER" | "ELECTRICAL_POWER" | "ELECTRICALPOWER" => "power",
        "CURRENT" | "ELECTRICAL_CURRENT" | "ELECTRICALCURRENT" => "current",
        "ELECTRICAL_POTENTIAL" | "ELECTRICALPOTENTIAL" => "electrical_potential",
        "ILLUMINANCE" => "illuminance",
        _ => return None,
    })
}

/// Reads a legacy shared-parameter file into groups and definitions, keeping GUIDs; `mapping`
/// gives data types for tokens Reviera does not know. Unknown tokens left are refused, listed.
pub fn import_legacy(text: &str, mapping: &BTreeMap<String, String>) -> Result<(Vec<Group>, Vec<Definition>)> {
    let mut groups: Vec<Group> = Vec::new();
    let mut defs = Vec::new();
    let mut param_cols: Vec<String> = Vec::new();
    let mut unknown = BTreeSet::new();
    for line in text.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        match cols.first().copied() {
            Some("*PARAM") => param_cols = cols.iter().map(|c| c.to_uppercase()).collect(),
            Some("GROUP") if cols.len() >= 3 => groups.push(Group { id: format!("g{}", cols[1].trim()), name: cols[2].trim().to_owned() }),
            Some("PARAM") => {
                let col = |name: &str| param_cols.iter().position(|c| c == name).and_then(|k| cols.get(k)).map(|s| s.trim()).unwrap_or("");
                let guid = canonical(col("GUID")).ok_or_else(|| bad("paramlib.legacy", format!("{:?}: no GUID", col("NAME"))))?;
                let token = col("DATATYPE");
                let data_type = match mapping.get(token).map(String::as_str).or_else(|| legacy_type(token)) {
                    Some(t) => t.to_owned(),
                    None => {
                        unknown.insert(token.to_owned());
                        continue;
                    }
                };
                let flag = |c: &str, default: bool| match col(c) {
                    "1" => true,
                    "0" => false,
                    _ => default,
                };
                defs.push(Definition {
                    guid,
                    name: col("NAME").to_owned(),
                    data_type,
                    enum_values: None,
                    group: format!("g{}", col("GROUP")),
                    param_group: other(),
                    discipline: common(),
                    tooltip: col("DESCRIPTION").to_owned(),
                    visible: flag("VISIBLE", true),
                    user_modifiable: flag("USERMODIFIABLE", true),
                    hide_when_no_value: flag("HIDEWHENNOVALUE", false),
                    ifc: None,
                    labels: Vec::new(),
                    deprecated: false,
                    superseded_by: None,
                    min: None,
                    max: None,
                });
            }
            _ => {}
        }
    }
    if !unknown.is_empty() {
        return Err(bad("paramlib.legacy_types", format!("map these data types first: {}", unknown.into_iter().collect::<Vec<_>>().join(", "))));
    }
    Ok((groups, defs))
}

/// Definitions matching a search: text in name, tooltip or labels; any of `labels`; a data type.
pub fn matches(d: &Definition, text: Option<&str>, labels: &[String], data_type: Option<&str>) -> bool {
    let t = text.map(str::to_lowercase);
    t.as_deref().is_none_or(|t| d.name.to_lowercase().contains(t) || d.tooltip.to_lowercase().contains(t) || d.labels.iter().any(|l| l.to_lowercase().contains(t)))
        && (labels.is_empty() || labels.iter().any(|l| d.labels.contains(l)))
        && data_type.is_none_or(|dt| d.data_type == dt)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Library {
        let mut l = Library::new("Test");
        l.groups.push(Group { id: "g-a".into(), name: "Acoustics".into() });
        l.definitions.push(Definition {
            guid: "3b0d2a4e-5f61-4c7a-9e2b-1d0c8f6a7b51".into(),
            name: "Acoustic Class".into(),
            data_type: "text".into(),
            enum_values: Some(vec![Value::from("A"), Value::from("B")]),
            group: "g-a".into(),
            param_group: other(),
            discipline: common(),
            tooltip: String::new(),
            visible: true,
            user_modifiable: true,
            hide_when_no_value: false,
            ifc: Some(IfcHome { pset: "ACME_Common".into(), property: "AcousticClass".into() }),
            labels: vec!["acoustics".into()],
            deprecated: false,
            superseded_by: None,
            min: None,
            max: None,
        });
        l
    }

    #[test]
    fn round_trip_and_rules() {
        let l = sample();
        let text = write(&l);
        assert!(text.contains("\n  \"definitions\""), "2-space indentation");
        let keys: Vec<&str> = text.lines().filter(|x| x.starts_with("  \"")).map(|x| x.trim()).collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "sorted keys");
        assert_eq!(parse(&text).unwrap(), l);
        let mut bad = l.clone();
        bad.definitions[0].ifc = Some(IfcHome { pset: "Reviera_X".into(), property: "Y".into() });
        assert!(check(&bad).is_err());
        let mut changed = l.clone();
        changed.definitions[0].data_type = "length".into();
        changed.definitions[0].enum_values = None;
        assert_eq!(&*check_against(&changed, &l).unwrap_err().code, "paramlib.data_type_changed");
        assert!(parse("{\"schema\": \"other\"}").is_err());
    }

    #[test]
    fn legacy_import() {
        let text = "# shared parameters\n*META\tVERSION\tMINVERSION\nMETA\t2\t1\n*GROUP\tID\tNAME\nGROUP\t1\tIdentity\n*PARAM\tGUID\tNAME\tDATATYPE\tDATACATEGORY\tGROUP\tVISIBLE\tDESCRIPTION\tUSERMODIFIABLE\tHIDEWHENNOVALUE\nPARAM\t3B0D2A4E-5F61-4C7A-9E2B-1D0C8F6A7B51\tFire Rating\tTEXT\t\t1\t1\tthe rating\t1\t0\nPARAM\t4b0d2a4e-5f61-4c7a-9e2b-1d0c8f6a7b51\tOdd\tWEIRDTYPE\t\t1\t1\t\t1\t0\n";
        let e = import_legacy(text, &BTreeMap::new()).unwrap_err();
        assert!(e.message.contains("WEIRDTYPE"));
        let (g, d) = import_legacy(text, &BTreeMap::from([("WEIRDTYPE".to_string(), "number".to_string())])).unwrap();
        assert_eq!(g[0].name, "Identity");
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].guid, "3b0d2a4e-5f61-4c7a-9e2b-1d0c8f6a7b51");
        assert_eq!(d[0].tooltip, "the rating");
        assert_eq!(legacy_type("autodesk.spec.aec:length-2.0.0"), Some("length"));
    }
}
