// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Standard structural sections (STR-COL-001, STR-BEAM-001): catalogues of rolled and hollow
//! steel sections compiled from public sources (`data/sections`, with their provenance in its
//! README and the research log), and making a structural column type from a designation.
//!
//! A catalogue is a JSON file: its family, the standard it follows, its units (`mm` or `in`),
//! the shape of its sections, the public sources its numbers come from, and its sections with
//! their dimensions (I shapes and tees `d`, `bf`, `tw`, `tf`; channels the same; angles `h`, `b`,
//! `t`; rectangular hollow `h`, `b`, `t`; round hollow and pipe `od`, `t`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::columns::{ColumnType, MaterialKind, Section};
use crate::error::{Error, Result};

const CATALOGS: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/section_catalogs.rs"));

/// A catalogue of standard sections.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Catalog {
    /// The file it comes from, also its identifier (`aisc_w`, `en_heb`, …).
    #[serde(default)]
    pub id: String,
    pub family: String,
    pub standard: String,
    /// `mm` or `in`.
    pub units: String,
    /// The section shape: `i_shape`, `channel`, `angle`, `tee`, `rectangular_hollow`,
    /// `circular_hollow`.
    pub shape: Section,
    /// The steel grade a section of this family is usually rolled in, as the default material.
    #[serde(default)]
    pub default_material: Option<String>,
    #[serde(default)]
    pub source_ids: Vec<String>,
    pub sections: Vec<Value>,
}

/// Every catalogue embedded in Reviera, in file order.
pub fn catalogs() -> Vec<Catalog> {
    CATALOGS
        .iter()
        .filter_map(|(id, text)| {
            let mut c: Catalog = serde_json::from_str(text).ok()?;
            c.id = (*id).to_owned();
            Some(c)
        })
        .collect()
}

/// A catalogue by its identifier.
pub fn catalog(id: &str) -> Option<Catalog> {
    catalogs().into_iter().find(|c| c.id == id)
}

/// A catalogue's section as a structural column type (metres), named and designated by it.
pub fn column_type(c: &Catalog, designation: &str) -> Result<ColumnType> {
    let s = c
        .sections
        .iter()
        .find(|s| s["name"].as_str() == Some(designation))
        .ok_or_else(|| {
            Error::new(
                "section.not_found",
                format!("{} has no section {designation}", c.family),
            )
        })?;
    let scale = if c.units == "in" { 0.0254 } else { 0.001 };
    let dim = |k: &str| -> Result<f64> {
        s[k].as_f64().map(|v| v * scale).ok_or_else(|| {
            Error::new(
                "section.incomplete",
                format!("{designation} has no `{k}` in {}", c.id),
            )
        })
    };
    let mut t = ColumnType {
        section: c.shape,
        structural: true,
        material: c.default_material.clone().or_else(|| Some("Steel".into())),
        material_kind: Some(MaterialKind::Steel),
        designation: Some(designation.to_owned()),
        ..ColumnType::default()
    };
    match c.shape {
        Section::IShape | Section::Channel | Section::Tee => {
            t.depth = dim("d")?;
            t.width = dim("bf")?;
            t.web_thickness = dim("tw")?;
            t.flange_thickness = dim("tf")?;
        }
        Section::Angle | Section::RectangularHollow => {
            t.depth = dim("h")?;
            t.width = dim("b")?;
            t.wall_thickness = dim("t")?;
        }
        Section::CircularHollow => {
            t.width = dim("od")?;
            t.depth = t.width;
            t.wall_thickness = dim("t")?;
        }
        Section::Rectangular | Section::Round => {
            return Err(Error::new(
                "section.unsupported",
                "catalogues hold rolled and hollow sections",
            ));
        }
    }
    t.check()
        .map_err(|m| Error::new("section.invalid", format!("{designation}: {m}")))?;
    Ok(t)
}

/// A catalogue's listing.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CatalogInfo {
    pub id: String,
    pub family: String,
    pub standard: String,
    pub shape: Section,
    pub default_material: Option<String>,
    /// One of the model's region's usual catalogues (by its building code).
    pub preferred: bool,
    pub designations: Vec<String>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    /// One catalogue; default all.
    pub catalog: Option<String>,
}

/// The catalogues and their designations (`structural.catalogs`), the model's region's usual
/// ones first.
pub fn list(store: &crate::store::Store, p: &ListParams) -> Vec<CatalogInfo> {
    let preferred = crate::codes::settings(store).code.structural().catalogs;
    let mut all: Vec<CatalogInfo> = catalogs()
        .into_iter()
        .filter(|c| p.catalog.as_ref().is_none_or(|id| *id == c.id))
        .map(|c| CatalogInfo {
            preferred: preferred.contains(&c.id.as_str()),
            designations: c
                .sections
                .iter()
                .filter_map(|s| s["name"].as_str().map(str::to_owned))
                .collect(),
            id: c.id,
            family: c.family,
            standard: c.standard,
            shape: c.shape,
            default_material: c.default_material,
        })
        .collect();
    let rank = |id: &str| {
        preferred
            .iter()
            .position(|p| *p == id)
            .unwrap_or(usize::MAX)
    };
    all.sort_by_key(|c| rank(&c.id));
    all
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LoadParams {
    /// The catalogue (`structural.catalogs`), for example `aisc_w` or `en_heb`.
    pub catalog: String,
    /// The section's designation, for example `W12x40` or `HEB 300`.
    pub designation: String,
    /// The structural material; default the catalogue's usual grade.
    pub material: Option<String>,
}

/// Makes (or finds) a structural column type of a catalogue section (`column.load_section`).
pub(crate) fn load_column_section(
    txn: &mut crate::txn::Transaction<'_>,
    p: LoadParams,
) -> Result<crate::id::ElementId> {
    let c = catalog(&p.catalog)
        .ok_or_else(|| Error::new("section.no_catalog", format!("no catalogue {}", p.catalog)))?;
    let mut t = column_type(&c, &p.designation)?;
    if let Some(m) = p.material {
        t.material = Some(m);
    }
    // One type per designation and material.
    if let Some(e) = txn.store().iter().find(|e| {
        crate::columns::column_type(e).is_some_and(|x| {
            x.structural && x.designation == t.designation && x.material == t.material
        })
    }) {
        return Ok(e.id());
    }
    txn.create(
        crate::element::NewElement::new("IfcColumnType")
            .name(&p.designation)
            .param("@PredefinedType", "COLUMN")
            .param(
                crate::walls::RECIPE_KEY,
                serde_json::to_value(t).expect("serialises"),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_catalogue_section_makes_a_valid_type() {
        let all = catalogs();
        assert!(all.len() >= 12, "{} catalogues", all.len());
        let mut n = 0;
        for c in &all {
            assert!(!c.source_ids.is_empty(), "{} has no sources", c.id);
            for s in &c.sections {
                let name = s["name"].as_str().unwrap();
                let t = column_type(c, name).unwrap_or_else(|e| panic!("{}: {e:?}", c.id));
                assert!(t.area() > 0.0, "{name}");
                n += 1;
            }
        }
        assert!(n >= 250, "{n} sections");
        // HEB 300: 300 x 300, tw 11, tf 19 (EN 10365).
        let heb = catalog("eu_heb").unwrap();
        let t = column_type(&heb, "HEB 300").unwrap();
        assert!((t.depth - 0.3).abs() < 1e-12 && (t.web_thickness - 0.011).abs() < 1e-12);
        assert_eq!(t.section, Section::IShape);
        // Inches converted: W8X24 is 7.93 in deep.
        let w = catalog("aisc_w").unwrap();
        let t = column_type(&w, "W8X24").unwrap();
        assert!((t.depth - 7.93 * 0.0254).abs() < 1e-12);
        assert_eq!(t.material.as_deref(), Some("Steel ASTM A992"));
    }

    #[test]
    fn section_properties_from_dimensions() {
        let rect = ColumnType {
            width: 0.3,
            depth: 0.5,
            ..ColumnType::default()
        };
        let p = rect.properties();
        assert!((p.area - 0.15).abs() < 1e-12);
        assert!((p.ix - 0.3 * 0.5f64.powi(3) / 12.0).abs() < 1e-12);
        assert!((p.r_min - 0.3 / 12f64.sqrt()).abs() < 1e-12);
        // HEB 300: Iy 25 170 cm4 with its root fillets (EN 10365); without them a little less.
        let heb = column_type(&catalog("eu_heb").unwrap(), "HEB 300").unwrap();
        let strong = heb.properties().ix * 1e8;
        assert!(strong > 0.95 * 25_170.0 && strong < 25_170.0, "{strong}");
    }
}
