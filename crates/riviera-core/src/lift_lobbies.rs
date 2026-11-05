// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Lobbies of occupant evacuation elevators (ANA-ROUTE-007): on each floor an evacuation elevator
//! (ANA-ROUTE-006) serves, people wait for it in an enclosed lobby (IBC 3008.6). The lobby is the
//! room in front of its landing; it holds a quarter of the floor's occupant load at 3 ft² (0.28 m²)
//! a person and one 30 × 48 in (0.76 × 1.22 m) wheelchair space for each 50 of the load or part of
//! 50 (IBC 3008.6.3), and gives direct access to an exit stair (IBC 3008.6.2).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::elevators::EgressRole;
use crate::error::{Error, Result};
use crate::id::ElementId;
use crate::store::Store;

type V2 = [f64; 2];

/// Square metres a waiting person (3 ft²), a wheelchair space's area (30 × 48 in), the share of
/// the floor's load the lobby holds, and the persons a wheelchair space is counted for.
pub const AREA_PER_PERSON: f64 = 0.2787;
pub const WHEELCHAIR_SPACE: f64 = 0.762 * 1.219;
pub const SHARE: f64 = 0.25;
pub const PER_WHEELCHAIR: f64 = 50.0;
/// The floor area per occupant when no load is given (150 ft² gross, IBC Table 1004.5 business).
pub const DEFAULT_LOAD_FACTOR: f64 = 13.94;
/// How near a stair's end on the floor must be to the lobby to count as direct access (m).
pub const STAIR_REACH: f64 = 3.0;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckParams {
    /// The occupant load of each floor served (persons); omitted, the floors on the level at
    /// `load_factor`.
    #[serde(default)]
    pub occupants: Option<f64>,
    /// Square metres of floor an occupant (default 13.94: 150 ft² gross).
    #[serde(default)]
    pub load_factor: Option<f64>,
}

/// An evacuation elevator's lobby on one floor.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Lobby {
    pub elevator: ElementId,
    pub level: ElementId,
    /// The room in front of the landing, if any, and its area (m²).
    pub room: Option<ElementId>,
    pub area: f64,
    /// The floor's occupant load, the persons the lobby holds (a quarter), the wheelchair spaces,
    /// and the area they need (m²).
    pub occupants: f64,
    pub waiting: f64,
    pub wheelchair_spaces: u32,
    pub required: f64,
    /// The nearest stair end on the floor, its distance from the lobby (m), and whether that is
    /// direct access.
    pub stair: Option<ElementId>,
    pub stair_distance: Option<f64>,
    pub stair_access: bool,
    /// What is wrong, if anything.
    pub problems: Vec<String>,
}

fn dist_to_ring(p: V2, ring: &[V2]) -> f64 {
    (0..ring.len())
        .map(|i| {
            let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let l2 = dx * dx + dy * dy;
            let t = if l2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
            (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dy)
        })
        .fold(f64::INFINITY, f64::min)
}

/// The ends of every stair on a level: the foot of those rising from it, the head of those
/// arriving at it.
fn stair_ends(store: &Store, guid: &str) -> Vec<(ElementId, V2)> {
    let mut out = Vec::new();
    for (id, _) in crate::stairs::all(store) {
        let Some(e) = store.get(id) else { continue };
        let Some(r) = crate::stairs::stair_recipe(e) else { continue };
        let crate::stairs::StairTop::Level(top) = &r.top else { continue };
        let Ok(g) = crate::stairs::regenerate(store, e) else { continue };
        let r = r.extended(g.risers);
        let (Some(first), Some(last)) = (r.runs.first(), r.runs.last()) else { continue };
        if r.base_level == guid {
            out.push((id, first.start));
        }
        if top == guid {
            out.push((id, last.end(r.tread_depth)));
        }
    }
    out
}

/// `elevator.check_lobbies`: each occupant evacuation elevator's lobby on each floor it serves.
pub fn check(store: &Store, p: Option<CheckParams>) -> Result<Vec<Lobby>> {
    let p = p.unwrap_or_default();
    if p.occupants.is_some_and(|v| !(v.is_finite() && v >= 0.0)) {
        return Err(bad("vt.invalid", "the occupant load must be 0 or more"));
    }
    let factor = p.load_factor.unwrap_or(DEFAULT_LOAD_FACTOR);
    if !(factor.is_finite() && factor > 0.0) {
        return Err(bad("vt.invalid", "the load factor must be above 0"));
    }
    let levels = crate::levels::list(store);
    let rooms = crate::rooms::all(store);
    let floors = crate::floors::all(store);
    let mut out = Vec::new();
    for (lift, el) in crate::elevators::elevators(store) {
        if el.egress_role != EgressRole::OccupantEvacuation {
            continue;
        }
        let landing = el.at(el.door_x(), -(el.wall_thickness + 0.6));
        for guid in &el.levels {
            let Some(level) = levels.iter().find(|l| &l.guid == guid) else { continue };
            let mut problems = Vec::new();
            let room = rooms.iter().filter(|r| !r.redundant && &r.level == guid).find(|r| r.geometry.region.as_ref().is_some_and(|g| g.contains(landing)));
            let area = room.and_then(|r| r.geometry.region.as_ref()).map_or(0.0, |g| g.area());
            let occupants = match p.occupants {
                Some(v) => v,
                None => {
                    let a: f64 = floors.iter().filter(|(id, _)| store.get(*id).and_then(crate::floors::floor_recipe).is_some_and(|r| &r.level == guid)).map(|(_, g)| g.area()).sum();
                    (a / factor).ceil()
                }
            };
            let waiting = (occupants * SHARE).ceil();
            let wheelchair_spaces = (occupants / PER_WHEELCHAIR).ceil() as u32;
            let required = waiting * AREA_PER_PERSON + f64::from(wheelchair_spaces) * WHEELCHAIR_SPACE;
            // Direct access: a stair's end on this floor in the lobby or just beyond its walls.
            let near = room.and_then(|r| r.geometry.region.as_ref()).and_then(|g| {
                let ring = g.outer_ring();
                stair_ends(store, guid).into_iter().map(|(s, q)| (s, if g.contains(q) { 0.0 } else { dist_to_ring(q, &ring) })).min_by(|a, b| a.1.total_cmp(&b.1))
            });
            let stair_access = near.is_some_and(|(_, d)| d <= STAIR_REACH);
            match room {
                None => problems.push(format!("no enclosed lobby on {}: its landing is in no room (IBC 3008.6)", level.name)),
                Some(_) => {
                    if area + 1e-9 < required {
                        problems.push(format!(
                            "the lobby on {} is {area:.1} m²; {waiting:.0} waiting (a quarter of {occupants:.0}) at 0.28 m² and {wheelchair_spaces} wheelchair space(s) need {required:.1} m² (IBC 3008.6.3)",
                            level.name
                        ));
                    }
                    if !stair_access {
                        problems.push(format!("the lobby on {} has no exit stair within {STAIR_REACH} m: it needs direct access to one (IBC 3008.6.2)", level.name));
                    }
                }
            }
            out.push(Lobby {
                elevator: lift,
                level: level.id,
                room: room.map(|r| r.id),
                area,
                occupants,
                waiting,
                wheelchair_spaces,
                required,
                stair: near.map(|(s, _)| s),
                stair_distance: near.map(|(_, d)| d),
                stair_access,
                problems,
            });
        }
    }
    Ok(out)
}
