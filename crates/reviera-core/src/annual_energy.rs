// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Annual heating and cooling energy (ANA-ENV-005), the built-in provider: a lumped hourly
//! zone model for early design, Reviera's own simplification. The editor distils the energy
//! model's spaces (as the loads do, [`crate::hvac_loads::spaces`]), the weather year and the
//! options into an [`EnergyInput`]; the built-in runner (`analysis.energy`, out of process) calls
//! [`run`]; the editor records the run on the project.
//!
//! - **Network**, per space, three nodes: air, the room's surfaces, and one thermal mass.
//!   Outdoor air and infiltration couple outdoors to the air node; glazing and doors couple
//!   outdoors to the surface node; opaque walls and roofs couple outdoors (by sol-air
//!   temperature) to the mass node, through the mass–surface conductance; slabs on grade
//!   couple the ground (the year's mean outdoor temperature) to the mass node; the air and
//!   surface nodes are coupled by the internal surface conductance. Air has no capacity; the
//!   mass is integrated implicitly (backward Euler, one-hour steps).
//! - **Gains**: half the internal gains to the air, half radiant, split with the solar gains
//!   between the mass and surface nodes by area (EXPERT_REVIEW E-39).
//! - **Ideal thermostat**: in each hour the air is held at the heating or cooling set point
//!   when it would float outside them (set back by 4 K when unoccupied), and the power needed
//!   is the hour's load; latent loads are counted when cooling dehumidifies the outdoor air,
//!   and from people.
//! - **Schedules**: occupied from opening to closing time on weekdays (the weather file's
//!   first day of the week, Sunday when it does not say); lighting and equipment at full load
//!   when occupied, 10% and 30% otherwise; outdoor air only when occupied; infiltration always.
//! - **Energy**: heating by the heating source's efficiency (gas boiler, electric resistance or
//!   heat pump), cooling by its COP; lighting and equipment as electricity; operational carbon
//!   from the grid factor (the user's; none by default) and the gas factor.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::energy_model::{self, SurfaceClass};
use crate::error::{Error, Result};
use crate::hvac_loads::{Infiltration, LoadSpace};
use crate::solar::SolarInput;
use crate::store::Store;
use crate::txn::Transaction;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

pub const KEY: &str = "rxd:energy";
const AIR: f64 = 1.2;
const AIR_LATENT: f64 = 3.0;
const SOL_AIR: f64 = 0.026;
const ROOF_SKY: f64 = 3.9;
/// Internal surface conductance, W/(m²K) of the surface area (4.5 × the floor area).
const H_IS: f64 = 3.45;
const AT_RATIO: f64 = 4.5;
/// Mass–surface conductance, W/(m²K) of the effective mass area.
const H_MS: f64 = 9.1;
const SETBACK: f64 = 4.0;
const MONTH_DAYS: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

/// An hour of the weather year as the model needs it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WeatherHour {
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    /// °C.
    pub dry_bulb: f64,
    pub dew_point: f64,
    /// W/m².
    pub dni: f64,
    pub dhi: f64,
}

/// Reads an EPW file's hours and the weekday of the first day (0 Sunday … 6 Saturday).
pub fn epw_year(text: &str) -> Result<(Vec<WeatherHour>, u8)> {
    let mut first = 0u8;
    if let Some(dp) = text.lines().take(8).find(|l| l.starts_with("DATA PERIODS")) {
        let day = dp.split(',').nth(4).unwrap_or("").trim().to_ascii_lowercase();
        let days = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];
        first = days.iter().position(|d| *d == day).unwrap_or(0) as u8;
    }
    let mut out = Vec::new();
    for (k, line) in text.lines().enumerate().skip(8) {
        if line.trim().is_empty() {
            continue;
        }
        let c: Vec<&str> = line.split(',').collect();
        let n = k + 1;
        if c.len() < 16 {
            return Err(bad("weather.bad_epw", format!("line {n}: an hourly record has {} fields", c.len())));
        }
        let num = |i: usize| c[i].trim().parse::<f64>().map_err(|_| bad("weather.bad_epw", format!("line {n}: field {} is not a number", i + 1)));
        let ok = |v: f64| if (0.0..2000.0).contains(&v) { v } else { 0.0 };
        let (db, dp) = (num(6)?, num(7)?);
        if !(-90.0..70.0).contains(&db) || !(-90.0..70.0).contains(&dp) {
            return Err(bad("weather.bad_epw", format!("line {n}: the dry-bulb or dew-point temperature is missing")));
        }
        out.push(WeatherHour { month: num(1)? as u8, day: num(2)? as u8, hour: num(3)? as u8, dry_bulb: db, dew_point: dp.min(db), dni: ok(num(14)?), dhi: ok(num(15)?) });
    }
    if out.len() < 8760 {
        return Err(bad("weather.short_epw", format!("the weather file has {} hourly records, not a year", out.len())));
    }
    out.truncate(8760);
    Ok((out, first))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HeatingSource {
    #[default]
    Gas,
    Electric,
    HeatPump,
}

/// The construction's thermal mass class: (heat capacity J/K and effective mass area, each per
/// m² of floor).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mass {
    Light,
    #[default]
    Medium,
    Heavy,
}

impl Mass {
    fn values(self) -> (f64, f64) {
        match self {
            Mass::Light => (110_000.0, 2.5),
            Mass::Medium => (165_000.0, 2.5),
            Mass::Heavy => (260_000.0, 3.0),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct EnergyOptions {
    pub infiltration: Infiltration,
    pub mass: Mass,
    pub heating: HeatingSource,
    /// Seasonal efficiency (boiler, resistance) or COP (heat pump); the source's default when 0.
    pub heating_efficiency: f64,
    pub cooling_cop: f64,
    /// kg CO2e per kWh of grid electricity; operational carbon of electricity is left out
    /// without it.
    pub grid_factor: Option<f64>,
    /// kg CO2e per kWh of gas.
    pub gas_factor: f64,
}

impl Default for EnergyOptions {
    fn default() -> Self {
        EnergyOptions { infiltration: Infiltration::Medium, mass: Mass::Medium, heating: HeatingSource::Gas, heating_efficiency: 0.0, cooling_cop: 3.0, grid_factor: None, gas_factor: 0.202 }
    }
}

impl EnergyOptions {
    pub fn efficiency(&self) -> f64 {
        if self.heating_efficiency > 0.0 {
            return self.heating_efficiency;
        }
        match self.heating {
            HeatingSource::Gas => 0.9,
            HeatingSource::Electric => 1.0,
            HeatingSource::HeatPump => 3.0,
        }
    }

    pub fn check(&self) -> Result<()> {
        let e = self.efficiency();
        let max = if self.heating == HeatingSource::HeatPump { 8.0 } else { 1.1 };
        if !(0.3..=max).contains(&e) {
            return Err(bad("energy.bad_option", format!("the heating efficiency {e} is outside 0.3–{max}")));
        }
        if !(1.0..=10.0).contains(&self.cooling_cop) {
            return Err(bad("energy.bad_option", "the cooling COP must be 1–10"));
        }
        if self.grid_factor.is_some_and(|g| !(0.0..=2.0).contains(&g)) || !(0.0..=1.0).contains(&self.gas_factor) {
            return Err(bad("energy.bad_option", "the emission factors must be 0–2 (grid) and 0–1 (gas) kg CO2e/kWh"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EnergyInput {
    pub spaces: Vec<LoadSpace>,
    pub latitude: f64,
    pub longitude: f64,
    pub time_zone: f64,
    pub true_north: f64,
    /// 8760 hours.
    pub weather: Vec<WeatherHour>,
    /// Weekday of the first day, 0 Sunday … 6 Saturday.
    pub first_weekday: u8,
    pub options: EnergyOptions,
}

/// A month's (or the year's) energy, kWh, and carbon, kg CO2e.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct EndUses {
    /// Thermal loads met.
    pub heating_load: f64,
    pub cooling_load: f64,
    /// Delivered energy.
    pub heating: f64,
    pub cooling: f64,
    pub lighting: f64,
    pub equipment: f64,
    pub electricity: f64,
    pub gas: f64,
    /// None when the grid factor was not given.
    pub carbon: Option<f64>,
}

impl EndUses {
    fn add(&mut self, o: &EndUses) {
        self.heating_load += o.heating_load;
        self.cooling_load += o.cooling_load;
        self.heating += o.heating;
        self.cooling += o.cooling;
        self.lighting += o.lighting;
        self.equipment += o.equipment;
        self.electricity += o.electricity;
        self.gas += o.gas;
        self.carbon = match (self.carbon, o.carbon) {
            (Some(a), Some(b)) => Some(a + b),
            (a, b) => a.or(b),
        };
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SpaceEnergy {
    pub key: String,
    pub zone: Option<String>,
    /// kWh of thermal load a year.
    pub heating_load: f64,
    pub cooling_load: f64,
    /// W.
    pub peak_heating: f64,
    pub peak_cooling: f64,
    /// Hours the air floated above the cooling set point in occupied time with no cooling
    /// (always 0 with ideal loads; kept for providers with sized systems).
    pub unmet_hours: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EnergyResults {
    pub months: Vec<EndUses>,
    pub year: EndUses,
    pub spaces: Vec<SpaceEnergy>,
    /// m² of the spaces' floor.
    pub floor_area: f64,
    /// kWh/m² a year of delivered energy.
    pub eui: f64,
    /// W.
    pub peak_heating: f64,
    pub peak_cooling: f64,
}

fn saturation(t: f64) -> f64 {
    610.94 * (17.625 * t / (t + 243.04)).exp()
}

/// Humidity ratio (g/kg) at a dew point, sea-level pressure.
fn ratio_at_dew(dp: f64) -> f64 {
    let p = saturation(dp);
    621.945 * p / (101_325.0 - p)
}

/// Solves a 3×3 linear system (Cramer's rule; the network's matrix is diagonally dominant).
fn solve3(a: [[f64; 3]; 3], b: [f64; 3]) -> [f64; 3] {
    let det = |m: [[f64; 3]; 3]| m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let d = det(a);
    let mut x = [0.0; 3];
    for (i, xi) in x.iter_mut().enumerate() {
        let mut m = a;
        for r in 0..3 {
            m[r][i] = b[r];
        }
        *xi = det(m) / d;
    }
    x
}

/// One space's network: conductances (W/K) and capacity (J/K).
struct Network {
    h_ve_inf: f64,
    h_ve_oa: f64,
    h_w: f64,
    h_em: f64,
    h_ms: f64,
    h_is: f64,
    h_g: f64,
    cm: f64,
    /// Share of the radiant and solar gains to the mass, and to the surfaces.
    to_mass: f64,
    to_surface: f64,
}

impl Network {
    fn new(s: &LoadSpace, o: &EnergyOptions) -> Network {
        let af = s.area.max(1.0);
        let (cap, am_ratio) = o.mass.values();
        let am = am_ratio * af;
        let at = AT_RATIO * af;
        let ext_area: f64 = s.surfaces.iter().filter(|f| f.class.exterior() && f.class != SurfaceClass::SlabOnGrade).map(|f| f.area).sum();
        let ua = |c: &[SurfaceClass]| s.surfaces.iter().filter(|f| c.contains(&f.class)).map(|f| f.u * f.area).sum::<f64>();
        let h_w = ua(&[SurfaceClass::Window, SurfaceClass::Door]);
        let h_op = ua(&[SurfaceClass::ExteriorWall, SurfaceClass::Roof]);
        let h_ms = H_MS * am;
        // The opaque envelope in series with the mass–surface conductance.
        let h_em = if h_op > 0.0 { 1.0 / (1.0 / h_op - 1.0 / h_ms).max(1e-9) } else { 0.0 };
        let to_mass = am / at;
        Network {
            h_ve_inf: AIR * o.infiltration.rate() * ext_area,
            h_ve_oa: AIR * s.outdoor_air,
            h_w,
            h_em: h_em.min(h_op * 1e3),
            h_ms,
            h_is: H_IS * at,
            h_g: ua(&[SurfaceClass::SlabOnGrade]),
            cm: cap * af,
            to_mass,
            to_surface: (1.0 - to_mass - h_w / (H_MS * at)).max(0.0),
        }
    }

    /// One hour: returns (air, surface, mass end temperatures) for a heating (+) or cooling (−)
    /// power `phi` to the air.
    #[allow(clippy::too_many_arguments)]
    fn step(&self, occupied: bool, te: f64, te_op: f64, tg: f64, int: f64, sol: f64, tm0: f64, phi: f64) -> [f64; 3] {
        let h_ve = self.h_ve_inf + if occupied { self.h_ve_oa } else { 0.0 };
        let rad = 0.5 * int + sol;
        let (phi_ia, phi_st, phi_m) = (0.5 * int + phi, self.to_surface * rad, self.to_mass * rad);
        let c = self.cm / 3600.0;
        // Unknowns: air, surface, mass.
        let a = [
            [h_ve + self.h_is, -self.h_is, 0.0],
            [-self.h_is, self.h_is + self.h_w + self.h_ms, -self.h_ms],
            [0.0, -self.h_ms, self.h_ms + self.h_em + self.h_g + c],
        ];
        let b = [h_ve * te + phi_ia, self.h_w * te + phi_st, self.h_em * te_op + self.h_g * tg + phi_m + c * tm0];
        solve3(a, b)
    }
}

/// Runs the year.
pub fn run(input: &EnergyInput) -> std::result::Result<EnergyResults, String> {
    if input.weather.len() != 8760 {
        return Err(format!("the weather year has {} hours, not 8760", input.weather.len()));
    }
    let o = &input.options;
    let sun_in = SolarInput { latitude: input.latitude, longitude: input.longitude, time_zone: input.time_zone, true_north: input.true_north, from: [1, 1], to: [12, 31], hours: Vec::new(), faces: Vec::new(), shading: Vec::new(), grid: 1.0, pv: None };
    let ground = input.weather.iter().map(|h| h.dry_bulb).sum::<f64>() / 8760.0;
    // Sun vectors by hour, shared by the spaces.
    let suns: Vec<Option<[f64; 3]>> = input
        .weather
        .iter()
        .map(|h| {
            let doy = MONTH_DAYS[..(h.month.clamp(1, 12) as usize - 1)].iter().sum::<u32>() + h.day as u32;
            crate::solar::sun(&sun_in, doy, h.hour as f64 - 0.5)
        })
        .collect();
    let incident = |n: [f64; 3], k: usize| -> f64 {
        let h = &input.weather[k];
        let direct = suns[k].map_or(0.0, |s| (s[0] * n[0] + s[1] * n[1] + s[2] * n[2]).max(0.0) * h.dni);
        direct + h.dhi * (1.0 + n[2]) / 2.0
    };
    let mut months = vec![EndUses::default(); 12];
    let mut spaces = Vec::new();
    let mut building_heat = vec![0.0f64; 8760];
    let mut building_cool = vec![0.0f64; 8760];
    for s in &input.spaces {
        let net = Network::new(s, o);
        let ext_area: f64 = s.surfaces.iter().filter(|f| f.class.exterior() && f.class != SurfaceClass::SlabOnGrade).map(|f| f.area).sum();
        let infil = o.infiltration.rate() * ext_area;
        let w_in = crate::hvac_loads::humidity_ratio(s.cooling_set_point, s.cooling_set_point - 6.0);
        let opaque: Vec<_> = s.surfaces.iter().filter(|f| matches!(f.class, SurfaceClass::ExteriorWall | SurfaceClass::Roof)).collect();
        let h_op: f64 = opaque.iter().map(|f| f.u * f.area).sum();
        let windows: Vec<_> = s.surfaces.iter().filter(|f| f.class == SurfaceClass::Window).collect();
        let mut tm = (s.heating_set_point + s.cooling_set_point) / 2.0;
        let mut se = SpaceEnergy { key: s.key.clone(), zone: s.zone.clone(), heating_load: 0.0, cooling_load: 0.0, peak_heating: 0.0, peak_cooling: 0.0, unmet_hours: 0 };
        // Two warm-up passes over January settle the mass.
        let order = (0..744).chain(0..744).map(|k| (k, true)).chain((0..8760).map(|k| (k, false)));
        for (k, warm) in order {
            let h = &input.weather[k];
            let day = k / 24;
            let weekday = (input.first_weekday as usize + day) % 7;
            let t = h.hour as f64;
            let occupied = (1..=5).contains(&weekday) && t > s.open[0] && t <= s.open[1];
            let te = h.dry_bulb;
            // The opaque envelope's sol-air temperature, weighted by U·A.
            let te_op = if h_op > 0.0 { opaque.iter().map(|f| f.u * f.area * (te + SOL_AIR * incident(f.normal, k) - if f.class == SurfaceClass::Roof { ROOF_SKY } else { 0.0 })).sum::<f64>() / h_op } else { te };
            let sol: f64 = windows.iter().map(|f| f.shgc.unwrap_or(energy_model::DEFAULT_SHGC) * f.area * incident(f.normal, k)).sum();
            let (light, equip) = if occupied { (s.lighting * s.area, s.equipment * s.area) } else { (0.1 * s.lighting * s.area, 0.3 * s.equipment * s.area) };
            let people = if occupied { s.people * s.sensible_per_person } else { 0.0 };
            let int = light + equip + people;
            let (hsp, csp) = if occupied { (s.heating_set_point, s.cooling_set_point) } else { (s.heating_set_point - SETBACK, s.cooling_set_point + SETBACK) };
            let free = net.step(occupied, te, te_op, ground, int, sol, tm, 0.0);
            let (phi, temps) = if free[0] < hsp || free[0] > csp {
                // Linear in the power: find the power that holds the set point.
                let target = if free[0] < hsp { hsp } else { csp };
                let probe = 10.0 * s.area.max(1.0);
                let with = net.step(occupied, te, te_op, ground, int, sol, tm, probe);
                let phi = probe * (target - free[0]) / (with[0] - free[0]);
                (phi, net.step(occupied, te, te_op, ground, int, sol, tm, phi))
            } else {
                (0.0, free)
            };
            tm = temps[2];
            if warm {
                continue;
            }
            let mut latent = 0.0;
            if phi < 0.0 {
                let oa = infil + if occupied { s.outdoor_air } else { 0.0 };
                latent += AIR_LATENT * oa * (ratio_at_dew(h.dew_point) - w_in).max(0.0);
            }
            if occupied {
                latent += s.people * s.latent_per_person;
            }
            let heat = phi.max(0.0);
            let cool = (-phi).max(0.0) + latent;
            se.heating_load += heat / 1000.0;
            se.cooling_load += cool / 1000.0;
            se.peak_heating = se.peak_heating.max(heat);
            se.peak_cooling = se.peak_cooling.max(cool);
            building_heat[k] += heat;
            building_cool[k] += cool;
            let m = &mut months[h.month.clamp(1, 12) as usize - 1];
            m.heating_load += heat / 1000.0;
            m.cooling_load += cool / 1000.0;
            m.lighting += light / 1000.0;
            m.equipment += equip / 1000.0;
        }
        spaces.push(se);
    }
    let eff = o.efficiency();
    let mut year = EndUses::default();
    for m in &mut months {
        m.heating = m.heating_load / eff;
        m.cooling = m.cooling_load / o.cooling_cop;
        let heat_elec = if o.heating == HeatingSource::Gas { 0.0 } else { m.heating };
        m.gas = if o.heating == HeatingSource::Gas { m.heating } else { 0.0 };
        m.electricity = heat_elec + m.cooling + m.lighting + m.equipment;
        m.carbon = o.grid_factor.map(|g| g * m.electricity + o.gas_factor * m.gas);
        year.add(m);
    }
    let floor_area: f64 = input.spaces.iter().map(|s| s.area).sum();
    let delivered = year.electricity + year.gas;
    Ok(EnergyResults {
        months,
        year,
        spaces,
        floor_area,
        eui: if floor_area > 0.0 { delivered / floor_area } else { 0.0 },
        peak_heating: building_heat.iter().copied().fold(0.0, f64::max),
        peak_cooling: building_cool.iter().copied().fold(0.0, f64::max),
    })
}

// ---- In the editor ----

/// A run as kept.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EnergyRun {
    pub id: String,
    pub at: i64,
    pub options: EnergyOptions,
    pub results: EnergyResults,
    /// Opaque key → GlobalId (spaces and zones).
    pub key_map: BTreeMap<String, String>,
    pub weather_revision: u64,
    pub inputs_hash: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct EnergyData {
    pub runs: Vec<EnergyRun>,
}

pub fn data(store: &Store) -> EnergyData {
    store.iter().find(|e| e.class == "IfcProject").and_then(|p| p.param(KEY)).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordParams {
    pub job_id: String,
    pub options: EnergyOptions,
    pub results: EnergyResults,
    pub key_map: BTreeMap<String, String>,
    pub weather_revision: u64,
    pub inputs_hash: String,
    #[serde(default)]
    pub at: i64,
}

/// `energy.record`: keeps a run (the last ten) on the project.
pub(crate) fn record(txn: &mut Transaction<'_>, p: RecordParams) -> Result<EnergyRun> {
    let run = EnergyRun { id: p.job_id, at: p.at, options: p.options, results: p.results, key_map: p.key_map, weather_revision: p.weather_revision, inputs_hash: p.inputs_hash };
    let store = txn.store().clone();
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReportSpace {
    pub space: Option<crate::id::ElementId>,
    pub number: String,
    pub name: String,
    pub energy: SpaceEnergy,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Report {
    pub id: String,
    pub options: EnergyOptions,
    pub months: Vec<EndUses>,
    pub year: EndUses,
    pub floor_area: f64,
    pub eui: f64,
    pub peak_heating: f64,
    pub peak_cooling: f64,
    pub spaces: Vec<ReportSpace>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportParams {
    pub run_id: Option<String>,
}

/// `energy.results`.
pub(crate) fn report(store: &Store, p: ReportParams) -> Result<Report> {
    let d = data(store);
    let r = match &p.run_id {
        Some(id) => d.runs.iter().find(|x| &x.id == id),
        None => d.runs.last(),
    }
    .ok_or_else(|| bad("energy.no_run", "no annual energy run yet"))?;
    let find = |k: &str| r.key_map.get(k).and_then(|g| crate::id::GlobalId::parse(g)).and_then(|g| store.find_by_guid(&g));
    Ok(Report {
        id: r.id.clone(),
        options: r.options.clone(),
        months: r.results.months.clone(),
        year: r.results.year.clone(),
        floor_area: r.results.floor_area,
        eui: r.results.eui,
        peak_heating: r.results.peak_heating,
        peak_cooling: r.results.peak_cooling,
        spaces: r
            .results
            .spaces
            .iter()
            .map(|s| {
                let id = find(&s.key);
                let (number, name) = id.and_then(|i| store.get(i)).map(crate::rooms::number_and_name).unwrap_or_default();
                ReportSpace { space: id, number, name, energy: s.clone() }
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hvac_loads::LoadSurface;

    fn space(u: f64) -> LoadSpace {
        LoadSpace {
            key: "s1".into(),
            zone: None,
            area: 100.0,
            volume: 300.0,
            surfaces: vec![LoadSurface { class: SurfaceClass::ExteriorWall, area: 120.0, u, normal: [0.0, -1.0, 0.0], shgc: None }],
            lighting: 0.0,
            equipment: 0.0,
            people: 0.0,
            sensible_per_person: 0.0,
            latent_per_person: 0.0,
            outdoor_air: 0.0,
            heating_set_point: 20.0,
            cooling_set_point: 26.0,
            open: [8.0, 18.0],
        }
    }

    fn year(t: f64) -> Vec<WeatherHour> {
        let mut out = Vec::new();
        for (m, d) in MONTH_DAYS.iter().enumerate() {
            for day in 1..=*d {
                for hour in 1..=24 {
                    out.push(WeatherHour { month: m as u8 + 1, day: day as u8, hour, dry_bulb: t, dew_point: t - 5.0, dni: 0.0, dhi: 0.0 });
                }
            }
        }
        out
    }

    fn input(u: f64, t: f64) -> EnergyInput {
        EnergyInput { spaces: vec![space(u)], latitude: 50.0, longitude: 0.0, time_zone: 0.0, true_north: 0.0, weather: year(t), first_weekday: 0, options: EnergyOptions { infiltration: Infiltration::None, ..Default::default() } }
    }

    /// A constant 0 °C outdoors, no gains, no air: in steady state the heating is U·A·ΔT, held at
    /// 20 °C occupied and 16 °C otherwise, every hour of the year.
    #[test]
    fn steady_heating_by_hand() {
        let r = run(&input(0.5, 0.0)).unwrap();
        let ua = 0.5 * 120.0;
        // Weekday occupied hours (8–18) at 20 °C, the rest at 16 °C; the mass lags at the
        // switches, so allow 3%.
        let mut occupied = 0;
        for day in 0..365 {
            if (1..=5).contains(&(day % 7)) {
                occupied += 10;
            }
        }
        let expect = ua * (20.0 * occupied as f64 + 16.0 * (8760 - occupied) as f64) / 1000.0;
        assert!((r.year.heating_load - expect).abs() / expect < 0.03, "{} vs {expect}", r.year.heating_load);
        assert!(r.year.cooling_load < 1e-6);
        assert!((r.year.gas - r.year.heating_load / 0.9).abs() < 1e-6);
        // The morning recovery recharges the mass, so the peak is above the steady value.
        assert!(r.spaces[0].peak_heating >= ua * 20.0 - 0.5);
        assert_eq!(r.year.carbon, None);
    }

    #[test]
    fn heat_pump_grid_carbon_and_cooling() {
        let mut i = input(0.5, 35.0);
        i.options.heating = HeatingSource::HeatPump;
        i.options.grid_factor = Some(0.2);
        let r = run(&i).unwrap();
        assert!(r.year.heating_load < 1e-6);
        assert!(r.year.cooling_load > 0.0);
        assert!((r.year.cooling - r.year.cooling_load / 3.0).abs() < 1e-6);
        assert_eq!(r.year.gas, 0.0);
        assert!((r.year.carbon.unwrap() - 0.2 * r.year.electricity).abs() < 1e-6);
        let better = run(&EnergyInput { spaces: vec![space(0.2)], ..i }).unwrap();
        assert!(better.year.cooling_load < r.year.cooling_load);
    }
}
