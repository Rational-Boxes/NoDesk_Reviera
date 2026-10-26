// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The EnergyPlus provider for annual energy (ANA-ENV-007): the same distillation as the
//! built-in model ([`crate::annual_energy`]), written as an epJSON input file for EnergyPlus,
//! which the `analysis.energy.energyplus` compute kind runs where EnergyPlus is installed
//! (never bundled); the kind reads `eplusout.sql` back and [`results`] turns it into the
//! built-in model's results, so both are recorded and shown alike.
//!
//! - **Geometry** from the HBJSON export ([`crate::hbjson`]): a zone per space (named by its
//!   opaque key), `BuildingSurface:Detailed` per face in world coordinates, counter-clockwise from
//!   outside, with `Outdoors`, `Ground`, `Adiabatic` or `Surface` (the twin face) boundaries,
//!   `FenestrationSurface:Detailed` per window and door; constructions of one `Material:NoMass`
//!   layer, windows of `WindowMaterial:SimpleGlazingSystem` (E-41); separation lines as
//!   `Construction:AirBoundary`. The building's north axis is the project's true north.
//! - **Loads and schedules** as the built-in model has them: occupied from opening to closing
//!   time on weekdays; lights and equipment at the space type's densities (10% and 30% when
//!   unoccupied), people with their sensible and latent heat, outdoor air when occupied
//!   (balanced ventilation without fan heat), infiltration always (the class's rate per m² of
//!   exterior surface); the ground at the year's mean outdoor temperature.
//! - **Ideal loads**: `ZoneHVAC:IdealLoadsAirSystem` per zone with a dual set-point thermostat
//!   at the space type's set points, set back 4 K when unoccupied. Heating and cooling energy are
//!   the ideal loads' supply air totals, monthly; peaks from hourly rates; delivered energy and
//!   carbon from them by the options, as the built-in model does ([`crate::annual_energy::finish`]).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::annual_energy::{self as ae, EndUses, EnergyInput, EnergyOptions, EnergyResults, SpaceEnergy};
use crate::error::{Error, Result};
use crate::store::Store;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// The EnergyPlus version the input is written for; the runner sets the installed version's.
pub const VERSION: &str = "26.2";
/// Unoccupied set-back, K (as the built-in model).
const SETBACK: f64 = 4.0;
pub const HEATING: &str = "Zone Ideal Loads Supply Air Total Heating Energy";
pub const COOLING: &str = "Zone Ideal Loads Supply Air Total Cooling Energy";
pub const HEATING_RATE: &str = "Zone Ideal Loads Supply Air Total Heating Rate";
pub const COOLING_RATE: &str = "Zone Ideal Loads Supply Air Total Cooling Rate";
pub const LIGHTS: &str = "Zone Lights Electricity Energy";
pub const EQUIPMENT: &str = "Zone Electric Equipment Electricity Energy";
const IDEAL: &str = " Ideal Loads";

/// What the `analysis.energy.energyplus` kind takes: the epJSON, the weather file, and what
/// turns EnergyPlus's output into results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EplusInput {
    pub epjson: Value,
    /// The EPW text.
    pub epw: String,
    pub options: EnergyOptions,
    /// Zone (space key) → floor area, m².
    pub areas: BTreeMap<String, f64>,
    /// Zone → system-zone key.
    #[serde(default)]
    pub zones: BTreeMap<String, String>,
}

/// One value from `eplusout.sql`'s ReportData (`key` as EnergyPlus wrote it).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Row {
    pub key: String,
    pub name: String,
    pub frequency: String,
    pub month: u8,
    pub time: i64,
    pub value: f64,
}

fn object(m: &mut Map<String, Value>, class: &str, name: &str, v: Value) {
    m.entry(class.to_owned()).or_insert_with(|| json!({}))[name] = v;
}

/// `HH:MM` of an hour of the day, to the quarter hour.
fn clock(h: f64) -> String {
    let q = (h.clamp(0.0, 24.0) * 4.0).round() as i64;
    format!("{:02}:{:02}", q / 4, (q % 4) * 15)
}

/// A `Schedule:Compact`'s data: `on` on weekdays from `open[0]` to `open[1]`, `off` otherwise.
fn weekdays(open: [f64; 2], on: f64, off: f64, limits: &str) -> Value {
    let (a, b) = (clock(open[0]), clock(open[1]));
    let mut f: Vec<Value> = vec![json!("Through: 12/31"), json!("For: Weekdays")];
    if a < b {
        if a != "00:00" {
            f.extend([json!(format!("Until: {a}")), json!(off)]);
        }
        f.extend([json!(format!("Until: {b}")), json!(on)]);
        if b != "24:00" {
            f.extend([json!("Until: 24:00"), json!(off)]);
        }
    } else {
        f.extend([json!("Until: 24:00"), json!(off)]);
    }
    f.extend([json!("For: AllOtherDays"), json!("Until: 24:00"), json!(off)]);
    json!({ "schedule_type_limits_name": limits, "data": f.into_iter().map(|v| json!({ "field": v })).collect::<Vec<_>>() })
}

fn vertices(pts: &Value) -> Vec<Value> {
    pts.as_array().into_iter().flatten().map(|p| json!({ "vertex_x_coordinate": p[0], "vertex_y_coordinate": p[1], "vertex_z_coordinate": p[2] })).collect()
}

/// The epJSON for an energy distillation (`input`, keyed as `key_map`) and the store's
/// geometry. Errors when the energy model has no spaces.
pub fn build(store: &Store, input: &EnergyInput, key_map: &BTreeMap<String, String>) -> Result<(Value, Vec<String>)> {
    let (hb, report) = crate::hbjson::export(store)?;
    let mut warnings = report.warnings;
    let key_of_guid: BTreeMap<&str, &str> = key_map.iter().filter(|(k, _)| k.starts_with('s')).map(|(k, g)| (g.as_str(), k.as_str())).collect();
    let spaces: BTreeMap<&str, &crate::hvac_loads::LoadSpace> = input.spaces.iter().map(|s| (s.key.as_str(), s)).collect();
    // Room and face identifiers → zone keys and face names.
    let mut rename: BTreeMap<String, String> = BTreeMap::new();
    for room in hb["rooms"].as_array().into_iter().flatten() {
        let rid = room["identifier"].as_str().unwrap_or_default();
        let Some(key) = crate::gbxml::guid_of(rid).and_then(|g| key_of_guid.get(g.as_str()).copied()) else { continue };
        rename.insert(rid.to_owned(), key.to_owned());
        for f in room["faces"].as_array().into_iter().flatten() {
            let fid = f["identifier"].as_str().unwrap_or_default();
            rename.insert(fid.to_owned(), fid.replacen(rid, key, 1));
        }
    }
    let name = |id: &Value| -> String {
        let s = id.as_str().unwrap_or_default();
        rename.get(s).cloned().unwrap_or_else(|| s.to_owned())
    };
    let mut m = Map::new();
    let o = &input.options;
    object(&mut m, "Version", "Version 1", json!({ "version_identifier": VERSION }));
    object(
        &mut m,
        "SimulationControl",
        "SimulationControl 1",
        json!({ "do_zone_sizing_calculation": "No", "do_system_sizing_calculation": "No", "do_plant_sizing_calculation": "No", "run_simulation_for_sizing_periods": "No", "run_simulation_for_weather_file_run_periods": "Yes" }),
    );
    object(&mut m, "Building", "Building", json!({ "north_axis": input.true_north, "terrain": "Suburbs", "solar_distribution": "FullExterior", "maximum_number_of_warmup_days": 25, "minimum_number_of_warmup_days": 6 }));
    object(&mut m, "Timestep", "Timestep 1", json!({ "number_of_timesteps_per_hour": 4 }));
    object(&mut m, "GlobalGeometryRules", "GlobalGeometryRules 1", json!({ "starting_vertex_position": "UpperLeftCorner", "vertex_entry_direction": "Counterclockwise", "coordinate_system": "World" }));
    const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
    object(
        &mut m,
        "RunPeriod",
        "Year",
        json!({ "begin_month": 1, "begin_day_of_month": 1, "end_month": 12, "end_day_of_month": 31, "day_of_week_for_start_day": DAYS[input.first_weekday as usize % 7], "use_weather_file_holidays_and_special_days": "No", "use_weather_file_daylight_saving_period": "No", "apply_weekend_holiday_rule": "No", "use_weather_file_rain_indicators": "Yes", "use_weather_file_snow_indicators": "Yes" }),
    );
    let mean = if input.weather.is_empty() { 10.0 } else { input.weather.iter().map(|h| h.dry_bulb).sum::<f64>() / input.weather.len() as f64 };
    const MONTHS: [&str; 12] = ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"];
    let ground: Map<String, Value> = MONTHS.iter().map(|mo| (format!("{mo}_ground_temperature"), json!((mean * 100.0).round() / 100.0))).collect();
    object(&mut m, "Site:GroundTemperature:BuildingSurface", "Site:GroundTemperature:BuildingSurface 1", Value::Object(ground));
    object(&mut m, "ScheduleTypeLimits", "Fraction", json!({ "lower_limit_value": 0.0, "upper_limit_value": 1.0, "numeric_type": "Continuous" }));
    object(&mut m, "ScheduleTypeLimits", "Temperature", json!({ "numeric_type": "Continuous", "unit_type": "Temperature" }));
    object(&mut m, "ScheduleTypeLimits", "Any Number", json!({}));
    object(&mut m, "ScheduleTypeLimits", "Control Type", json!({ "lower_limit_value": 0.0, "upper_limit_value": 4.0, "numeric_type": "Discrete" }));
    object(&mut m, "Schedule:Constant", "Always On", json!({ "schedule_type_limits_name": "Fraction", "hourly_value": 1.0 }));
    object(&mut m, "Schedule:Constant", "Dual Set Point", json!({ "schedule_type_limits_name": "Control Type", "hourly_value": 4 }));

    // Materials and constructions, from the HBJSON's (opaque: one no-mass layer).
    let energy = &hb["properties"]["energy"];
    let materials: BTreeMap<&str, &Value> = energy["materials"].as_array().into_iter().flatten().filter_map(|x| Some((x["identifier"].as_str()?, x))).collect();
    for c in energy["constructions"].as_array().into_iter().flatten() {
        let (Some(id), Some(layer)) = (c["identifier"].as_str(), c["materials"][0].as_str()) else { continue };
        let Some(mat) = materials.get(layer) else { continue };
        if mat["type"] == "EnergyWindowMaterialSimpleGlazSys" {
            let u = mat["u_factor"].as_f64().unwrap_or(1.4).clamp(0.1, 7.0);
            object(&mut m, "WindowMaterial:SimpleGlazingSystem", layer, json!({ "u_factor": u, "solar_heat_gain_coefficient": mat["shgc"].as_f64().unwrap_or(0.6).clamp(0.01, 0.99), "visible_transmittance": mat["vt"].as_f64().unwrap_or(0.7).clamp(0.01, 0.99) }));
        } else {
            object(&mut m, "Material:NoMass", layer, json!({ "roughness": "MediumRough", "thermal_resistance": mat["r_value"].as_f64().unwrap_or(1.0).max(0.001) }));
        }
        object(&mut m, "Construction", id, json!({ "outside_layer": layer }));
    }
    object(&mut m, "Construction:AirBoundary", "Air Boundary", json!({ "air_exchange_method": "SimpleMixing", "simple_mixing_air_changes_per_hour": 0.5 }));

    // Zones, surfaces and openings.
    let mut zones = Vec::new();
    for room in hb["rooms"].as_array().into_iter().flatten() {
        let Some(key) = rename.get(room["identifier"].as_str().unwrap_or_default()).cloned() else { continue };
        object(&mut m, "Zone", &key, json!({}));
        zones.push(key.clone());
        for f in room["faces"].as_array().into_iter().flatten() {
            let fname = name(&f["identifier"]);
            let bc = &f["boundary_condition"];
            let outdoors = bc["type"] == "Outdoors";
            let surface_type = match f["face_type"].as_str() {
                Some("Floor") => "Floor",
                Some("RoofCeiling") if outdoors => "Roof",
                Some("RoofCeiling") => "Ceiling",
                _ => "Wall",
            };
            let mut s = json!({
                "surface_type": surface_type,
                "zone_name": key,
                "outside_boundary_condition": bc["type"].as_str().unwrap_or("Adiabatic"),
                "sun_exposure": if outdoors { "SunExposed" } else { "NoSun" },
                "wind_exposure": if outdoors { "WindExposed" } else { "NoWind" },
                "vertices": vertices(&f["geometry"]["boundary"]),
            });
            s["construction_name"] = if f["face_type"] == "AirBoundary" { json!("Air Boundary") } else { f["properties"]["energy"]["construction"].clone() };
            if bc["type"] == "Surface" {
                s["outside_boundary_condition_object"] = json!(name(&bc["boundary_condition_objects"][0]));
            }
            if s["construction_name"].is_null() {
                s["construction_name"] = json!("Air Boundary");
            }
            object(&mut m, "BuildingSurface:Detailed", &fname, s);
            for (list, kind) in [("apertures", "Window"), ("doors", "Door")] {
                for sub in f[list].as_array().into_iter().flatten() {
                    let pts = sub["geometry"]["boundary"].as_array().cloned().unwrap_or_default();
                    if !(3..=4).contains(&pts.len()) {
                        warnings.push(format!("an opening of {fname} has {} vertices: EnergyPlus takes 3 or 4; left out", pts.len()));
                        continue;
                    }
                    let mut w = json!({ "surface_type": kind, "construction_name": sub["properties"]["energy"]["construction"], "building_surface_name": fname, "number_of_vertices": pts.len() });
                    for (i, p) in pts.iter().enumerate() {
                        for (a, c) in ["x", "y", "z"].iter().enumerate() {
                            w[format!("vertex_{}_{c}_coordinate", i + 1)] = p[a].clone();
                        }
                    }
                    let rid = room["identifier"].as_str().unwrap_or_default();
                    object(&mut m, "FenestrationSurface:Detailed", &sub["identifier"].as_str().unwrap_or_default().replacen(rid, &key, 1), w);
                }
            }
        }
        let Some(s) = spaces.get(key.as_str()) else { continue };
        // Schedules.
        let occ = format!("{key} Occupancy");
        object(&mut m, "Schedule:Compact", &occ, weekdays(s.open, 1.0, 0.0, "Fraction"));
        object(&mut m, "Schedule:Compact", &format!("{key} Lights"), weekdays(s.open, 1.0, 0.1, "Fraction"));
        object(&mut m, "Schedule:Compact", &format!("{key} Equipment"), weekdays(s.open, 1.0, 0.3, "Fraction"));
        object(&mut m, "Schedule:Compact", &format!("{key} Heating Set Point"), weekdays(s.open, s.heating_set_point, s.heating_set_point - SETBACK, "Temperature"));
        object(&mut m, "Schedule:Compact", &format!("{key} Cooling Set Point"), weekdays(s.open, s.cooling_set_point, s.cooling_set_point + SETBACK, "Temperature"));
        // Gains.
        if s.lighting > 0.0 {
            object(&mut m, "Lights", &format!("{key} Lights"), json!({ "zone_or_zonelist_or_space_or_spacelist_name": key, "schedule_name": format!("{key} Lights"), "design_level_calculation_method": "Watts/Area", "watts_per_floor_area": s.lighting, "fraction_radiant": 0.5 }));
        }
        if s.equipment > 0.0 {
            object(&mut m, "ElectricEquipment", &format!("{key} Equipment"), json!({ "zone_or_zonelist_or_space_or_spacelist_name": key, "schedule_name": format!("{key} Equipment"), "design_level_calculation_method": "Watts/Area", "watts_per_floor_area": s.equipment, "fraction_radiant": 0.5 }));
        }
        let per_person = s.sensible_per_person + s.latent_per_person;
        if s.people > 0.0 && per_person > 0.0 {
            let act = format!("{key} Activity");
            object(&mut m, "Schedule:Constant", &act, json!({ "schedule_type_limits_name": "Any Number", "hourly_value": per_person }));
            object(
                &mut m,
                "People",
                &format!("{key} People"),
                json!({ "zone_or_zonelist_or_space_or_spacelist_name": key, "number_of_people_schedule_name": occ, "number_of_people_calculation_method": "People", "number_of_people": s.people, "fraction_radiant": 0.3, "sensible_heat_fraction": (s.sensible_per_person / per_person).clamp(0.0, 1.0), "activity_level_schedule_name": act }),
            );
        }
        // Air: infiltration always, outdoor air when occupied.
        let exterior: f64 = s.surfaces.iter().filter(|x| x.class.exterior()).map(|x| x.area).sum();
        let infil = o.infiltration.rate() * exterior / 1000.0;
        if infil > 0.0 {
            object(&mut m, "ZoneInfiltration:DesignFlowRate", &format!("{key} Infiltration"), json!({ "zone_or_zonelist_or_space_or_spacelist_name": key, "schedule_name": "Always On", "design_flow_rate_calculation_method": "Flow/Zone", "design_flow_rate": infil, "constant_term_coefficient": 1.0, "temperature_term_coefficient": 0.0, "velocity_term_coefficient": 0.0, "velocity_squared_term_coefficient": 0.0 }));
        }
        if s.outdoor_air > 0.0 {
            object(&mut m, "ZoneVentilation:DesignFlowRate", &format!("{key} Outdoor Air"), json!({ "zone_or_zonelist_or_space_or_spacelist_name": key, "schedule_name": occ, "design_flow_rate_calculation_method": "Flow/Zone", "design_flow_rate": s.outdoor_air / 1000.0, "ventilation_type": "Balanced", "fan_pressure_rise": 0.0, "fan_total_efficiency": 1.0 }));
        }
        // Thermostat and ideal loads.
        object(&mut m, "ThermostatSetpoint:DualSetpoint", &format!("{key} Set Points"), json!({ "heating_setpoint_temperature_schedule_name": format!("{key} Heating Set Point"), "cooling_setpoint_temperature_schedule_name": format!("{key} Cooling Set Point") }));
        object(&mut m, "ZoneControl:Thermostat", &format!("{key} Thermostat"), json!({ "zone_or_zonelist_name": key, "control_type_schedule_name": "Dual Set Point", "control_1_object_type": "ThermostatSetpoint:DualSetpoint", "control_1_name": format!("{key} Set Points") }));
        let ideal = format!("{key}{IDEAL}");
        object(&mut m, "ZoneHVAC:IdealLoadsAirSystem", &ideal, json!({ "zone_supply_air_node_name": format!("{key} Supply Inlet") }));
        object(
            &mut m,
            "ZoneHVAC:EquipmentList",
            &format!("{key} Equipment List"),
            json!({ "load_distribution_scheme": "SequentialLoad", "equipment": [{ "zone_equipment_object_type": "ZoneHVAC:IdealLoadsAirSystem", "zone_equipment_name": ideal, "zone_equipment_cooling_sequence": 1, "zone_equipment_heating_or_no_load_sequence": 1 }] }),
        );
        object(&mut m, "ZoneHVAC:EquipmentConnections", &format!("{key} Connections"), json!({ "zone_name": key, "zone_conditioning_equipment_list_name": format!("{key} Equipment List"), "zone_air_inlet_node_or_nodelist_name": format!("{key} Supply Inlet"), "zone_air_node_name": format!("{key} Zone Air") }));
    }
    if zones.is_empty() {
        return Err(bad("energyplus.no_zones", "no space of the energy model could be written as a zone"));
    }
    // Output.
    object(&mut m, "Output:SQLite", "Output:SQLite 1", json!({ "option_type": "SimpleAndTabular" }));
    for (i, (var, freq)) in [(HEATING, "Monthly"), (COOLING, "Monthly"), (LIGHTS, "Monthly"), (EQUIPMENT, "Monthly"), (HEATING_RATE, "Hourly"), (COOLING_RATE, "Hourly")].iter().enumerate() {
        object(&mut m, "Output:Variable", &format!("Output:Variable {}", i + 1), json!({ "key_value": "*", "variable_name": var, "reporting_frequency": freq }));
    }
    object(&mut m, "Output:Table:SummaryReports", "Output:Table:SummaryReports 1", json!({ "reports": [{ "report_name": "AllSummary" }] }));
    Ok((Value::Object(m), warnings))
}

/// The results from EnergyPlus's report rows (energy in J, rates in W): monthly loads and
/// lighting and equipment by zone, peaks from the hourly rates, then delivered energy as the
/// built-in model does.
pub fn results(rows: &[Row], input: &EplusInput) -> EnergyResults {
    // EnergyPlus may write names in upper case: match zones regardless of case.
    let zone_of: BTreeMap<String, &str> = input.areas.keys().map(|k| (k.to_uppercase(), k.as_str())).collect();
    let zone = |key: &str| {
        let k = key.to_uppercase();
        let k = k.strip_suffix(&IDEAL.to_uppercase()).unwrap_or(&k);
        zone_of.get(k).copied()
    };
    let mut months = vec![EndUses::default(); 12];
    let mut spaces: BTreeMap<&str, SpaceEnergy> = input.areas.keys().map(|k| (k.as_str(), SpaceEnergy { key: k.clone(), zone: input.zones.get(k).cloned(), ..Default::default() })).collect();
    let (mut heat_at, mut cool_at): (BTreeMap<i64, f64>, BTreeMap<i64, f64>) = (BTreeMap::new(), BTreeMap::new());
    for r in rows {
        let Some(z) = zone(&r.key) else { continue };
        let se = spaces.get_mut(z).expect("zone");
        let kwh = r.value / 3.6e6;
        let mi = r.month.clamp(1, 12) as usize - 1;
        let monthly = r.frequency.eq_ignore_ascii_case("monthly");
        match r.name.as_str() {
            n if n.eq_ignore_ascii_case(HEATING) && monthly => {
                months[mi].heating_load += kwh;
                se.heating_load += kwh;
            }
            n if n.eq_ignore_ascii_case(COOLING) && monthly => {
                months[mi].cooling_load += kwh;
                se.cooling_load += kwh;
            }
            n if n.eq_ignore_ascii_case(LIGHTS) && monthly => months[mi].lighting += kwh,
            n if n.eq_ignore_ascii_case(EQUIPMENT) && monthly => months[mi].equipment += kwh,
            n if n.eq_ignore_ascii_case(HEATING_RATE) => {
                se.peak_heating = se.peak_heating.max(r.value);
                *heat_at.entry(r.time).or_default() += r.value;
            }
            n if n.eq_ignore_ascii_case(COOLING_RATE) => {
                se.peak_cooling = se.peak_cooling.max(r.value);
                *cool_at.entry(r.time).or_default() += r.value;
            }
            _ => {}
        }
    }
    let floor_area = input.areas.values().sum();
    let peak = |m: &BTreeMap<i64, f64>| m.values().copied().fold(0.0, f64::max);
    ae::finish(months, spaces.into_values().collect(), floor_area, peak(&heat_at), peak(&cool_at), &input.options)
}

/// The version identifier (`major.minor`) from `energyplus --version` output, such as
/// "EnergyPlus, Version 26.2.0-4bd7a1f26f".
pub fn version_of(text: &str) -> Option<String> {
    let v = text.split("Version").nth(1)?.trim().split(|c: char| !(c.is_ascii_digit() || c == '.')).next()?;
    let mut parts = v.split('.');
    let (a, b) = (parts.next()?.parse::<u32>().ok()?, parts.next()?.parse::<u32>().ok()?);
    Some(format!("{a}.{b}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_schedules() {
        let s = weekdays([8.0, 18.0], 1.0, 0.1, "Fraction");
        let f: Vec<String> = s["data"].as_array().unwrap().iter().map(|v| v["field"].to_string()).collect();
        assert_eq!(f, ["\"Through: 12/31\"", "\"For: Weekdays\"", "\"Until: 08:00\"", "0.1", "\"Until: 18:00\"", "1.0", "\"Until: 24:00\"", "0.1", "\"For: AllOtherDays\"", "\"Until: 24:00\"", "0.1"]);
        // Open all day, and never.
        assert_eq!(weekdays([0.0, 24.0], 1.0, 0.0, "Fraction")["data"].as_array().unwrap().len(), 7);
        assert_eq!(weekdays([18.0, 8.0], 1.0, 0.0, "Fraction")["data"].as_array().unwrap().len(), 7);
        assert_eq!(clock(7.6), "07:30");
    }

    #[test]
    fn versions() {
        assert_eq!(version_of("EnergyPlus, Version 26.2.0-4bd7a1f26f").as_deref(), Some("26.2"));
        assert_eq!(version_of("EnergyPlus, Version 9.6.0-f420c06a69\n").as_deref(), Some("9.6"));
        assert_eq!(version_of("nothing"), None);
    }

    #[test]
    fn rows_to_results() {
        let input = EplusInput { epjson: json!({}), epw: String::new(), options: EnergyOptions::default(), areas: [("s1".to_owned(), 10.0), ("s2".to_owned(), 30.0)].into(), zones: BTreeMap::new() };
        let row = |key: &str, name: &str, freq: &str, month: u8, time: i64, value: f64| Row { key: key.into(), name: name.into(), frequency: freq.into(), month, time, value };
        let rows = vec![
            row("S1 IDEAL LOADS", HEATING, "Monthly", 1, 1, 3.6e9),
            row("S2 IDEAL LOADS", HEATING, "Monthly", 1, 1, 7.2e9),
            row("s2 Ideal Loads", COOLING, "Monthly", 7, 7, 3.6e9),
            row("S1", LIGHTS, "Monthly", 1, 1, 3.6e8),
            row("S1 IDEAL LOADS", HEATING_RATE, "Hourly", 1, 100, 2000.0),
            row("S2 IDEAL LOADS", HEATING_RATE, "Hourly", 1, 100, 3000.0),
            row("S2 IDEAL LOADS", HEATING_RATE, "Hourly", 1, 101, 4000.0),
            row("OTHER", HEATING, "Monthly", 1, 1, 1e12),
        ];
        let r = results(&rows, &input);
        assert!((r.months[0].heating_load - 3000.0).abs() < 1e-9);
        assert!((r.months[6].cooling_load - 1000.0).abs() < 1e-9);
        assert!((r.year.lighting - 100.0).abs() < 1e-9);
        assert_eq!(r.peak_heating, 5000.0);
        assert_eq!(r.spaces.iter().find(|s| s.key == "s2").unwrap().peak_heating, 4000.0);
        assert_eq!(r.floor_area, 40.0);
        assert!(r.eui > 0.0);
    }
}
