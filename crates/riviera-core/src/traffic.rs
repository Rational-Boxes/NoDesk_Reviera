// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Traffic (ARC-VT-018, ARC-VT-019): the up-peak round-trip time, interval and handling capacity
//! of each group of elevators against the targets for its building's use, and the capacity of
//! escalators and walks against a demand, with one unit out of service.
//!
//! The up-peak relations are the textbook ones (research/16 I3), with every coefficient exposed:
//! N floors above the lobby, P average passengers (loading × the car's persons, the lesser of by
//! mass and by area), S = N·(1 − (1 − 1/N)^P) probable stops, H = N − Σ (i/N)^P the highest
//! reversal floor, RTT = 2·H·t_v + (S + 1)·t_s + 2·P·t_p, interval = RTT / L, HC5 = 300·P·L / RTT.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::elevators::{Confidence, Elevator, Pack, active, elevators};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::store::Store;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// What a building is used for, for its traffic targets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BuildingUse {
    #[default]
    Office,
    Residential,
    Hotel,
    Hospital,
    Clinic,
}

/// One use's targets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Target {
    #[serde(rename = "use")]
    pub building_use: BuildingUse,
    /// The longest average interval (s).
    pub interval: f64,
    /// The least handling capacity: a fraction of the population in the peak 5 minutes.
    pub handling: f64,
    /// Car floor area per passenger (m²), for persons by area.
    pub car_area_per_person: f64,
    /// Floor area per person, to estimate a population from the floors served (m²).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor_area_per_person: Option<f64>,
}

/// Traffic targets and the up-peak coefficients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Rules {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub targets: Vec<Target>,
    /// Average car load as a fraction of its persons.
    pub loading: f64,
    /// Time per stop: acceleration, levelling, doors (s).
    pub stop_time: f64,
    /// Passenger transfer time each way (s).
    pub transfer_time: f64,
    /// The handling a group needs where another serves the same floors, as a multiple.
    pub overlap: f64,
    /// Average waiting time as a fraction of the interval, low and high.
    pub waiting: [f64; 2],
    pub source: String,
    pub confidence: Confidence,
}

fn rules(packs: &[Pack]) -> Result<(&Pack, &Rules)> {
    packs
        .iter()
        .find_map(|p| p.traffic.as_ref().map(|r| (p, r)))
        .ok_or_else(|| bad("vt.no_sizing", "no rule pack in use gives traffic targets (add practice_us or practice_en)"))
}

// ---- Elevators (ARC-VT-018) ----

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrafficParams {
    /// One group of elevators; omitted, every group of passenger elevators serving the same levels
    /// at the same speed with the same car.
    #[serde(default)]
    pub elevators: Vec<ElementRef>,
    #[serde(default)]
    pub building_use: BuildingUse,
    /// The people above the lobby a group serves; omitted, the floors' area over the use's area
    /// per person (offices only).
    #[serde(default)]
    pub population: Option<f64>,
    /// Floor area per person (m²) for the estimate.
    #[serde(default)]
    pub area_per_person: Option<f64>,
    /// The coefficients, if not the pack's.
    #[serde(default)]
    pub loading: Option<f64>,
    #[serde(default)]
    pub stop_time: Option<f64>,
    #[serde(default)]
    pub transfer_time: Option<f64>,
}

/// One group's up-peak.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Group {
    pub elevators: Vec<ElementId>,
    pub cars: u32,
    /// Floors served above the lobby.
    pub floors: u32,
    pub population: f64,
    /// Whether the population was estimated from the floors' area.
    pub estimated: bool,
    pub persons_by_mass: u32,
    pub persons_by_area: u32,
    /// Average passengers per trip.
    pub passengers: f64,
    pub probable_stops: f64,
    pub highest_reversal: f64,
    /// Average floor-to-floor height (m) and the rated speed (m/s).
    pub interfloor: f64,
    pub speed: f64,
    /// Round-trip time and average interval (s).
    pub rtt: f64,
    pub interval: f64,
    /// Handling capacity: persons in 5 minutes, and as a fraction of the population.
    pub hc5: f64,
    pub handling: f64,
    /// Average waiting time, low and high estimates (s).
    pub waiting: [f64; 2],
    pub target_interval: f64,
    pub target_handling: f64,
    pub meets: bool,
    pub pack: String,
    pub rule: String,
    pub confidence: Confidence,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// One car's up-peak round trip with `persons` over `floors` floors of `interfloor` height:
/// average passengers, probable stops, highest reversal floor and the round-trip time.
pub fn up_peak(floors: u32, persons: f64, interfloor: f64, speed: f64, loading: f64, stop_time: f64, transfer_time: f64) -> (f64, f64, f64, f64) {
    let n = f64::from(floors.max(1));
    let p = loading * persons;
    let s = n * (1.0 - (1.0 - 1.0 / n).powf(p));
    let h = n - (1..floors).map(|i| (f64::from(i) / n).powf(p)).sum::<f64>();
    let rtt = 2.0 * h * interfloor / speed + (s + 1.0) * stop_time + 2.0 * p * transfer_time;
    (p, s, h, rtt)
}

/// `elevator.traffic`: each group's up-peak against the targets of the building's use.
pub(crate) fn elevator_traffic(store: &Store, p: Option<TrafficParams>) -> Result<Vec<Group>> {
    let p = p.unwrap_or_default();
    let packs = active(store);
    let (pack, r) = rules(&packs)?;
    let target = r.targets.iter().find(|t| t.building_use == p.building_use).ok_or_else(|| bad("vt.params", format!("{} gives no targets for this use", pack.id)))?;
    let all = elevators(store);
    let groups: Vec<Vec<(ElementId, Elevator)>> = if p.elevators.is_empty() {
        let mut gs: Vec<Vec<(ElementId, Elevator)>> = Vec::new();
        for (id, el) in all.iter().filter(|(_, e)| e.sized.kind.carries_people()) {
            // A group: cars serving the same levels at the same speed with the same car.
            let alike = |o: &Elevator| o.levels == el.levels && (o.sized.speed - el.sized.speed).abs() < 1e-9 && o.sized.car == el.sized.car;
            match gs.iter_mut().find(|g| alike(&g[0].1)) {
                Some(g) => g.push((*id, el.clone())),
                None => gs.push(vec![(*id, el.clone())]),
            }
        }
        gs
    } else {
        let chosen: Vec<(ElementId, Elevator)> = p
            .elevators
            .iter()
            .map(|e| {
                let id = store.resolve(e).ok_or_else(|| bad("element.not_found", "no such elevator"))?;
                all.iter().find(|(c, _)| *c == id).cloned().ok_or_else(|| bad("vt.not_elevator", format!("element {id} is not an elevator's car")))
            })
            .collect::<Result<_>>()?;
        vec![chosen]
    };
    let levels = crate::levels::list(store);
    let elevation = |g: &String| levels.iter().find(|l| &l.guid == g).map(|l| l.elevation);
    let floors_area: Vec<(String, f64)> = crate::floors::all(store)
        .into_iter()
        .filter_map(|(id, g)| Some((crate::floors::floor_recipe(store.get(id)?)?.level, g.area())))
        .collect();
    let loading = p.loading.unwrap_or(r.loading);
    let (ts, tp) = (p.stop_time.unwrap_or(r.stop_time), p.transfer_time.unwrap_or(r.transfer_time));
    let mut out = Vec::new();
    for g in &groups {
        let Some((_, first)) = g.first() else { continue };
        let mut notes = Vec::new();
        // The levels every car serves; the lowest is the lobby.
        let served: Vec<&String> = first.levels.iter().filter(|l| g.iter().all(|(_, e)| e.levels.contains(l))).collect();
        let floors = served.len().saturating_sub(1) as u32;
        if floors == 0 {
            continue;
        }
        let (Some(z0), Some(z1)) = (served.first().and_then(|l| elevation(l)), served.last().and_then(|l| elevation(l))) else { continue };
        let interfloor = (z1 - z0) / f64::from(floors);
        let speed = g.iter().map(|(_, e)| e.sized.speed).fold(f64::INFINITY, f64::min);
        let by_mass = g.iter().map(|(_, e)| e.sized.persons).min().unwrap_or(0);
        let by_area = g.iter().map(|(_, e)| (e.sized.car[0] * e.sized.car[1] / target.car_area_per_person).floor() as u32).min().unwrap_or(0);
        let persons = by_mass.min(by_area);
        if by_area < by_mass {
            notes.push(format!("by area the cars take {by_area} passengers, fewer than the {by_mass} their load allows"));
        }
        let cars = g.len() as u32;
        let (passengers, stops, reversal, rtt) = up_peak(floors, f64::from(persons), interfloor, speed, loading, ts, tp);
        let interval = rtt / f64::from(cars);
        let hc5 = 300.0 * passengers * f64::from(cars) / rtt;
        let (population, estimated) = match p.population {
            Some(n) => (n, false),
            None => {
                let per = p.area_per_person.or(target.floor_area_per_person).ok_or_else(|| bad("vt.params", "give the population the group serves (an estimate from floor area is offered for offices only)"))?;
                let area: f64 = floors_area.iter().filter(|(l, _)| served[1..].contains(&l)).map(|(_, a)| a).sum();
                if area <= 0.0 {
                    return Err(bad("vt.params", "no floors on the levels served above the lobby to estimate a population from; give the population"));
                }
                notes.push(format!("population estimated from {area:.0} m² of floor at {per:.0} m² a person (gross area: net area gives fewer)"));
                (area / per, true)
            }
        };
        // Floors another group also serves: migration between groups.
        let shared = groups.iter().any(|o| !std::ptr::eq(o, g) && o.first().is_some_and(|(_, e)| served[1..].iter().any(|l| e.levels.contains(l))));
        let target_handling = if shared {
            notes.push(format!("another group serves some of these floors: {:.0} % of the target allows for migration", r.overlap * 100.0));
            target.handling * r.overlap
        } else {
            target.handling
        };
        let handling = hc5 / population;
        out.push(Group {
            elevators: g.iter().map(|(id, _)| *id).collect(),
            cars,
            floors,
            population,
            estimated,
            persons_by_mass: by_mass,
            persons_by_area: by_area,
            passengers,
            probable_stops: stops,
            highest_reversal: reversal,
            interfloor,
            speed,
            rtt,
            interval,
            hc5,
            handling,
            waiting: [interval * r.waiting[0], interval * r.waiting[1]],
            target_interval: target.interval,
            target_handling,
            meets: interval <= target.interval + 1e-9 && handling + 1e-9 >= target_handling,
            pack: pack.id.clone(),
            rule: r.id.clone(),
            confidence: r.confidence,
            notes,
        });
    }
    Ok(out)
}

// ---- Escalators and walks (ARC-VT-019) ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EscalatorTrafficParams {
    /// The units carrying people up, and down (escalators or walks).
    #[serde(default)]
    pub up: Vec<ElementRef>,
    #[serde(default)]
    pub down: Vec<ElementRef>,
    /// The peak demand each way (persons an hour).
    pub demand_up: f64,
    #[serde(default)]
    pub demand_down: f64,
    /// Check with the largest unit each way out of service (default true).
    #[serde(default)]
    pub one_out: Option<bool>,
    /// What the escalators may count toward the egress capacity (ARC-VT-021).
    #[serde(default)]
    pub egress: Option<EgressParams>,
}

/// Whose rule decides what escalators count in egress.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EgressMode {
    /// IBC 1003.7: escalators are never part of the means of egress.
    #[default]
    Ibc,
    /// NFPA 130 5.3.5.4: at most half the required capacity at any level.
    Nfpa130,
    /// SCDF (rapid transit): at most half, the worst unit out, the tread width counted.
    Scdf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Way {
    #[default]
    Up,
    Down,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EgressParams {
    #[serde(default)]
    pub mode: EgressMode,
    /// The way out: the units running that way count (default up, out of a station).
    #[serde(default)]
    pub exit: Way,
    /// The egress capacity the level needs, and its stairs' (persons a minute).
    pub required: f64,
    pub stairs: f64,
}

/// What the escalators add to a level's egress capacity.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Egress {
    pub mode: EgressMode,
    /// Persons a minute: the units running the way out at 120 a minute per metre of tread, the
    /// worst unit out; what of it counts; the stairs; and the whole.
    pub escalators: f64,
    pub counted: f64,
    pub stairs: f64,
    pub total: f64,
    pub required: f64,
    pub meets: bool,
    pub rule: String,
}

/// Persons a minute per metre of a working escalator's tread in an evacuation (TfL).
pub const EVACUATION_RATE: f64 = 120.0;
/// A transit unit's planning flow, persons a minute.
pub const UNIT_FLOW: f64 = 100.0;

/// One direction's capacity against its demand.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Direction {
    pub units: usize,
    /// Persons an hour: all units, and with the largest out.
    pub capacity: f64,
    pub capacity_one_out: f64,
    pub demand: f64,
    /// Demand over the capacity counted (one out where asked).
    pub utilisation: f64,
    pub meets: bool,
    /// The units a station needs this way at 100 persons a minute each, one more for the unit
    /// out where asked.
    pub units_needed: u32,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct EscalatorTraffic {
    pub up: Direction,
    pub down: Direction,
    pub one_out: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub egress: Option<Egress>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// `escalator.traffic`: each direction's practical capacity against its demand.
pub(crate) fn escalator_traffic(store: &Store, p: EscalatorTrafficParams) -> Result<EscalatorTraffic> {
    let one_out = p.one_out.unwrap_or(true);
    let mut notes = Vec::new();
    let all = crate::escalators::escalators(store);
    let caps = |refs: &[ElementRef]| -> Result<Vec<f64>> {
        refs.iter()
            .map(|r| {
                let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such escalator"))?;
                all.iter().find(|(e, _)| *e == id).map(|(_, e)| e.sized.capacity).ok_or_else(|| bad("vt.not_escalator", format!("element {id} is not an escalator or walk")))
            })
            .collect()
    };
    let mut direction = |name: &str, refs: &[ElementRef], demand: f64| -> Result<Direction> {
        let c = caps(refs)?;
        let capacity: f64 = c.iter().sum();
        let largest = c.iter().copied().fold(0.0, f64::max);
        let capacity_one_out = capacity - largest;
        if one_out && demand > 0.0 && c.len() == 1 {
            notes.push(format!("with its only {name} unit out, the {name} demand falls to the stairs"));
        }
        let counted = if one_out { capacity_one_out } else { capacity };
        let utilisation = if counted > 0.0 { demand / counted } else if demand > 0.0 { f64::INFINITY } else { 0.0 };
        let units_needed = (demand / 60.0 / UNIT_FLOW).ceil() as u32 + u32::from(one_out && demand > 0.0);
        Ok(Direction { units: c.len(), capacity, capacity_one_out, demand, utilisation, meets: demand <= counted + 1e-9, units_needed })
    };
    let up = direction("up", &p.up, p.demand_up)?;
    let down = direction("down", &p.down, p.demand_down)?;
    let egress = match &p.egress {
        None => None,
        Some(e) => {
            let refs = match e.exit {
                Way::Up => &p.up,
                Way::Down => &p.down,
            };
            let widths: Vec<f64> = refs
                .iter()
                .map(|r| {
                    let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such escalator"))?;
                    all.iter().find(|(x, _)| *x == id).map(|(_, x)| x.sized.step_width).ok_or_else(|| bad("vt.not_escalator", format!("element {id} is not an escalator or walk")))
                })
                .collect::<Result<_>>()?;
            let rates: Vec<f64> = widths.iter().map(|w| w * EVACUATION_RATE).collect();
            let working: f64 = rates.iter().sum::<f64>() - rates.iter().copied().fold(0.0, f64::max);
            let (counted, rule) = match e.mode {
                EgressMode::Ibc => (0.0, "IBC 1003.7: escalators are not a means of egress".to_owned()),
                EgressMode::Nfpa130 => (working.min(e.required / 2.0), "NFPA 130 5.3.5.4: at most half the required capacity".to_owned()),
                EgressMode::Scdf => (working.min(e.required / 2.0), "SCDF rapid transit 2.2.3: at most half, one unit out, tread width".to_owned()),
            };
            let total = e.stairs + counted;
            Some(Egress { mode: e.mode, escalators: working, counted, stairs: e.stairs, total, required: e.required, meets: total + 1e-9 >= e.required, rule })
        }
    };
    Ok(EscalatorTraffic { up, down, one_out, egress, notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_up_peak_of_a_textbook_group() {
        // Four 21-person cars over 10 floors of 3.5 m at 2.5 m/s, 80 % loading, 10 s a stop and
        // 1.2 s a passenger each way.
        let (p, s, h, rtt) = up_peak(10, 21.0, 3.5, 2.5, 0.8, 10.0, 1.2);
        assert!((p - 16.8).abs() < 1e-9);
        assert!((s - 10.0 * (1.0 - 0.9f64.powf(16.8))).abs() < 1e-9);
        let hh = 10.0 - (1..10).map(|i| (f64::from(i) / 10.0).powf(16.8)).sum::<f64>();
        assert!((h - hh).abs() < 1e-9);
        assert!((rtt - (2.0 * hh * 1.4 + (s + 1.0) * 10.0 + 2.0 * 16.8 * 1.2)).abs() < 1e-9);
        // About 9.6 of 10 floors reversed and 8.3 stops: about 160 s, a 40 s interval.
        assert!(rtt > 150.0 && rtt < 170.0, "{rtt}");
    }
}
