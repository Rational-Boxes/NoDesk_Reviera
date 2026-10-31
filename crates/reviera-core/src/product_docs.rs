// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Product documentation of materials (COL-FE-006, COL-FE-007): each project material may link to
//! one FileEngine folder of its documents (EPD, HPD, datasheets), and carry values read from
//! them, each with its source (file, version, extraction run, field). The application reads the
//! documents and runs the extraction; this module keeps what the user accepted, in RXD on the
//! project (`rxd:product_docs`), and turns an EPD's GWP into an embodied-carbon factor.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::Element;
use crate::embodied_carbon::{self, Factor, Stages, Unit};
use crate::error::{Error, Result};
use crate::store::Store;
use crate::txn::Transaction;

pub const KEY: &str = "rxd:product_docs";

/// The carbon library EPD factors go in.
pub const EPD_LIBRARY: &str = "Product EPDs";

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// A material's documentation folder on FileEngine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Link {
    /// The FileEngine connection it was linked through (`fileengine.list_connections`).
    pub connection_id: String,
    /// The server's address, for other tools and the IFC reference.
    #[serde(default)]
    pub url: String,
    pub folder_uid: String,
    #[serde(default)]
    pub folder_name: String,
}

/// Where a value came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Source {
    pub file_uid: String,
    #[serde(default)]
    pub file_name: String,
    /// The document's version the value was read from.
    pub version: String,
    /// The extraction run (the service keeps its quotes).
    #[serde(default)]
    pub run_id: String,
    /// Whether the service answered from its cache.
    #[serde(default)]
    pub cached: bool,
}

/// A value taken from a document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Taken {
    /// A string, number or boolean, as the document gives it.
    pub value: Value,
    /// The unit the document prints, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// How people read it (`GWP A1-A3`).
    #[serde(default)]
    pub label: String,
    /// The IFC property it is written as (`GWP_A1A3`).
    #[serde(default)]
    pub property: String,
    pub source: Source,
}

/// A material's documentation: its folder, and its values by field set and field.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct MaterialDocs {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<Link>,
    /// Field set (`epd`, `hpd`) → field → value.
    pub values: BTreeMap<String, BTreeMap<String, Taken>>,
    /// The IFC property set of each field set (`Reviera_EPD`).
    pub psets: BTreeMap<String, String>,
}

/// The project's product documentation, by material name.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ProductDocs {
    pub materials: BTreeMap<String, MaterialDocs>,
}

fn project(store: &Store) -> Option<&Element> {
    store.iter().find(|e| e.class == "IfcProject")
}

pub fn data(store: &Store) -> ProductDocs {
    project(store).and_then(|p| p.param(KEY)).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

fn put(txn: &mut Transaction<'_>, mut d: ProductDocs) -> Result<()> {
    d.materials.retain(|_, m| m.link.is_some() || !m.values.is_empty());
    let p = project(txn.store()).map(Element::id).ok_or_else(|| bad("project.missing", "the model has no project"))?;
    let v = serde_json::to_value(&d).expect("serialises");
    txn.modify(p, |e| {
        if d.materials.is_empty() {
            e.params.remove(KEY);
        } else {
            e.params.insert(KEY.into(), v);
        }
    })
}

fn material_name(m: &str) -> Result<String> {
    let m = m.trim();
    if m.is_empty() {
        return Err(bad("docs.invalid_input", "name the material, as its layers name it (docs.materials lists them)"));
    }
    Ok(m.to_owned())
}

// ---- Declared units ----

/// What a declared unit (`1 m2`, `1,000 ft2`, `1 m² of board`, `1 tonne`) is in Reviera's carbon
/// units: the unit and how many of it one declared unit holds.
pub fn declared_unit(text: &str) -> Result<(Unit, f64)> {
    let t = text.trim().to_lowercase().replace('²', "2").replace('³', "3");
    let start = t.find(|c: char| c.is_ascii_digit());
    let (count, rest) = match start {
        Some(k) if t[..k].trim().is_empty() => {
            let end = t[k..].find(|c: char| !(c.is_ascii_digit() || c == ',' || c == '.')).map_or(t.len(), |e| k + e);
            let n: f64 = t[k..end].replace(',', "").parse().map_err(|_| bad("docs.unit", format!("cannot read the number in the declared unit {text:?}")))?;
            (n, t[end..].trim_start().to_owned())
        }
        _ => (1.0, t.clone()),
    };
    let rest = rest.trim_start_matches(|c: char| c == '-' || c.is_whitespace());
    let unit_words: &[(&[&str], Unit, f64)] = &[
        (&["m2", "sq m", "sqm", "square metre", "square meter"], Unit::M2, 1.0),
        (&["ft2", "sq ft", "sf ", "sqft", "square foot", "square feet"], Unit::M2, 0.092_903_04),
        (&["msf"], Unit::M2, 92.903_04),
        (&["m3", "cubic metre", "cubic meter", "cu m"], Unit::M3, 1.0),
        (&["ft3", "cubic foot", "cubic feet", "cu ft"], Unit::M3, 0.028_316_846_592),
        (&["yd3", "cubic yard", "cu yd"], Unit::M3, 0.764_554_857_984),
        (&["kg", "kilogram"], Unit::Kg, 1.0),
        (&["tonne", "metric ton", "t "], Unit::Kg, 1000.0),
        (&["lb", "pound"], Unit::Kg, 0.453_592_37),
        (&["piece", "unit", "item", "each", "pc"], Unit::Piece, 1.0),
    ];
    let padded = format!("{rest} ");
    for (words, unit, size) in unit_words {
        if words.iter().any(|w| padded.starts_with(w)) {
            if count <= 0.0 {
                break;
            }
            return Ok((*unit, count * size));
        }
    }
    Err(bad("docs.unit", format!("Reviera cannot use the declared unit {text:?} for a carbon factor: it reads m², ft², m³, ft³, yd³, kg, t, lb and pieces")))
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetLinkParams {
    pub material: String,
    /// The folder; null unlinks it (the values stay).
    pub link: Option<Link>,
}

/// `docs.set_link`.
pub(crate) fn set_link(txn: &mut Transaction<'_>, p: SetLinkParams) -> Result<()> {
    let m = material_name(&p.material)?;
    let mut d = data(txn.store());
    if let Some(l) = &p.link
        && (l.folder_uid.trim().is_empty() || l.connection_id.trim().is_empty())
    {
        return Err(bad("docs.invalid_input", "a link needs a connection and a folder uid"));
    }
    d.materials.entry(m).or_default().link = p.link;
    put(txn, d)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnlinkParams {
    pub material: String,
    /// Also forget the values taken from its documents (default false).
    #[serde(default)]
    pub forget_values: bool,
}

/// `docs.unlink`.
pub(crate) fn unlink(txn: &mut Transaction<'_>, p: UnlinkParams) -> Result<()> {
    let mut d = data(txn.store());
    let Some(m) = d.materials.get_mut(p.material.trim()) else {
        return Err(bad("docs.not_linked", format!("{:?} has no documentation", p.material)));
    };
    m.link = None;
    if p.forget_values {
        m.values.clear();
        m.psets.clear();
    }
    put(txn, d)
}

/// A carbon factor to make from an EPD and match to the material.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CarbonFromEpd {
    /// kg CO₂e per declared unit, A1–A3.
    pub gwp_a1a3: f64,
    /// The declared unit as printed (`1,000 ft2`).
    pub declared_unit: String,
    /// The EPD's number, for the factor's id and source.
    pub epd_number: String,
    #[serde(default)]
    pub program_operator: String,
    #[serde(default)]
    pub product: String,
    /// YYYY-MM-DD.
    #[serde(default)]
    pub valid_until: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetValuesParams {
    pub material: String,
    /// The field set (`epd`, `hpd`).
    pub set: String,
    /// The IFC property set its values are written as (default `Reviera_<SET>`).
    #[serde(default)]
    pub pset: Option<String>,
    /// Field → value with its source; null removes the field.
    pub values: BTreeMap<String, Option<Taken>>,
    /// Make an embodied-carbon factor from the EPD and match the material to it.
    #[serde(default)]
    pub carbon: Option<CarbonFromEpd>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SetValuesOut {
    /// The fields written.
    pub written: Vec<String>,
    /// The carbon factor made (`library_id/factor_id`), with its value per Reviera unit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub factor_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub factor_per_unit: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub factor_unit: Option<Unit>,
}

fn factor_id(epd: &str) -> String {
    let id: String = epd.trim().chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
    format!("epd-{id}")
}

/// `docs.set_values`.
pub(crate) fn set_values(txn: &mut Transaction<'_>, p: SetValuesParams) -> Result<SetValuesOut> {
    let m = material_name(&p.material)?;
    let set = p.set.trim().to_lowercase();
    if set.is_empty() || !set.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(bad("docs.invalid_input", "a field set is named in letters, digits and _ (epd, hpd)"));
    }
    let mut d = data(txn.store());
    let md = d.materials.entry(m.clone()).or_default();
    let pset = p.pset.filter(|s| !s.trim().is_empty()).unwrap_or_else(|| format!("Reviera_{}", set.to_uppercase()));
    md.psets.insert(set.clone(), pset);
    let values = md.values.entry(set.clone()).or_default();
    let mut written = Vec::new();
    for (field, v) in p.values {
        match v {
            Some(mut t) => {
                if t.property.is_empty() {
                    t.property.clone_from(&field);
                }
                if t.label.is_empty() {
                    t.label.clone_from(&field);
                }
                values.insert(field.clone(), t);
                written.push(field);
            }
            None => {
                values.remove(&field);
            }
        }
    }
    if values.is_empty() {
        md.values.remove(&set);
        md.psets.remove(&set);
    }
    put(txn, d)?;
    let mut out = SetValuesOut { written, factor_ref: None, factor_per_unit: None, factor_unit: None };
    if let Some(c) = p.carbon {
        if !(c.gwp_a1a3.is_finite() && c.gwp_a1a3 >= 0.0) {
            return Err(bad("docs.invalid_input", "the EPD's GWP must be a number of zero or more"));
        }
        if c.epd_number.trim().is_empty() {
            return Err(bad("docs.invalid_input", "a carbon factor from an EPD needs the EPD's number"));
        }
        let (unit, size) = declared_unit(&c.declared_unit)?;
        let per = c.gwp_a1a3 / size;
        let factor = Factor {
            id: factor_id(&c.epd_number),
            name: if c.product.trim().is_empty() { format!("{m} (EPD {})", c.epd_number.trim()) } else { format!("{} (EPD {})", c.product.trim(), c.epd_number.trim()) },
            class: String::new(),
            unit,
            stages: Stages { a1_a3: per, ..Stages::default() },
            density: None,
            service_life: None,
            source: format!("EPD {}{}, per {}", c.epd_number.trim(), if c.program_operator.trim().is_empty() { String::new() } else { format!(" ({})", c.program_operator.trim()) }, c.declared_unit.trim()),
            valid_until: c.valid_until.filter(|v| !v.trim().is_empty()),
        };
        // Into the EPD library, replacing a factor of the same id, then matched.
        let cd = embodied_carbon::data(txn.store());
        let mut factors: Vec<Factor> = cd.libraries.iter().find(|l| l.name == EPD_LIBRARY).map(|l| l.factors.clone()).unwrap_or_default();
        factors.retain(|f| f.id != factor.id);
        let fid = factor.id.clone();
        factors.push(factor);
        let lib = embodied_carbon::add_library(txn, embodied_carbon::AddLibraryParams { name: EPD_LIBRARY.into(), file: String::new(), factors })?;
        let r = format!("{lib}/{fid}");
        embodied_carbon::match_material(txn, embodied_carbon::MatchParams { material: m, factor_ref: Some(r.clone()) })?;
        out.factor_ref = Some(r);
        out.factor_per_unit = Some(per);
        out.factor_unit = Some(unit);
    }
    Ok(out)
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct MaterialOut {
    pub material: String,
    /// Whether a wall, floor, ceiling or roof uses it.
    pub in_use: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<Link>,
    /// Field set → field → value.
    pub values: BTreeMap<String, BTreeMap<String, Taken>>,
}

/// `docs.materials`.
pub(crate) fn materials(store: &Store, _: embodied_carbon::Empty) -> Result<Vec<MaterialOut>> {
    let d = data(store);
    let (qs, _) = embodied_carbon::quantities(store);
    let mut names: BTreeMap<String, bool> = qs.into_iter().map(|q| (q.material, true)).collect();
    for k in d.materials.keys() {
        names.entry(k.clone()).or_insert(false);
    }
    Ok(names
        .into_iter()
        .map(|(material, in_use)| {
            let md = d.materials.get(&material).cloned().unwrap_or_default();
            MaterialOut { material, in_use, link: md.link, values: md.values }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_units_read_as_carbon_units() {
        let (u, n) = declared_unit("1,000 ft2").unwrap();
        assert_eq!(u, Unit::M2);
        assert!((n - 92.903_04).abs() < 1e-9);
        assert!((126.0 / n - 1.356_253).abs() < 1e-6);
        assert_eq!(declared_unit("1m² of board").unwrap(), (Unit::M2, 1.0));
        assert_eq!(declared_unit("1 m3").unwrap(), (Unit::M3, 1.0));
        assert_eq!(declared_unit("1 metric ton").unwrap(), (Unit::Kg, 1000.0));
        assert_eq!(declared_unit("1 tonne of steel").unwrap(), (Unit::Kg, 1000.0));
        assert_eq!(declared_unit("kg").unwrap(), (Unit::Kg, 1.0));
        assert_eq!(declared_unit("1 MSF").unwrap().0, Unit::M2);
        assert!(declared_unit("1 linear metre").is_err());
    }
}
