// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Glazing types for glazed enclosures (spec Ch 3 §3.6a.3, ARC-GLZ-003): a library of glazing
//! build-ups with the values analyses and checks need, and roof (Sloped Glazing) and wall
//! (curtain wall) types made from them.
//!
//! A glazing type keeps its values in the `Reviera_GlazingUnit` property set (ARC-GLZ-007):
//! light and PAR transmission, haze, U-value, solar heat gain coefficient, safety class,
//! thickness, weight, minimum roof pitch and, for multiwall sheet, its structure. Its U-value is
//! also its common property set's `ThermalTransmittance` (`Pset_RoofCommon`,
//! `Pset_CurtainWallCommon`), which the energy model reads. The library's values are typical
//! ones from public maker and extension data (research/09 §3), provisional (EXPERT_REVIEW E-46),
//! to be replaced by the specified product's.
//!
//! Frame systems (the bars) are `crate::frame_library`'s. Not yet: optical values on the material (`Pset_MaterialOptical`),
//! and the energy model's use of light transmission and SHGC for glazed roofs.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::curtain::{CurtainType, Layout, Mullion, Rule};
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::RECIPE_KEY;

pub const PSET: &str = "Reviera_GlazingUnit";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Safety {
    Annealed,
    Toughened,
    Laminated,
    Plastic,
}

/// A glazing build-up's values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Glazing {
    /// Visible light transmission, PAR transmission and haze (0–1).
    pub light: f64,
    pub par: f64,
    #[serde(default)]
    pub haze: f64,
    /// U-value, W/m²K.
    pub u: f64,
    /// Solar heat gain coefficient (g-value), 0–1.
    pub g: f64,
    pub safety: Safety,
    /// The laminate's interlayer, metres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interlayer: Option<f64>,
    /// Thickness, metres; weight, kg/m².
    pub thickness: f64,
    pub weight: f64,
    /// The least roof pitch it is laid at (radians).
    pub min_pitch: f64,
    /// Multiwall sheet: its structure (twin-wall, five-wall, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structure: Option<String>,
}

/// A library entry: its name, its values and their sources.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Entry {
    pub name: &'static str,
    pub glazing: Glazing,
    /// Research source IDs (research/09).
    pub sources: &'static str,
}

fn deg(d: f64) -> f64 {
    d.to_radians()
}

/// The built-in glazing build-ups (provisional values, EXPERT_REVIEW E-46).
pub fn library() -> Vec<Entry> {
    let glass = |light: f64, u: f64, g: f64, safety: Safety, thickness: f64| Glazing {
        light,
        par: light - 0.02,
        haze: 0.0,
        u,
        g,
        safety,
        interlayer: None,
        thickness,
        weight: 2500.0 * thickness,
        min_pitch: deg(2.5),
        structure: None,
    };
    let sheet = |light: f64, u: f64, thickness: f64, weight: f64, structure: &str| Glazing {
        light,
        par: light - 0.03,
        haze: 0.1,
        u,
        g: light - 0.05,
        safety: Safety::Plastic,
        interlayer: None,
        thickness,
        weight,
        min_pitch: deg(5.0),
        structure: Some(structure.to_owned()),
    };
    vec![
        Entry { name: "Horticultural Float 4 mm", glazing: glass(0.90, 5.8, 0.85, Safety::Annealed, 0.004), sources: "G034 G053 G076" },
        Entry { name: "Toughened 4 mm", glazing: glass(0.90, 5.8, 0.85, Safety::Toughened, 0.004), sources: "G034 G053 G076" },
        Entry {
            name: "Diffuse AR Glass 4 mm",
            glazing: Glazing { haze: 0.72, ..glass(0.83, 5.8, 0.80, Safety::Toughened, 0.004) },
            sources: "G062",
        },
        Entry {
            name: "Laminated 6.4 mm",
            glazing: Glazing { interlayer: Some(0.00038), ..glass(0.88, 5.7, 0.75, Safety::Laminated, 0.0064) },
            sources: "G022 G034",
        },
        Entry {
            name: "Low-E Double Unit 24 mm",
            glazing: Glazing { weight: 20.0, ..glass(0.77, 1.1, 0.55, Safety::Toughened, 0.024) },
            sources: "G055 G075",
        },
        Entry { name: "Twin-Wall Polycarbonate 8 mm", glazing: sheet(0.88, 3.4, 0.008, 1.5, "twin-wall"), sources: "G034" },
        Entry { name: "Twin-Wall Polycarbonate 10 mm", glazing: sheet(0.88, 3.2, 0.010, 1.7, "twin-wall"), sources: "G034" },
        Entry { name: "Five-Wall Polycarbonate 16 mm", glazing: sheet(0.73, 1.9, 0.016, 2.7, "five-wall"), sources: "G034" },
        Entry { name: "Five-Wall Polycarbonate 25 mm", glazing: sheet(0.68, 1.6, 0.025, 3.4, "five-wall"), sources: "G034" },
        Entry { name: "Acrylic Multi-Skin 16 mm", glazing: sheet(0.91, 2.5, 0.016, 2.5, "multi-skin"), sources: "G078" },
        Entry {
            name: "Polyethylene Film 0.15 mm",
            glazing: Glazing { haze: 0.0, min_pitch: deg(5.0), ..sheet(0.86, 6.8, 0.00015, 0.14, "single film") },
            sources: "G003 G004 G011 G076",
        },
        Entry {
            name: "Double Inflated Polyethylene",
            glazing: Glazing { haze: 0.0, ..sheet(0.75, 4.0, 0.0003, 0.28, "double inflated film") },
            sources: "G003 G011 G035 G076",
        },
    ]
}

/// A library entry by name.
pub fn entry(name: &str) -> Option<Entry> {
    library().into_iter().find(|e| e.name.eq_ignore_ascii_case(name))
}

fn typed(t: &str, v: Value) -> Value {
    json!({ "type": t, "value": v })
}

/// The type's parameters for its glazing values.
fn params(class: &str, g: &Glazing) -> Vec<(String, Value)> {
    let k = |p: &str| format!("{PSET}.{p}");
    let mut out = vec![
        (k("VisibleTransmittance"), typed("IfcNormalisedRatioMeasure", json!(g.light))),
        (k("ParTransmittance"), typed("IfcNormalisedRatioMeasure", json!(g.par))),
        (k("Haze"), typed("IfcNormalisedRatioMeasure", json!(g.haze))),
        (k("ThermalTransmittance"), typed("IfcThermalTransmittanceMeasure", json!(g.u))),
        (k("SolarHeatGainCoefficient"), typed("IfcNormalisedRatioMeasure", json!(g.g))),
        (k("SafetyClass"), typed("IfcLabel", json!(serde_json::to_value(g.safety).expect("serialises")))),
        (k("Thickness"), typed("IfcPositiveLengthMeasure", json!(g.thickness))),
        (k("MassPerArea"), typed("IfcMassPerLengthMeasure", json!(g.weight))),
        (k("MinimumPitch"), typed("IfcPlaneAngleMeasure", json!(g.min_pitch))),
    ];
    if let Some(i) = g.interlayer {
        out.push((k("Interlayer"), typed("IfcPositiveLengthMeasure", json!(i))));
    }
    if let Some(s) = &g.structure {
        out.push((k("Structure"), typed("IfcLabel", json!(s))));
    }
    // The energy model's U-value (ANA-EM-003).
    out.push((format!("{}.ThermalTransmittance", crate::energy_model::common_pset(class)), typed("IfcThermalTransmittanceMeasure", json!(g.u))));
    out
}

/// A type's glazing values, if it has them.
pub fn of_type(e: &Element) -> Option<Glazing> {
    let num = |p: &str| e.param(&format!("{PSET}.{p}")).and_then(|v| v.get("value")?.as_f64());
    let text = |p: &str| e.param(&format!("{PSET}.{p}")).and_then(|v| v.get("value")?.as_str().map(str::to_owned));
    Some(Glazing {
        light: num("VisibleTransmittance")?,
        par: num("ParTransmittance").unwrap_or(0.0),
        haze: num("Haze").unwrap_or(0.0),
        u: num("ThermalTransmittance")?,
        g: num("SolarHeatGainCoefficient").unwrap_or(0.0),
        safety: text("SafetyClass").and_then(|s| serde_json::from_value(json!(s)).ok()).unwrap_or(Safety::Annealed),
        interlayer: num("Interlayer"),
        thickness: num("Thickness").unwrap_or(0.004),
        weight: num("MassPerArea").unwrap_or(0.0),
        min_pitch: num("MinimumPitch").unwrap_or(0.0),
        structure: text("Structure"),
    })
}

/// Writes glazing values onto type `id`.
pub(crate) fn write(txn: &mut Transaction<'_>, id: ElementId, g: &Glazing) -> Result<()> {
    let class = txn.store().get(id).expect("exists").class.clone();
    let values = params(&class, g);
    txn.modify(id, |e| {
        for (k, v) in values {
            e.params.insert(k, v);
        }
    })
}

/// Where a glazing type goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Use {
    /// A Sloped Glazing roof type.
    #[default]
    Roof,
    /// A curtain wall type.
    Wall,
}

/// Makes (or finds) a glazing type of library entry `name` for `use_`, with `grid` the bars'
/// spacing (along, up) and `bar` their section, on library frame system `frame` if given.
pub(crate) fn type_for(txn: &mut Transaction<'_>, name: &str, use_: Use, grid: [f64; 2], bar: Mullion, frame: Option<&str>) -> Result<ElementId> {
    let e = entry(name).ok_or_else(|| Error::new("glazing.unknown", format!("no glazing {name} in the library (glazing.list_types)")))?;
    let frame = frame.map(|f| crate::frame_library::entry(f).ok_or_else(|| Error::new("frame.unknown", format!("no frame system {f} in the library (frame.list_types)")))).transpose()?;
    let on = frame.as_ref().map_or(String::new(), |f| format!(" on {}", f.name));
    let label = format!("{}{on} ({})", e.name, if use_ == Use::Roof { "roof" } else { "wall" });
    let is = |x: &Element| x.name.as_deref() == Some(label.as_str()) && of_type(x).is_some();
    if let Some(x) = txn.store().iter().find(|x| is(x)) {
        return Ok(x.id());
    }
    let ty = CurtainType {
        u: Rule { layout: Layout::MaximumSpacing, spacing: grid[0], number: 0 },
        v: Rule { layout: if grid[1] > 0.0 { Layout::MaximumSpacing } else { Layout::None }, spacing: grid[1], number: 0 },
        interior: Some(bar),
        border: Some(bar),
        panel_thickness: e.glazing.thickness.max(0.002),
        ..CurtainType::default()
    };
    let id = match use_ {
        Use::Roof => txn.create(NewElement::new(crate::roofs::TYPE_CLASS).name(&label).param("@PredefinedType", "NOTDEFINED").param(
            RECIPE_KEY,
            json!({ "kind": crate::roofs::ROOF_TYPE_RECIPE, "thickness": ty.panel_thickness, "glazing": serde_json::to_value(&ty).expect("serialises") }),
        ))?,
        Use::Wall => txn.create(
            NewElement::new("IfcCurtainWallType")
                .name(&label)
                .param("@PredefinedType", "NOTDEFINED")
                .param(RECIPE_KEY, serde_json::to_value(ty).expect("serialises")),
        )?,
    };
    write(txn, id, &e.glazing)?;
    if let Some(f) = frame {
        crate::frame_library::write(txn, id, &f.frame)?;
    }
    Ok(id)
}

// ---- Commands ----

/// What `glazing.list_types` returns: the library, and the project's glazing types.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Listing {
    pub library: Vec<Entry>,
    pub types: Vec<TypeInfo>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TypeInfo {
    pub id: ElementId,
    pub name: String,
    pub glazing: Glazing,
}

/// `glazing.list_types`.
pub(crate) fn list(store: &Store, _: crate::builtin::NoParams) -> Result<Listing> {
    let types = store
        .iter()
        .filter_map(|e| Some(TypeInfo { id: e.id(), name: e.name.clone().unwrap_or_default(), glazing: of_type(e)? }))
        .collect();
    Ok(Listing { library: library(), types })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// A library entry's name (`glazing.list_types`).
    pub glazing: String,
    #[serde(default, rename = "for")]
    pub use_: Use,
    /// The bars' spacing along the eave (roof) or wall, metres (default 1.125 roof, 0.8 wall).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub spacing: Option<f64>,
    /// The bars' spacing up the slope or wall (default 2.0 roof, 1.5 wall; 0 for none).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub rise: Option<f64>,
}

/// `glazing.create_type`.
pub(crate) fn create_type(txn: &mut Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let (along, up) = match p.use_ {
        Use::Roof => (p.spacing.unwrap_or(1.125), p.rise.unwrap_or(2.0)),
        Use::Wall => (p.spacing.unwrap_or(0.8), p.rise.unwrap_or(1.5)),
    };
    if !(0.2..=5.0).contains(&along) || !(up == 0.0 || (0.2..=10.0).contains(&up)) {
        return Err(Error::new("glazing.bad_spacing", "bars are 0.2 to 5 m apart along, and 0 (none) or 0.2 to 10 m up"));
    }
    type_for(txn, &p.glazing, p.use_, [along, up], Mullion { width: 0.04, depth: 0.08 }, None)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub type_id: ElementRef,
    /// Values to change (any of `light`, `par`, `haze`, `u`, `g`, `safety`, `thickness`, `weight`,
    /// `min_pitch`, `structure`), or a library entry to take them all from.
    pub glazing: Option<String>,
    pub values: Option<Value>,
}

/// `glazing.set_type`.
pub(crate) fn set_type(txn: &mut Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let id = store.resolve(&p.type_id).ok_or_else(|| Error::new("element.not_found", "no such type"))?;
    let e = store.get(id).expect("resolved");
    if roofs_glazed(e).is_none() && crate::curtain::curtain_type(e).is_none() {
        return Err(Error::new("glazing.not_a_type", format!("element {id} is not a sloped glazing or curtain wall type")));
    }
    let mut g = match &p.glazing {
        Some(n) => entry(n).ok_or_else(|| Error::new("glazing.unknown", format!("no glazing {n} in the library")))?.glazing,
        None => of_type(e).unwrap_or_else(|| library()[0].glazing.clone()),
    };
    if let Some(v) = &p.values {
        let mut current = serde_json::to_value(&g).expect("serialises");
        if let (Some(c), Some(o)) = (current.as_object_mut(), v.as_object()) {
            for (k, x) in o {
                c.insert(k.clone(), x.clone());
            }
        }
        g = serde_json::from_value(current).map_err(|e| Error::new("glazing.bad_values", e.to_string()))?;
    }
    let ratio = |x: f64| (0.0..=1.0).contains(&x);
    if !(ratio(g.light) && ratio(g.par) && ratio(g.haze) && ratio(g.g) && g.u > 0.0 && g.u < 20.0 && g.thickness > 0.0) {
        return Err(Error::new("glazing.bad_values", "transmissions, haze and g are 0 to 1; U above 0 and below 20 W/m²K; thickness above 0"));
    }
    write(txn, id, &g)
}

fn roofs_glazed(e: &Element) -> Option<CurtainType> {
    crate::roofs::glazing::of_type(e)
}

/// The published definitions of Reviera's property sets (ARC-GLZ-007; MIT).
pub const DEFINITIONS: &str = include_str!("../../../schemas/psets/reviera-psets-1.json");
pub const DEFINITIONS_SCHEMA: &str = include_str!("../../../schemas/psets/reviera.psets-1.schema.json");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_published_definitions_cover_what_is_written() {
        let defs: Value = serde_json::from_str(DEFINITIONS).unwrap();
        let schema: Value = serde_json::from_str(DEFINITIONS_SCHEMA).unwrap();
        assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&defs), "the definitions follow their schema");
        let set = defs["property_sets"].as_array().unwrap().iter().find(|s| s["name"] == PSET).expect("Reviera_GlazingUnit");
        let props = set["properties"].as_array().unwrap();
        // Every value a glazing type gets is defined, with the measure it is written as.
        let mut g = library()[3].glazing.clone();
        g.structure = Some("twin-wall".into());
        for (key, value) in params("IfcRoofType", &g) {
            let Some(name) = key.strip_prefix(&format!("{PSET}.")) else { continue };
            let def = props.iter().find(|p| p["name"] == name).unwrap_or_else(|| panic!("{name} is not published"));
            assert_eq!(def["measure"], value["type"], "{name}");
        }
    }

    #[test]
    fn the_library_is_sane() {
        let lib = library();
        assert!(lib.len() >= 10);
        for e in &lib {
            let g = &e.glazing;
            assert!((0.0..=1.0).contains(&g.light) && g.par <= g.light && g.u > 0.5 && g.u < 8.0, "{}", e.name);
        }
        assert!(entry("twin-wall polycarbonate 10 mm").is_some());
        // Insulating a roof costs light: the five-wall sheet lets less through than the twin-wall.
        assert!(entry("Five-Wall Polycarbonate 16 mm").unwrap().glazing.light < entry("Twin-Wall Polycarbonate 10 mm").unwrap().glazing.light);
    }
}
