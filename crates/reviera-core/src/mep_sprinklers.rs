// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Sprinklers (MEP-PIPE-007): components of the Sprinkler category, placed by how their head is
//! mounted.
//!
//! - **Pendent** heads (and recessed, concealed and ceiling heads) hang below a ceiling, piped
//!   from above: under the ceiling over the point on the level (or the one given), their top at its
//!   underside; they then follow the ceiling up and down. With no ceiling there, they hang at the
//!   default ceiling height.
//! - **Upright** heads stand on the pipe below the deck, piped from below: at an offset above the
//!   level (or on top of a ceiling given).
//! - **Sidewall** heads go on a wall face as plumbing fixtures do, their back against it.
//!
//! Heads are chosen, not sized: `sprinkler_type.set` sets a type's K-factor, activation
//! temperature, response, activation, coverage and connection size, kept as
//! `Pset_FireSuppressionTerminalTypeSprinkler` on the type (IFC4 has no KFactor property: the
//! K-factor is its `DischargeCoefficient`, in L/min/bar^½; temperatures are in kelvin, the IFC
//! default unit). A bulb's liquid colour follows its temperature rating.
//!
//! In IFC each head is an `IfcFireSuppressionTerminal` (`SPRINKLER`) of an
//! `IfcFireSuppressionTerminalType`.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::mesh_objects::{self, Category};
use crate::store::Store;
use crate::txn::Transaction;

/// The property set kept on sprinkler types.
pub const PSET: &str = "Pset_FireSuppressionTerminalTypeSprinkler";
/// Mounting height of sidewall heads above the level (metres; EXPERT_REVIEW E-27).
pub const SIDEWALL_HEIGHT: f64 = 2.45;
/// Height of upright heads above the level with no ceiling given (metres; E-27).
pub const UPRIGHT_HEIGHT: f64 = 3.0;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

fn key(property: &str) -> String {
    format!("{PSET}.{property}")
}

/// How a head is mounted (the pset's `Type`, `PEnum_SprinklerType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mounting {
    Pendant,
    RecessedPendant,
    Concealed,
    Ceiling,
    Upright,
    Sidewall,
    Cutoff,
    Other,
}

impl Mounting {
    fn ifc(self) -> &'static str {
        match self {
            Mounting::Pendant => "PENDANT",
            Mounting::RecessedPendant => "RECESSEDPENDANT",
            Mounting::Concealed => "CONCEALED",
            Mounting::Ceiling => "CEILING",
            Mounting::Upright => "UPRIGHT",
            Mounting::Sidewall => "SIDEWALL",
            Mounting::Cutoff => "CUTOFF",
            Mounting::Other => "OTHER",
        }
    }

    fn of_ifc(s: &str) -> Option<Mounting> {
        [
            Mounting::Pendant,
            Mounting::RecessedPendant,
            Mounting::Concealed,
            Mounting::Ceiling,
            Mounting::Upright,
            Mounting::Sidewall,
            Mounting::Cutoff,
            Mounting::Other,
        ]
        .into_iter()
        .find(|m| m.ifc() == s)
    }

    /// Hangs below a ceiling.
    fn hangs(self) -> bool {
        matches!(self, Mounting::Pendant | Mounting::RecessedPendant | Mounting::Concealed | Mounting::Ceiling | Mounting::Other | Mounting::Cutoff)
    }
}

/// A sprinkler type's mounting (pendent when it does not say).
pub fn mounting(t: &Element) -> Mounting {
    t.param(&key("Type")).and_then(|v| v.get("value").unwrap_or(v).as_str()).and_then(Mounting::of_ifc).unwrap_or(Mounting::Pendant)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Response {
    Quick,
    Standard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Activation {
    Bulb,
    FusibleSolder,
}

/// The liquid colour of a glass bulb rated at `celsius` (the usual bulb colour code: up to 57 °C
/// orange, 68 red, 79 yellow, 93 green, 141 blue, 182 mauve; E-27).
pub fn bulb_colour(celsius: f64) -> &'static str {
    match celsius {
        t if t <= 57.0 => "ORANGE",
        t if t <= 68.0 => "RED",
        t if t <= 79.0 => "YELLOW",
        t if t <= 100.0 => "GREEN",
        t if t <= 141.0 => "BLUE",
        t if t <= 182.0 => "MAUVE",
        _ => "OTHER",
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetTypeParams {
    pub type_id: ElementRef,
    pub mounting: Option<Mounting>,
    /// K-factor (L/min/bar^½; 80 is the common standard-orifice head).
    pub k_factor: Option<f64>,
    /// Activation temperature (°C).
    pub activation_temperature: Option<f64>,
    pub response: Option<Response>,
    pub activation: Option<Activation>,
    /// Area one head protects (m²).
    #[schemars(extend("x-reviera-data-type" = "area"))]
    pub coverage_area: Option<f64>,
    /// Inlet connection (thread) size (metres).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub connection_size: Option<f64>,
}

fn typed(ty: &str, v: impl Into<Value>) -> Value {
    json!({ "type": ty, "value": v.into() })
}

/// `sprinkler_type.set`: the head's properties, on the type.
pub(crate) fn set_type(txn: &mut Transaction<'_>, p: SetTypeParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let id = store.resolve(&p.type_id).ok_or_else(|| bad("element.not_found", "no such type"))?;
    let e = store.get(id).expect("resolved");
    let ty = mesh_objects::mesh_object_type(e).ok_or_else(|| bad("mesh_object.not_a_type", format!("element {id} is not a component type")))?;
    if ty.category != Category::Sprinkler {
        return Err(bad("sprinkler.not_a_sprinkler", format!("{} is a {}, not a sprinkler", e.name.clone().unwrap_or_default(), ty.category.label())));
    }
    let positive = |name: &str, v: Option<f64>| -> Result<()> {
        match v {
            Some(x) if !(x > 0.0 && x.is_finite()) => Err(bad("sprinkler.bad_value", format!("the {name} must be above zero"))),
            _ => Ok(()),
        }
    };
    positive("K-factor", p.k_factor)?;
    positive("coverage area", p.coverage_area)?;
    positive("connection size", p.connection_size)?;
    if let Some(t) = p.activation_temperature
        && !(t > 0.0 && t < 400.0)
    {
        return Err(bad("sprinkler.bad_value", "the activation temperature must be between 0 and 400 °C"));
    }
    let mut set: Vec<(String, Value)> = Vec::new();
    if let Some(m) = p.mounting {
        set.push((key("Type"), typed("IfcLabel", m.ifc())));
    }
    if let Some(k) = p.k_factor {
        set.push((key("DischargeCoefficient"), typed("IfcReal", k)));
    }
    if let Some(t) = p.activation_temperature {
        set.push((key("ActivationTemperature"), typed("IfcThermodynamicTemperatureMeasure", t + 273.15)));
    }
    if let Some(r) = p.response {
        set.push((key("Response"), typed("IfcLabel", if r == Response::Quick { "QUICK" } else { "STANDARD" })));
    }
    if let Some(a) = p.activation {
        set.push((key("Activation"), typed("IfcLabel", if a == Activation::Bulb { "BULB" } else { "FUSIBLESOLDER" })));
    }
    if let Some(a) = p.coverage_area {
        set.push((key("CoverageArea"), typed("IfcAreaMeasure", a)));
    }
    if let Some(c) = p.connection_size {
        set.push((key("ConnectionSize"), typed("IfcPositiveLengthMeasure", c)));
    }
    // The bulb's colour, from its rating, while it has a bulb.
    let temperature = p.activation_temperature.or_else(|| e.param(&key("ActivationTemperature")).and_then(|v| v["value"].as_f64()).map(|k| k - 273.15));
    let bulb = p.activation.map(|a| a == Activation::Bulb).unwrap_or_else(|| e.param(&key("Activation")).is_none_or(|v| v["value"] == "BULB"));
    let colour = temperature.filter(|_| bulb).map(bulb_colour);
    txn.modify(id, |x| {
        for (k, v) in set {
            x.params.insert(k, v);
        }
        match colour {
            Some(c) => {
                x.params.insert(key("BulbLiquidColor"), typed("IfcLabel", c));
            }
            None if !bulb => {
                x.params.remove(&key("BulbLiquidColor"));
            }
            None => {}
        }
    })?;
    Ok(id)
}

/// The starter sprinkler types' heads: standard-orifice, quick-response 68 °C bulbs.
pub(crate) fn starter_head(name: &str) -> Option<SetTypeParams> {
    let mounting = match name {
        "Pendent Sprinkler" => Mounting::Pendant,
        "Upright Sprinkler" => Mounting::Upright,
        "Sidewall Sprinkler" => Mounting::Sidewall,
        _ => return None,
    };
    Some(SetTypeParams {
        type_id: ElementRef::Id(ElementId(0)),
        mounting: Some(mounting),
        k_factor: Some(80.0),
        activation_temperature: Some(68.0),
        response: Some(Response::Quick),
        activation: Some(Activation::Bulb),
        coverage_area: Some(12.0),
        connection_size: Some(0.015),
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceParams {
    /// A sprinkler type.
    pub type_id: ElementRef,
    /// In plan (metres).
    pub point: [f64; 2],
    pub level: ElementRef,
    /// Height of its insertion point (bottom): from the ceiling's underside under a ceiling
    /// (pendent heads: minus their height, so their top is at it); else above the level.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: Option<f64>,
    /// The ceiling it hangs from (or an upright head stands on); omitted, the ceiling on `level`
    /// over the point, for pendent heads.
    pub host_ceiling: Option<ElementRef>,
    /// Sidewall heads: the wall whose face they go on (else the nearest within 300 mm).
    pub host_wall: Option<ElementRef>,
    /// Hosted by nothing: at `offset` above the level.
    #[serde(default)]
    pub work_plane: bool,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: f64,
}

/// The Reviera ceilings on `level` over `p`, lowest first.
pub(crate) fn ceilings_over(store: &Store, level: &str, p: [f64; 2]) -> Vec<ElementId> {
    let mut v: Vec<(ElementId, f64)> = store
        .iter()
        .filter_map(|e| {
            let (kind, r) = crate::floors::slab_recipe(e)?;
            (kind == crate::floors::SlabKind::Ceiling && r.level == level).then_some(())?;
            let g = crate::floors::regenerate(store, e).ok()?;
            g.profiles.iter().any(|pr| pr.contains(p)).then(|| (e.id(), g.top - g.thickness))
        })
        .collect();
    v.sort_by(|a, b| a.1.total_cmp(&b.1));
    v.into_iter().map(|x| x.0).collect()
}

/// `sprinkler.place`.
pub(crate) fn place(txn: &mut Transaction<'_>, p: PlaceParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let type_id = store.resolve(&p.type_id).ok_or_else(|| bad("element.not_found", "no such type"))?;
    let t = store.get(type_id).expect("resolved");
    let ty = mesh_objects::mesh_object_type(t).ok_or_else(|| bad("mesh_object.not_a_type", format!("element {type_id} is not a component type")))?;
    if ty.category != Category::Sprinkler {
        return Err(bad("sprinkler.not_a_sprinkler", format!("{} is a {}, not a sprinkler", t.name.clone().unwrap_or_default(), ty.category.label())));
    }
    let m = mounting(t);
    if m == Mounting::Sidewall {
        if p.host_ceiling.is_some() {
            return Err(bad("sprinkler.sidewall_ceiling", "sidewall heads go on a wall, not a ceiling"));
        }
        let args = json!({ "type_id": type_id.0, "point": p.point, "level": p.level, "offset": p.offset.unwrap_or(SIDEWALL_HEIGHT), "rotation": p.rotation, "host_wall": p.host_wall, "work_plane": p.work_plane });
        let cp = serde_json::from_value(args).map_err(|e| bad("sprinkler.params", e.to_string()))?;
        return crate::mep_controls::place_hosted(txn, cp, |c| c == Category::Sprinkler, "sprinkler.not_a_sprinkler", "a sprinkler", SIDEWALL_HEIGHT);
    }
    if p.host_wall.is_some() {
        return Err(bad("sprinkler.not_sidewall", "only sidewall heads go on a wall"));
    }
    let level_id = store.resolve(&p.level).ok_or_else(|| bad("element.not_found", "no such level"))?;
    let level = store.get(level_id).filter(|e| e.class == "IfcBuildingStorey").ok_or_else(|| bad("level.not_a_level", format!("element {level_id} is not a level")))?;
    let height = ty.size[2] * ty.scale;
    let ceiling = match (&p.host_ceiling, p.work_plane) {
        (Some(c), _) => {
            let c = store.resolve(c).ok_or_else(|| bad("element.not_found", "no such ceiling"))?;
            store.get(c).filter(|e| crate::floors::slab_recipe(e).is_some_and(|(k, _)| k == crate::floors::SlabKind::Ceiling)).ok_or_else(|| bad("sprinkler.host", format!("element {c} is not a ceiling")))?;
            Some(c)
        }
        (None, true) => None,
        (None, false) => m.hangs().then(|| ceilings_over(&store, level.guid().to_string().as_str(), p.point).into_iter().next()).flatten(),
    };
    let offset = match (p.offset, ceiling) {
        (Some(o), _) => o,
        // Pendent: its top at the underside; upright: standing on the ceiling's top.
        (None, Some(c)) => {
            if m.hangs() {
                -height
            } else {
                crate::floors::regenerate(&store, store.get(c).expect("resolved"))?.thickness
            }
        }
        (None, None) if m.hangs() => crate::floors::DEFAULT_CEILING_OFFSET - height,
        (None, None) => UPRIGHT_HEIGHT,
    };
    let args = json!({ "type_id": type_id.0, "point": p.point, "level": level_id.0, "offset": offset, "rotation": p.rotation });
    let id = mesh_objects::place(txn, serde_json::from_value(args).map_err(|e| bad("sprinkler.params", e.to_string()))?)?;
    if let Some(c) = ceiling {
        let mut r = mesh_objects::mesh_object_recipe(txn.store().get(id).expect("placed")).expect("a copy");
        r.ceiling = Some(store.get(c).expect("resolved").guid().to_string());
        let v = serde_json::to_value(&r).expect("serialises");
        txn.modify(id, |e| {
            e.params.insert(crate::walls::RECIPE_KEY.into(), v);
        })?;
        // Connected to pipes where it now is.
        crate::mep::connect_coincident(txn, &[id])?;
    }
    Ok(id)
}
