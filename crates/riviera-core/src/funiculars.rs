// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Funiculars (ARC-VT-024): two cars on one haul rope, counterbalancing each other, running along
//! an alignment (ARC-ALN-001) from its start (the bottom station) to its end (the top station),
//! up its curves and grades. Two rails with an Abt passing loop half way, where the cars pass,
//! or four rails (two tracks) throughout; the drive at the top station. The rails, the two cars
//! and the drive house are one transport object for now. Reference lines and layouts:
//! research/16 Q4 C3 (E-98).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::alignments::Alignment;
use crate::elevators::measure;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::inclined_elevators::{beam, merge};
use crate::store::Store;
use crate::txn::Transaction;

type V3 = [f64; 3];

pub const DATA_KEY: &str = "rxd:funicular";
/// Each switch of an Abt passing loop, along the track (m).
pub const SWITCH_LENGTH: f64 = 20.0;
/// Clearance between the cars where they pass (m), and between a car and the gauge each side.
pub const PASSING_CLEARANCE: f64 = 0.6;
/// Starting and stopping (m/s²).
pub const ACCELERATION: f64 = 0.5;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// How the track is laid.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    /// Two rails, with an Abt passing loop half way (no moving parts).
    #[default]
    AbtLoop,
    /// Four rails: two tracks the whole way.
    FourRail,
}

/// A car floor kept level (a levelling or stepped car on level platforms) or along the slope.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CarFloor {
    #[default]
    Level,
    Stepped,
}

/// A funicular's data, kept on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Funicular {
    /// Its alignment (GlobalId).
    pub alignment: String,
    pub layout: Layout,
    /// Rail gauge, car length, width and height (m).
    pub gauge: f64,
    pub car_length: f64,
    pub car_width: f64,
    pub car_height: f64,
    pub car_floor: CarFloor,
    /// Persons a car, top speed (m/s) and the stop at each station (s).
    pub car_capacity: u32,
    pub speed: f64,
    pub dwell: f64,
    /// A walkway beside the track to evacuate stranded cars along.
    pub walkway: bool,
    /// The track along the slope (m), the rise (m), the steepest grade (%), the passing loop's
    /// middle (chainage) and length (m), the trip (s), and the capacity each way (persons an hour).
    pub length: f64,
    pub rise: f64,
    pub max_grade: f64,
    pub passing_loop: Option<[f64; 2]>,
    pub trip: f64,
    pub capacity: f64,
}

pub fn funicular(e: &crate::element::Element) -> Option<Funicular> {
    serde_json::from_value(e.param(DATA_KEY)?.clone()).ok()
}

pub fn funiculars(store: &Store) -> Vec<(ElementId, Funicular)> {
    store.iter().filter_map(|e| Some((e.id(), funicular(e)?))).collect()
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// The alignment it runs along, from its start (the bottom station) to its end (the top).
    pub alignment_id: ElementRef,
    #[serde(default)]
    pub layout: Layout,
    /// Rail gauge (m; default 1.0).
    #[serde(default)]
    pub gauge: Option<f64>,
    /// Car length (default 12 m), width (default the gauge + 1.2 m) and height (default 3 m).
    #[serde(default)]
    pub car_length: Option<f64>,
    #[serde(default)]
    pub car_width: Option<f64>,
    #[serde(default)]
    pub car_height: Option<f64>,
    #[serde(default)]
    pub car_floor: CarFloor,
    /// Persons a car (default 60), top speed (default 5 m/s), stop at each station (default 60 s).
    #[serde(default)]
    pub car_capacity: Option<u32>,
    #[serde(default)]
    pub speed: Option<f64>,
    #[serde(default)]
    pub dwell: Option<f64>,
    #[serde(default)]
    pub walkway: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Created {
    pub funicular: ElementId,
    #[serde(flatten)]
    pub data: Funicular,
}

/// A point along the alignment at chainage `s`, moved `off` to the left, `dz` above its track
/// level (relative to the alignment's level).
fn at(a: &Alignment, s: f64, off: f64, dz: f64) -> V3 {
    let (q, d, z, _) = a.at(s);
    [q[0] - d[1] * off, q[1] + d[0] * off, z + dz]
}

/// The steepest grade and the slope length of an alignment, worked along it every metre or less.
fn slope(a: &Alignment) -> (f64, f64) {
    let n = (a.length.ceil() as usize).max(1);
    let step = a.length / n as f64;
    let (mut length, mut max) = (0.0, 0.0f64);
    for k in 0..n {
        let s = a.recipe.start_station + step * (k as f64 + 0.5);
        let (_, _, _, g) = a.at(s);
        length += step * (1.0 + g * g).sqrt();
        max = max.max(g.abs());
    }
    (max * 100.0, length)
}

/// `funicular.create`.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<Created> {
    let store = txn.store();
    let aid = store.resolve(&p.alignment_id).ok_or_else(|| bad("element.not_found", "no such alignment"))?;
    let rec = store.get(aid).and_then(crate::alignments::recipe).ok_or_else(|| bad("vt.invalid", format!("element {aid} is not an alignment")))?;
    let guid = store.get(aid).expect("resolved").guid().to_string();
    let a = Alignment::of(rec)?;
    let gauge = p.gauge.unwrap_or(1.0);
    let car_length = p.car_length.unwrap_or(12.0);
    let car_width = p.car_width.unwrap_or(gauge + 1.2);
    let car_height = p.car_height.unwrap_or(3.0);
    let speed = p.speed.unwrap_or(5.0);
    let dwell = p.dwell.unwrap_or(60.0);
    let car_capacity = p.car_capacity.unwrap_or(60);
    let finite = [gauge, car_length, car_width, car_height, speed, dwell].iter().all(|v| v.is_finite());
    if !finite || !(0.5..=2.0).contains(&gauge) || car_length <= 0.0 || car_width < gauge || car_height <= 0.0 || !(speed > 0.0 && speed <= 15.0) || dwell < 0.0 {
        return Err(bad("vt.invalid", "gauge 0.5–2 m, car at least as wide as the gauge, speed above 0 to 15 m/s, a dwell of 0 s or more"));
    }
    let start = a.recipe.start_station;
    let (rise_start, rise_end) = (a.at(start).2, a.at(a.end_station()).2);
    let rise = rise_end - rise_start;
    if rise <= 0.0 {
        return Err(bad("vt.invalid", "a funicular climbs from its alignment's start to its end: give the alignment a rising profile"));
    }
    // Room for a car at each station and, with an Abt loop, the loop between them.
    let loop_length = car_length + 2.0 * SWITCH_LENGTH;
    let need = 2.0 * car_length + if p.layout == Layout::AbtLoop { loop_length } else { 0.0 };
    if a.length < need {
        return Err(bad("vt.invalid", format!("the alignment is {:.1} m long; the cars and passing loop need {need:.1} m", a.length)));
    }
    let (max_grade, length) = slope(&a);
    // A trip: speeding up and slowing down at the acceleration, the rest at full speed.
    let ramp = speed / ACCELERATION;
    let trip = if length > speed * ramp { length / speed + ramp } else { 2.0 * (length / ACCELERATION).sqrt() };
    let capacity = f64::from(car_capacity) * 3600.0 / (trip + dwell);
    let middle = start + a.length / 2.0;
    let f = Funicular {
        alignment: guid,
        layout: p.layout,
        gauge,
        car_length,
        car_width,
        car_height,
        car_floor: p.car_floor,
        car_capacity,
        speed,
        dwell,
        walkway: p.walkway,
        length,
        rise,
        max_grade,
        passing_loop: (p.layout == Layout::AbtLoop).then_some([middle, loop_length]),
        trip,
        capacity,
    };
    // The object (relative to the alignment's level): rails, cars and the drive house.
    let sep = car_width + PASSING_CLEARANCE;
    // A track's centre offset at chainage s: 0 on single track, ±sep/2 through the loop, eased
    // across each switch.
    let centre = |s: f64, side: f64| -> f64 {
        match p.layout {
            Layout::FourRail => side * sep / 2.0,
            Layout::AbtLoop => {
                let (l0, l1) = (middle - loop_length / 2.0, middle + loop_length / 2.0);
                let t = if s <= l0 || s >= l1 {
                    0.0
                } else if s < l0 + SWITCH_LENGTH {
                    (s - l0) / SWITCH_LENGTH
                } else if s > l1 - SWITCH_LENGTH {
                    (l1 - s) / SWITCH_LENGTH
                } else {
                    1.0
                };
                side * sep / 2.0 * t
            }
        }
    };
    let n = ((a.length / 2.0).ceil() as usize).max(2);
    let samples: Vec<f64> = (0..=n).map(|k| start + a.length * k as f64 / n as f64).collect();
    let mut mesh: crate::bodies::Mesh = (Vec::new(), Vec::new());
    for side in [-1.0, 1.0] {
        for rail in [-1.0, 1.0] {
            let pts: Vec<V3> = samples.iter().map(|s| at(&a, *s, centre(*s, side) + rail * gauge / 2.0, 0.0)).collect();
            for w in pts.windows(2) {
                merge(&mut mesh, beam(w[0], w[1], 0.07, 0.15));
            }
        }
        if p.layout == Layout::AbtLoop && side > 0.0 {
            // On single track both cars share the two rails: drawn once.
            break;
        }
    }
    // The two cars: one at each station, on their own tracks (the loop's sides), level-floored or
    // along the slope.
    for (s0, side) in [(start + car_length / 2.0 + 1.0, -1.0), (a.end_station() - car_length / 2.0 - 1.0, 1.0)] {
        let c = centre(s0, side);
        let (p0, p1) = (at(&a, s0 - car_length / 2.0, c, 0.3), at(&a, s0 + car_length / 2.0, c, 0.3));
        match p.car_floor {
            CarFloor::Stepped => merge(&mut mesh, beam([p0[0], p0[1], p0[2] + car_height], [p1[0], p1[1], p1[2] + car_height], car_width, car_height)),
            CarFloor::Level => {
                let z = p0[2].min(p1[2]);
                merge(&mut mesh, beam([p0[0], p0[1], z + car_height], [p1[0], p1[1], z + car_height], car_width, car_height));
            }
        }
    }
    // The drive house beyond the top of the track.
    let end = a.end_station();
    let (q, d, z, _) = a.at(end);
    let (d0, d1) = ([q[0] + d[0] * 1.0, q[1] + d[1] * 1.0], [q[0] + d[0] * 7.0, q[1] + d[1] * 7.0]);
    merge(&mut mesh, beam([d0[0], d0[1], z + 4.0], [d1[0], d1[1], z + 4.0], (car_width + 2.0).max(5.0), 4.0));
    let tris: Vec<[u32; 3]> = mesh.1.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
    let name = format!("Funicular, {:.0} m rising {:.0} m", length, rise);
    let ty = crate::mesh_objects::create_generated_type(txn, &name, crate::mesh_objects::Category::Transport, &mesh.0, &tris, [0.62, 0.30, 0.22, 1.0], Vec::new(), Some("USERDEFINED"))?;
    txn.modify(ty, |e| {
        e.params.insert("@ElementType".into(), "FUNICULAR".into());
    })?;
    let level = crate::alignments::recipe(txn.store().get(aid).expect("resolved")).expect("an alignment").level;
    let level_id = crate::levels::list(txn.store()).into_iter().find(|l| l.guid == level).map(|l| l.id).ok_or_else(|| bad("vt.invalid", "the alignment's level is gone"))?;
    let id = crate::mesh_objects::place(
        txn,
        crate::mesh_objects::PlaceParams { type_id: ElementRef::Id(ty), point: [0.0, 0.0], level: ElementRef::Id(level_id), offset: 0.0, rotation: 0.0, mirror: false, scale: 1.0 },
    )?;
    let data = serde_json::to_value(&f).expect("serialises");
    let props = properties(&f);
    txn.modify(id, |e| {
        e.name = Some("Funicular".into());
        e.params.insert("@ObjectType".into(), "Funicular".into());
        e.params.insert(DATA_KEY.into(), data);
        e.params.extend(props);
    })?;
    Ok(Created { funicular: id, data: f })
}

/// Its property set: `Riviera_Funicular`.
pub fn properties(f: &Funicular) -> Vec<(String, Value)> {
    let len = |v: f64| measure("IfcPositiveLengthMeasure", v);
    vec![
        ("Pset_TransportElementCommon.CapacityPeople".into(), measure("IfcCountMeasure", f.car_capacity)),
        ("Riviera_Funicular.Gauge".into(), len(f.gauge)),
        ("Riviera_Funicular.Layout".into(), measure("IfcLabel", serde_json::to_value(f.layout).expect("serialises"))),
        ("Riviera_Funicular.TrackLength".into(), len(f.length)),
        ("Riviera_Funicular.Rise".into(), len(f.rise)),
        ("Riviera_Funicular.MaxGrade".into(), measure("IfcPositiveRatioMeasure", f.max_grade / 100.0)),
        ("Riviera_Funicular.Speed".into(), measure("IfcLinearVelocityMeasure", f.speed)),
        ("Riviera_Funicular.TripTime".into(), measure("IfcTimeMeasure", f.trip)),
        ("Riviera_Funicular.LineCapacity".into(), measure("IfcReal", f.capacity)),
        ("Riviera_Funicular.EvacuationWalkway".into(), measure("IfcBoolean", f.walkway)),
    ]
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckParams {
    /// Persons an hour each way it must carry.
    #[serde(default)]
    pub demand: Option<f64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Info {
    pub funicular: ElementId,
    #[serde(flatten)]
    pub data: Funicular,
}

/// `funicular.list`.
pub(crate) fn list(store: &Store, _: Option<crate::inclined_elevators::NoParams>) -> Result<Vec<Info>> {
    Ok(funiculars(store).into_iter().map(|(id, data)| Info { funicular: id, data }).collect())
}

/// A problem found.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Finding {
    pub funicular: ElementId,
    /// `error` or `note`.
    pub severity: String,
    pub rule: String,
    pub message: String,
}

/// `funicular.check`: the gauge, the capacity against a demand, and the evacuation walkway.
pub(crate) fn check(store: &Store, p: Option<CheckParams>) -> Result<Vec<Finding>> {
    let p = p.unwrap_or_default();
    let mut out = Vec::new();
    for (id, f) in funiculars(store) {
        let mut add = |severity: &str, rule: &str, message: String| out.push(Finding { funicular: id, severity: severity.into(), rule: rule.into(), message });
        if !(0.762 - 1e-9..=1.524 + 1e-9).contains(&f.gauge) {
            add("note", "gauge", format!("a {:.0} mm gauge is outside the 762–1524 mm of the reference lines", f.gauge * 1000.0));
        }
        if let Some(d) = p.demand.filter(|d| *d > f.capacity + 1e-9) {
            add("error", "capacity", format!("it carries {:.0} persons an hour each way; {d:.0} are needed (trip {:.0} s and {:.0} s at each station)", f.capacity, f.trip, f.dwell));
        }
        if !f.walkway {
            add("note", "evacuation", "no walkway beside the track to evacuate stranded cars along (Angels Flight added one in 2017)".into());
        }
    }
    Ok(out)
}
