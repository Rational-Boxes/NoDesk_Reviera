// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Egress routes across levels (ANA-ROUTE-004): the quickest way from a point on one level to a
//! point on another, walking each level round its obstacles (ANA-ROUTE-001's engine) and taking
//! stairs between levels; escalators too where the egress mode counts them (ARC-VT-021).
//!
//! A stair links the point 0.4 m before its first riser on its base level with the point 0.4 m
//! beyond its last riser on its top level, its length along the middle of its flights and
//! landings, at the stair speed. An escalator, where allowed, links the points 0.6 m beyond each
//! end of its truss, walked as a stair.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::store::Store;
use crate::traffic::EgressMode;

type V2 = [f64; 2];

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// A point on a level.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct At {
    pub point: V2,
    pub level: ElementRef,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RouteParams {
    pub start: At,
    pub end: At,
    /// What may be used between levels: stairs always; escalators under `nfpa130` and `scdf`
    /// (default `ibc`: stairs only).
    #[serde(default)]
    pub mode: EgressMode,
    /// Take occupant evacuation elevators too (default false; ANA-ROUTE-006).
    #[serde(default)]
    pub lifts: bool,
    /// The wait for a lift's car, seconds (default 60).
    #[serde(default)]
    pub lift_wait: Option<f64>,
}

/// A lift's default wait for the car (s), its door time at the two stops together (s), and the
/// acceleration and deceleration it travels with (m/s²).
pub const LIFT_WAIT: f64 = 60.0;
pub const LIFT_DOORS: f64 = 10.0;
pub const LIFT_ACCELERATION: f64 = 1.0;

/// A lift's time between two of its levels `rise` apart: the wait, the doors, and the ride at
/// its rated speed with the time lost speeding up and slowing down.
pub fn lift_time(rise: f64, speed: f64, wait: f64) -> f64 {
    wait + LIFT_DOORS + rise.abs() / speed + speed / LIFT_ACCELERATION
}

/// A stretch walked on one level.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Leg {
    pub level: ElementId,
    pub points: Vec<V2>,
    pub length: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    Stair,
    Escalator,
    Lift,
}

/// A stair or escalator taken between levels.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Link {
    pub kind: LinkKind,
    pub element: ElementId,
    pub from: ElementId,
    pub to: ElementId,
    /// Along its flights (m; a lift's rise), and the time taken (s; a lift's with its wait).
    pub length: f64,
    pub time: f64,
}

/// The route: legs on levels and links between them, in order.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Route {
    pub legs: Vec<Leg>,
    pub links: Vec<Link>,
    /// Walked and climbed (m), and the time (s).
    pub length: f64,
    pub time: f64,
}

struct Node {
    level: ElementId,
    p: V2,
}

struct Edge {
    a: usize,
    b: usize,
    kind: LinkKind,
    element: ElementId,
    length: f64,
    time: f64,
}

fn level_id(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&crate::id::GlobalId::parse(guid)?)
}

fn polyline(p: &[[f64; 3]]) -> f64 {
    p.windows(2).map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2) + (w[1][2] - w[0][2]).powi(2)).sqrt()).sum()
}

/// The links between levels: stairs, escalators where allowed, and occupant evacuation lifts if
/// asked.
fn links(store: &Store, p: &RouteParams, stair_speed: f64, nodes: &mut Vec<Node>) -> Vec<Edge> {
    let mode = p.mode;
    let mut out = Vec::new();
    for (id, _) in crate::stairs::all(store) {
        let Some(e) = store.get(id) else { continue };
        let Some(r) = crate::stairs::stair_recipe(e) else { continue };
        let crate::stairs::StairTop::Level(top_guid) = &r.top else { continue };
        let (Some(base), Some(top)) = (level_id(store, &r.base_level), level_id(store, top_guid)) else { continue };
        let Ok(g) = crate::stairs::regenerate(store, e) else { continue };
        let Ok((z_base, _)) = crate::stairs::elevations(store, e) else { continue };
        let r = r.extended(g.risers);
        let (Some(first), Some(last)) = (r.runs.first(), r.runs.last()) else { continue };
        // Along the middle of the flights and landings.
        let middle = crate::stairs::side_path(&r, z_base, g.riser_height, true, first.width / 2.0);
        let d0 = first.direction;
        let d1 = last.end_direction(r.tread_depth);
        let e1 = last.end(r.tread_depth);
        let a = nodes.len();
        nodes.push(Node { level: base, p: [first.start[0] - d0[0] * 0.4, first.start[1] - d0[1] * 0.4] });
        nodes.push(Node { level: top, p: [e1[0] + d1[0] * 0.4, e1[1] + d1[1] * 0.4] });
        let length = polyline(&middle) + 0.8;
        out.push(Edge { a, b: a + 1, kind: LinkKind::Stair, element: id, length, time: length / stair_speed });
    }
    if mode != EgressMode::Ibc {
        for (id, el) in crate::escalators::escalators(store) {
            let (Some(lo), Some(hi)) = (level_id(store, &el.levels[0]), level_id(store, &el.levels[1])) else { continue };
            if lo == hi {
                continue;
            }
            let s = &el.sized;
            let a = nodes.len();
            nodes.push(Node { level: lo, p: el.at(-0.6, 0.0) });
            nodes.push(Node { level: hi, p: el.at(s.length() + 0.6, 0.0) });
            let length = s.lower_end + s.run.hypot(s.rise) + s.upper_end + 1.2;
            out.push(Edge { a, b: a + 1, kind: LinkKind::Escalator, element: id, length, time: length / stair_speed });
        }
    }
    if p.lifts {
        let wait = p.lift_wait.unwrap_or(LIFT_WAIT);
        let levels = crate::levels::list(store);
        for (id, el) in crate::elevators::elevators(store) {
            if el.egress_role != crate::elevators::EgressRole::OccupantEvacuation {
                continue;
            }
            // In front of the entrance on each level served.
            let landing = el.at(el.door_x(), -(el.wall_thickness + 0.6));
            let stops: Vec<(usize, f64)> = el
                .levels
                .iter()
                .filter_map(|g| levels.iter().find(|l| l.guid == *g))
                .map(|l| {
                    nodes.push(Node { level: l.id, p: landing });
                    (nodes.len() - 1, l.elevation)
                })
                .collect();
            for (i, &(a, za)) in stops.iter().enumerate() {
                for &(b, zb) in &stops[i + 1..] {
                    out.push(Edge { a, b, kind: LinkKind::Lift, element: id, length: (zb - za).abs(), time: lift_time(zb - za, el.sized.speed, wait) });
                }
            }
        }
    }
    out
}

/// `egress.route`: the quickest route between two points on (possibly) different levels.
pub fn route(store: &Store, p: RouteParams) -> Result<Route> {
    let s = crate::path_of_travel::settings(store);
    let level = |r: &ElementRef| -> Result<ElementId> {
        let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such level"))?;
        crate::levels::list(store).iter().any(|l| l.id == id).then_some(id).ok_or_else(|| bad("route.not_a_level", format!("element {id} is not a level")))
    };
    if p.lift_wait.is_some_and(|w| !(w.is_finite() && w >= 0.0)) {
        return Err(bad("route.invalid", "the lift wait must be 0 s or more"));
    }
    let (l0, l1) = (level(&p.start.level)?, level(&p.end.level)?);
    let mut nodes = vec![Node { level: l0, p: p.start.point }, Node { level: l1, p: p.end.point }];
    let edges = links(store, &p, s.stair_speed, &mut nodes);
    // Each level's obstacles, once.
    let mut obstacles: std::collections::HashMap<ElementId, Vec<crate::path_of_travel::Poly>> = std::collections::HashMap::new();
    for n in &nodes {
        obstacles.entry(n.level).or_insert_with(|| crate::path_of_travel::obstacles(store, n.level, &s));
    }
    // Walks between the nodes of each level, worked out as needed.
    let mut walks: std::collections::HashMap<(usize, usize), Option<Vec<V2>>> = std::collections::HashMap::new();
    let mut walk = |a: usize, b: usize| -> Option<Vec<V2>> {
        let key = (a.min(b), a.max(b));
        let w = walks.entry(key).or_insert_with(|| crate::path_of_travel::shortest(nodes[key.0].p, nodes[key.1].p, &obstacles[&nodes[key.0].level])).clone();
        w.map(|mut pts| {
            if a > b {
                pts.reverse();
            }
            pts
        })
    };
    let len = |pts: &[V2]| pts.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum::<f64>();
    // Dijkstra over the nodes, by time.
    let n = nodes.len();
    let mut best = vec![f64::INFINITY; n];
    let mut prev: Vec<Option<(usize, Option<usize>)>> = vec![None; n];
    let mut done = vec![false; n];
    best[0] = 0.0;
    while let Some(u) = (0..n).filter(|&i| !done[i] && best[i].is_finite()).min_by(|&a, &b| best[a].total_cmp(&best[b])) {
        if u == 1 {
            break;
        }
        done[u] = true;
        for v in 0..n {
            if done[v] || v == u || nodes[v].level != nodes[u].level {
                continue;
            }
            if let Some(pts) = walk(u, v) {
                let t = best[u] + len(&pts) / s.walking_speed;
                if t < best[v] {
                    best[v] = t;
                    prev[v] = Some((u, None));
                }
            }
        }
        for (k, e) in edges.iter().enumerate() {
            let v = match (e.a == u, e.b == u) {
                (true, _) => e.b,
                (_, true) => e.a,
                _ => continue,
            };
            let t = best[u] + e.time;
            if !done[v] && t < best[v] {
                best[v] = t;
                prev[v] = Some((u, Some(k)));
            }
        }
    }
    if !best[1].is_finite() {
        return Err(bad("route.no_route", "no route between the points: a point is inside an obstacle, or no stair links their levels"));
    }
    // Back from the end.
    let mut steps = Vec::new();
    let mut at = 1;
    while let Some((from, via)) = prev[at] {
        steps.push((from, at, via));
        at = from;
    }
    steps.reverse();
    // Walks on one level in a row make one leg; a link ends it.
    let mut legs: Vec<Leg> = Vec::new();
    let mut out_links = Vec::new();
    let mut current: Option<Leg> = None;
    for (a, b, via) in steps {
        match via {
            Some(k) => {
                if let Some(g) = current.take() {
                    legs.push(g);
                }
                let e = &edges[k];
                out_links.push(Link { kind: e.kind, element: e.element, from: nodes[a].level, to: nodes[b].level, length: e.length, time: e.time });
            }
            None => {
                let pts = walk(a, b).expect("walked");
                let l = len(&pts);
                match current.as_mut() {
                    Some(g) if g.level == nodes[a].level => {
                        g.points.extend(pts.into_iter().skip(1));
                        g.length += l;
                    }
                    _ => {
                        if let Some(g) = current.take() {
                            legs.push(g);
                        }
                        current = Some(Leg { level: nodes[a].level, points: pts, length: l });
                    }
                }
            }
        }
    }
    if let Some(g) = current {
        legs.push(g);
    }
    let length = legs.iter().map(|g| g.length).sum::<f64>() + out_links.iter().map(|l| l.length).sum::<f64>();
    Ok(Route { legs, links: out_links, length, time: best[1] })
}

/// What `egress.draw_route` drew.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Drawn {
    #[serde(flatten)]
    pub route: Route,
    /// A path of travel for each leg, in a plan of its level (none where the level has no plan).
    pub paths: Vec<ElementId>,
    /// Levels the route crosses that have no floor plan to draw it in.
    pub unplanned: Vec<ElementId>,
}

/// `egress.draw_route`: the route, each leg drawn as a path of travel (ANA-ROUTE-001) in the first
/// floor plan of its level, with its length and time.
pub(crate) fn draw(txn: &mut crate::txn::Transaction<'_>, p: RouteParams) -> Result<Drawn> {
    let store = txn.store().clone();
    let r = route(&store, p)?;
    let (mut paths, mut unplanned) = (Vec::new(), Vec::new());
    for leg in &r.legs {
        let Some(level_guid) = store.get(leg.level).map(|e| e.guid().to_string()) else { continue };
        let mut plans: Vec<(String, ElementId)> = store
            .iter()
            .filter_map(|e| crate::views::view_data(e).filter(|d| d.kind == crate::plans::KIND_PLAN && d.level.as_deref() == Some(level_guid.as_str())).map(|_| (e.name.clone().unwrap_or_default(), e.id())))
            .collect();
        plans.sort();
        let Some((_, view)) = plans.first() else {
            unplanned.push(leg.level);
            continue;
        };
        let (Some(&start), Some(&end)) = (leg.points.first(), leg.points.last()) else { continue };
        let waypoints = leg.points[1..leg.points.len().saturating_sub(1)].to_vec();
        let id = crate::path_of_travel::create(txn, crate::path_of_travel::CreateParams { view_id: ElementRef::Id(*view), start, end, waypoints })?;
        paths.push(id);
    }
    Ok(Drawn { route: r, paths, unplanned })
}

// ---- Platform clearing (ANA-ROUTE-005) ----

/// NFPA 130 (2020) 5.5.6.3: stair capacity, persons a minute per millimetre of width.
pub const STAIR_CAPACITY: f64 = 0.0555;
/// NFPA 130 walking speed on platforms, corridors and ramps (m/min).
pub const WALK_SPEED: f64 = 37.7;
/// Vertical speed on stairs and stopped escalators (m/min), up and down.
pub const STAIR_UP_SPEED: f64 = 15.0;
pub const STAIR_DOWN_SPEED: f64 = 18.0;
/// The platform cleared within 4 minutes; the most remote point to a point of safety within 6.
pub const PLATFORM_LIMIT: f64 = 4.0;
pub const SAFETY_LIMIT: f64 = 6.0;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClearingParams {
    /// The platform's level.
    pub level: ElementRef,
    /// Its occupant load (persons), given; or `occupancy` to work it out.
    #[serde(default)]
    pub load: Option<f64>,
    #[serde(default)]
    pub occupancy: Option<Occupancy>,
    /// The most remote point on the platform.
    pub remote: V2,
    /// A point of safety (on any level).
    pub safety: At,
    /// Whether escalators count among the exits (default `nfpa130`; `ibc`: stairs only).
    #[serde(default = "nfpa130")]
    pub mode: EgressMode,
    /// Count occupant evacuation elevators serving the platform and the point of safety's level
    /// in the exit capacity (NFPA 130 5.5.6.3.3; default false: some adopting codes delete it).
    #[serde(default)]
    pub lifts: bool,
    /// Those already on the concourse who leave through the same fare lines (persons; default 0).
    #[serde(default)]
    pub concourse_load: f64,
}

/// A fare line on the way out, and its wait.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FareCrossing {
    pub fare_line: ElementId,
    /// Persons a minute; minutes for the platform and concourse loads to pass; and the wait (the
    /// flow time less the platform time, not below 0).
    pub capacity: f64,
    pub flow_time: f64,
    pub wait: f64,
}

/// Seconds a passenger takes to board, and again to leave, a lift's car.
pub const LIFT_PASSENGER_TIME: f64 = 1.2;

/// A lift's evacuation capacity, persons a minute: its rated persons each round trip between
/// levels `rise` apart (the run both ways, the doors, everyone boarding and leaving), which
/// over 30 minutes is NFPA 130's carrying capacity.
pub fn lift_capacity(persons: f64, rise: f64, speed: f64) -> f64 {
    let round_trip = 2.0 * (rise.abs() / speed + speed / LIFT_ACCELERATION) + LIFT_DOORS + 2.0 * persons * LIFT_PASSENGER_TIME;
    persons * 60.0 / round_trip
}

/// What the occupant load is worked out from (NFPA 130 5.5.5.1): the greater of the trains
/// entering at once on every track with those waiting to board, and the area at a density.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Occupancy {
    /// Persons on each train entering the station at the same time, one per track in the normal
    /// direction of traffic.
    pub trains: Vec<f64>,
    /// Those waiting to board them at the same time.
    #[serde(default)]
    pub entraining: f64,
    /// Square metres an occupant for the area figure: 0.65 (7 ft²) for stations serving event
    /// venues or dense neighbourhoods, 1.39 (15 ft²) for outlying ones; omitted, no area figure.
    #[serde(default)]
    pub area_per_person: Option<f64>,
    /// The area counted (m²); omitted, the floors on the platform's level, or with `train` the
    /// platform's standing area.
    #[serde(default)]
    pub area: Option<f64>,
    /// The train's length along the platform, from one end to the other: the area counted is
    /// then the standing area (SCDF CPFPRTS 2.2.2c), the floors on the level within the train's
    /// length less the stairs, escalators and lift shafts and a 6 m run-off zone in front of
    /// each stair and escalator end on the platform.
    #[serde(default)]
    pub train: Option<[V2; 2]>,
}

/// The zone kept clear in front of a stair or escalator end on a platform, not counted as
/// standing area (m).
pub const STANDING_RUN_OFF: f64 = 6.0;

/// A rectangle from `a` along unit `d` for `len`, `w` wide (counter-clockwise).
fn strip(a: V2, d: V2, len: f64, w: f64) -> Vec<V2> {
    let n = [-d[1] * w / 2.0, d[0] * w / 2.0];
    let b = [a[0] + d[0] * len, a[1] + d[1] * len];
    vec![[a[0] - n[0], a[1] - n[1]], [b[0] - n[0], b[1] - n[1]], [b[0] + n[0], b[1] + n[1]], [a[0] + n[0], a[1] + n[1]]]
}

fn ccw(mut r: Vec<V2>) -> Vec<V2> {
    if crate::polygons::area(&r) < 0.0 {
        r.reverse();
    }
    r
}

/// The platform's standing area (SCDF CPFPRTS 2.2.2c): the floors on `level` within the train's
/// length, less what stands on the platform and the run-off zones in front of the ways off it.
pub fn standing_area(store: &Store, level: &crate::levels::LevelInfo, train: [V2; 2]) -> f64 {
    use crate::polygons::{Shape, difference, intersection};
    let floors: Vec<Shape> = crate::floors::all(store)
        .into_iter()
        .filter(|(id, _)| store.get(*id).and_then(crate::floors::floor_recipe).is_some_and(|r| r.level == level.guid))
        .flat_map(|(_, g)| g.profiles.into_iter().map(|p| Shape { outer: ccw(p.outer), holes: p.holes }))
        .collect();
    let [a, b] = train;
    let len = (b[0] - a[0]).hypot(b[1] - a[1]);
    if len < 1e-9 {
        return 0.0;
    }
    let d = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
    let band = Shape { outer: strip(a, d, len, 2000.0), holes: Vec::new() };
    let within = intersection(&floors, &[band]);
    let mut out: Vec<Vec<V2>> = Vec::new();
    for (id, _) in crate::stairs::all(store) {
        let Some(e) = store.get(id) else { continue };
        let Some(r) = crate::stairs::stair_recipe(e) else { continue };
        let crate::stairs::StairTop::Level(top) = &r.top else { continue };
        let (base_here, top_here) = (r.base_level == level.guid, *top == level.guid);
        if !base_here && !top_here {
            continue;
        }
        let Ok(g) = crate::stairs::regenerate(store, e) else { continue };
        let r = r.extended(g.risers);
        let (Some(first), Some(last)) = (r.runs.first(), r.runs.last()) else { continue };
        // The stair itself, between its sides (standing on the platform, or its opening over the
        // stair below); and the zone before its foot or beyond its head.
        let left: Vec<V2> = crate::stairs::side_path(&r, 0.0, g.riser_height, true, 0.0).iter().map(|p| [p[0], p[1]]).collect();
        let right: Vec<V2> = crate::stairs::side_path(&r, 0.0, g.riser_height, false, 0.0).iter().map(|p| [p[0], p[1]]).collect();
        out.push(ccw(left.into_iter().chain(right.into_iter().rev()).collect()));
        if base_here {
            let d0 = first.direction;
            out.push(strip(first.start, [-d0[0], -d0[1]], STANDING_RUN_OFF, first.width));
        } else {
            out.push(strip(last.end(r.tread_depth), last.end_direction(r.tread_depth), STANDING_RUN_OFF, last.width));
        }
    }
    for (_, el) in crate::escalators::escalators(store) {
        let (lo, hi) = (el.levels[0] == level.guid, el.levels[1] == level.guid);
        if !lo && !hi {
            continue;
        }
        let (l, w) = (el.sized.length(), el.sized.truss_width);
        let (s, c) = el.rotation.sin_cos();
        let along = [c, s];
        out.push(strip(el.at(0.0, 0.0), along, l, w));
        if lo {
            out.push(strip(el.at(0.0, 0.0), [-c, -s], STANDING_RUN_OFF, w));
        }
        if hi && el.levels[0] != el.levels[1] {
            out.push(strip(el.at(l, 0.0), along, STANDING_RUN_OFF, w));
        }
    }
    for (_, el) in crate::elevators::elevators(store) {
        if el.levels.contains(&level.guid) {
            let ([w, dd], t) = (el.sized.hoistway, el.wall_thickness);
            out.push(ccw(vec![el.at(-w / 2.0 - t, -t), el.at(w / 2.0 + t, -t), el.at(w / 2.0 + t, dd + t), el.at(-w / 2.0 - t, dd + t)]));
        }
    }
    difference(&within, &out).iter().map(Shape::area).sum()
}

/// Square metres an occupant: dense stations (7 ft²) and outlying ones (15 ft²).
pub const DENSE_AREA: f64 = 0.65;
pub const OUTLYING_AREA: f64 = 1.39;

/// How the load was found.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LoadBasis {
    /// The trains and those waiting; the area and the load at the density (where asked).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trains: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub area: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by_area: Option<f64>,
    /// `given`, `trains` or `area`: which one the load is.
    pub from: String,
}

fn occupant_load(store: &Store, level: &crate::levels::LevelInfo, p: &ClearingParams) -> Result<(f64, LoadBasis)> {
    let positive = |v: f64| v.is_finite() && v > 0.0;
    match (p.load, &p.occupancy) {
        (Some(load), None) => {
            if !positive(load) {
                return Err(bad("route.invalid", "the occupant load must be above 0"));
            }
            Ok((load, LoadBasis { trains: None, area: None, by_area: None, from: "given".into() }))
        }
        (None, Some(o)) => {
            if o.trains.iter().chain([&o.entraining]).any(|v| !(v.is_finite() && *v >= 0.0)) {
                return Err(bad("route.invalid", "train loads and those waiting must be 0 or more"));
            }
            let trains = o.trains.iter().sum::<f64>() + o.entraining;
            let (area, by_area) = match o.area_per_person {
                Some(a) if !positive(a) => return Err(bad("route.invalid", "the area an occupant must be above 0")),
                Some(a) => {
                    let area = match (o.area, o.train) {
                        (Some(x), _) if !(x.is_finite() && x >= 0.0) => return Err(bad("route.invalid", "the area must be 0 or more")),
                        (Some(x), _) => x,
                        (None, Some(train)) => standing_area(store, level, train),
                        (None, None) => crate::floors::all(store)
                            .into_iter()
                            .filter(|(id, _)| store.get(*id).and_then(crate::floors::floor_recipe).is_some_and(|r| r.level == level.guid))
                            .map(|(_, g)| g.area())
                            .sum(),
                    };
                    (Some(area), Some(area / a))
                }
                None => (None, None),
            };
            let (load, from) = match by_area {
                Some(b) if b > trains => (b, "area"),
                _ => (trains, "trains"),
            };
            if !positive(load) {
                return Err(bad("route.invalid", "the occupant load works out at 0"));
            }
            Ok((load, LoadBasis { trains: Some(trains), area, by_area, from: from.into() }))
        }
        _ => Err(bad("route.invalid", "give the occupant load, or the occupancy to work it out from, not both")),
    }
}

fn nfpa130() -> EgressMode {
    EgressMode::Nfpa130
}

/// A way off the platform.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Exit {
    pub kind: LinkKind,
    pub element: ElementId,
    /// Clear width (m) and capacity (persons a minute).
    pub width: f64,
    pub capacity: f64,
}

/// The platform's clearing times.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Clearing {
    pub mode: EgressMode,
    pub load: f64,
    pub load_basis: LoadBasis,
    pub exits: Vec<Exit>,
    /// Persons a minute: the stairs; the escalators with the worst unit out; what of them
    /// counts (at most half the whole); the evacuation lifts left after one out and one kept for
    /// firefighters, and what of them counts (at most half the whole); and the platform's exit
    /// capacity.
    pub stairs: f64,
    pub escalators: f64,
    pub counted: f64,
    pub lifts: f64,
    pub lifts_counted: f64,
    pub capacity: f64,
    /// Minutes: the load through the exits (at most 4).
    pub platform_time: f64,
    /// The route from the most remote point to the point of safety, and its walking time at the
    /// NFPA 130 speeds; the walk to the first exit; the wait there (the platform time less that
    /// walk); and the whole (at most 6).
    pub route: Route,
    pub walking_time: f64,
    pub walk_to_exit: f64,
    pub wait: f64,
    /// The fare lines the route passes through (ARC-VT-022), and their waits together.
    pub fare_lines: Vec<FareCrossing>,
    pub fare_wait: f64,
    pub safety_time: f64,
    pub meets_platform: bool,
    pub meets_safety: bool,
    pub notes: Vec<String>,
}

/// `egress.platform_clearing`: NFPA 130's two tests for a station platform.
pub fn platform_clearing(store: &Store, p: ClearingParams) -> Result<Clearing> {
    let levels = crate::levels::list(store);
    let id = store.resolve(&p.level).ok_or_else(|| bad("element.not_found", "no such level"))?;
    let platform = levels.iter().find(|l| l.id == id).ok_or_else(|| bad("route.not_a_level", format!("element {id} is not a level")))?;
    let (load, load_basis) = occupant_load(store, platform, &p)?;
    let mut exits = Vec::new();
    let mut notes = Vec::new();
    for (sid, _) in crate::stairs::all(store) {
        let Some(e) = store.get(sid) else { continue };
        let Some(r) = crate::stairs::stair_recipe(e) else { continue };
        let crate::stairs::StairTop::Level(top) = &r.top else { continue };
        if (r.base_level == platform.guid) != (*top == platform.guid) {
            let width = r.runs.iter().map(|x| x.width).fold(f64::INFINITY, f64::min);
            exits.push(Exit { kind: LinkKind::Stair, element: sid, width, capacity: width * 1000.0 * STAIR_CAPACITY });
        }
    }
    let stairs: f64 = exits.iter().map(|x| x.capacity).sum();
    let mut esc = Vec::new();
    if p.mode != EgressMode::Ibc {
        for (eid, el) in crate::escalators::escalators(store) {
            if el.levels[0] != el.levels[1] && el.levels.contains(&platform.guid) {
                let width = el.sized.step_width;
                esc.push(Exit { kind: LinkKind::Escalator, element: eid, width, capacity: width * crate::traffic::EVACUATION_RATE });
            }
        }
    }
    // (+ 0.0: an empty sum is −0.)
    let escalators = esc.iter().map(|x| x.capacity).sum::<f64>() - esc.iter().map(|x| x.capacity).fold(0.0, f64::max) + 0.0;
    // At most half the exit capacity: no more than the stairs give.
    let counted = escalators.min(stairs);
    if counted < escalators {
        notes.push(format!("escalators limited to half the exit capacity: {counted:.0} of {escalators:.0} persons a minute counted"));
    }
    if !esc.is_empty() {
        notes.push("the escalator out of service is the widest; the rest run in the exit direction".to_owned());
    }
    exits.extend(esc);
    // Evacuation lifts serving the platform and the point of safety's level.
    let mut lifts = 0.0;
    if p.lifts {
        use crate::elevators::EgressRole;
        let safe_id = store.resolve(&p.safety.level).ok_or_else(|| bad("element.not_found", "no such level"))?;
        let safe = levels.iter().find(|l| l.id == safe_id).ok_or_else(|| bad("route.not_a_level", format!("element {safe_id} is not a level")))?;
        let rise = safe.elevation - platform.elevation;
        let serving: Vec<(ElementId, crate::elevators::Elevator)> = crate::elevators::elevators(store).into_iter().filter(|(_, el)| el.levels.contains(&platform.guid)).collect();
        let mut evac: Vec<Exit> = serving
            .iter()
            .filter(|(_, el)| el.egress_role == EgressRole::OccupantEvacuation && el.levels.contains(&safe.guid) && rise.abs() > 1e-9)
            .map(|(eid, el)| Exit { kind: LinkKind::Lift, element: *eid, width: el.sized.car[0], capacity: lift_capacity(f64::from(el.sized.persons), rise, el.sized.speed) })
            .collect();
        evac.sort_by(|a, b| b.capacity.total_cmp(&a.capacity));
        let firefighters = serving.iter().any(|(_, el)| el.egress_role == EgressRole::FireService);
        let out = 1 + usize::from(!firefighters);
        notes.push(if firefighters {
            "of the evacuation lifts the largest is out of service; a fire service lift is kept for firefighters".to_owned()
        } else {
            "of the evacuation lifts the two largest are left out: one out of service, one kept for firefighters (no fire service lift serves the platform)".to_owned()
        });
        lifts = evac.iter().skip(out).map(|x| x.capacity).sum::<f64>() + 0.0;
        exits.extend(evac);
    }
    // At most half the exit capacity: no more than the stairs and escalators give.
    let lifts_counted = lifts.min(stairs + counted);
    if lifts_counted < lifts {
        notes.push(format!("lifts limited to half the exit capacity: {lifts_counted:.0} of {lifts:.0} persons a minute counted"));
    }
    let capacity = stairs + counted + lifts_counted;
    if capacity <= 0.0 {
        return Err(bad("route.no_exits", format!("no stair (or counted escalator) leaves {}", platform.name)));
    }
    let platform_time = load / capacity;
    let r = route(store, RouteParams { start: At { point: p.remote, level: ElementRef::Id(id) }, end: p.safety, mode: p.mode, lifts: false, lift_wait: None })?;
    let elevation = |l: ElementId| levels.iter().find(|x| x.id == l).map_or(0.0, |x| x.elevation);
    let climb: f64 = r
        .links
        .iter()
        .map(|l| {
            let rise = elevation(l.to) - elevation(l.from);
            rise.abs() / if rise > 0.0 { STAIR_UP_SPEED } else { STAIR_DOWN_SPEED }
        })
        .sum();
    let walked: f64 = r.legs.iter().map(|g| g.length).sum();
    let walking_time = walked / WALK_SPEED + climb;
    let walk_to_exit = if r.links.is_empty() { walking_time } else { r.legs.first().map_or(0.0, |g| g.length) / WALK_SPEED };
    let wait = (platform_time - walk_to_exit).max(0.0);
    if !(p.concourse_load.is_finite() && p.concourse_load >= 0.0) {
        return Err(bad("route.invalid", "the concourse load must be 0 or more"));
    }
    // NFPA 130 Annex C: the wait at a fare line is how much longer its flow takes than the
    // platform's.
    let fare_lines: Vec<FareCrossing> = r
        .legs
        .iter()
        .flat_map(|g| crate::fare_gates::crossed(store, g.level, &g.points))
        .map(|(fid, f)| {
            let flow_time = (load + p.concourse_load) / f.capacity;
            FareCrossing { fare_line: fid, capacity: f.capacity, flow_time, wait: (flow_time - platform_time).max(0.0) }
        })
        .collect();
    let fare_wait = fare_lines.iter().map(|x| x.wait).sum::<f64>() + 0.0;
    let safety_time = walking_time + wait + fare_wait;
    Ok(Clearing {
        mode: p.mode,
        load,
        load_basis,
        exits,
        stairs,
        escalators,
        counted,
        lifts,
        lifts_counted,
        capacity,
        platform_time,
        route: r,
        walking_time,
        walk_to_exit,
        wait,
        fare_lines,
        fare_wait,
        safety_time,
        meets_platform: platform_time <= PLATFORM_LIMIT + 1e-9,
        meets_safety: safety_time <= SAFETY_LIMIT + 1e-9,
        notes,
    })
}
