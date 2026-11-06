// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Automated people movers (ARC-VT-025, ARC-VT-026): a guideway of one or two lanes along an
//! alignment (ARC-ALN-001), each lane's dynamic envelope kept clear, and stations at chainages
//! with side or centre platforms and the train standing at them. `apm.check` checks curve radii,
//! grades and line capacity. Planning values from ACRP Report 37 as noted in research/16 Q5
//! (E-99); ASCE 21's own numbers were not reachable, so the rules are advisory.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::alignments::{Alignment, Piece};
use crate::clearances::{Clearance, ClearanceKind};
use crate::elevators::measure;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::inclined_elevators::{beam, merge};
use crate::store::Store;
use crate::txn::Transaction;

type V3 = [f64; 3];

pub const GUIDEWAY_KEY: &str = "rxd:apm_guideway";
pub const STATION_KEY: &str = "rxd:apm_station";

/// Track separation (m), a lane's dynamic envelope (width and height above the running surface,
/// m), the deck's depth, the vehicle floor above the running surface, and an emergency walkway's
/// width (m).
pub const TRACK_SEPARATION: f64 = 4.75;
pub const ENVELOPE_WIDTH: f64 = 3.66;
pub const ENVELOPE_HEIGHT: f64 = 3.6;
pub const DECK_DEPTH: f64 = 0.6;
pub const FLOOR_HEIGHT: f64 = 1.0;
pub const WALKWAY_WIDTH: f64 = 0.61;
/// Curve radii (m): mainline, and the absolute least.
pub const MIN_RADIUS: f64 = 91.0;
pub const ABSOLUTE_MIN_RADIUS: f64 = 46.0;
/// Grades: between stations (6 %) and in them (level, within 0.5 %).
pub const MAX_GRADE: f64 = 0.06;
pub const STATION_GRADE: f64 = 0.005;
/// A vehicle's length, width and height (m), and the least headway (s).
pub const CAR: [f64; 3] = [12.2, 2.9, 3.5];
pub const MIN_HEADWAY: f64 = 90.0;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// A guideway's data, kept on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Guideway {
    /// Its alignment (GlobalId), lanes (1 or 2), track separation and dynamic envelope width (m).
    pub alignment: String,
    pub lanes: u32,
    pub track_separation: f64,
    pub envelope_width: f64,
    pub walkway: bool,
    /// The deck's width (m) and the guideway's length (m).
    pub deck_width: f64,
    pub length: f64,
}

/// Side platforms (outside the lanes) or one centre platform (between two lanes).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    #[default]
    Side,
    Centre,
}

/// A station's data, kept on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Station {
    /// Its guideway (GlobalId) and the chainage of its middle.
    pub guideway: String,
    pub station: f64,
    pub name: String,
    pub platform: Platform,
    /// Cars a train, the platform's length and width (m), and platform screen doors.
    pub cars: u32,
    pub platform_length: f64,
    pub platform_width: f64,
    pub screen_doors: bool,
}

pub fn guideway(e: &crate::element::Element) -> Option<Guideway> {
    serde_json::from_value(e.param(GUIDEWAY_KEY)?.clone()).ok()
}

pub fn station(e: &crate::element::Element) -> Option<Station> {
    serde_json::from_value(e.param(STATION_KEY)?.clone()).ok()
}

pub fn guideways(store: &Store) -> Vec<(ElementId, Guideway)> {
    store.iter().filter_map(|e| Some((e.id(), guideway(e)?))).collect()
}

pub fn stations(store: &Store) -> Vec<(ElementId, Station)> {
    store.iter().filter_map(|e| Some((e.id(), station(e)?))).collect()
}

fn alignment_of(store: &Store, guid: &str) -> Option<(ElementId, Alignment)> {
    let id = store.find_by_guid(&crate::id::GlobalId::parse(guid)?)?;
    Some((id, Alignment::of(crate::alignments::recipe(store.get(id)?)?).ok()?))
}

/// A point at chainage `s`, `off` to the left, `dz` above the running surface (relative to the
/// alignment's level).
fn at(a: &Alignment, s: f64, off: f64, dz: f64) -> V3 {
    let (q, d, z, _) = a.at(s);
    [q[0] - d[1] * off, q[1] + d[0] * off, z + dz]
}

/// A band along the alignment from `s0` to `s1`, centred `off` to the left, `w` wide, from `dz`
/// down by `depth`.
fn band(a: &Alignment, s0: f64, s1: f64, off: f64, w: f64, dz: f64, depth: f64) -> crate::bodies::Mesh {
    let n = (((s1 - s0) / 4.0).ceil() as usize).max(1);
    let mut mesh = (Vec::new(), Vec::new());
    for k in 0..n {
        let (u, v) = (s0 + (s1 - s0) * k as f64 / n as f64, s0 + (s1 - s0) * (k + 1) as f64 / n as f64);
        merge(&mut mesh, beam(at(a, u, off, dz), at(a, v, off, dz), w, depth));
    }
    mesh
}

/// Lane centres: one on the alignment, or two either side of it.
fn lane_offsets(g: &Guideway) -> Vec<f64> {
    if g.lanes == 1 { vec![0.0] } else { vec![g.track_separation / 2.0, -g.track_separation / 2.0] }
}

fn level_of(store: &Store, guid: &str) -> Result<ElementId> {
    crate::levels::list(store).into_iter().find(|l| l.guid == guid).map(|l| l.id).ok_or_else(|| bad("vt.invalid", "the alignment's level is gone"))
}

fn place(txn: &mut Transaction<'_>, level: ElementId, name: &str, kind: &str, mesh: crate::bodies::Mesh, color: [f32; 4]) -> Result<ElementId> {
    let tris: Vec<[u32; 3]> = mesh.1.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
    let ty = crate::mesh_objects::create_generated_type(txn, name, crate::mesh_objects::Category::Transport, &mesh.0, &tris, color, Vec::new(), Some("USERDEFINED"))?;
    let kind = kind.to_owned();
    txn.modify(ty, |e| {
        e.params.insert("@ElementType".into(), kind.into());
    })?;
    crate::mesh_objects::place(
        txn,
        crate::mesh_objects::PlaceParams { type_id: ElementRef::Id(ty), point: [0.0, 0.0], level: ElementRef::Id(level), offset: 0.0, rotation: 0.0, mirror: false, scale: 1.0 },
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GuidewayParams {
    pub alignment_id: ElementRef,
    /// 1 or 2 (default 2).
    #[serde(default)]
    pub lanes: Option<u32>,
    /// Between the lanes' centres (m; default 4.75).
    #[serde(default)]
    pub track_separation: Option<f64>,
    /// An emergency walkway along the deck (default true).
    #[serde(default)]
    pub walkway: Option<bool>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct GuidewayCreated {
    pub guideway: ElementId,
    #[serde(flatten)]
    pub data: Guideway,
}

/// `apm.guideway`.
pub(crate) fn create_guideway(txn: &mut Transaction<'_>, p: GuidewayParams) -> Result<GuidewayCreated> {
    let store = txn.store();
    let aid = store.resolve(&p.alignment_id).ok_or_else(|| bad("element.not_found", "no such alignment"))?;
    let rec = store.get(aid).and_then(crate::alignments::recipe).ok_or_else(|| bad("vt.invalid", format!("element {aid} is not an alignment")))?;
    let guid = store.get(aid).expect("resolved").guid().to_string();
    let level = level_of(store, &rec.level)?;
    let a = Alignment::of(rec)?;
    let lanes = p.lanes.unwrap_or(2);
    let sep = p.track_separation.unwrap_or(TRACK_SEPARATION);
    if !(lanes == 1 || lanes == 2) || !(sep.is_finite() && sep >= ENVELOPE_WIDTH) {
        return Err(bad("vt.invalid", "an APM guideway has 1 or 2 lanes, at least a dynamic envelope's width (3.66 m) apart"));
    }
    let walkway = p.walkway.unwrap_or(true);
    let deck_width = if lanes == 1 { ENVELOPE_WIDTH } else { sep + ENVELOPE_WIDTH } + if walkway { WALKWAY_WIDTH } else { 0.0 };
    let g = Guideway { alignment: guid, lanes, track_separation: sep, envelope_width: ENVELOPE_WIDTH, walkway, deck_width, length: a.length };
    // The deck, centred on the lanes (the walkway on the left).
    let shift = if walkway { WALKWAY_WIDTH / 2.0 } else { 0.0 };
    let mesh = band(&a, a.recipe.start_station, a.end_station(), shift, deck_width, 0.0, DECK_DEPTH);
    let id = place(txn, level, &format!("APM Guideway, {lanes} lane(s), {:.0} m", a.length), "APMGUIDEWAY", mesh, [0.72, 0.72, 0.70, 1.0])?;
    let data = serde_json::to_value(&g).expect("serialises");
    let props = vec![
        ("Riviera_ApmGuideway.Lanes".to_owned(), measure("IfcCountMeasure", lanes)),
        ("Riviera_ApmGuideway.TrackSeparation".to_owned(), measure("IfcPositiveLengthMeasure", sep)),
        ("Riviera_ApmGuideway.DynamicEnvelopeWidth".to_owned(), measure("IfcPositiveLengthMeasure", ENVELOPE_WIDTH)),
        ("Riviera_ApmGuideway.EmergencyWalkway".to_owned(), measure("IfcBoolean", walkway)),
    ];
    txn.modify(id, |e| {
        e.name = Some("APM Guideway".into());
        e.params.insert("@ObjectType".into(), "APMGuideway".into());
        e.params.insert(GUIDEWAY_KEY.into(), data);
        e.params.extend(props);
    })?;
    Ok(GuidewayCreated { guideway: id, data: g })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StationParams {
    pub guideway_id: ElementRef,
    /// The chainage of its middle.
    pub station: f64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub platform: Platform,
    /// Cars a train (default 2, at most 6).
    #[serde(default)]
    pub cars: Option<u32>,
    /// Platform width (m; default 4).
    #[serde(default)]
    pub platform_width: Option<f64>,
    /// Platform screen doors (default true).
    #[serde(default)]
    pub screen_doors: Option<bool>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct StationCreated {
    pub station_id: ElementId,
    #[serde(flatten)]
    pub data: Station,
}

/// `apm.station`.
pub(crate) fn create_station(txn: &mut Transaction<'_>, p: StationParams) -> Result<StationCreated> {
    let store = txn.store();
    let gid = store.resolve(&p.guideway_id).ok_or_else(|| bad("element.not_found", "no such guideway"))?;
    let g = store.get(gid).and_then(guideway).ok_or_else(|| bad("vt.invalid", format!("element {gid} is not an APM guideway")))?;
    let (_, a) = alignment_of(store, &g.alignment).ok_or_else(|| bad("vt.invalid", "the guideway's alignment is gone"))?;
    let level = level_of(store, &a.recipe.level)?;
    let cars = p.cars.unwrap_or(2);
    let width = p.platform_width.unwrap_or(4.0);
    if !(1..=6).contains(&cars) || !(width.is_finite() && width >= 2.0) {
        return Err(bad("vt.invalid", "a train has 1 to 6 cars and a platform is at least 2 m wide"));
    }
    // As long as the train and 3 m at each end (door alignment margin).
    let length = f64::from(cars) * CAR[0] + 6.0;
    if p.station - length / 2.0 < a.recipe.start_station - 1e-9 || p.station + length / 2.0 > a.end_station() + 1e-9 {
        return Err(bad("vt.invalid", format!("a {length:.1} m station at {:.1} runs off the guideway ({:.1} to {:.1})", p.station, a.recipe.start_station, a.end_station())));
    }
    let lanes = lane_offsets(&g);
    let gap = ENVELOPE_WIDTH / 2.0;
    let offsets: Vec<f64> = match p.platform {
        // Outside each lane (left of a single lane).
        Platform::Side => lanes
            .iter()
            .map(|o| {
                let out = if *o < 0.0 { -1.0 } else { 1.0 };
                o + out * (gap + width / 2.0)
            })
            .collect(),
        Platform::Centre => {
            if g.lanes != 2 || g.track_separation < ENVELOPE_WIDTH + width - 1e-9 {
                return Err(bad("vt.invalid", format!("a centre platform needs two lanes at least {:.2} m apart (the envelope and the platform)", ENVELOPE_WIDTH + width)));
            }
            vec![0.0]
        }
    };
    let (s0, s1) = (p.station - length / 2.0, p.station + length / 2.0);
    let mut mesh = (Vec::new(), Vec::new());
    for o in &offsets {
        merge(&mut mesh, band(&a, s0, s1, *o, width, FLOOR_HEIGHT, FLOOR_HEIGHT));
    }
    // The train at the platform, on each lane.
    let train = f64::from(cars) * CAR[0];
    for o in &lanes {
        merge(&mut mesh, band(&a, p.station - train / 2.0, p.station + train / 2.0, *o, CAR[1], 0.3 + CAR[2], CAR[2]));
    }
    let n = stations(store).len() + 1;
    let name = p.name.unwrap_or_else(|| format!("Station {n}"));
    let id = place(txn, level, &format!("APM Station {name}"), "APMSTATION", mesh, [0.80, 0.78, 0.72, 1.0])?;
    let s = Station { guideway: store_guid(txn.store(), gid), station: p.station, name: name.clone(), platform: p.platform, cars, platform_length: length, platform_width: width, screen_doors: p.screen_doors.unwrap_or(true) };
    let data = serde_json::to_value(&s).expect("serialises");
    let props = vec![
        ("Riviera_ApmStation.PlatformType".to_owned(), measure("IfcLabel", serde_json::to_value(s.platform).expect("serialises"))),
        ("Riviera_ApmStation.PlatformLength".to_owned(), measure("IfcPositiveLengthMeasure", length)),
        ("Riviera_ApmStation.PlatformWidth".to_owned(), measure("IfcPositiveLengthMeasure", width)),
        ("Riviera_ApmStation.ScreenDoors".to_owned(), measure("IfcBoolean", s.screen_doors)),
    ];
    txn.modify(id, |e| {
        e.name = Some(name);
        e.params.insert("@ObjectType".into(), "APMStation".into());
        e.params.insert(STATION_KEY.into(), data);
        e.params.extend(props);
    })?;
    Ok(StationCreated { station_id: id, data: s })
}

fn store_guid(store: &Store, id: ElementId) -> String {
    store.get(id).expect("exists").guid().to_string()
}

/// Each lane's dynamic envelope along the guideway, kept clear (stations' own platforms and
/// trains may stand beside it, not in it).
pub fn zones(store: &Store) -> Vec<Clearance> {
    let mut out = Vec::new();
    for (id, g) in guideways(store) {
        let Some((_, a)) = alignment_of(store, &g.alignment) else { continue };
        for o in lane_offsets(&g) {
            let mesh = band(&a, a.recipe.start_station, a.end_station(), o, g.envelope_width, ENVELOPE_HEIGHT, ENVELOPE_HEIGHT - 0.05);
            out.push(Clearance { element: id, kind: ClearanceKind::ApmEnvelope, rule: "ASCE 21: the dynamic envelope".into(), mesh, door_swing_forbidden: false });
        }
    }
    out
}

/// What may stand in an envelope besides the guideway: its alignment, and stations (their trains
/// run in it).
pub fn may_share(store: &Store, other: ElementId) -> bool {
    store.get(other).is_some_and(|e| crate::alignments::recipe(e).is_some() || station(e).is_some())
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoParams {}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct GuidewayInfo {
    pub guideway: ElementId,
    #[serde(flatten)]
    pub data: Guideway,
    /// Its stations, in chainage order.
    pub stations: Vec<StationInfo>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct StationInfo {
    pub station_id: ElementId,
    #[serde(flatten)]
    pub data: Station,
}

/// `apm.list`.
pub(crate) fn list(store: &Store, _: Option<NoParams>) -> Result<Vec<GuidewayInfo>> {
    let all = stations(store);
    Ok(guideways(store)
        .into_iter()
        .map(|(id, data)| {
            let guid = store_guid(store, id);
            let mut st: Vec<StationInfo> = all.iter().filter(|(_, s)| s.guideway == guid).map(|(sid, s)| StationInfo { station_id: *sid, data: s.clone() }).collect();
            st.sort_by(|a, b| a.data.station.total_cmp(&b.data.station));
            GuidewayInfo { guideway: id, data, stations: st }
        })
        .collect())
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckParams {
    /// The service: seconds between trains (default 120), cars a train (default 2), persons a car
    /// (default 60, a planning load), and the peak demand each way (persons an hour).
    #[serde(default)]
    pub headway: Option<f64>,
    #[serde(default)]
    pub cars: Option<u32>,
    #[serde(default)]
    pub passengers: Option<f64>,
    #[serde(default)]
    pub demand: Option<f64>,
}

/// A problem found.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Finding {
    /// The guideway it is about (none for the line's service).
    pub element: Option<ElementId>,
    /// `error`, `warning` or `note`.
    pub severity: String,
    pub rule: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Check {
    /// Persons an hour each way: 3600 ÷ headway × cars × persons a car.
    pub capacity: f64,
    pub findings: Vec<Finding>,
}

/// `apm.check`: curves, grades and stations on each guideway, and the line's capacity.
pub(crate) fn check(store: &Store, p: Option<CheckParams>) -> Result<Check> {
    let p = p.unwrap_or_default();
    let headway = p.headway.unwrap_or(120.0);
    let cars = p.cars.unwrap_or(2);
    let passengers = p.passengers.unwrap_or(60.0);
    if !(headway > 0.0 && passengers >= 0.0) {
        return Err(bad("vt.invalid", "the headway must be above 0 and the persons a car 0 or more"));
    }
    let capacity = 3600.0 / headway * f64::from(cars) * passengers;
    let mut findings = Vec::new();
    let all = stations(store);
    for (id, g) in guideways(store) {
        let Some((_, a)) = alignment_of(store, &g.alignment) else { continue };
        let mut add = |severity: &str, rule: &str, message: String| findings.push(Finding { element: Some(id), severity: severity.into(), rule: rule.into(), message });
        for p in &a.pieces {
            if let Piece::Arc { radius, .. } = p {
                if *radius < ABSOLUTE_MIN_RADIUS {
                    add("error", "curve radius", format!("a {radius:.0} m curve is below the {ABSOLUTE_MIN_RADIUS:.0} m least"));
                } else if *radius < MIN_RADIUS {
                    add("warning", "curve radius", format!("a {radius:.0} m curve is below the {MIN_RADIUS:.0} m mainline radius"));
                }
            }
        }
        let guid = store_guid(store, id);
        let here: Vec<&Station> = all.iter().filter(|(_, s)| s.guideway == guid).map(|(_, s)| s).collect();
        let in_station = |s: f64| here.iter().any(|st| (s - st.station).abs() <= st.platform_length / 2.0);
        let n = (a.length.ceil() as usize).max(1);
        let (mut steepest, mut station_steepest) = (0.0f64, 0.0f64);
        for k in 0..=n {
            let s = a.recipe.start_station + a.length * k as f64 / n as f64;
            let g = a.at(s).3.abs();
            if in_station(s) { station_steepest = station_steepest.max(g) } else { steepest = steepest.max(g) }
        }
        if steepest > MAX_GRADE + 1e-9 {
            add("error", "grade", format!("{:.1} % between stations is steeper than 6 %", steepest * 100.0));
        }
        if station_steepest > STATION_GRADE + 1e-9 {
            add("error", "station grade", format!("{:.1} % at a station: stations are level", station_steepest * 100.0));
        }
        for st in &here {
            let (s0, s1) = (st.station - st.platform_length / 2.0, st.station + st.platform_length / 2.0);
            let mut c0 = a.recipe.start_station;
            for p in &a.pieces {
                let c1 = c0 + match p {
                    Piece::Line { length, .. } | Piece::Arc { length, .. } | Piece::Spiral { length, .. } => *length,
                };
                if matches!(p, Piece::Arc { .. } | Piece::Spiral { .. }) && c0 < s1 && c1 > s0 {
                    add("warning", "station on a curve", format!("{} stands on a curve: platforms are straight, for the gap to the doors", st.name));
                }
                c0 = c1;
            }
        }
    }
    if headway < MIN_HEADWAY {
        findings.push(Finding { element: None, severity: "warning".into(), rule: "headway".into(), message: format!("a {headway:.0} s headway is below the {MIN_HEADWAY:.0} s APMs run at") });
    }
    if let Some(d) = p.demand.filter(|d| *d > capacity + 1e-9) {
        findings.push(Finding { element: None, severity: "error".into(), rule: "capacity".into(), message: format!("{capacity:.0} persons an hour each way for {d:.0} needed: more cars or a shorter headway") });
    }
    Ok(Check { capacity, findings })
}
