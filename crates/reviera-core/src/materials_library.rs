// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The structural materials library (STR-SEC-001): steel grades, concrete classes and timber
//! grades and strength classes with their key properties, compiled from public sources
//! (`data/materials.json`; S-numbers in the source register). Strengths and moduli are in MPa,
//! densities in kg/m³; values are reference values before design factors. Types name their
//! material by its `name`, which is how the region's defaults are written.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::columns::MaterialKind;

const DATA: &str = include_str!("../data/materials.json");

/// A material of the library.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Material {
    /// How types name it, for example `Steel S355 (EN 10025-2)`.
    pub name: String,
    pub kind: MaterialKind,
    pub standard: String,
    /// The regions it is usual in (`united_states`, `canada`, `europe`, `united_kingdom`,
    /// `australia`).
    pub regions: Vec<String>,
    #[serde(default)]
    pub r#use: String,
    /// Key properties: `fy_MPa`, `fu_MPa` (steel); `fck_MPa`, `fcm_MPa`, `fctm_MPa` (concrete);
    /// `fm_k_MPa`, `fc0_k_MPa`, `Fb_MPa`, `Fc_MPa` and the like (timber); `E_MPa`, `G_MPa`,
    /// `poisson`, `density_kg_m3`.
    pub properties: BTreeMap<String, f64>,
    #[serde(default)]
    pub source_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Deserialize)]
struct Data {
    materials: Vec<Material>,
}

/// Every material of the library.
pub fn all() -> Vec<Material> {
    serde_json::from_str::<Data>(DATA)
        .map(|d| d.materials)
        .unwrap_or_default()
}

/// A material by its name.
pub fn find(name: &str) -> Option<Material> {
    all().into_iter().find(|m| m.name == name)
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    /// Only this kind: `steel`, `concrete` or `wood`.
    pub kind: Option<MaterialKind>,
}

/// A listed material: the library entry, and whether it is usual in the model's region.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Listed {
    #[serde(flatten)]
    pub material: Material,
    pub preferred: bool,
}

/// The library's materials (`structural.materials`), the model's region's first.
pub fn list(store: &crate::store::Store, p: &ListParams) -> Vec<Listed> {
    let region = serde_json::to_value(crate::codes::settings(store).code.structural().region)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    let mut out: Vec<Listed> = all()
        .into_iter()
        .filter(|m| p.kind.is_none_or(|k| m.kind == k))
        .map(|m| Listed {
            preferred: m.regions.contains(&region),
            material: m,
        })
        .collect();
    out.sort_by_key(|l| !l.preferred);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_library_reads_and_has_the_regional_defaults() {
        let all = all();
        assert!(all.len() >= 30);
        for code in crate::codes::Code::ALL {
            let d = code.structural();
            for name in [d.steel, d.concrete, d.timber] {
                assert!(find(name).is_some(), "{name} is not in the library");
            }
        }
        let s355 = find("Steel S355 (EN 10025-2)").unwrap();
        assert_eq!(s355.properties["fy_MPa"], 355.0);
        let c30 = find("Concrete C30/37").unwrap();
        assert_eq!(c30.properties["fck_MPa"], 30.0);
        assert!(
            all.iter()
                .all(|m| !m.source_ids.is_empty() || m.kind == MaterialKind::Concrete)
        );
    }
}
