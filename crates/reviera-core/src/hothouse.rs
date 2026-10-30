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
//! Not yet: screens in the light path, the hemispherical (diffuse) transmission of the
//! glazing, lamps switched off in bright sun, a uniformity map from photometry, lights in
//! rooms, and the lamps' heat in the loads.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
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
    let months: Vec<Month> = out
        .iter()
        .enumerate()
        .map(|(m, o)| {
            let natural = o * t.total;
            let lamps = lamp_ppfd * hours * HOUR;
            let total = natural + lamps;
            Month { month: m as u8 + 1, outdoor: *o, natural, lamps, total, meets_target: c.as_ref().is_none_or(|c| total >= c.target_dli - 1e-9) }
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
        for (pset, written) in [(CLIMATE_PSET, climate_params(&c)), (LIGHT_PSET, fixture_params(&r).into_iter().chain(layout_params(&s)).collect())] {
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
