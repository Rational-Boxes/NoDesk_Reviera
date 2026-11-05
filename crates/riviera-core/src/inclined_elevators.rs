// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Inclined elevators (ARC-VT-023): a car running on a straight track up a slope between a landing
//! on one level and a landing on another, the first guideway device on the alignment foundation
//! (ARC-ALN-001). The track is an alignment rising from the bottom landing to the top one; the
//! rails, the car parked at the bottom and the spring buffer are one transport object (ELEVATOR,
//! "InclinedElevator"). Car sizes, capacities, track widths and drive limits are maker-style
//! values (Hill Hiker's CSI specification, research/16 Q4 C2); A17.1 §5.1 covers inclines below
//! 70° (E-97).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::clearances::{Clearance, ClearanceKind};
use crate::elevators::measure;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::store::Store;
use crate::txn::Transaction;

type V2 = [f64; 2];
type V3 = [f64; 3];

pub const DATA_KEY: &str = "rxd:inclined_elevator";
const LB: f64 = 0.453_592_37;
const FT: f64 = 0.3048;

/// A17.1 §5.1: an inclined elevator runs at less than 70° from the horizontal.
pub const MAX_ANGLE_DEG: f64 = 70.0;
/// The level landing in front of each gate (m), and its headroom.
pub const LANDING_DEPTH: f64 = 1.5;
pub const LANDING_HEADROOM: f64 = 2.0;
/// The rails' top under the parked car's uphill end, below its floor (m).
const RAIL_DROP: f64 = 0.2;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// A car size.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CarSize {
    /// 3 × 3 ft.
    Small,
    /// 3 × 4 ft.
    Medium,
    /// 4 × 4 ft.
    #[default]
    Large,
    /// 4 × 5 ft.
    Xl,
    /// 5 × 5 ft, the accessible car.
    Ada,
    /// 5 × 6 ft.
    Hd,
    /// 5 × 7 ft.
    Hdx,
    /// 5 × 8 ft.
    Hdxl,
}

/// A car size's interior (W × L, m), frame outside (W × L, m), door (m) and capacity (residential,
/// commercial; kg) where published.
struct CarRow {
    car: [f64; 2],
    frame: [f64; 2],
    door: f64,
    capacity: Option<[f64; 2]>,
}

fn row(size: CarSize) -> CarRow {
    let i = |v: f64| v * 0.0254;
    let (car, frame, door, cap): ([f64; 2], [f64; 2], f64, Option<[f64; 2]>) = match size {
        CarSize::Small => ([3.0, 3.0], [40.75, 55.5], 19.75, Some([428.0, 789.0])),
        CarSize::Medium => ([3.0, 4.0], [40.75, 67.5], 31.75, Some([855.0, 1038.0])),
        CarSize::Large => ([4.0, 4.0], [52.75, 67.5], 31.75, Some([1141.0, 1439.0])),
        CarSize::Xl => ([4.0, 5.0], [52.75, 79.5], 36.0, Some([1391.0, 1814.0])),
        CarSize::Ada => ([5.0, 5.0], [64.75, 79.5], 36.0, Some([1738.0, 2371.0])),
        CarSize::Hd => ([5.0, 6.0], [64.6, 91.5], 36.0, None),
        CarSize::Hdx => ([5.0, 7.0], [64.6, 103.5], 36.0, None),
        CarSize::Hdxl => ([5.0, 8.0], [64.6, 115.5], 36.0, Some([4078.0, 4078.0])),
    };
    CarRow { car: car.map(|v| v * FT), frame: frame.map(i), door: i(door), capacity: cap.map(|c| c.map(|v| v * LB)) }
}

/// Residential (A17.1 §5.4) or commercial (§5.1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Use {
    Residential,
    #[default]
    Commercial,
}

/// An open car (42 in rails) or an enclosed one (75 in walls and a roof).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Enclosure {
    Open,
    #[default]
    Enclosed,
}

/// The drive.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Drive {
    /// Winding drum, under the rail on a slab: ≤ 500 ft, ≤ 2000 lb.
    #[default]
    Wd,
    /// Winding drum on top of the rail: ≤ 155 ft, ≤ 1200 lb.
    Wdx,
    /// Continuous-loop traction on top of the rail: any travel, ≤ 1200 lb.
    Clx,
    /// Double-cable drum on a slab: ≤ 500 ft, ≤ 3800 lb.
    Wdd,
}

impl Drive {
    /// The longest track (m; none: any) and the heaviest load (kg) it takes.
    pub fn limits(self) -> (Option<f64>, f64) {
        match self {
            Drive::Wd => (Some(500.0 * FT), 2000.0 * LB),
            Drive::Wdx => (Some(155.0 * FT), 1200.0 * LB),
            Drive::Clx => (None, 1200.0 * LB),
            Drive::Wdd => (Some(500.0 * FT), 3800.0 * LB),
        }
    }
}

/// A landing: a point in plan on a level (GlobalId).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct End {
    pub point: V2,
    pub level: String,
}

/// An inclined elevator's data, kept on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InclinedElevator {
    pub bottom: End,
    pub top: End,
    /// Its track's alignment (GlobalId).
    pub alignment: String,
    pub size: CarSize,
    #[serde(rename = "use")]
    pub use_: Use,
    pub enclosure: Enclosure,
    pub drive: Drive,
    /// Rail centres (m) and speed (m/s).
    pub track_width: f64,
    pub speed: f64,
    /// Car interior and frame (W × L, m), door, wall height (m) and capacity (kg, if published).
    pub car: [f64; 2],
    pub frame: [f64; 2],
    pub door: f64,
    pub wall_height: f64,
    pub capacity: Option<f64>,
    /// On an accessible route.
    #[serde(default)]
    pub accessible: bool,
    /// Along the slope (m), the rise (m), the run (m), the angle (degrees) and the trip (s).
    pub length: f64,
    pub rise: f64,
    pub run: f64,
    pub angle: f64,
    pub trip: f64,
}

pub fn inclined_elevator(e: &crate::element::Element) -> Option<InclinedElevator> {
    serde_json::from_value(e.param(DATA_KEY)?.clone()).ok()
}

pub fn inclined_elevators(store: &Store) -> Vec<(ElementId, InclinedElevator)> {
    store.iter().filter_map(|e| Some((e.id(), inclined_elevator(e)?))).collect()
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct At {
    pub point: V2,
    pub level: ElementRef,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// The bottom landing (the car's gate) and the top one, on a higher level.
    pub bottom: At,
    pub top: At,
    #[serde(default)]
    pub size: CarSize,
    #[serde(default, rename = "use")]
    pub use_: Use,
    #[serde(default)]
    pub enclosure: Enclosure,
    #[serde(default)]
    pub drive: Drive,
    /// Rail centres (m; default 0.914 for a 3 ft wide car, 1.219 otherwise).
    #[serde(default)]
    pub track_width: Option<f64>,
    /// m/s (default 0.25).
    #[serde(default)]
    pub speed: Option<f64>,
    /// On an accessible route: the car must be the accessible one (default false).
    #[serde(default)]
    pub accessible: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Created {
    pub inclined_elevator: ElementId,
    pub alignment: ElementId,
    #[serde(flatten)]
    pub data: InclinedElevator,
}

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

/// A box from `a` to `b` (its axis), `w` wide across (horizontal) and `h` deep below the axis.
pub(crate) fn beam(a: V3, b: V3, w: f64, h: f64) -> crate::bodies::Mesh {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l = d[0].hypot(d[1]).max(1e-9);
    let n = [-d[1] / l * w / 2.0, d[0] / l * w / 2.0, 0.0];
    let down = [0.0, 0.0, -h];
    let c = [add(a, scale(n, -1.0)), add(a, n), add(b, n), add(b, scale(n, -1.0))];
    let mut pts: Vec<V3> = c.to_vec();
    pts.extend(c.iter().map(|p| add(*p, down)));
    // Top 0..4, bottom 4..8.
    let quads: [[u32; 4]; 6] = [[0, 1, 2, 3], [7, 6, 5, 4], [0, 4, 5, 1], [1, 5, 6, 2], [2, 6, 7, 3], [3, 7, 4, 0]];
    let mut idx = Vec::new();
    for q in quads {
        idx.extend([q[0], q[1], q[2], q[0], q[2], q[3]]);
    }
    (pts, idx)
}

pub(crate) fn merge(into: &mut crate::bodies::Mesh, m: crate::bodies::Mesh) {
    let base = into.0.len() as u32;
    into.0.extend(m.0);
    into.1.extend(m.1.into_iter().map(|i| i + base));
}

/// `inclined_elevator.create`.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<Created> {
    let store = txn.store();
    let levels = crate::levels::list(store);
    let level = |r: &ElementRef| -> Result<crate::levels::LevelInfo> {
        let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such level"))?;
        levels.iter().find(|l| l.id == id).cloned().ok_or_else(|| bad("vt.not_a_level", format!("element {id} is not a level")))
    };
    let (lb, lt) = (level(&p.bottom.level)?, level(&p.top.level)?);
    let rise = lt.elevation - lb.elevation;
    let (b, t) = (p.bottom.point, p.top.point);
    let run = (t[0] - b[0]).hypot(t[1] - b[1]);
    if rise <= 0.0 {
        return Err(bad("vt.invalid", "the top landing must be on a higher level than the bottom one"));
    }
    if run < 1e-3 {
        return Err(bad("vt.invalid", "the landings are one above the other: that is a vertical elevator"));
    }
    let r = row(p.size);
    let track_width = p.track_width.unwrap_or(if r.car[0] < 1.0 { 0.914 } else { 1.219 });
    let speed = p.speed.unwrap_or(0.25);
    if !(track_width > 0.3 && track_width < 3.0) || !(speed > 0.0 && speed <= 2.0) {
        return Err(bad("vt.invalid", "the track width runs from 0.3 to 3 m and the speed from above 0 to 2 m/s"));
    }
    let length = run.hypot(rise);
    let angle = rise.atan2(run).to_degrees();
    let wall_height = if p.enclosure == Enclosure::Open { 1.067 } else { 1.905 };
    // The track: an alignment rising from the bottom landing to the top one.
    let track = crate::alignments::create(
        txn,
        crate::alignments::CreateParams {
            level: ElementRef::Id(lb.id),
            points: vec![b, t],
            radii: Vec::new(),
            start_station: 0.0,
            profile: vec![crate::alignments::Pvi { station: 0.0, elevation: 0.0, curve: None }, crate::alignments::Pvi { station: run, elevation: rise, curve: None }],
            name: Some("Inclined Elevator Track".into()),
        },
    )?;
    let track_guid = txn.store().get(track.alignment).expect("made").guid().to_string();
    let el = InclinedElevator {
        bottom: End { point: b, level: lb.guid.clone() },
        top: End { point: t, level: lt.guid.clone() },
        alignment: track_guid,
        size: p.size,
        use_: p.use_,
        enclosure: p.enclosure,
        drive: p.drive,
        track_width,
        speed,
        car: r.car,
        frame: r.frame,
        door: r.door,
        wall_height,
        capacity: r.capacity.map(|c| if p.use_ == Use::Residential { c[0] } else { c[1] }),
        accessible: p.accessible,
        length,
        rise,
        run,
        angle,
        trip: length / speed,
    };
    // The object, relative to the bottom level: the rails (dipping into a pit below the bottom
    // landing so the parked car's floor is level with it), the car and the spring buffer.
    let d = [(t[0] - b[0]) / run, (t[1] - b[1]) / run];
    let n = [-d[1], d[0]];
    let tan = rise / run;
    let fl = el.frame[1];
    let rail_z = |s: f64| -RAIL_DROP + tan * (s - fl);
    let at = |s: f64, off: f64| -> V2 { [b[0] + d[0] * s + n[0] * off, b[1] + d[1] * s + n[1] * off] };
    let mut mesh: crate::bodies::Mesh = (Vec::new(), Vec::new());
    for side in [-1.0, 1.0] {
        let off = side * track_width / 2.0;
        let (p0, p1) = (at(0.0, off), at(run, off));
        merge(&mut mesh, beam([p0[0], p0[1], rail_z(0.0)], [p1[0], p1[1], rail_z(run)], 0.08, 0.15));
    }
    let (w, l) = (el.frame[0], fl);
    let ring = vec![at(0.0, -w / 2.0), at(0.0, w / 2.0), at(l, w / 2.0), at(l, -w / 2.0)];
    let ring = if crate::polygons::area(&ring) < 0.0 { ring.into_iter().rev().collect::<Vec<_>>() } else { ring };
    merge(&mut mesh, crate::bodies::extrude(&ring, &[], 0.0, wall_height));
    let (q0, q1) = (at(-0.3, 0.0), at(-0.05, 0.0));
    merge(&mut mesh, beam([q0[0], q0[1], rail_z(-0.3)], [q1[0], q1[1], rail_z(-0.05)], 0.3, 0.3));
    let tris: Vec<[u32; 3]> = mesh.1.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
    let name = format!("Inclined Elevator, {:?} car, {:.1}° over {:.1} m", p.size, angle, length);
    let ty = crate::mesh_objects::create_generated_type(txn, &name, crate::mesh_objects::Category::Transport, &mesh.0, &tris, [0.55, 0.6, 0.66, 1.0], Vec::new(), Some("ELEVATOR"))?;
    let id = crate::mesh_objects::place(
        txn,
        crate::mesh_objects::PlaceParams { type_id: ElementRef::Id(ty), point: [0.0, 0.0], level: ElementRef::Id(lb.id), offset: 0.0, rotation: 0.0, mirror: false, scale: 1.0 },
    )?;
    let data = serde_json::to_value(&el).expect("serialises");
    let props = properties(&el);
    txn.modify(id, |e| {
        e.name = Some("Inclined Elevator".into());
        e.params.insert("@ObjectType".into(), "InclinedElevator".into());
        e.params.insert(DATA_KEY.into(), data);
        e.params.extend(props);
    })?;
    Ok(Created { inclined_elevator: id, alignment: track.alignment, data: el })
}

/// Its property sets: IFC's clear car size, and `Riviera_InclinedElevator`.
pub fn properties(el: &InclinedElevator) -> Vec<(String, Value)> {
    let len = |v: f64| measure("IfcPositiveLengthMeasure", v);
    let mut out = vec![
        ("Pset_TransportElementElevator.ClearWidth".into(), len(el.car[0])),
        ("Pset_TransportElementElevator.ClearDepth".into(), len(el.car[1])),
        ("Pset_TransportElementElevator.ClearHeight".into(), len(el.wall_height)),
        ("Riviera_InclinedElevator.Angle".into(), measure("IfcPlaneAngleMeasure", el.angle.to_radians())),
        ("Riviera_InclinedElevator.TrackLength".into(), len(el.length)),
        ("Riviera_InclinedElevator.TrackWidth".into(), len(el.track_width)),
        ("Riviera_InclinedElevator.Speed".into(), measure("IfcLinearVelocityMeasure", el.speed)),
        ("Riviera_InclinedElevator.DriveType".into(), measure("IfcLabel", serde_json::to_value(el.drive).expect("serialises"))),
        ("Riviera_InclinedElevator.DoorWidth".into(), len(el.door)),
    ];
    if let Some(c) = el.capacity {
        out.push(("Pset_TransportElementCommon.CapacityWeight".into(), measure("IfcMassMeasure", c)));
    }
    out
}

/// The level landing in front of each gate: beyond the bottom of the track at the bottom level,
/// beyond its top at the top level.
pub fn zones(store: &Store) -> Vec<Clearance> {
    let mut out = Vec::new();
    for (id, el) in inclined_elevators(store) {
        let (Ok(zb), Ok(zt)) = (crate::levels::elevation_of(store, &el.bottom.level), crate::levels::elevation_of(store, &el.top.level)) else { continue };
        let (b, t) = (el.bottom.point, el.top.point);
        let d = [(t[0] - b[0]) / el.run, (t[1] - b[1]) / el.run];
        let n = [-d[1], d[0]];
        let w = el.frame[0].max(LANDING_DEPTH) / 2.0;
        let rect = |o: V2, dir: V2| -> Vec<V2> {
            let far = [o[0] + dir[0] * LANDING_DEPTH, o[1] + dir[1] * LANDING_DEPTH];
            let r = vec![[o[0] - n[0] * w, o[1] - n[1] * w], [o[0] + n[0] * w, o[1] + n[1] * w], [far[0] + n[0] * w, far[1] + n[1] * w], [far[0] - n[0] * w, far[1] - n[1] * w]];
            if crate::polygons::area(&r) < 0.0 { r.into_iter().rev().collect() } else { r }
        };
        for (ring, z) in [(rect(b, [-d[0], -d[1]]), zb), (rect(t, d), zt)] {
            out.push(Clearance { element: id, kind: ClearanceKind::InclinedLanding, rule: "A17.1 5.1: a level landing at each gate".into(), mesh: crate::bodies::extrude(&ring, &[], z + 0.01, z + LANDING_HEADROOM), door_swing_forbidden: false });
        }
    }
    out
}

/// What may stand in its landings besides itself: its track.
pub fn may_share(store: &Store, other: ElementId) -> bool {
    store.get(other).is_some_and(|e| crate::alignments::recipe(e).is_some())
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoParams {}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Info {
    pub inclined_elevator: ElementId,
    #[serde(flatten)]
    pub data: InclinedElevator,
}

/// `inclined_elevator.list`.
pub(crate) fn list(store: &Store, _: Option<NoParams>) -> Result<Vec<Info>> {
    Ok(inclined_elevators(store).into_iter().map(|(id, data)| Info { inclined_elevator: id, data }).collect())
}

/// A problem found.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Finding {
    pub inclined_elevator: ElementId,
    /// `error` or `note`.
    pub severity: String,
    pub rule: String,
    pub message: String,
}

/// `inclined_elevator.check`: the incline, the drive's travel and load, and the accessible car.
pub(crate) fn check(store: &Store, _: Option<NoParams>) -> Result<Vec<Finding>> {
    let mut out = Vec::new();
    for (id, el) in inclined_elevators(store) {
        let mut f = |severity: &str, rule: &str, message: String| out.push(Finding { inclined_elevator: id, severity: severity.into(), rule: rule.into(), message });
        if el.angle >= MAX_ANGLE_DEG {
            f("error", "A17.1 5.1", format!("{:.1}° is not an incline: at 70° or more it is a vertical elevator (A17.1 Part 2)", el.angle));
        }
        let (travel, load) = el.drive.limits();
        if let Some(max) = travel.filter(|m| el.length > *m) {
            f("error", "drive", format!("the {:?} drive runs at most {max:.0} m of track; this one is {:.1} m", el.drive, el.length));
        }
        match el.capacity {
            Some(c) if c > load + 1e-9 => f("error", "drive", format!("the {:?} drive lifts at most {load:.0} kg; this car carries {c:.0} kg", el.drive)),
            None => f("note", "capacity", format!("no published capacity for the {:?} car: take the maker's", el.size)),
            _ => {}
        }
        if el.accessible && (el.car[0] < 1.524 - 1e-6 || el.car[1] < 1.524 - 1e-6 || el.door < 0.914 - 1e-6) {
            f("error", "ADA 407 (by analogy)", format!("on an accessible route the car is the accessible one: 5 × 5 ft inside with a 36 in door; this is {:.2} × {:.2} m with a {:.0} mm door", el.car[0], el.car[1], el.door * 1000.0));
        }
    }
    Ok(out)
}
