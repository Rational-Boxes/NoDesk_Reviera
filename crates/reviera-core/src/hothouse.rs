// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Hothouse climate and crop light (spec Ch 3 §3.6a.8–9, ARC-GLZ-008, ARC-GLZ-009): a glazed
//! enclosure's growing climate by crop, the daily light integral (DLI) its crop gets month by
//! month through the glazing and from grow lights, and grow lights laid out for a target.
//!
//! - **Crops.** A small library of crop presets from public extension sources (research/11
//!   §B1, §B2, §B5): the least and the target DLI, day and night air temperature, relative
//!   humidity, the CO₂ set point with the vents shut, and the longest day the crop takes under
//!   lamps (tomato needs 4–6 h of dark). Provisional (EXPERT_REVIEW E-57); a value the source
//!   gives none for stays empty.
//! - **Climate.** `enclosure.set_climate` keeps a crop's values (or the user's) on the
//!   enclosure (`rxd:growing_climate`) and as `Reviera_GrowingClimate`, temperatures in kelvin
//!   as IFC measures them.
//! - **Crop light.** The outdoor DLI each month is the weather file's mean daily global
//!   radiation (kept by `weather.import`), else an estimate: the month's mean daily
//!   extraterrestrial radiation at the site's latitude × a clearness index (0.5 unless given).
//!   Global radiation becomes PAR photons at 1.96 mol per MJ (about 43 % of sunlight's energy is
//!   PAR, research G092). Inside, it is × the roof glazing's PAR transmission (area-weighted
//!   over the enclosure's roofs, the visible transmission when a type has no PAR value) × the
//!   part its bars leave unshaded. Grow lights add their mean PPFD × the photoperiod.
//! - **Grow lights** (`growing.lights`): an `IfcElementAssembly` (ObjectType
//!   `GrowLightLayout`) owning top-lights, `IfcLightFixture` (USERDEFINED, ObjectType
//!   `GrowLight`), hung `height` above the floor on a grid across each span: each fixture's
//!   photons × a maintenance factor over the target PPFD gives the floor it covers, so the
//!   mean is at least the target. The target is the crop's need in its darkest month when the
//!   enclosure has a climate (its shortfall over the photoperiod), else 110 µmol·m⁻²·s⁻¹. A
//!   fixture below 2.3 µmol/J (DLC's current minimum) is flagged. The layout follows its
//!   enclosure.
//!
//! - **Screens** (ARC-GLZ-010): energy, shade and blackout screens from a small library,
//!   drawn flat under the gutters span by span (`IfcShadingDevice`, `Reviera_Screen`); a shade
//!   screen's light in the crop light, an energy screen's saving in the heat loss.
//! - **Quick heat loss** (ARC-GLZ-011): Q = Σ(U·A)·C·W·ΔT + 0.33·N·V·ΔT by the published table
//!   method, with an energy screen closed too.
//!
//! Not yet: screens on tunnels, the hemispherical (diffuse) transmission of the
//! glazing, lamps switched off in bright sun, a uniformity map from photometry, lights in
//! rooms, and the lamps' heat in the loads.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::bodies::Mesh;
use crate::element::{Element, NewElement};
use crate::enclosures::{self, EnclosureRecipe};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::RECIPE_KEY;

pub const CLIMATE_KEY: &str = "rxd:growing_climate";
pub const CLIMATE_PSET: &str = "Reviera_GrowingClimate";
pub const LIGHTS_RECIPE: &str = "recipe.grow_lights";
pub const LIGHT_RECIPE: &str = "recipe.grow_light";
pub const LIGHTS_OBJECT_TYPE: &str = "GrowLightLayout";
pub const LIGHT_PSET: &str = "Reviera_GrowLight";
/// The parameter by which a fixture names its layout (GlobalId).
pub const LIGHTS_KEY: &str = "rxd:grow_lights";
/// Global radiation to PAR photons: mol per MJ (research G092).
pub const PAR_PER_MJ: f64 = 1.96;
/// The clearness index an estimate uses when none is given (E-57).
pub const CLEARNESS: f64 = 0.5;
/// The least photon efficacy (µmol/J) a new fixture should have (DLC V3.0, research/11 §B1.3).
pub const MIN_EFFICACY: f64 = 2.3;
/// A grow light's body: length, width and depth (E-58).
const FIXTURE: [f64; 3] = [1.1, 0.12, 0.08];
/// µmol·m⁻²·s⁻¹ for an hour, in mol·m⁻².
const HOUR: f64 = 0.0036;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

fn typed(t: &str, v: Value) -> Value {
    json!({ "type": t, "value": v })
}

// ---- Crops ----

/// A crop preset.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Crop {
    pub id: &'static str,
    pub name: &'static str,
    /// The least and the target daily light integral (mol·m⁻²·d⁻¹).
    pub dli_min: f64,
    pub dli_target: f64,
    /// Day and night air temperature (°C).
    pub day_temperature: Option<f64>,
    pub night_temperature: Option<f64>,
    /// Relative humidity, least and most (percent).
    pub humidity: Option<[f64; 2]>,
    /// The CO₂ set point with the vents shut (ppm).
    pub co2: f64,
    /// The longest day under lamps (hours).
    pub photoperiod: f64,
    /// Research source IDs (research/11).
    pub sources: &'static str,
}

/// The crop library (provisional, EXPERT_REVIEW E-57).
pub fn crops() -> Vec<Crop> {
    let c = |id, name, dli_min, dli_target, day, night, humidity, co2, photoperiod, sources| Crop { id, name, dli_min, dli_target, day_temperature: day, night_temperature: night, humidity, co2, photoperiod, sources };
    vec![
        c("tomato", "Tomato (high-wire)", 20.0, 25.0, Some(21.0), Some(18.0), None, 1000.0, 18.0, "C035 C041 C038"),
        c("cucumber", "Cucumber (high-wire)", 15.0, 25.0, Some(25.0), Some(18.0), None, 1000.0, 18.0, "C035 C043 C044 C038"),
        c("pepper", "Sweet pepper", 20.0, 25.0, None, None, None, 1000.0, 18.0, "C035 C038"),
        c("lettuce", "Head lettuce", 12.0, 17.0, Some(24.0), Some(19.0), Some([50.0, 70.0]), 1000.0, 18.0, "C035 C037 C038"),
        c("strawberry", "Strawberry", 17.0, 20.0, None, None, None, 1000.0, 16.0, "C035 C038"),
        c("bedding", "Bedding and potted plants", 8.0, 11.0, None, None, None, 1000.0, 16.0, "C035 C038"),
        c("propagation", "Propagation (cuttings, seedlings)", 4.0, 6.0, None, None, None, 800.0, 18.0, "G039 C038"),
    ]
}

pub fn crop(id: &str) -> Option<Crop> {
    crops().into_iter().find(|c| c.id == id)
}

/// `growing.crops`.
pub(crate) fn list_crops(_: &Store, _: crate::builtin::NoParams) -> Result<Vec<Crop>> {
    Ok(crops())
}

// ---- Climate ----

/// An enclosure's growing climate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Climate {
    /// The crop preset it started from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<String>,
    /// mol·m⁻²·d⁻¹.
    pub target_dli: f64,
    /// °C.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day_temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub night_temperature: Option<f64>,
    /// Percent, least and most.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub humidity: Option<[f64; 2]>,
    /// ppm.
    pub co2: f64,
    /// Hours of light a day, lamps included.
    pub photoperiod: f64,
}

impl Climate {
    fn of_crop(c: &Crop) -> Climate {
        Climate { crop: Some(c.id.into()), target_dli: c.dli_target, day_temperature: c.day_temperature, night_temperature: c.night_temperature, humidity: c.humidity, co2: c.co2, photoperiod: c.photoperiod }
    }
}

/// An enclosure's climate, if it has one.
pub fn climate(e: &Element) -> Option<Climate> {
    serde_json::from_value(e.param(CLIMATE_KEY)?.clone()).ok()
}

fn climate_params(c: &Climate) -> Vec<(String, Value)> {
    let k = |p: &str| format!("{CLIMATE_PSET}.{p}");
    let kelvin = |t: f64| json!(t + 273.15);
    let mut out = vec![
        (k("TargetDailyLightIntegral"), typed("IfcReal", json!(c.target_dli))),
        (k("CarbonDioxideSetpoint"), typed("IfcReal", json!(c.co2))),
        (k("Photoperiod"), typed("IfcReal", json!(c.photoperiod))),
    ];
    if let Some(id) = &c.crop {
        out.push((k("Crop"), typed("IfcLabel", json!(crop(id).map_or(id.clone(), |c| c.name.to_owned())))));
    }
    if let Some(t) = c.day_temperature {
        out.push((k("DayTemperature"), typed("IfcThermodynamicTemperatureMeasure", kelvin(t))));
    }
    if let Some(t) = c.night_temperature {
        out.push((k("NightTemperature"), typed("IfcThermodynamicTemperatureMeasure", kelvin(t))));
    }
    if let Some([lo, hi]) = c.humidity {
        out.push((k("RelativeHumidityMin"), typed("IfcNormalisedRatioMeasure", json!(lo / 100.0))));
        out.push((k("RelativeHumidityMax"), typed("IfcNormalisedRatioMeasure", json!(hi / 100.0))));
    }
    out
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetClimateParams {
    pub enclosure_id: ElementRef,
    /// A crop preset (`growing.crops`); its values unless others are given.
    pub crop: Option<String>,
    pub target_dli: Option<f64>,
    /// °C.
    pub day_temperature: Option<f64>,
    pub night_temperature: Option<f64>,
    /// Percent, least and most.
    pub humidity: Option<[f64; 2]>,
    /// ppm.
    pub co2: Option<f64>,
    /// Hours.
    pub photoperiod: Option<f64>,
    /// Removes the climate.
    #[serde(default)]
    pub clear: bool,
}

fn enclosure(store: &Store, r: &ElementRef) -> Result<(ElementId, EnclosureRecipe)> {
    let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such enclosure"))?;
    let rec = enclosures::recipe(store.get(id).expect("resolved")).ok_or_else(|| bad("enclosure.not_an_enclosure", format!("element {id} is not a glazed enclosure")))?;
    Ok((id, rec))
}

/// `enclosure.set_climate`.
pub(crate) fn set_climate(txn: &mut Transaction<'_>, p: SetClimateParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, _) = enclosure(&store, &p.enclosure_id)?;
    let clear = |e: &mut Element| e.params.retain(|k, _| k != CLIMATE_KEY && !k.starts_with(&format!("{CLIMATE_PSET}.")));
    if p.clear {
        return txn.modify(id, clear);
    }
    let mut c = match &p.crop {
        Some(name) => Climate::of_crop(&crop(name).ok_or_else(|| bad("growing.no_crop", format!("no crop {name:?}; growing.crops lists them")))?),
        None => climate(store.get(id).expect("resolved")).unwrap_or_else(|| Climate::of_crop(&crop("tomato").expect("in the library"))),
    };
    if let Some(v) = p.target_dli {
        c.target_dli = v;
    }
    for (v, slot) in [(p.day_temperature, &mut c.day_temperature), (p.night_temperature, &mut c.night_temperature)] {
        if v.is_some() {
            *slot = v;
        }
    }
    if p.humidity.is_some() {
        c.humidity = p.humidity;
    }
    if let Some(v) = p.co2 {
        c.co2 = v;
    }
    if let Some(v) = p.photoperiod {
        c.photoperiod = v;
    }
    let out_of = |m: &str| Err(bad("growing.bad_value", m.to_owned()));
    if !(0.5..=70.0).contains(&c.target_dli) {
        return out_of("the target DLI is 0.5 to 70 mol·m⁻²·d⁻¹");
    }
    if !(1.0..=24.0).contains(&c.photoperiod) {
        return out_of("the photoperiod is 1 to 24 hours");
    }
    if !(0.0..=5000.0).contains(&c.co2) {
        return out_of("the CO₂ set point is 0 to 5,000 ppm (5,000 ppm harms people)");
    }
    if [c.day_temperature, c.night_temperature].iter().flatten().any(|t| !(-10.0..=50.0).contains(t)) {
        return out_of("air temperatures are −10 to 50 °C");
    }
    if c.humidity.is_some_and(|[lo, hi]| !(0.0 <= lo && lo <= hi && hi <= 100.0)) {
        return out_of("relative humidity runs from its least to its most, 0 to 100 %");
    }
    let v = serde_json::to_value(&c).expect("serialises");
    let values = climate_params(&c);
    txn.modify(id, |e| {
        clear(e);
        e.params.insert(CLIMATE_KEY.into(), v);
        for (k, v) in values {
            e.params.insert(k, v);
        }
    })
}

// ---- Crop light ----

/// The mean daily extraterrestrial radiation on a horizontal surface (MJ/m²) over each month,
/// at latitude `lat` (degrees), day by day (solar constant 1 367 W/m²).
pub fn extraterrestrial(lat: f64) -> [f64; 12] {
    const DAYS: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let phi = lat.to_radians();
    let mut out = [0.0; 12];
    let mut n = 0u32;
    for (m, days) in DAYS.iter().enumerate() {
        let mut sum = 0.0;
        for _ in 0..*days {
            n += 1;
            let x = 2.0 * std::f64::consts::PI * f64::from(n) / 365.0;
            let dr = 1.0 + 0.033 * x.cos();
            let delta = 23.45f64.to_radians() * (2.0 * std::f64::consts::PI * (284.0 + f64::from(n)) / 365.0).sin();
            let ws = (-phi.tan() * delta.tan()).clamp(-1.0, 1.0).acos();
            let h = 24.0 * 3600.0 * 1367.0 / std::f64::consts::PI * dr * (phi.cos() * delta.cos() * ws.sin() + ws * phi.sin() * delta.sin());
            sum += h.max(0.0) / 1e6;
        }
        out[m] = sum / f64::from(*days);
    }
    out
}

/// How much of the sky's light reaches the crop through the roof.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, JsonSchema)]
pub struct Transmission {
    /// The roof glazing's PAR transmission (area-weighted).
    pub glazing: f64,
    /// The share the bars shade.
    pub frame_shading: f64,
    /// Through both.
    pub total: f64,
}

/// The roof glazing of `r`'s roofs, area-weighted, and its bars.
pub fn transmission(store: &Store, r: &EnclosureRecipe) -> Transmission {
    let (mut sum, mut area) = (0.0, 0.0);
    for e in r.parts.iter().filter_map(|g| store.get(store.find_by_guid(&GlobalId::parse(g)?)?)) {
        if crate::roofs::roof_recipe(e).is_none() {
            continue;
        }
        let Some(g) = e.param("@type").and_then(|v| store.get(store.find_by_guid(&GlobalId::parse(v.as_str()?)?)?)).and_then(crate::glazing_library::of_type) else { continue };
        let a = crate::roofs::regenerate(store, e).map_or(1.0, |g| g.area().max(1e-6));
        sum += a * if g.par > 0.0 { g.par } else { g.light };
        area += a;
    }
    let glazing = if area > 0.0 { sum / area } else { 0.0 };
    let frame_shading = r.frame_shading();
    Transmission { glazing, frame_shading, total: glazing * (1.0 - frame_shading) }
}

/// Where the outdoor light came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LightSource {
    /// The weather file's monthly global radiation.
    Weather,
    /// Extraterrestrial radiation × a clearness index.
    Estimate,
    /// Typed by the user.
    Given,
}

/// One month's light (mol·m⁻²·d⁻¹).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, JsonSchema)]
pub struct Month {
    pub month: u8,
    pub outdoor: f64,
    /// Through the roof.
    pub natural: f64,
    /// From the grow lights.
    pub lamps: f64,
    pub total: f64,
    pub meets_target: bool,
    /// With the shade screen closed (when there is one), and whether it would be (the natural
    /// light above the target by more than 30 %).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shaded: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shade_closed: Option<bool>,
}

/// What `enclosure.crop_light` returns.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CropLight {
    pub source: LightSource,
    pub latitude: Option<f64>,
    pub transmission: Transmission,
    pub target_dli: Option<f64>,
    pub photoperiod: Option<f64>,
    pub months: Vec<Month>,
    /// The month with the least natural light (1–12).
    pub darkest_month: u8,
    /// The lamps' mean PPFD (µmol·m⁻²·s⁻¹) the darkest month needs to meet the target over the
    /// photoperiod, and the grow lights' own.
    pub required_ppfd: Option<f64>,
    pub lamp_ppfd: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CropLightParams {
    pub enclosure_id: ElementRef,
    /// A clearness index for every month, for the estimate (default 0.5).
    pub clearness: Option<f64>,
    /// The outdoor DLI each month, January first, over the weather's.
    pub outdoor_dli: Option<Vec<f64>>,
}

/// The outdoor DLI each month and where it came from.
fn outdoor(store: &Store, p: &CropLightParams) -> Result<(LightSource, Option<f64>, [f64; 12])> {
    let loc = crate::location::get(store);
    if let Some(v) = &p.outdoor_dli {
        let a: [f64; 12] = v.as_slice().try_into().map_err(|_| bad("growing.bad_value", "give twelve months of outdoor DLI"))?;
        if a.iter().any(|d| !(0.0..=80.0).contains(d)) {
            return Err(bad("growing.bad_value", "an outdoor DLI is 0 to 80 mol·m⁻²·d⁻¹"));
        }
        return Ok((LightSource::Given, loc.latitude, a));
    }
    if loc.weather.monthly_global.len() == 12 && p.clearness.is_none() {
        let mut a = [0.0; 12];
        for (d, g) in a.iter_mut().zip(&loc.weather.monthly_global) {
            *d = g * PAR_PER_MJ;
        }
        return Ok((LightSource::Weather, loc.latitude, a));
    }
    let lat = loc.latitude.ok_or_else(|| bad("growing.no_location", "set the project location (Manage › Project Location) or import a weather file for the sun"))?;
    let kt = p.clearness.unwrap_or(CLEARNESS);
    if !(0.05..=0.85).contains(&kt) {
        return Err(bad("growing.bad_value", "a clearness index is 0.05 to 0.85"));
    }
    Ok((LightSource::Estimate, Some(lat), extraterrestrial(lat).map(|h| h * kt * PAR_PER_MJ)))
}

/// `enclosure.crop_light`.
pub(crate) fn crop_light(store: &Store, p: CropLightParams) -> Result<CropLight> {
    let (id, r) = enclosure(store, &p.enclosure_id)?;
    let e = store.get(id).expect("resolved");
    let (source, latitude, out) = outdoor(store, &p)?;
    let t = transmission(store, &r);
    let c = climate(e);
    let guid = e.guid().to_string();
    let lamp_ppfd: f64 = layouts_on(store, &guid).iter().map(|(_, l)| summarise(store, l, &r).average_ppfd).sum();
    let hours = c.as_ref().map_or(16.0, |c| c.photoperiod);
    let shade = r.screens.iter().find(|s| s.kind == ScreenKind::Shade).map(|s| material_of(s).light);
    let months: Vec<Month> = out
        .iter()
        .enumerate()
        .map(|(m, o)| {
            let natural = o * t.total;
            let lamps = lamp_ppfd * hours * HOUR;
            let total = natural + lamps;
            let shaded = shade.map(|light| natural * light);
            let closed = shade.map(|_| c.as_ref().is_some_and(|c| natural > 1.3 * c.target_dli));
            Month { month: m as u8 + 1, outdoor: *o, natural, lamps, total, meets_target: c.as_ref().is_none_or(|c| total >= c.target_dli - 1e-9), shaded, shade_closed: closed }
        })
        .collect();
    let darkest = months.iter().min_by(|a, b| a.natural.total_cmp(&b.natural)).map_or(1, |m| m.month);
    let required = c.as_ref().map(|c| ((c.target_dli - months[darkest as usize - 1].natural) / (c.photoperiod * HOUR)).max(0.0));
    Ok(CropLight { source, latitude, transmission: t, target_dli: c.as_ref().map(|c| c.target_dli), photoperiod: c.as_ref().map(|c| c.photoperiod), months, darkest_month: darkest, required_ppfd: required, lamp_ppfd })
}

// ---- Grow lights ----

/// A grow-light layout's stored recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LightsRecipe {
    pub kind: String,
    /// The enclosure (GlobalId).
    pub enclosure: String,
    /// One fixture's photon flux (µmol/s) and electrical power (W).
    pub photon_flux: f64,
    pub power: f64,
    /// The mean PPFD laid out for (µmol·m⁻²·s⁻¹).
    pub target_ppfd: f64,
    /// The fixtures' underside above the floor.
    pub height: f64,
    /// Output kept after lamp ageing and dirt (0–1).
    pub maintenance: f64,
    #[serde(default)]
    pub parts: Vec<String>,
}

/// One fixture's stored recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LightRecipe {
    pub kind: String,
    pub level: String,
    /// Its centre in plan, the direction of its length, and its underside above the level.
    pub at: [f64; 2],
    pub rotation: f64,
    pub z: f64,
}

pub fn lights_recipe(e: &Element) -> Option<LightsRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == LIGHTS_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

pub fn light_recipe(e: &Element) -> Option<LightRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == LIGHT_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// A fixture's body: a box `FIXTURE` long, wide and deep.
pub fn light_mesh(p: &LightRecipe, level_z: f64) -> Mesh {
    let (s, c) = p.rotation.sin_cos();
    let [l, w, d] = FIXTURE;
    let z0 = level_z + p.z;
    let corner = |i: f64, j: f64| [p.at[0] + c * i * l / 2.0 - s * j * w / 2.0, p.at[1] + s * i * l / 2.0 + c * j * w / 2.0];
    let ring = [corner(-1.0, -1.0), corner(1.0, -1.0), corner(1.0, 1.0), corner(-1.0, 1.0)];
    let mut pts = Vec::with_capacity(8);
    for z in [z0, z0 + d] {
        for q in ring {
            pts.push([q[0], q[1], z]);
        }
    }
    let idx: Vec<u32> = vec![0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7, 6, 3, 0, 4, 3, 4, 7];
    (pts, idx)
}

/// Every grow light with its body.
pub fn all(store: &Store) -> Vec<(ElementId, Mesh)> {
    store
        .iter()
        .filter_map(|e| {
            let p = light_recipe(e)?;
            let z = crate::levels::elevation_of(store, &p.level).unwrap_or(0.0);
            Some((e.id(), light_mesh(&p, z)))
        })
        .collect()
}

/// The fixtures' centres over the enclosure's floor: across each span `n` to a row, rows along
/// the length close enough that each covers its share of the target.
pub fn plan(r: &LightsRecipe, e: &EnclosureRecipe) -> Vec<LightRecipe> {
    let cover = r.photon_flux * r.maintenance / r.target_ppfd;
    let side = cover.sqrt();
    let across = (e.span / side).round().max(1.0);
    let step = e.span / across;
    let along = (e.length / (cover / step)).ceil().max(1.0);
    let pitch = e.length / along;
    let mut out = Vec::new();
    for i in 0..e.spans {
        for j in 0..across as u32 {
            let y = e.span * f64::from(i) + step * (f64::from(j) + 0.5);
            for k in 0..along as u32 {
                let x = pitch * (f64::from(k) + 0.5);
                out.push(LightRecipe { kind: LIGHT_RECIPE.into(), level: e.level.clone(), at: e.at(x, y), rotation: e.rotation, z: r.height });
            }
        }
    }
    out
}

/// The light layouts on the enclosure `guid`.
fn layouts_on(store: &Store, guid: &str) -> Vec<(ElementId, LightsRecipe)> {
    store.iter().filter_map(|e| lights_recipe(e).filter(|r| r.enclosure == guid).map(|r| (e.id(), r))).collect()
}

fn fixture_params(r: &LightsRecipe) -> Vec<(String, Value)> {
    let k = |p: &str| format!("{LIGHT_PSET}.{p}");
    vec![
        (k("PhotonFlux"), typed("IfcReal", json!(r.photon_flux))),
        (k("ElectricalPower"), typed("IfcPowerMeasure", json!(r.power))),
        (k("PhotonEfficacy"), typed("IfcReal", json!(r.photon_flux / r.power.max(1e-9)))),
    ]
}

/// The highest a fixture's underside may hang: under the gutters (a tunnel's crown, less a
/// metre).
fn max_height(e: &EnclosureRecipe) -> f64 {
    if e.form.tunnel() { e.ridge() - 1.0 } else { e.eave - 0.3 }
}

/// The default hang height: 400 mm under the gutters (a tunnel: 1.2 m under the crown), at
/// least 2.2 m up (E-58).
fn default_height(e: &EnclosureRecipe) -> f64 {
    let h = if e.form.tunnel() { e.ridge() - 1.2 } else { e.eave - 0.4 };
    h.max(2.2).min(max_height(e))
}

fn build(txn: &mut Transaction<'_>, guid: &str, r: &LightsRecipe) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let e = GlobalId::parse(&r.enclosure)
        .and_then(|g| store.find_by_guid(&g))
        .and_then(|id| store.get(id))
        .and_then(enclosures::recipe)
        .ok_or_else(|| bad("growing.no_enclosure", "the grow lights' enclosure no longer exists"))?;
    let mut ids = Vec::new();
    for p in plan(r, &e) {
        let mut n = NewElement::new("IfcLightFixture")
            .name("Grow Light")
            .param("@PredefinedType", "USERDEFINED")
            .param("@ObjectType", "GrowLight")
            .param(LIGHTS_KEY, json!(guid))
            .param(RECIPE_KEY, serde_json::to_value(&p).expect("serialises"));
        for (k, v) in fixture_params(r) {
            n = n.param(&k, v);
        }
        ids.push(txn.create(n)?);
    }
    Ok(ids)
}

fn layout_params(s: &LightsSummary) -> Vec<(String, Value)> {
    let k = |p: &str| format!("{LIGHT_PSET}.{p}");
    vec![
        (k("FixtureCount"), typed("IfcCountMeasure", json!(s.fixtures))),
        (k("TargetPPFD"), typed("IfcReal", json!(s.target_ppfd))),
        (k("AveragePPFD"), typed("IfcReal", json!(s.average_ppfd))),
        (k("PowerDensity"), typed("IfcReal", json!(s.power_density))),
        (k("MaintenanceFactor"), typed("IfcNormalisedRatioMeasure", json!(s.maintenance))),
    ]
}

/// The layout's fixtures made again, and its values written.
fn rebuild(txn: &mut Transaction<'_>, id: ElementId, mut r: LightsRecipe) -> Result<()> {
    let store = txn.store().clone();
    for g in &r.parts {
        if let Some(part) = GlobalId::parse(g).and_then(|g| store.find_by_guid(&g))
            && txn.store().contains(part)
        {
            txn.delete(part)?;
        }
    }
    let guid = store.get(id).expect("exists").guid().to_string();
    let parts = build(txn, &guid, &r)?;
    r.parts = parts.iter().map(|p| txn.store().get(*p).expect("made").guid().to_string()).collect();
    let e = GlobalId::parse(&r.enclosure).and_then(|g| txn.store().get(txn.store().find_by_guid(&g)?)).and_then(enclosures::recipe).expect("built from it");
    let values = layout_params(&summarise(txn.store(), &r, &e));
    let v = serde_json::to_value(&r).expect("serialises");
    txn.modify(id, |e| {
        e.params.retain(|k, _| !k.starts_with(&format!("{LIGHT_PSET}.")));
        e.params.insert(RECIPE_KEY.into(), v);
        for (k, v) in values {
            e.params.insert(k, v);
        }
    })
}

/// Lays out again every grow-light layout on the enclosure `guid` (after the enclosure changed).
pub(crate) fn follow(txn: &mut Transaction<'_>, guid: &str) -> Result<()> {
    for (id, r) in layouts_on(txn.store(), guid) {
        let e = txn.store().get(txn.store().find_by_guid(&GlobalId::parse(guid).expect("a guid")).expect("exists")).and_then(enclosures::recipe).expect("an enclosure");
        let r = LightsRecipe { height: r.height.min(max_height(&e)), ..r };
        rebuild(txn, id, r)?;
    }
    Ok(())
}

/// The sizes of `growing.lights` and `growing.set_lights`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LightSizes {
    /// One fixture's photon flux, µmol/s (default 1,500).
    pub photon_flux: Option<f64>,
    /// One fixture's power, W (default 600).
    pub power: Option<f64>,
    /// The mean PPFD to lay out for, µmol·m⁻²·s⁻¹ (default: the crop's need in its darkest
    /// month, else 110).
    pub target_ppfd: Option<f64>,
    /// The fixtures' underside above the floor (default 400 mm under the gutters).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub height: Option<f64>,
    /// Output kept after ageing and dirt, 0–1 (default 0.9).
    pub maintenance: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LightsParams {
    pub enclosure_id: ElementRef,
    #[serde(flatten)]
    pub sizes: LightSizes,
}

fn apply(r: &mut LightsRecipe, s: &LightSizes, e: &EnclosureRecipe) -> Result<()> {
    for (v, slot) in [(s.photon_flux, &mut r.photon_flux), (s.power, &mut r.power), (s.target_ppfd, &mut r.target_ppfd), (s.height, &mut r.height), (s.maintenance, &mut r.maintenance)] {
        if let Some(v) = v {
            *slot = v;
        }
    }
    let out_of = |m: String| Err(bad("growing.bad_value", m));
    if !(50.0..=10_000.0).contains(&r.photon_flux) || !(10.0..=3_000.0).contains(&r.power) {
        return out_of("a fixture gives 50 to 10,000 µmol/s and draws 10 to 3,000 W".into());
    }
    if !(10.0..=1_500.0).contains(&r.target_ppfd) {
        return out_of("the target PPFD is 10 to 1,500 µmol·m⁻²·s⁻¹".into());
    }
    if !(0.3..=1.0).contains(&r.maintenance) {
        return out_of("the maintenance factor is 0.3 to 1".into());
    }
    let top = max_height(e);
    if !(r.height >= 0.5 && r.height <= top + 1e-9) {
        return out_of(format!("the lights hang 0.5 m to {top:.2} m above the floor (under the gutters)"));
    }
    Ok(())
}

/// `growing.lights`.
pub(crate) fn lights(txn: &mut Transaction<'_>, p: LightsParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let (eid, e) = enclosure(&store, &p.enclosure_id)?;
    let target = match p.sizes.target_ppfd {
        Some(t) => t,
        None => crop_light(&store, CropLightParams { enclosure_id: ElementRef::Id(eid), clearness: None, outdoor_dli: None }).ok().and_then(|l| l.required_ppfd).filter(|t| *t >= 10.0).unwrap_or(110.0).min(1_500.0),
    };
    let mut r = LightsRecipe { kind: LIGHTS_RECIPE.into(), enclosure: store.get(eid).expect("resolved").guid().to_string(), photon_flux: 1_500.0, power: 600.0, target_ppfd: target, height: default_height(&e), maintenance: 0.9, parts: Vec::new() };
    apply(&mut r, &LightSizes { target_ppfd: None, ..p.sizes }, &e)?;
    let id = txn.create(
        NewElement::new("IfcElementAssembly")
            .name("Grow Lights")
            .param("@PredefinedType", "USERDEFINED")
            .param("@ObjectType", LIGHTS_OBJECT_TYPE)
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )?;
    rebuild(txn, id, r)?;
    Ok(id)
}

fn resolve(store: &Store, r: &ElementRef) -> Result<(ElementId, LightsRecipe)> {
    let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such grow-light layout"))?;
    let rec = lights_recipe(store.get(id).expect("resolved")).ok_or_else(|| bad("growing.not_lights", format!("element {id} is not a grow-light layout")))?;
    Ok((id, rec))
}

fn host(store: &Store, r: &LightsRecipe) -> Result<EnclosureRecipe> {
    GlobalId::parse(&r.enclosure).and_then(|g| store.get(store.find_by_guid(&g)?)).and_then(enclosures::recipe).ok_or_else(|| bad("growing.no_enclosure", "the grow lights' enclosure no longer exists"))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetLightsParams {
    pub lights_id: ElementRef,
    #[serde(flatten)]
    pub sizes: LightSizes,
}

/// `growing.set_lights`.
pub(crate) fn set_lights(txn: &mut Transaction<'_>, p: SetLightsParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = resolve(&store, &p.lights_id)?;
    let e = host(&store, &r)?;
    apply(&mut r, &p.sizes, &e)?;
    rebuild(txn, id, r)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LightsSummaryParams {
    pub lights_id: ElementRef,
}

/// What `growing.lights_summary` returns.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LightsSummary {
    pub fixtures: usize,
    pub target_ppfd: f64,
    /// The fixtures' photons, maintained, over the growing floor (µmol·m⁻²·s⁻¹).
    pub average_ppfd: f64,
    pub maintenance: f64,
    /// W, and W/m² of floor.
    pub power: f64,
    pub power_density: f64,
    /// µmol/J.
    pub efficacy: f64,
    /// The DLI the lamps add over the enclosure's photoperiod (16 h without a climate).
    pub lamp_dli: f64,
    pub warnings: Vec<String>,
}

fn summarise(store: &Store, r: &LightsRecipe, e: &EnclosureRecipe) -> LightsSummary {
    let n = r.parts.iter().filter(|g| GlobalId::parse(g).and_then(|g| store.find_by_guid(&g)).is_some()).count();
    let floor = (e.width() * e.length).max(1e-9);
    let average = n as f64 * r.photon_flux * r.maintenance / floor;
    let hours = GlobalId::parse(&r.enclosure).and_then(|g| store.get(store.find_by_guid(&g)?)).and_then(climate).map_or(16.0, |c| c.photoperiod);
    let efficacy = r.photon_flux / r.power.max(1e-9);
    let mut warnings = Vec::new();
    if efficacy < MIN_EFFICACY - 1e-9 {
        warnings.push(format!("the fixtures give {efficacy:.2} µmol/J, below the {MIN_EFFICACY} µmol/J new grow lights should reach"));
    }
    LightsSummary { fixtures: n, target_ppfd: r.target_ppfd, average_ppfd: average, maintenance: r.maintenance, power: n as f64 * r.power, power_density: n as f64 * r.power / floor, efficacy, lamp_dli: average * hours * HOUR, warnings }
}

/// `growing.lights_summary`.
pub(crate) fn lights_summary(store: &Store, p: LightsSummaryParams) -> Result<LightsSummary> {
    let (_, r) = resolve(store, &p.lights_id)?;
    Ok(summarise(store, &r, &host(store, &r)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extraterrestrial_radiation_follows_the_seasons() {
        // About 41 MJ/m² a day at 52° N in June and 6–7 in December.
        let h = extraterrestrial(52.0);
        assert!((40.0..43.5).contains(&h[5]), "{h:?}");
        assert!((5.5..8.0).contains(&h[11]), "{h:?}");
        // The southern hemisphere's mirror, and the equator's near-constant year.
        let s = extraterrestrial(-52.0);
        assert!(s[11] > 40.0 && s[5] < 9.0, "{s:?}");
        let eq = extraterrestrial(0.0);
        assert!(eq.iter().all(|v| (33.0..39.0).contains(v)), "{eq:?}");
    }

    #[test]
    fn the_published_definitions_cover_what_is_written() {
        let defs: Value = serde_json::from_str(crate::glazing_library::DEFINITIONS).unwrap();
        let sets = defs["property_sets"].as_array().unwrap();
        let c = Climate { humidity: Some([50.0, 70.0]), ..Climate::of_crop(&crop("lettuce").unwrap()) };
        let r = LightsRecipe { kind: LIGHTS_RECIPE.into(), enclosure: String::new(), photon_flux: 1500.0, power: 600.0, target_ppfd: 110.0, height: 3.0, maintenance: 0.9, parts: vec![] };
        let s = LightsSummary { fixtures: 1, target_ppfd: 1.0, average_ppfd: 1.0, maintenance: 0.9, power: 1.0, power_density: 1.0, efficacy: 2.5, lamp_dli: 1.0, warnings: vec![] };
        let screen = screen_params(&screen_library()[0]);
        for (pset, written) in [(CLIMATE_PSET, climate_params(&c)), (LIGHT_PSET, fixture_params(&r).into_iter().chain(layout_params(&s)).collect()), (SCREEN_PSET, screen)] {
            let set = sets.iter().find(|s| s["name"] == pset).unwrap_or_else(|| panic!("{pset}"));
            let props = set["properties"].as_array().unwrap();
            for (key, value) in written {
                let name = key.strip_prefix(&format!("{pset}.")).unwrap();
                let def = props.iter().find(|p| p["name"] == name).unwrap_or_else(|| panic!("{pset}.{name} is not published"));
                assert_eq!(def["measure"], value["type"], "{name}");
            }
        }
    }
}

// ---- Screens (ARC-GLZ-010) ----

pub const SCREEN_RECIPE: &str = "recipe.greenhouse_screen";
pub const SCREEN_PSET: &str = "Reviera_Screen";
/// A screen's thickness as drawn.
const SCREEN_THICKNESS: f64 = 0.002;

/// What a screen is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScreenKind {
    /// Closed at night to keep the heat in.
    Energy,
    /// Closed in bright sun.
    Shade,
    /// Closed to shorten the day for short-day crops.
    Blackout,
}

impl ScreenKind {
    fn object_type(self) -> &'static str {
        match self {
            ScreenKind::Energy => "EnergyScreen",
            ScreenKind::Shade => "ShadeScreen",
            ScreenKind::Blackout => "BlackoutScreen",
        }
    }
}

/// A screen of an enclosure: its kind and its material (a library entry; the kind's default
/// when absent).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Screen {
    pub kind: ScreenKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
}

/// A screen material.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ScreenMaterial {
    pub name: &'static str,
    pub kind: ScreenKind,
    /// Light let through (0–1), and the heating energy saved while closed (0–1).
    pub light: f64,
    pub saving: f64,
    /// Air passes through (porous fabrics).
    pub porous: bool,
    pub sources: &'static str,
}

/// The screen library (provisional, EXPERT_REVIEW E-59), from research/11 §B4.
pub fn screen_library() -> Vec<ScreenMaterial> {
    let m = |name, kind, light, saving, porous| ScreenMaterial { name, kind, light, saving, porous, sources: "C047 C048" };
    vec![
        m("Shade screen 50 % (porous, aluminised)", ScreenKind::Shade, 0.50, 0.20, true),
        m("Shade screen 70 % (porous, aluminised)", ScreenKind::Shade, 0.30, 0.50, true),
        m("Energy screen, semi-porous 27 % light", ScreenKind::Energy, 0.27, 0.30, true),
        m("Energy screen 35 % light (non-porous)", ScreenKind::Energy, 0.35, 0.62, false),
        m("Energy screen 25 % light (non-porous)", ScreenKind::Energy, 0.25, 0.67, false),
        m("Blackout screen 99.9 %", ScreenKind::Blackout, 0.001, 0.70, false),
    ]
}

/// The kind's default material.
fn default_material(kind: ScreenKind) -> &'static str {
    match kind {
        ScreenKind::Energy => "Energy screen 35 % light (non-porous)",
        ScreenKind::Shade => "Shade screen 50 % (porous, aluminised)",
        ScreenKind::Blackout => "Blackout screen 99.9 %",
    }
}

/// A screen's material.
pub fn material_of(s: &Screen) -> ScreenMaterial {
    let name = s.material.as_deref().unwrap_or(default_material(s.kind));
    screen_library().into_iter().find(|m| m.name.eq_ignore_ascii_case(name)).unwrap_or_else(|| screen_library().into_iter().find(|m| m.name == default_material(s.kind)).expect("in the library"))
}

/// `enclosure.screen_library`.
pub(crate) fn list_screens(_: &Store, _: crate::builtin::NoParams) -> Result<Vec<ScreenMaterial>> {
    Ok(screen_library())
}

/// The highest a screen hangs below the eaves, and the step between screens.
const SCREEN_DROP: f64 = 0.25;
const SCREEN_STEP: f64 = 0.05;

/// Whether `screens` suit enclosure `r`.
pub(crate) fn check_screens(r: &EnclosureRecipe, screens: &[Screen]) -> Result<()> {
    if screens.is_empty() {
        return Ok(());
    }
    if r.form.tunnel() {
        return Err(bad("enclosure.bad_value", "screens go under the gutters of a glasshouse; not yet in tunnels"));
    }
    if screens.len() > 3 {
        return Err(bad("enclosure.bad_value", "an enclosure has at most three screens"));
    }
    if r.eave - SCREEN_DROP - SCREEN_STEP * (screens.len() as f64 - 1.0) < 2.2 {
        return Err(bad("enclosure.bad_value", "the eaves are too low for screens (they hang 250 mm under the gutters, at least 2.2 m up)"));
    }
    for s in screens {
        if let Some(m) = &s.material
            && !screen_library().iter().any(|x| x.name.eq_ignore_ascii_case(m))
        {
            return Err(bad("enclosure.bad_value", format!("no screen material {m:?}; enclosure.screen_library lists them")));
        }
    }
    Ok(())
}

/// One screen sheet's stored recipe: its corners in plan and its height above the level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScreenRecipe {
    pub kind: String,
    pub level: String,
    pub corners: [[f64; 2]; 4],
    pub z: f64,
}

pub fn screen_recipe(e: &Element) -> Option<ScreenRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == SCREEN_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

fn screen_mesh(p: &ScreenRecipe, level_z: f64) -> Mesh {
    let z0 = level_z + p.z;
    let mut pts = Vec::with_capacity(8);
    for z in [z0, z0 + SCREEN_THICKNESS] {
        for q in p.corners {
            pts.push([q[0], q[1], z]);
        }
    }
    let idx: Vec<u32> = vec![0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7, 6, 3, 0, 4, 3, 4, 7];
    (pts, idx)
}

/// Every screen sheet with its body.
pub fn all_screens(store: &Store) -> Vec<(ElementId, Mesh)> {
    store
        .iter()
        .filter_map(|e| {
            let p = screen_recipe(e)?;
            let z = crate::levels::elevation_of(store, &p.level).unwrap_or(0.0);
            Some((e.id(), screen_mesh(&p, z)))
        })
        .collect()
}

fn screen_params(m: &ScreenMaterial) -> Vec<(String, Value)> {
    let k = |p: &str| format!("{SCREEN_PSET}.{p}");
    vec![
        (k("Kind"), typed("IfcLabel", serde_json::to_value(m.kind).expect("serialises"))),
        (k("Material"), typed("IfcLabel", json!(m.name))),
        (k("LightTransmission"), typed("IfcNormalisedRatioMeasure", json!(m.light))),
        (k("EnergySaving"), typed("IfcNormalisedRatioMeasure", json!(m.saving))),
        (k("Porous"), typed("IfcBoolean", json!(m.porous))),
    ]
}

/// The screens of enclosure `r`: a sheet per screen and span, under the gutters, inset clear
/// of the posts on the inner gutter lines (80 mm) and of the outer and end posts, which stand
/// inside the walls (350 mm from the walls).
pub(crate) fn build_screens(txn: &mut Transaction<'_>, r: &EnclosureRecipe) -> Result<Vec<ElementId>> {
    let mut out = Vec::new();
    if r.screens.is_empty() || r.form.tunnel() {
        return Ok(out);
    }
    let (wall, post) = (0.35, 0.08);
    for (k, sc) in r.screens.iter().enumerate() {
        let m = material_of(sc);
        let z = r.eave - SCREEN_DROP - SCREEN_STEP * k as f64;
        for i in 0..r.spans {
            let y0 = r.span * f64::from(i) + if i == 0 { wall } else { post };
            let y1 = r.span * f64::from(i + 1) - if i + 1 == r.spans { wall } else { post };
            let (x0, x1) = (wall, r.length - wall);
            let corners = [r.at(x0, y0), r.at(x1, y0), r.at(x1, y1), r.at(x0, y1)];
            let recipe = ScreenRecipe { kind: SCREEN_RECIPE.into(), level: r.level.clone(), corners, z };
            let mut n = NewElement::new("IfcShadingDevice")
                .name(m.name)
                .param("@PredefinedType", "USERDEFINED")
                .param("@ObjectType", sc.kind.object_type())
                .param(RECIPE_KEY, serde_json::to_value(&recipe).expect("serialises"));
            for (key, v) in screen_params(&m) {
                n = n.param(&key, v);
            }
            out.push(txn.create(n)?);
        }
    }
    Ok(out)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetScreensParams {
    pub enclosure_id: ElementRef,
    /// The screens, replacing those it has ([] for none).
    pub screens: Vec<Screen>,
}

/// `enclosure.set_screens`.
pub(crate) fn set_screens(txn: &mut Transaction<'_>, p: SetScreensParams) -> Result<()> {
    crate::enclosures::set(txn, crate::enclosures::SetParams { enclosure_id: p.enclosure_id, params: crate::enclosures::Params { screens: Some(p.screens), ..Default::default() } })
}

// ---- Quick heat loss (ARC-GLZ-011) ----

/// One part's transmission.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct HeatPart {
    /// roof, glazed walls, knee walls.
    pub part: String,
    pub area: f64,
    /// Area-weighted U (W/m²K) and U·A (W/K).
    pub u: f64,
    pub ua: f64,
}

/// What `enclosure.heat_loss` returns (W unless said).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct HeatLoss {
    pub parts: Vec<HeatPart>,
    /// The construction factor, the wind factor, air changes per hour, volume (m³).
    pub construction_factor: f64,
    pub wind_factor: f64,
    pub air_changes: f64,
    pub volume: f64,
    /// Inside and outside temperatures (°C) and their difference (K).
    pub inside: f64,
    pub outside: f64,
    pub delta_t: f64,
    pub transmission: f64,
    pub infiltration: f64,
    pub total: f64,
    pub floor_area: f64,
    /// W per m² of floor.
    pub per_area: f64,
    /// With the energy (or blackout) screen closed: the material, its saving and the loss.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screen: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_screen: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeatLossParams {
    pub enclosure_id: ElementRef,
    /// °C: the inside set point (default the climate's night temperature, else 15) and the
    /// outside design temperature (default the weather's heating design dry-bulb).
    pub inside: Option<f64>,
    pub outside: Option<f64>,
    /// The design wind speed, m/s (default 6.7, 15 mph: no wind factor).
    pub wind: Option<f64>,
    /// Air changes per hour (default by the glazing).
    pub air_changes: Option<f64>,
}

/// The glazed roofs' bar spacing across the slope (their glazing grid).
const ROOF_BAR_SPACING: f64 = 1.125;

/// The wind factor of the table method: 1.00 to 15 mph, rising 0.008 per mph, at most 1.16.
pub fn wind_factor(speed: f64) -> f64 {
    let mph = speed / 0.44704;
    (1.0 + 0.008 * (mph - 15.0).max(0.0)).min(1.16)
}

/// `enclosure.heat_loss`.
pub(crate) fn heat_loss(store: &Store, p: HeatLossParams) -> Result<HeatLoss> {
    let (id, r) = enclosure(store, &p.enclosure_id)?;
    let e = store.get(id).expect("resolved");
    let climate = climate(e);
    let inside = p.inside.or(climate.as_ref().and_then(|c| c.night_temperature)).unwrap_or(15.0);
    let outside = match p.outside.or(crate::weather::get(store).design().heating_dry_bulb) {
        Some(t) => t,
        None => return Err(bad("growing.no_design", "give the outside design temperature, or import a weather file (Sustainability › Site › Weather)")),
    };
    if inside.partial_cmp(&outside) != Some(std::cmp::Ordering::Greater) {
        return Err(bad("growing.bad_value", "the inside temperature must be above the outside one"));
    }
    let type_of = |x: &Element| x.param("@type").and_then(|v| store.get(store.find_by_guid(&GlobalId::parse(v.as_str()?)?)?));
    let mut sums: BTreeMap<&'static str, (f64, f64)> = BTreeMap::new();
    let mut plastic = false;
    for part in r.parts.iter().filter_map(|g| store.get(store.find_by_guid(&GlobalId::parse(g)?)?)) {
        let (key, area, u) = if crate::roofs::roof_recipe(part).is_some() {
            let g = type_of(part).and_then(crate::glazing_library::of_type);
            plastic |= g.as_ref().is_some_and(|g| g.safety == crate::glazing_library::Safety::Plastic);
            let a = crate::roofs::regenerate(store, part).map_or(0.0, |g| g.area());
            ("roof", a, g.map_or(5.8, |g| g.u))
        } else if crate::curtain::curtain_recipe(part).is_some() {
            let g = type_of(part).and_then(crate::glazing_library::of_type);
            // Panels and mullions together: the wall's face.
            let a = crate::curtain::regenerate(store, part).map_or(0.0, |g| g.parts.iter().map(|q| (q.x[1] - q.x[0]) * (q.z[1] - q.z[0])).sum());
            ("glazed walls", a, g.map_or(5.8, |g| g.u))
        } else if crate::walls::wall_recipe(part).is_some() {
            let Ok(g) = crate::walls::regenerate(store, part) else { continue };
            let u = type_of(part).and_then(|t| t.param("Pset_WallCommon.ThermalTransmittance")).and_then(|v| v.get("value")?.as_f64()).unwrap_or(1.0);
            ("knee walls", g.length() * g.height, u)
        } else {
            continue;
        };
        let s = sums.entry(key).or_default();
        s.0 += area;
        s.1 += area * u;
    }
    let parts: Vec<HeatPart> = sums.into_iter().map(|(k, (a, ua))| HeatPart { part: k.into(), area: a, u: if a > 0.0 { ua / a } else { 0.0 }, ua }).collect();
    let ua: f64 = parts.iter().map(|x| x.ua).sum();
    // The construction factor by frame: glass on metal bars 1.08 when they are at most 0.6 m
    // apart (16–24 in), 1.05 wider (48 in; the greenhouse glass's bars are 1.125 m apart); film
    // or sheet on metal 1.02; anything on timber 1.00 (E-60).
    let timber = r.frame_system.as_deref().and_then(crate::frame_library::entry).is_some_and(|f| f.frame.material == crate::frame_library::Material::Timber);
    let construction_factor = if timber {
        1.0
    } else if r.form.tunnel() || plastic {
        1.02
    } else if ROOF_BAR_SPACING <= 0.6 {
        1.08
    } else {
        1.05
    };
    let wind = p.wind.unwrap_or(6.7);
    if !(0.0..=60.0).contains(&wind) {
        return Err(bad("growing.bad_value", "the design wind speed is 0 to 60 m/s"));
    }
    let wind_factor = wind_factor(wind);
    let air_changes = p.air_changes.unwrap_or(if r.form.tunnel() { 0.8 } else { 1.0 });
    if !(0.0..=10.0).contains(&air_changes) {
        return Err(bad("growing.bad_value", "air changes are 0 to 10 an hour"));
    }
    let floor_area = r.width() * r.length;
    let volume = floor_area * (r.eave + 0.5 * (r.ridge() - r.eave));
    let delta_t = inside - outside;
    let transmission = ua * construction_factor * wind_factor * delta_t;
    let infiltration = 0.33 * air_changes * volume * delta_t;
    let total = transmission + infiltration;
    let screen = r.screens.iter().find(|s| matches!(s.kind, ScreenKind::Energy | ScreenKind::Blackout)).map(material_of);
    Ok(HeatLoss {
        parts,
        construction_factor,
        wind_factor,
        air_changes,
        volume,
        inside,
        outside,
        delta_t,
        transmission,
        infiltration,
        total,
        floor_area,
        per_area: total / floor_area.max(1e-9),
        with_screen: screen.as_ref().map(|m| total * (1.0 - m.saving)),
        screen: screen.map(|m| m.name.to_owned()),
    })
}

// ---- Ventilation sizing (ARC-GLZ-012) ----

/// m³/s per m² of floor for 1 cfm per ft².
const CFM_PER_FT2: f64 = 0.000_471_947 / 0.092_903;

/// The climate the fans are sized for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VentClimate {
    #[default]
    Temperate,
    /// 10 cfm/ft² (southern climates).
    Hot,
}

/// Evaporative pads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Pads {
    #[default]
    None,
    /// 100 mm (4 in) cellulose pads, 1.27 m/s face velocity.
    #[serde(rename = "100")]
    Mm100,
    /// 150 mm (6 in), 2.03 m/s.
    #[serde(rename = "150")]
    Mm150,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VentilationParams {
    pub enclosure_id: ElementRef,
    #[serde(default)]
    pub climate: VentClimate,
    /// Used in summer (default true; ¾ of the airflow otherwise).
    pub summer: Option<bool>,
    #[serde(default)]
    pub pads: Pads,
    /// One fan's airflow, m³/s (default 10), and its diameter, m (default 1.22).
    pub fan_airflow: Option<f64>,
    pub fan_diameter: Option<f64>,
    /// The site's elevation, m (default the project location's).
    pub elevation: Option<f64>,
}

/// What `enclosure.ventilation` returns (m³/s, m², m).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Ventilation {
    pub floor_area: f64,
    /// The design airflow (with the altitude and pads allowances), and per m² of floor.
    pub airflow: f64,
    pub airflow_per_area: f64,
    pub altitude_factor: f64,
    pub fans: u32,
    pub fan_airflow: f64,
    /// The inlets' least free area: 1.25 × the fans' face area.
    pub inlet_area: f64,
    /// Pad area, and the 1-in-20 rule's area for comparison.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pad_area: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pad_rule_area: Option<f64>,
    /// Horizontal air-flow fans' and winter fan-jets' airflow.
    pub haf_airflow: f64,
    pub winter_airflow: f64,
    /// The natural vents' free area as a share of the floor (percent): the roof's and the
    /// side walls', against 15–20 % each.
    pub roof_vent_percent: f64,
    pub side_vent_percent: f64,
    pub notes: Vec<String>,
}

/// `enclosure.ventilation`.
pub(crate) fn ventilation(store: &Store, p: VentilationParams) -> Result<Ventilation> {
    let (id, r) = enclosure(store, &p.enclosure_id)?;
    let floor_area = r.width() * r.length;
    let rate = match p.climate {
        VentClimate::Temperate => 8.0,
        VentClimate::Hot => 10.0,
    } * CFM_PER_FT2;
    let summer = p.summer.unwrap_or(true);
    let elevation = p.elevation.unwrap_or_else(|| crate::location::get(store).elevation);
    if !(-500.0..=6000.0).contains(&elevation) {
        return Err(bad("growing.bad_value", "the elevation is −500 to 6,000 m"));
    }
    // Thinner air carries less heat: the same mass flow needs more volume.
    let altitude_factor = 1.0 / (1.0 - 2.25577e-5 * elevation).powf(5.25588);
    let pads_factor = if p.pads == Pads::None { 1.0 } else { 1.15 };
    let airflow = floor_area * rate * if summer { 1.0 } else { 0.75 } * altitude_factor * pads_factor;
    let fan_airflow = p.fan_airflow.unwrap_or(10.0);
    let fan_diameter = p.fan_diameter.unwrap_or(1.22);
    if !(0.1..=50.0).contains(&fan_airflow) || !(0.2..=3.0).contains(&fan_diameter) {
        return Err(bad("growing.bad_value", "a fan moves 0.1 to 50 m³/s and is 0.2 to 3 m across"));
    }
    let fans = (airflow / fan_airflow).ceil().max(1.0) as u32;
    let face = std::f64::consts::PI * fan_diameter * fan_diameter / 4.0;
    let inlet_area = 1.25 * face * f64::from(fans);
    let (pad_area, pad_rule_area) = match p.pads {
        Pads::None => (None, None),
        Pads::Mm100 => (Some(airflow / 1.27), Some(floor_area / 20.0)),
        Pads::Mm150 => (Some(airflow / 2.03), Some(floor_area / 20.0)),
    };
    let info = crate::enclosures::get(store, crate::enclosures::GetParams { enclosure_id: ElementRef::Id(id) })?;
    let roof_free = info.free_vent_area - info.side_vent_area.min(info.free_vent_area);
    let roof_vent_percent = 100.0 * roof_free / floor_area.max(1e-9);
    let side_vent_percent = 100.0 * info.side_vent_area / floor_area.max(1e-9);
    let mut notes = Vec::new();
    let draw = r.length;
    if draw > 46.0 {
        notes.push(format!("the house is {draw:.0} m long: fans draw at most 46 m (150 ft), so draw from both ends or across"));
    }
    for (what, pc) in [("roof", roof_vent_percent), ("side", side_vent_percent)] {
        if pc < 15.0 {
            notes.push(format!("the {what} vents open {pc:.1} % of the floor; 15–20 % is the guidance (more in warm climates)"));
        }
    }
    notes.push("fans and inlets sit about 0.9 m above the floor; aim for at least 15 cfm/W (7 L/s per W)".into());
    Ok(Ventilation {
        floor_area,
        airflow,
        airflow_per_area: airflow / floor_area.max(1e-9),
        altitude_factor,
        fans,
        fan_airflow,
        inlet_area,
        pad_area,
        pad_rule_area,
        haf_airflow: floor_area * 2.0 * CFM_PER_FT2,
        winter_airflow: floor_area * 0.5 * CFM_PER_FT2,
        roof_vent_percent,
        side_vent_percent,
        notes,
    })
}

// ---- CO₂ enrichment (ARC-GLZ-013) ----

/// What the glazing is, for the CO₂ supply's leakage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Cladding {
    Glass,
    Plastic,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Co2Params {
    pub enclosure_id: ElementRef,
    /// ppm (default the climate's set point, else 1,000).
    pub setpoint: Option<f64>,
    /// Default by the roof glazing.
    pub glazing: Option<Cladding>,
}

/// What `enclosure.co2` returns.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Co2 {
    pub floor_area: f64,
    pub setpoint: f64,
    pub glazing: Cladding,
    /// kg/h to hold the set point with the vents shut: the estimate and its range.
    pub supply: f64,
    pub supply_range: [f64; 2],
    /// Natural-gas burner capacity, kW (range), and the gas it burns at the estimated supply.
    pub burner_kw: [f64; 2],
    pub gas_m3_per_hour: f64,
    pub water_l_per_hour: f64,
    pub notes: Vec<String>,
}

/// `enclosure.co2`.
pub(crate) fn co2(store: &Store, p: Co2Params) -> Result<Co2> {
    let (id, r) = enclosure(store, &p.enclosure_id)?;
    let floor_area = r.width() * r.length;
    let setpoint = p.setpoint.or(climate(store.get(id).expect("resolved")).map(|c| c.co2)).unwrap_or(1000.0);
    if !(400.0..=2000.0).contains(&setpoint) {
        return Err(bad("growing.bad_value", "an enrichment set point is 400 to 2,000 ppm (5,000 ppm harms people)"));
    }
    let glazing = p.glazing.unwrap_or_else(|| if r.form.tunnel() || plastic_roof(store, &r) { Cladding::Plastic } else { Cladding::Glass });
    // kg/h per 100 m² to hold 1,300 ppm, scaled by the excess over 400 ppm outdoors.
    let (lo, hi, kw) = match glazing {
        Cladding::Glass => (0.50, 0.60, [30.0, 36.0]),
        Cladding::Plastic => (0.25, 0.35, [15.0, 18.0]),
    };
    let scale = (setpoint - 400.0).max(0.0) / 900.0 * floor_area / 100.0;
    let supply = (lo + hi) / 2.0 * scale;
    let gas = supply / 1.8;
    let notes = vec![
        "dose from half an hour after sunrise to near sunset; drop to 400–600 ppm, or stop, once the vents are more than 10 % open".into(),
        "keep CO₂ below 5,000 ppm where people work; cut burners off at 6–10 ppm CO in the flue".into(),
        "ethylene harms crops from 0.05 ppm and SO₂ from 0.2 ppm: fuel sulphur at most 0.02 %, low-NOx burners".into(),
    ];
    Ok(Co2 { floor_area, setpoint, glazing, supply, supply_range: [lo * scale, hi * scale], burner_kw: [kw[0] * floor_area / 1000.0, kw[1] * floor_area / 1000.0], gas_m3_per_hour: gas, water_l_per_hour: gas * 1.4, notes })
}

/// The roof glazing is plastic (film, sheet).
fn plastic_roof(store: &Store, r: &EnclosureRecipe) -> bool {
    r.parts.iter().filter_map(|g| store.get(store.find_by_guid(&GlobalId::parse(g)?)?)).filter(|e| crate::roofs::roof_recipe(e).is_some()).any(|e| {
        e.param("@type")
            .and_then(|v| store.get(store.find_by_guid(&GlobalId::parse(v.as_str()?)?)?))
            .and_then(crate::glazing_library::of_type)
            .is_some_and(|g| g.safety == crate::glazing_library::Safety::Plastic)
    })
}

// ---- Heating sizing (ARC-GLZ-014) ----

/// Perimeter fin pipe's output (W/m), under-bench fin pipe's (W/m), root-zone heat (W/m²).
const PERIMETER_FIN: f64 = 1150.0;
const UNDER_BENCH_FIN: f64 = 150.0;
const ROOT_ZONE: f64 = 63.0;
/// One unit heater per this much house length; a heater's range (kW).
const HEATER_REACH: f64 = 18.0;
const HEATER_KW: [f64; 2] = [3.5, 205.0];

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeatingParams {
    pub enclosure_id: ElementRef,
    /// As enclosure.heat_loss.
    pub inside: Option<f64>,
    pub outside: Option<f64>,
    pub wind: Option<f64>,
    /// One unit heater's output, kW (default the load over the least count).
    pub heater_kw: Option<f64>,
}

/// What `enclosure.heating` returns (kW, m).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Heating {
    /// The design load, and with the energy screen closed.
    pub load_kw: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_screened_kw: Option<f64>,
    pub unit_heaters: u32,
    pub heater_kw: f64,
    /// Perimeter fin pipe: length, the house's perimeter, and runs round it.
    pub perimeter_fin_m: f64,
    pub perimeter_m: f64,
    pub perimeter_runs: f64,
    /// Under-bench fin pipe for the whole load, and the runs the benches take (four under a
    /// bench 1.5 m wide or more, else two) with their length.
    pub under_bench_fin_m: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bench_runs_m: Option<f64>,
    /// Root-zone heat over the growing area, and its share of the load (percent).
    pub growing_area: f64,
    pub root_zone_kw: f64,
    pub root_zone_percent: f64,
    pub notes: Vec<String>,
}

/// `enclosure.heating`.
pub(crate) fn heating(store: &Store, p: HeatingParams) -> Result<Heating> {
    let loss = heat_loss(store, HeatLossParams { enclosure_id: p.enclosure_id.clone(), inside: p.inside, outside: p.outside, wind: p.wind, air_changes: None })?;
    let (id, r) = enclosure(store, &p.enclosure_id)?;
    let load_kw = loss.total / 1000.0;
    let least = (r.length / HEATER_REACH).ceil().max(1.0) as u32;
    let heater_kw = match p.heater_kw {
        Some(k) if (HEATER_KW[0]..=HEATER_KW[1]).contains(&k) => k,
        Some(_) => return Err(bad("growing.bad_value", "a unit heater gives 3.5 to 205 kW")),
        None => (load_kw / f64::from(least)).clamp(HEATER_KW[0], HEATER_KW[1]),
    };
    let unit_heaters = least.max((load_kw / heater_kw).ceil() as u32);
    let perimeter_m = 2.0 * (r.length + r.width());
    let perimeter_fin_m = loss.total / PERIMETER_FIN;
    // The growing layouts on this enclosure: their pieces' area and bench runs.
    let guid = store.get(id).expect("resolved").guid().to_string();
    let mut growing_area = 0.0;
    let mut bench_runs = 0.0;
    let mut benches = false;
    for l in store.iter().filter_map(crate::growing::recipe).filter(|l| l.enclosure == guid) {
        let parts: Vec<crate::growing::PartRecipe> = l.parts.iter().filter_map(|g| store.get(store.find_by_guid(&GlobalId::parse(g)?)?)).filter_map(crate::growing::part_recipe).collect();
        growing_area += crate::growing::growing_area(&parts);
        if matches!(l.system, crate::growing::System::Benches | crate::growing::System::RollingBenches | crate::growing::System::EbbFlow) {
            benches = true;
            let runs = if l.width >= 1.5 { 4.0 } else { 2.0 };
            bench_runs += parts.iter().map(|q| (q.end[0] - q.start[0]).hypot(q.end[1] - q.start[1])).sum::<f64>() * runs;
        }
    }
    let root_zone_kw = growing_area * ROOT_ZONE / 1000.0;
    let mut notes = vec![
        "each 0.56 K (1 °F) lower set point saves about 3 % of the fuel".to_owned(),
        "keep heating pipes at least 0.3 m from the plants".to_owned(),
        "annual heating demand for reference: tomato 45–413, cucumber about 460, pepper about 487 kWh/m² (by region and strategy)".to_owned(),
    ];
    if r.width() > 9.0 {
        notes.insert(0, format!("at {:.1} m wide, heat under the crop or benches as well as round the perimeter", r.width()));
    }
    if benches && bench_runs * UNDER_BENCH_FIN / 1000.0 < load_kw {
        notes.push(format!("under-bench pipe in the benches' runs gives about {:.0} kW of the {load_kw:.0} kW: the rest from the perimeter or unit heaters", bench_runs * UNDER_BENCH_FIN / 1000.0));
    }
    Ok(Heating {
        load_kw,
        load_screened_kw: loss.with_screen.map(|w| w / 1000.0),
        unit_heaters,
        heater_kw,
        perimeter_fin_m,
        perimeter_m,
        perimeter_runs: perimeter_fin_m / perimeter_m.max(1e-9),
        under_bench_fin_m: loss.total / UNDER_BENCH_FIN,
        bench_runs_m: benches.then_some(bench_runs),
        growing_area,
        root_zone_kw,
        root_zone_percent: 100.0 * root_zone_kw / load_kw.max(1e-9),
        notes,
    })
}

// ---- Heating pipes laid out (ARC-GLZ-016) ----

/// The key marking a pipe run made by `enclosure.heating_pipes`: {enclosure, system}.
pub const HEATING_PIPES_KEY: &str = "rxd:heating_pipes";

/// Heating pipe: 1¼ in steel (42.2 mm outside), as the fin-pipe outputs are given for.
const HEATING_PIPE_OD: f64 = 0.0422;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PipeSystem {
    /// Fin pipe round the inside of the walls, in loops one above another.
    Perimeter,
    /// Fin pipe under the growing layouts' benches, along their runs.
    UnderBench,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeatingPipesParams {
    pub enclosure_id: ElementRef,
    pub system: PipeSystem,
    /// Perimeter: the loops (default the heating sizing's runs, rounded up); under-bench: the runs
    /// under each bench (default 4 under a bench 1.5 m wide or more, else 2).
    pub runs: Option<u32>,
    /// The lowest run's height above the floor (default 0.3 m round the perimeter, 0.3 m below the
    /// bench top under benches).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub height: Option<f64>,
    /// Perimeter: the pipe's distance in from the walls (default 0.15 m).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: Option<f64>,
    /// The pipe's outside diameter (default 1¼ in steel, 42.2 mm).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub diameter: Option<f64>,
    /// For the default perimeter loops: the outside design temperature (°C), as enclosure.heating.
    pub outside: Option<f64>,
    /// Take this system's pipes away instead of laying them.
    #[serde(default)]
    pub remove: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct HeatingPipes {
    pub system: PipeSystem,
    pub runs: u32,
    /// The pipe segments and fittings made (the earlier ones of this system replaced).
    pub segments: Vec<ElementId>,
    pub fittings: Vec<ElementId>,
    /// Pipe laid (m) and its output at the fin-pipe rating (kW).
    pub length: f64,
    pub output_kw: f64,
    pub replaced: usize,
}

fn tag(e: &Element) -> Option<(String, PipeSystem)> {
    let v = e.param(HEATING_PIPES_KEY)?;
    Some((v["enclosure"].as_str()?.to_owned(), serde_json::from_value(v["system"].clone()).ok()?))
}

/// The heating pipes laid for an enclosure (its GlobalId), by system.
pub fn heating_pipes_of(store: &Store, enclosure: &str) -> Vec<(ElementId, PipeSystem)> {
    store.iter().filter_map(|e| tag(e).filter(|(g, _)| g.eq_ignore_ascii_case(enclosure)).map(|(_, s)| (e.id(), s))).collect()
}

/// `enclosure.heating_pipes`: fin pipe laid round the perimeter or under the benches, in the
/// hydronic supply system, replacing what this system laid before.
pub(crate) fn heating_pipes(txn: &mut Transaction<'_>, p: HeatingPipesParams) -> Result<HeatingPipes> {
    let store = txn.store();
    let id = store.resolve(&p.enclosure_id).ok_or_else(|| bad("element.not_found", "no such enclosure"))?;
    let host = store.get(id).expect("resolved");
    let r = enclosures::recipe(host).ok_or_else(|| bad("enclosure.not_found", format!("element {id} is not a glazed enclosure")))?;
    let guid = host.guid().to_string();
    let level = GlobalId::parse(&r.level).and_then(|g| store.find_by_guid(&g)).ok_or_else(|| bad("enclosure.no_level", "the enclosure's level is gone"))?;
    let diameter = p.diameter.unwrap_or(HEATING_PIPE_OD);
    if !(0.01..=0.2).contains(&diameter) {
        return Err(bad("growing.bad_value", "the pipe's diameter is between 10 and 200 mm"));
    }
    let (s, c) = r.rotation.sin_cos();
    let at = |x: f64, y: f64, z: f64| vec![r.origin[0] + c * x - s * y, r.origin[1] + s * x + c * y, z];
    // Each run as its points in plan with heights above the level.
    let mut lines: Vec<Vec<Vec<f64>>> = Vec::new();
    let runs;
    match p.system {
        _ if p.remove => runs = 0,
        PipeSystem::Perimeter => {
            runs = match p.runs {
                Some(n) => n,
                None => {
                    let h = heating(store, HeatingParams { enclosure_id: ElementRef::Id(id), inside: None, outside: p.outside, wind: None, heater_kw: None })
                        .map_err(|e| bad("growing.no_design", format!("give the number of loops (runs), or what the heating is sized for: {}", e.message)))?;
                    (h.perimeter_runs - 1e-6).ceil().max(1.0) as u32
                }
            };
            if !(1..=8).contains(&runs) {
                return Err(bad("growing.bad_value", "1 to 8 loops round the perimeter"));
            }
            let o = p.offset.unwrap_or(0.15);
            let (l, w) = (r.length, r.width());
            if !(0.05..w / 2.0).contains(&o) {
                return Err(bad("growing.bad_value", "the pipe's distance from the walls is between 0.05 m and half the width"));
            }
            let z0 = p.height.unwrap_or(0.3);
            for k in 0..runs {
                let z = z0 + f64::from(k) * 0.15;
                // A loop starting by a corner, ending short of it for the flow and return.
                lines.push(vec![at(o + 0.3, o, z), at(l - o, o, z), at(l - o, w - o, z), at(o, w - o, z), at(o, o + 0.3, z)]);
            }
        }
        PipeSystem::UnderBench => {
            let benches: Vec<crate::growing::PartRecipe> = store
                .iter()
                .filter(|e| crate::growing::recipe(e).is_some_and(|l| l.enclosure.eq_ignore_ascii_case(&guid)))
                .flat_map(|e| crate::growing::recipe(e).map(|l| l.parts).unwrap_or_default())
                .filter_map(|g| store.get(store.find_by_guid(&GlobalId::parse(&g)?)?))
                .filter_map(crate::growing::part_recipe)
                .filter(|q| !matches!(q.system, crate::growing::System::Nft | crate::growing::System::HighWire))
                .collect();
            if benches.is_empty() {
                return Err(bad("growing.no_benches", "the enclosure has no benches or tables to lay pipe under (Growing Layout)"));
            }
            runs = p.runs.unwrap_or(0);
            if p.runs.is_some_and(|n| !(1..=8).contains(&n)) {
                return Err(bad("growing.bad_value", "1 to 8 runs under each bench"));
            }
            for b in &benches {
                let n = if runs > 0 { runs } else if b.width >= 1.5 { 4 } else { 2 };
                let (dx, dy) = (b.end[0] - b.start[0], b.end[1] - b.start[1]);
                let len = dx.hypot(dy).max(1e-9);
                let (nx, ny) = (-dy / len, dx / len);
                let top = b.top[0].min(b.top[1]);
                let z = p.height.unwrap_or((top - 0.3).max(0.1));
                for k in 0..n {
                    let across = b.width * ((f64::from(k) + 0.5) / f64::from(n) - 0.5);
                    lines.push(vec![vec![b.start[0] + nx * across, b.start[1] + ny * across, z], vec![b.end[0] + nx * across, b.end[1] + ny * across, z]]);
                }
            }
        }
    }
    // The earlier pipes of this system, replaced.
    let old: Vec<ElementId> = heating_pipes_of(txn.store(), &guid).into_iter().filter(|(_, s)| *s == p.system).map(|(e, _)| e).collect();
    for e in &old {
        txn.delete(*e)?;
    }
    let mark = json!({ "enclosure": guid, "system": p.system });
    let mut out = HeatingPipes { system: p.system, runs: 0, segments: vec![], fittings: vec![], length: 0.0, output_kw: 0.0, replaced: old.len() };
    for pts in lines {
        out.length += pts.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum::<f64>();
        let made = crate::mep_runs::create_pipe(
            txn,
            crate::mep_runs::CreateParams { type_id: None, points: pts, level: ElementRef::Id(level), middle_elevation: None, shape: None, diameter: Some(diameter), width: None, height: None, system_type_id: Some("hydronic_supply".into()), slope: 0.0, bend_radius: None },
        )?;
        for e in made.segments.iter().chain(&made.fittings) {
            let m = mark.clone();
            txn.modify(*e, |el| {
                el.params.insert(HEATING_PIPES_KEY.into(), m);
            })?;
        }
        out.segments.extend(made.segments);
        out.fittings.extend(made.fittings);
        out.runs += 1;
    }
    let rating = match p.system {
        PipeSystem::Perimeter => PERIMETER_FIN,
        PipeSystem::UnderBench => UNDER_BENCH_FIN,
    };
    out.output_kw = out.length * rating / 1000.0;
    if p.system == PipeSystem::Perimeter {
        out.runs = runs;
    }
    Ok(out)
}
