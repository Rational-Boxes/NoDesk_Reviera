// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Design heating and cooling loads (ANA-HCL-001, ANA-ENV-004), the built-in provider: a
//! simplified design-day method for early design, Reviera's own (not the radiant time series).
//! The editor distils the energy model (ANA-ENV-003), the spaces' loads and set points, the
//! design conditions and a clear design day from the weather file into a [`LoadsInput`]; the
//! built-in runner (`analysis.loads`, out of process) calls [`run`]; the editor records the peaks
//! on the spaces.
//!
//! - **Heating** (steady state at the heating design temperature, no internal or solar gains):
//!   Σ U·A·ΔT over exterior surfaces (slabs on grade at half the ΔT), plus outdoor air and
//!   infiltration at 1.2 W per L/s per K.
//! - **Infiltration**: L/s per m² of exterior surface by class: loose 0.39, medium 0.19, tight
//!   0.10, none 0 (the spec's defaults).
//! - **Cooling**, hour by hour over the design day: outdoor temperature from the cooling design
//!   dry-bulb less the daily range by a cosine profile (warmest at 15:00, coolest at 05:00);
//!   opaque exterior surfaces by sol-air temperature (absorptance ÷ outside film coefficient
//!   0.026 m²K/W × incident irradiance, less 3.9 K on roofs for sky radiation), without thermal
//!   lag; windows by U·A·ΔT plus SHGC × incident irradiance × area; incident irradiance from the
//!   design day's hourly DNI and DHI (the clearest hours of the warmest month in the weather
//!   file) on each surface's orientation; people (sensible and latent), lighting and equipment
//!   at full load in occupied hours; outdoor air and infiltration sensible and latent (latent
//!   from the humidity-ratio difference at the coincident wet-bulb). The peak hour is the one of
//!   largest total load.
//! - **Airflows**: cooling = sensible peak ÷ (1.2 × 10 K supply difference); heating = heating
//!   load ÷ (1.2 × 15 K) (EXPERT_REVIEW E-38).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::energy_model::{self, SurfaceClass};
use crate::error::{Error, Result};
use crate::id::ElementId;
use crate::solar::{Hour, SolarInput};
use crate::store::Store;
use crate::txn::Transaction;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

pub const KEY: &str = "rxd:hc_loads";
/// W per (L/s · K) of air.
const AIR: f64 = 1.2;
/// W per (L/s · g/kg) of latent air load.
const AIR_LATENT: f64 = 3.0;
const SOL_AIR: f64 = 0.026;
const ROOF_SKY: f64 = 3.9;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Infiltration {
    Loose,
    #[default]
    Medium,
    Tight,
    None,
}

impl Infiltration {
    /// L/s per m² of exterior surface.
    pub fn rate(self) -> f64 {
        match self {
            Infiltration::Loose => 0.39,
            Infiltration::Medium => 0.19,
            Infiltration::Tight => 0.10,
            Infiltration::None => 0.0,
        }
    }
}

/// One surface as the calculation needs it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LoadSurface {
    pub class: SurfaceClass,
    pub area: f64,
    pub u: f64,
    pub normal: [f64; 3],
    pub shgc: Option<f64>,
}

/// One space's inputs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LoadSpace {
    pub key: String,
    pub zone: Option<String>,
    pub area: f64,
    pub volume: f64,
    pub surfaces: Vec<LoadSurface>,
    /// W/m².
    pub lighting: f64,
    pub equipment: f64,
    pub people: f64,
    /// W per person.
    pub sensible_per_person: f64,
    pub latent_per_person: f64,
    /// L/s.
    pub outdoor_air: f64,
    /// °C.
    pub heating_set_point: f64,
    pub cooling_set_point: f64,
    /// Occupied hours (start, end), local.
    pub open: [f64; 2],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LoadsInput {
    pub spaces: Vec<LoadSpace>,
    pub heating_dry_bulb: f64,
    pub cooling_dry_bulb: f64,
    pub cooling_wet_bulb: f64,
    pub daily_range: f64,
    pub infiltration: Infiltration,
    /// The site, for sun positions.
    pub latitude: f64,
    pub longitude: f64,
    pub time_zone: f64,
    pub true_north: f64,
    /// The design day: month, day and 24 hourly DNI/DHI.
    pub design_day: Vec<Hour>,
    /// Allow negative (credit) loads in components.
    #[serde(default)]
    pub credits: bool,
}

/// Load components (W).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Components {
    pub walls: f64,
    pub roofs: f64,
    pub floors: f64,
    pub windows_conduction: f64,
    pub windows_solar: f64,
    pub doors: f64,
    pub ventilation: f64,
    pub infiltration: f64,
    pub people: f64,
    pub lighting: f64,
    pub equipment: f64,
}

impl Components {
    fn sum(&self) -> f64 {
        self.walls + self.roofs + self.floors + self.windows_conduction + self.windows_solar + self.doors + self.ventilation + self.infiltration + self.people + self.lighting + self.equipment
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SpaceLoad {
    pub key: String,
    pub zone: Option<String>,
    /// W.
    pub heating: f64,
    pub heating_components: Components,
    pub cooling_sensible: f64,
    pub cooling_latent: f64,
    pub cooling_total: f64,
    /// Hour of the day (1–24) of the cooling peak.
    pub peak_hour: u8,
    pub cooling_components: Components,
    /// L/s.
    pub cooling_airflow: f64,
    pub heating_airflow: f64,
    /// W/m².
    pub cooling_density: f64,
    pub heating_density: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LoadsResults {
    pub spaces: Vec<SpaceLoad>,
    /// By zone key: heating and cooling total (W), and the zone's coincident peak hour.
    pub zones: BTreeMap<String, (f64, f64, u8)>,
    pub heating_total: f64,
    pub cooling_total: f64,
    /// The building's coincident cooling peak (W) and its hour.
    pub cooling_coincident: f64,
    pub coincident_hour: u8,
}

/// Humidity ratio (g/kg) at a dry-bulb and wet-bulb (sea-level pressure).
fn humidity_ratio(db: f64, wb: f64) -> f64 {
    let ps = |t: f64| 610.94 * (17.625 * t / (t + 243.04)).exp();
    let p = 101_325.0;
    let ws = 621.945 * ps(wb) / (p - ps(wb));
    // The psychrometric balance solved for W (g/kg).
    ((2501.0 - 2.326 * wb) * ws - 1006.0 * (db - wb)) / (2501.0 + 1.86 * db - 4.186 * wb)
}

/// The outdoor dry-bulb at an hour (1–24) of the design day.
fn outdoor(input: &LoadsInput, hour: f64) -> f64 {
    let frac = 0.5 + 0.5 * (std::f64::consts::TAU * (hour - 15.0) / 24.0).cos();
    input.cooling_dry_bulb - input.daily_range * (1.0 - frac)
}

/// Runs the design loads.
pub fn run(input: &LoadsInput) -> std::result::Result<LoadsResults, String> {
    if input.cooling_dry_bulb <= input.heating_dry_bulb {
        return Err("the cooling design temperature must be above the heating one".into());
    }
    let sun_in = SolarInput { latitude: input.latitude, longitude: input.longitude, time_zone: input.time_zone, true_north: input.true_north, from: [1, 1], to: [12, 31], hours: Vec::new(), faces: Vec::new(), shading: Vec::new(), grid: 1.0, pv: None };
    let doy = |h: &Hour| [31u32, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][..(h.month.clamp(1, 12) as usize - 1)].iter().sum::<u32>() + h.day as u32;
    // Hourly sun and irradiance of the design day.
    let mut day: Vec<(f64, Option<[f64; 3]>, f64, f64)> = Vec::new();
    for h in &input.design_day {
        let t = h.hour as f64;
        day.push((t, crate::solar::sun(&sun_in, doy(h), t - 0.5), h.dni, h.dhi));
    }
    if day.len() != 24 {
        return Err(format!("the design day has {} hours, not 24", day.len()));
    }
    let incident = |n: [f64; 3], sun: Option<[f64; 3]>, dni: f64, dhi: f64| -> f64 {
        let direct = sun.map_or(0.0, |s| (s[0] * n[0] + s[1] * n[1] + s[2] * n[2]).max(0.0) * dni);
        // Isotropic diffuse: the share of the sky the surface sees.
        direct + dhi * (1.0 + n[2]) / 2.0
    };
    let w_out = humidity_ratio(input.cooling_dry_bulb, input.cooling_wet_bulb);
    let clamp = |v: f64| if input.credits { v } else { v.max(0.0) };
    let mut out = LoadsResults::default();
    let mut building = [0.0f64; 24];
    let mut zone_hours: BTreeMap<String, ([f64; 24], f64)> = BTreeMap::new();
    for s in &input.spaces {
        let ext_area: f64 = s.surfaces.iter().filter(|f| f.class.exterior() && f.class != SurfaceClass::SlabOnGrade).map(|f| f.area).sum();
        let infil = input.infiltration.rate() * ext_area;
        // Heating.
        let dt = s.heating_set_point - input.heating_dry_bulb;
        let mut hc = Components::default();
        for f in s.surfaces.iter().filter(|f| f.class.exterior()) {
            let q = f.u * f.area * dt;
            match f.class {
                SurfaceClass::ExteriorWall => hc.walls += q,
                SurfaceClass::Roof => hc.roofs += q,
                SurfaceClass::SlabOnGrade => hc.floors += 0.5 * q,
                SurfaceClass::Window => hc.windows_conduction += q,
                SurfaceClass::Door => hc.doors += q,
                _ => {}
            }
        }
        hc.ventilation = AIR * s.outdoor_air * dt;
        hc.infiltration = AIR * infil * dt;
        let heating = hc.sum().max(0.0);
        // Cooling, hour by hour.
        let mut best: Option<(f64, f64, f64, u8, Components)> = None;
        let w_in = humidity_ratio(s.cooling_set_point, s.cooling_set_point - 6.0).min(w_out);
        for (t, sun, dni, dhi) in &day {
            let to = outdoor(input, *t);
            let dtc = to - s.cooling_set_point;
            let occupied = *t > s.open[0] && *t <= s.open[1];
            let mut c = Components::default();
            for f in s.surfaces.iter().filter(|f| f.class.exterior()) {
                let i = incident(f.normal, *sun, *dni, *dhi);
                match f.class {
                    SurfaceClass::ExteriorWall | SurfaceClass::Roof | SurfaceClass::Door => {
                        let sky = if f.class == SurfaceClass::Roof { ROOF_SKY } else { 0.0 };
                        let tsa = to + SOL_AIR * i - sky;
                        let q = clamp(f.u * f.area * (tsa - s.cooling_set_point));
                        match f.class {
                            SurfaceClass::ExteriorWall => c.walls += q,
                            SurfaceClass::Roof => c.roofs += q,
                            _ => c.doors += q,
                        }
                    }
                    SurfaceClass::Window => {
                        c.windows_conduction += clamp(f.u * f.area * dtc);
                        c.windows_solar += f.shgc.unwrap_or(energy_model::DEFAULT_SHGC) * f.area * i;
                    }
                    _ => {}
                }
            }
            c.ventilation = clamp(AIR * s.outdoor_air * dtc);
            c.infiltration = clamp(AIR * infil * dtc);
            let mut latent = AIR_LATENT * (s.outdoor_air + infil) * (w_out - w_in).max(0.0);
            if occupied {
                c.people = s.people * s.sensible_per_person;
                c.lighting = s.lighting * s.area;
                c.equipment = s.equipment * s.area;
                latent += s.people * s.latent_per_person;
            }
            let sensible = c.sum();
            let total = sensible + latent;
            building[(*t as usize).clamp(1, 24) - 1] += total;
            if let Some(z) = &s.zone {
                zone_hours.entry(z.clone()).or_insert(([0.0; 24], 0.0)).0[(*t as usize).clamp(1, 24) - 1] += total;
            }
            if best.as_ref().is_none_or(|b| total > b.2) {
                best = Some((sensible, latent, total, *t as u8, c));
            }
        }
        let (sens, lat, tot, hour, comp) = best.expect("24 hours");
        if let Some(z) = &s.zone {
            zone_hours.get_mut(z).expect("added").1 += heating;
        }
        out.heating_total += heating;
        out.spaces.push(SpaceLoad {
            key: s.key.clone(),
            zone: s.zone.clone(),
            heating,
            heating_components: hc,
            cooling_sensible: sens,
            cooling_latent: lat,
            cooling_total: tot,
            peak_hour: hour,
            cooling_components: comp,
            cooling_airflow: sens.max(0.0) / (AIR * 10.0),
            heating_airflow: heating / (AIR * 15.0),
            cooling_density: if s.area > 0.0 { tot / s.area } else { 0.0 },
            heating_density: if s.area > 0.0 { heating / s.area } else { 0.0 },
        });
    }
    out.cooling_total = out.spaces.iter().map(|s| s.cooling_total).sum();
    let (k, v) = building.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).map(|(k, v)| (k, *v)).unwrap_or((0, 0.0));
    out.cooling_coincident = v;
    out.coincident_hour = k as u8 + 1;
    for (z, (hours, heat)) in zone_hours {
        let (k, v) = hours.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).map(|(k, v)| (k, *v)).unwrap_or((0, 0.0));
        out.zones.insert(z, (heat, v, k as u8 + 1));
    }
    Ok(out)
}

// ---- In the editor ----

/// The clear design day from a weather year: for each hour, the largest DNI of the warmest
/// month's days and the DHI of that same hour (on the 21st of the month).
pub fn design_day(hours: &[Hour], warm_month: u8) -> Vec<Hour> {
    (1..=24)
        .map(|h| {
            let best = hours.iter().filter(|x| x.month == warm_month && x.hour == h).max_by(|a, b| a.dni.total_cmp(&b.dni));
            Hour { month: warm_month, day: 21, hour: h, dni: best.map_or(0.0, |b| b.dni), dhi: best.map_or(0.0, |b| b.dhi) }
        })
        .collect()
}

fn hours_of(s: &str) -> Option<f64> {
    let (h, m) = s.trim().split_once(':')?;
    Some(h.parse::<f64>().ok()? + m.parse::<f64>().ok()? / 60.0)
}

/// The distillation's spaces, keyed by opaque keys (`s1`…), with the key → GlobalId map.
pub fn spaces(store: &Store) -> (Vec<LoadSpace>, BTreeMap<String, String>) {
    let m = energy_model::build(store);
    let mut map = BTreeMap::new();
    let mut zone_keys: BTreeMap<ElementId, String> = BTreeMap::new();
    let mut out = Vec::new();
    for (k, a) in m.spaces.iter().enumerate() {
        let key = format!("s{}", k + 1);
        if let Some(e) = store.get(a.space) {
            map.insert(key.clone(), e.guid().to_string());
        }
        let zone = a.zone.map(|z| {
            let n = zone_keys.len();
            let zk = zone_keys.entry(z).or_insert_with(|| format!("z{}", n + 1)).clone();
            if let Some(e) = store.get(z) {
                map.insert(zk.clone(), e.guid().to_string());
            }
            zk
        });
        let v = crate::mep_spaces::info(store, a.space).map(|i| i.values).unwrap_or_default();
        let people = if v.area_per_person > 0.0 { a.area / v.area_per_person } else { 0.0 };
        out.push(LoadSpace {
            key,
            zone,
            area: a.area,
            volume: a.volume,
            surfaces: a.surfaces.iter().filter(|f| f.class != SurfaceClass::Air).map(|f| LoadSurface { class: f.class, area: f.area, u: f.u, normal: f.normal, shgc: f.shgc }).collect(),
            lighting: v.lighting_load_density,
            equipment: v.power_load_density,
            people,
            sensible_per_person: v.sensible_heat_per_person,
            latent_per_person: v.latent_heat_per_person,
            outdoor_air: v.outside_air_per_person * people + v.outside_air_per_area * a.area,
            heating_set_point: v.heating_set_point,
            cooling_set_point: v.cooling_set_point,
            open: [hours_of(&v.opening_time).unwrap_or(8.0), hours_of(&v.closing_time).unwrap_or(18.0)],
        });
    }
    (out, map)
}

/// A run as kept.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LoadsRun {
    pub id: String,
    pub at: i64,
    pub infiltration: Infiltration,
    pub heating_dry_bulb: f64,
    pub cooling_dry_bulb: f64,
    pub results: LoadsResults,
    /// Opaque key → GlobalId (spaces and zones).
    pub key_map: BTreeMap<String, String>,
    /// The weather revision it used (ANA-EM-001), for staleness.
    pub weather_revision: u64,
    /// A hash of its input.
    pub inputs_hash: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct LoadsData {
    pub runs: Vec<LoadsRun>,
}

pub fn data(store: &Store) -> LoadsData {
    store.iter().find(|e| e.class == "IfcProject").and_then(|p| p.param(KEY)).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordParams {
    pub job_id: String,
    pub infiltration: Infiltration,
    pub heating_dry_bulb: f64,
    pub cooling_dry_bulb: f64,
    pub results: LoadsResults,
    pub key_map: BTreeMap<String, String>,
    pub weather_revision: u64,
    pub inputs_hash: String,
    #[serde(default)]
    pub at: i64,
}

/// `hc_loads.record`: keeps a run (the last ten) and writes each space's peaks to its
/// `Pset_SpaceThermalLoad` and `Pset_SpaceThermalDesign` (so they go to IFC).
pub(crate) fn record(txn: &mut Transaction<'_>, p: RecordParams) -> Result<LoadsRun> {
    let run = LoadsRun { id: p.job_id, at: p.at, infiltration: p.infiltration, heating_dry_bulb: p.heating_dry_bulb, cooling_dry_bulb: p.cooling_dry_bulb, results: p.results, key_map: p.key_map, weather_revision: p.weather_revision, inputs_hash: p.inputs_hash };
    let store = txn.store().clone();
    for s in &run.results.spaces {
        let Some(id) = run.key_map.get(&s.key).and_then(|g| crate::id::GlobalId::parse(g)).and_then(|g| store.find_by_guid(&g)) else { continue };
        let ty = |t: &str, v: f64| json!({ "type": t, "value": v });
        let vals = [
            ("Pset_SpaceThermalDesign.CoolingDesignAirflow", ty("IfcVolumetricFlowRateMeasure", s.cooling_airflow / 1000.0)),
            ("Pset_SpaceThermalDesign.HeatingDesignAirflow", ty("IfcVolumetricFlowRateMeasure", s.heating_airflow / 1000.0)),
            ("Pset_SpaceThermalDesign.TotalSensibleHeatGain", ty("IfcPowerMeasure", s.cooling_sensible)),
            ("Pset_SpaceThermalDesign.TotalHeatGain", ty("IfcPowerMeasure", s.cooling_total)),
            ("Pset_SpaceThermalDesign.TotalHeatLoss", ty("IfcPowerMeasure", s.heating)),
        ];
        txn.modify(id, |e| {
            for (k, v) in vals {
                e.params.insert(k.into(), v);
            }
        })?;
    }
    let mut d = data(&store);
    d.runs.retain(|x| x.id != run.id);
    d.runs.push(run.clone());
    if d.runs.len() > 10 {
        let n = d.runs.len() - 10;
        d.runs.drain(..n);
    }
    let pid = store.iter().find(|e| e.class == "IfcProject").map(crate::element::Element::id).ok_or_else(|| bad("project.missing", "the model has no project"))?;
    let v = serde_json::to_value(&d).expect("serialises");
    txn.modify(pid, |e| {
        e.params.insert(KEY.into(), v);
    })?;
    Ok(run)
}

/// A space's line of a report.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReportSpace {
    pub space: Option<ElementId>,
    pub number: String,
    pub name: String,
    pub load: SpaceLoad,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Report {
    pub id: String,
    pub infiltration: Infiltration,
    pub heating_dry_bulb: f64,
    pub cooling_dry_bulb: f64,
    pub heating_total: f64,
    pub cooling_total: f64,
    pub cooling_coincident: f64,
    pub coincident_hour: u8,
    pub spaces: Vec<ReportSpace>,
    /// By system-zone: (zone element, heating W, cooling W, peak hour).
    pub zones: Vec<(Option<ElementId>, f64, f64, u8)>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportParams {
    pub run_id: Option<String>,
}

/// `hc_loads.report`.
pub(crate) fn report(store: &Store, p: ReportParams) -> Result<Report> {
    let d = data(store);
    let r = match &p.run_id {
        Some(id) => d.runs.iter().find(|x| &x.id == id),
        None => d.runs.last(),
    }
    .ok_or_else(|| bad("hc_loads.no_run", "no heating and cooling loads run yet"))?;
    let find = |k: &str| r.key_map.get(k).and_then(|g| crate::id::GlobalId::parse(g)).and_then(|g| store.find_by_guid(&g));
    Ok(Report {
        id: r.id.clone(),
        infiltration: r.infiltration,
        heating_dry_bulb: r.heating_dry_bulb,
        cooling_dry_bulb: r.cooling_dry_bulb,
        heating_total: r.results.heating_total,
        cooling_total: r.results.cooling_total,
        cooling_coincident: r.results.cooling_coincident,
        coincident_hour: r.results.coincident_hour,
        spaces: r
            .results
            .spaces
            .iter()
            .map(|s| {
                let id = find(&s.key);
                let (number, name) = id.and_then(|i| store.get(i)).map(crate::rooms::number_and_name).unwrap_or_default();
                ReportSpace { space: id, number, name, load: s.clone() }
            })
            .collect(),
        zones: r.results.zones.iter().map(|(k, (h, c, hr))| (find(k), *h, *c, *hr)).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> LoadsInput {
        let day = (1..=24).map(|h| Hour { month: 7, day: 21, hour: h, dni: if (7..=18).contains(&h) { 700.0 } else { 0.0 }, dhi: if (7..=18).contains(&h) { 120.0 } else { 0.0 } }).collect();
        LoadsInput {
            spaces: vec![LoadSpace {
                key: "s1".into(),
                zone: None,
                area: 20.0,
                volume: 60.0,
                surfaces: vec![
                    LoadSurface { class: SurfaceClass::ExteriorWall, area: 10.0, u: 0.3, normal: [0.0, -1.0, 0.0], shgc: None },
                    LoadSurface { class: SurfaceClass::Window, area: 4.0, u: 1.4, normal: [0.0, -1.0, 0.0], shgc: Some(0.6) },
                    LoadSurface { class: SurfaceClass::Roof, area: 20.0, u: 0.2, normal: [0.0, 0.0, 1.0], shgc: None },
                    LoadSurface { class: SurfaceClass::SlabOnGrade, area: 20.0, u: 0.25, normal: [0.0, 0.0, -1.0], shgc: None },
                ],
                lighting: 8.0,
                equipment: 10.0,
                people: 2.0,
                sensible_per_person: 75.0,
                latent_per_person: 55.0,
                outdoor_air: 20.0,
                heating_set_point: 20.0,
                cooling_set_point: 24.0,
                open: [8.0, 18.0],
            }],
            heating_dry_bulb: -10.0,
            cooling_dry_bulb: 30.0,
            cooling_wet_bulb: 20.0,
            daily_range: 10.0,
            infiltration: Infiltration::Medium,
            latitude: 52.0,
            longitude: 0.0,
            time_zone: 0.0,
            true_north: 0.0,
            design_day: day,
            credits: false,
        }
    }

    #[test]
    fn heating_by_hand() {
        let r = run(&input()).unwrap();
        let s = &r.spaces[0];
        // ΔT 30 K: wall 0.3·10·30 = 90, window 1.4·4·30 = 168, roof 0.2·20·30 = 120, slab
        // 0.25·20·30/2 = 75; air 1.2·20·30 = 720; infiltration 1.2 · 0.19 · 34 m² · 30.
        let expect = 90.0 + 168.0 + 120.0 + 75.0 + 720.0 + 1.2 * 0.19 * 34.0 * 30.0;
        assert!((s.heating - expect).abs() < 1e-6, "{} against {expect}", s.heating);
        assert!((s.heating_airflow - expect / 18.0).abs() < 1e-9);
    }

    #[test]
    fn tighter_is_less_heating_and_cooling_peaks_in_the_day() {
        let mut i = input();
        let medium = run(&i).unwrap().spaces[0].heating;
        i.infiltration = Infiltration::Tight;
        let tight = run(&i).unwrap().spaces[0].heating;
        assert!(tight < medium);
        let r = run(&i).unwrap();
        let s = &r.spaces[0];
        assert!(s.cooling_total > s.cooling_sensible && s.cooling_sensible > 0.0);
        assert!((9..=18).contains(&s.peak_hour), "{}", s.peak_hour);
        assert!(s.cooling_components.windows_solar > 0.0);
        assert!((s.cooling_components.lighting - 160.0).abs() < 1e-9);
    }
}
