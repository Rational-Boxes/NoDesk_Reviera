// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Frame systems for glazed enclosures (spec Ch 3 §3.6a.3, ARC-GLZ-003): a library of glazing
//! bar systems (aluminium, galvanised steel, timber, uPVC) with their bar section, finish and
//! the deflection limit of members carrying glass, and frame types made from them.
//!
//! A frame type is an `IfcMemberType` (MULLION) whose values are the `Reviera_FrameSystem`
//! property set (ARC-GLZ-007). A glazing type made on a frame system (an enclosure's
//! `frame_system`) carries the same set beside its `Reviera_GlazingUnit`, its bars being that
//! frame's. The deflection limit, L/175 or 15 mm, is the common baseline for members carrying
//! glass (provisional, EXPERT_REVIEW E-52); the structural checks of a later phase use it.
//!
//! Not yet: profiles on the member types, the frame's own members (rafters, purlins, trusses),
//! and the frame's shading in the crop-light analysis.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::curtain::Mullion;
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::store::Store;
use crate::txn::Transaction;

pub const PSET: &str = "Reviera_FrameSystem";
pub const TYPE_CLASS: &str = "IfcMemberType";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Material {
    Aluminium,
    Steel,
    Timber,
    Upvc,
}

/// A frame system's values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Frame {
    pub material: Material,
    pub finish: String,
    /// The glazing bar's section: across the glazing's face (`width`) and through it (`depth`),
    /// metres.
    pub bar_width: f64,
    pub bar_depth: f64,
    /// The deflection limit of members carrying glass: span / `deflection_ratio`, and at most
    /// `deflection_max` metres when set.
    pub deflection_ratio: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deflection_max: Option<f64>,
}

impl Frame {
    pub fn bar(&self) -> Mullion {
        Mullion { width: self.bar_width, depth: self.bar_depth }
    }

    /// The deflection allowed over a member's span (metres).
    pub fn allowed_deflection(&self, span: f64) -> f64 {
        let d = span / self.deflection_ratio;
        self.deflection_max.map_or(d, |m| d.min(m))
    }
}

/// A library entry: its name, its values and their sources.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Entry {
    pub name: &'static str,
    pub frame: Frame,
    /// Research source IDs (research/09, SOURCE_REGISTER).
    pub sources: &'static str,
}

/// The glass enclosures' default frame, and the tunnels' hoops.
pub const DEFAULT: &str = "Aluminium Glazing Bar 40×80";
pub const TUNNEL_DEFAULT: &str = "Galvanised Steel Hoop 50";

/// The built-in frame systems (provisional values, EXPERT_REVIEW E-52).
pub fn library() -> Vec<Entry> {
    let frame = |material: Material, finish: &str, w: f64, d: f64| Frame {
        material,
        finish: finish.to_owned(),
        bar_width: w,
        bar_depth: d,
        deflection_ratio: 175.0,
        deflection_max: Some(0.015),
    };
    vec![
        Entry { name: DEFAULT, frame: frame(Material::Aluminium, "mill finish", 0.04, 0.08), sources: "G009 G088 G089" },
        Entry { name: "Aluminium Glazing Bar 50×100", frame: frame(Material::Aluminium, "anodised", 0.05, 0.10), sources: "G088 G089" },
        Entry { name: TUNNEL_DEFAULT, frame: frame(Material::Steel, "hot-dip galvanised", 0.05, 0.05), sources: "G003 G069" },
        Entry { name: "Timber Glazing Bar 50×100", frame: frame(Material::Timber, "stained", 0.05, 0.10), sources: "G088" },
        Entry { name: "uPVC Glazing Bar 60×90", frame: frame(Material::Upvc, "white", 0.06, 0.09), sources: "G088" },
    ]
}

/// A library entry by name.
pub fn entry(name: &str) -> Option<Entry> {
    library().into_iter().find(|e| e.name.eq_ignore_ascii_case(name))
}

fn typed(t: &str, v: Value) -> Value {
    json!({ "type": t, "value": v })
}

/// The parameters for a frame's values.
fn params(f: &Frame) -> Vec<(String, Value)> {
    let k = |p: &str| format!("{PSET}.{p}");
    let mut out = vec![
        (k("Material"), typed("IfcLabel", serde_json::to_value(f.material).expect("serialises"))),
        (k("Finish"), typed("IfcLabel", json!(f.finish))),
        (k("BarWidth"), typed("IfcPositiveLengthMeasure", json!(f.bar_width))),
        (k("BarDepth"), typed("IfcPositiveLengthMeasure", json!(f.bar_depth))),
        (k("DeflectionRatio"), typed("IfcPositiveRatioMeasure", json!(f.deflection_ratio))),
    ];
    if let Some(m) = f.deflection_max {
        out.push((k("MaximumDeflection"), typed("IfcPositiveLengthMeasure", json!(m))));
    }
    out
}

/// A type's frame values, if it has them.
pub fn of_type(e: &Element) -> Option<Frame> {
    let num = |p: &str| e.param(&format!("{PSET}.{p}")).and_then(|v| v.get("value")?.as_f64());
    let text = |p: &str| e.param(&format!("{PSET}.{p}")).and_then(|v| v.get("value")?.as_str().map(str::to_owned));
    Some(Frame {
        material: serde_json::from_value(json!(text("Material")?)).ok()?,
        finish: text("Finish").unwrap_or_default(),
        bar_width: num("BarWidth")?,
        bar_depth: num("BarDepth")?,
        deflection_ratio: num("DeflectionRatio").unwrap_or(175.0),
        deflection_max: num("MaximumDeflection"),
    })
}

/// Writes frame values onto type `id`.
pub(crate) fn write(txn: &mut Transaction<'_>, id: ElementId, f: &Frame) -> Result<()> {
    let values = params(f);
    txn.modify(id, |e| {
        for (k, v) in values {
            e.params.insert(k, v);
        }
    })
}

fn check(f: &Frame) -> Result<()> {
    if !(f.bar_width >= 0.01 && f.bar_width <= 0.5 && f.bar_depth >= 0.01 && f.bar_depth <= 1.0) {
        return Err(Error::new("frame.bad_values", "a glazing bar is 10–500 mm wide and 10 mm–1 m deep"));
    }
    if !(f.deflection_ratio >= 50.0 && f.deflection_ratio <= 2000.0) || f.deflection_max.is_some_and(|m| !(m > 0.0 && m < 0.5)) {
        return Err(Error::new("frame.bad_values", "the deflection limit is span / 50 to span / 2000, at most under 500 mm"));
    }
    Ok(())
}

// ---- Commands ----

/// What `frame.list_types` returns: the library, and the project's types with frame values
/// (frame types, and glazing types made on a frame).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Listing {
    pub library: Vec<Entry>,
    pub types: Vec<TypeInfo>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TypeInfo {
    pub id: ElementId,
    pub name: String,
    pub class: String,
    pub frame: Frame,
}

/// `frame.list_types`.
pub(crate) fn list(store: &Store, _: crate::builtin::NoParams) -> Result<Listing> {
    let types = store
        .iter()
        .filter_map(|e| Some(TypeInfo { id: e.id(), name: e.name.clone().unwrap_or_default(), class: e.class.clone(), frame: of_type(e)? }))
        .collect();
    Ok(Listing { library: library(), types })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// A library entry's name (`frame.list_types`).
    pub frame: String,
    /// The new type's name; the entry's by default.
    pub name: Option<String>,
}

/// `frame.create_type`: makes (or finds, by name) an `IfcMemberType` of a library frame system.
pub(crate) fn create_type(txn: &mut Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let e = entry(&p.frame).ok_or_else(|| Error::new("frame.unknown", format!("no frame system {} in the library (frame.list_types)", p.frame)))?;
    let name = p.name.unwrap_or_else(|| e.name.to_owned());
    if let Some(x) = txn.store().iter().find(|x| x.class == TYPE_CLASS && x.name.as_deref() == Some(name.as_str()) && of_type(x).is_some()) {
        return Ok(x.id());
    }
    let id = txn.create(NewElement::new(TYPE_CLASS).name(&name).param("@PredefinedType", "MULLION"))?;
    write(txn, id, &e.frame)?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub type_id: ElementRef,
    /// A library entry to take every value from, and/or `values` to change (any of `material`,
    /// `finish`, `bar_width`, `bar_depth`, `deflection_ratio`, `deflection_max`).
    pub frame: Option<String>,
    pub values: Option<Value>,
}

/// `frame.set_type`: a frame type's values (or a glazing type's frame values; its bars are
/// those its enclosure makes, and change with the enclosure's frame system).
pub(crate) fn set_type(txn: &mut Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let id = store.resolve(&p.type_id).ok_or_else(|| Error::new("element.not_found", "no such type"))?;
    let e = store.get(id).expect("resolved");
    let current = of_type(e);
    if current.is_none() && e.class != TYPE_CLASS {
        return Err(Error::new("frame.not_a_type", format!("element {id} is not a frame type")));
    }
    let mut f = match &p.frame {
        Some(n) => entry(n).ok_or_else(|| Error::new("frame.unknown", format!("no frame system {n} in the library")))?.frame,
        None => current.unwrap_or_else(|| library()[0].frame.clone()),
    };
    if let Some(v) = &p.values {
        let mut c = serde_json::to_value(&f).expect("serialises");
        if let (Some(c), Some(o)) = (c.as_object_mut(), v.as_object()) {
            for (k, x) in o {
                c.insert(k.clone(), x.clone());
            }
        }
        f = serde_json::from_value(c).map_err(|e| Error::new("frame.bad_values", e.to_string()))?;
    }
    check(&f)?;
    if f.deflection_max.is_none() {
        txn.modify(id, |e| {
            e.params.remove(&format!("{PSET}.MaximumDeflection"));
        })?;
    }
    write(txn, id, &f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_published_definitions_cover_what_is_written() {
        let defs: Value = serde_json::from_str(crate::glazing_library::DEFINITIONS).unwrap();
        let set = defs["property_sets"].as_array().unwrap().iter().find(|s| s["name"] == PSET).expect("Reviera_FrameSystem");
        let props = set["properties"].as_array().unwrap();
        for (key, value) in params(&library()[0].frame) {
            let name = key.strip_prefix(&format!("{PSET}.")).unwrap();
            let def = props.iter().find(|p| p["name"] == name).unwrap_or_else(|| panic!("{name} is not published"));
            assert_eq!(def["measure"], value["type"], "{name}");
        }
    }

    #[test]
    fn the_library_is_sane() {
        for e in library() {
            check(&e.frame).unwrap();
        }
        let f = entry(DEFAULT).unwrap().frame;
        // L/175 over a 2 m bar, capped at 15 mm over a long one.
        assert!((f.allowed_deflection(2.0) - 2.0 / 175.0).abs() < 1e-12);
        assert!((f.allowed_deflection(4.0) - 0.015).abs() < 1e-12);
        assert!(entry(TUNNEL_DEFAULT).is_some());
    }
}
