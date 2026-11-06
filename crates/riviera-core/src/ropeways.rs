// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Aerial ropeways (ARC-VT-027): a monocable detachable gondola (MGD) line along an alignment of
//! straight legs, its corners angle stations, with towers spaced along each leg over the ground (the
//! site's toposolids, or the level), ropes sagging between them, and stations at the ends. Checks:
//! cabin clearance to the ground in every span (RM2: 4 m, 2.5 m on spans under 200 m), the highest
//! overflight, and line capacity; the rope's turn over each tower is reported. One loaded sag per
//! span, a share of the span; RM2's change of slope between the empty and loaded cases (≤ 0.15
//! rad) needs the rope's tension and loads, and comes with the sag cases as compute jobs.
//! Research/16 R3 (E-100).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::alignments::{Alignment, Piece};
use crate::elevators::measure;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::inclined_elevators::{beam, merge};
use crate::store::Store;
use crate::txn::Transaction;

type V2 = [f64; 2];
type V3 = [f64; 3];

pub const DATA_KEY: &str = "rxd:ropeway";
/// MGD defaults: gauge (m), cabin persons, line speed (m/s), headway (s), tower height and largest
/// spacing (m, Pinellas), rope height at a station above its ground (m), loaded sag as a share of
/// the span, and the cabin below the rope (hanger and cabin, m).
pub const GAUGE: f64 = 6.4;
pub const CABIN: u32 = 10;
pub const SPEED: f64 = 6.0;
pub const HEADWAY: f64 = 15.0;
pub const TOWER_HEIGHT: f64 = 18.3;
pub const TOWER_SPACING: f64 = 152.0;
pub const STATION_HEIGHT: f64 = 6.0;
pub const SAG: f64 = 0.04;
pub const CABIN_DROP: f64 = 3.5;
/// RM2 safety distances to the ground (m).
pub const CLEARANCE: f64 = 4.0;
pub const SHORT_SPAN_CLEARANCE: f64 = 2.5;
pub const SHORT_SPAN: f64 = 200.0;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// What holds the rope: a tower, or a station (terminal or angle).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SupportKind {
    Terminal,
    Angle,
    Tower,
}

/// A support along the line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Support {
    pub kind: SupportKind,
    /// Chainage, plan point, ground height and rope height (absolute, m).
    pub station: f64,
    pub point: V2,
    pub ground: f64,
    pub rope: f64,
}

/// A span between two supports, as worked out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Span {
    /// Its supports (indices), length in plan (m), loaded sag (m), the least cabin clearance to
    /// the ground and the clearance required (m), and the highest cabin above the ground (m).
    pub from: usize,
    pub to: usize,
    pub length: f64,
    pub sag: f64,
    pub clearance: f64,
    pub required: f64,
    pub highest: f64,
}

/// A ropeway's data, kept on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Ropeway {
    pub alignment: String,
    pub gauge: f64,
    pub cabin_capacity: u32,
    pub speed: f64,
    pub headway: f64,
    pub tower_height: f64,
    pub tower_spacing: f64,
    pub supports: Vec<Support>,
    pub spans: Vec<Span>,
    /// The loaded rope's turn over each tower (rad, downward negative), the line's length (m), its
    /// capacity each way (persons an hour) and its trip time (s).
    pub turns: Vec<f64>,
    pub length: f64,
    pub capacity: f64,
    pub trip: f64,
}

pub fn ropeway(e: &crate::element::Element) -> Option<Ropeway> {
    serde_json::from_value(e.param(DATA_KEY)?.clone()).ok()
}

pub fn ropeways(store: &Store) -> Vec<(ElementId, Ropeway)> {
    store.iter().filter_map(|e| Some((e.id(), ropeway(e)?))).collect()
}

/// The ground at a plan point: the highest current toposolid there, or the level.
pub fn ground(store: &Store, p: V2, level: f64) -> f64 {
    store
        .iter()
        .filter(|e| crate::toposolids::recipe(e).is_some() && !crate::toposolids::superseded(store, e))
        .filter_map(|e| crate::toposolids::height_at(store, e, p))
        .fold(None, |m: Option<f64>, h| Some(m.map_or(h, |m| m.max(h))))
        .unwrap_or(level)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub alignment_id: ElementRef,
    /// Between the two directions' ropes (m; default 6.4).
    #[serde(default)]
    pub gauge: Option<f64>,
    /// Persons a cabin (default 10), line speed (default 6 m/s), seconds between cabins (default 15).
    #[serde(default)]
    pub cabin_capacity: Option<u32>,
    #[serde(default)]
    pub speed: Option<f64>,
    #[serde(default)]
    pub headway: Option<f64>,
    /// Tower height above the ground (default 18.3 m) and the largest spacing (default 152 m).
    #[serde(default)]
    pub tower_height: Option<f64>,
    #[serde(default)]
    pub tower_spacing: Option<f64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Created {
    pub ropeway: ElementId,
    #[serde(flatten)]
    pub data: Ropeway,
}

/// The rope's height at `x` along a span from `a` to `b` (`l` long in plan) sagging `f` at mid-span.
fn rope_at(za: f64, zb: f64, l: f64, f: f64, x: f64) -> f64 {
    za + (zb - za) * x / l - 4.0 * f * x * (l - x) / (l * l)
}

/// `ropeway.line`.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<Created> {
    let store = txn.store();
    let aid = store.resolve(&p.alignment_id).ok_or_else(|| bad("element.not_found", "no such alignment"))?;
    let rec = store.get(aid).and_then(crate::alignments::recipe).ok_or_else(|| bad("vt.invalid", format!("element {aid} is not an alignment")))?;
    let guid = store.get(aid).expect("resolved").guid().to_string();
    let (level_id, z0) = crate::levels::list(store).into_iter().find(|l| l.guid == rec.level).map(|l| (l.id, l.elevation)).ok_or_else(|| bad("vt.invalid", "the alignment's level is gone"))?;
    let a = Alignment::of(rec)?;
    if a.pieces.iter().any(|p| !matches!(p, Piece::Line { .. })) {
        return Err(bad("vt.invalid", "a ropeway runs in straight legs: give its alignment corners without curves (each corner becomes an angle station)"));
    }
    let gauge = p.gauge.unwrap_or(GAUGE);
    let cabin_capacity = p.cabin_capacity.unwrap_or(CABIN);
    let speed = p.speed.unwrap_or(SPEED);
    let headway = p.headway.unwrap_or(HEADWAY);
    let tower_height = p.tower_height.unwrap_or(TOWER_HEIGHT);
    let tower_spacing = p.tower_spacing.unwrap_or(TOWER_SPACING);
    let ok = [gauge, speed, headway, tower_height, tower_spacing].iter().all(|v| v.is_finite() && *v > 0.0) && tower_spacing >= 20.0;
    if !ok {
        return Err(bad("vt.invalid", "gauge, speed, headway and tower height must be above 0, and towers at least 20 m apart"));
    }
    // Supports: a terminal at each end, an angle station at each corner, towers evenly between.
    let mut supports = Vec::new();
    let mut s0 = a.recipe.start_station;
    let legs = a.pieces.len();
    for (k, piece) in a.pieces.iter().enumerate() {
        let Piece::Line { length, .. } = piece else { unreachable!("no arcs") };
        let kind = if k == 0 { SupportKind::Terminal } else { SupportKind::Angle };
        let n = (length / tower_spacing).ceil().max(1.0) as usize;
        for i in 0..n {
            let s = s0 + length * i as f64 / n as f64;
            supports.push((if i == 0 { kind } else { SupportKind::Tower }, s));
        }
        s0 += length;
        if k + 1 == legs {
            supports.push((SupportKind::Terminal, s0));
        }
    }
    let supports: Vec<Support> = supports
        .into_iter()
        .map(|(kind, s)| {
            let (q, _, _, _) = a.at(s);
            let g = ground(store, q, z0);
            let rope = g + if kind == SupportKind::Tower { tower_height } else { STATION_HEIGHT };
            Support { kind, station: s, point: q, ground: g, rope }
        })
        .collect();
    // Spans: the loaded sag, the least cabin clearance and the highest cabin over the ground.
    let mut spans = Vec::new();
    for (i, w) in supports.windows(2).enumerate() {
        let (sa, sb) = (&w[0], &w[1]);
        let l = sb.station - sa.station;
        let f = SAG * l;
        let n = (l.ceil() as usize).max(2);
        let (mut least, mut highest) = (f64::INFINITY, f64::NEG_INFINITY);
        for k in 0..=n {
            let x = l * k as f64 / n as f64;
            let (q, _, _, _) = a.at(sa.station + x);
            let cabin = rope_at(sa.rope, sb.rope, l, f, x) - CABIN_DROP;
            let above = cabin - ground(store, q, z0);
            // Near the stations cabins come down to board.
            let free = (sa.kind == SupportKind::Tower || x > 30.0) && (sb.kind == SupportKind::Tower || l - x > 30.0);
            if free {
                least = least.min(above);
            }
            highest = highest.max(above);
        }
        let required = if l < SHORT_SPAN { SHORT_SPAN_CLEARANCE } else { CLEARANCE };
        spans.push(Span { from: i, to: i + 1, length: l, sag: f, clearance: if least.is_finite() { least } else { highest }, required, highest });
    }
    // Over each tower: the slope leaving it less the slope arriving (end slopes of the loaded parabolas).
    let turns: Vec<f64> = (1..supports.len().saturating_sub(1))
        .filter(|i| supports[*i].kind == SupportKind::Tower)
        .map(|i| {
            let (sp_in, sp_out) = (&spans[i - 1], &spans[i]);
            let into = (supports[i].rope - supports[i - 1].rope) / sp_in.length + 4.0 * sp_in.sag / sp_in.length;
            let out = (supports[i + 1].rope - supports[i].rope) / sp_out.length - 4.0 * sp_out.sag / sp_out.length;
            out.atan() - into.atan()
        })
        .collect();
    let length = a.length;
    let capacity = 3600.0 * f64::from(cabin_capacity) / headway;
    let trip = length / speed;
    let r = Ropeway { alignment: guid, gauge, cabin_capacity, speed, headway, tower_height, tower_spacing, supports: supports.clone(), spans, turns, length, capacity, trip };
    // The object, relative to the level: towers, stations, and the two directions' ropes.
    let mut mesh = (Vec::new(), Vec::new());
    let dir = |s: f64| a.at(s).1;
    // A footprint's highest ground (relative to the level), so nothing is buried in the slope:
    // foundations below it come later.
    let seat = |c: V2, d: V2, half_l: f64, half_w: f64| -> f64 {
        let n = [-d[1], d[0]];
        [(-1.0, -1.0), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0), (0.0, 0.0)]
            .iter()
            .map(|(a, b)| ground(store, [c[0] + d[0] * half_l * a + n[0] * half_w * b, c[1] + d[1] * half_l * a + n[1] * half_w * b], z0))
            .fold(f64::NEG_INFINITY, f64::max)
            - z0
    };
    for sp in &supports {
        let d = dir(sp.station);
        let rz = sp.rope - z0;
        match sp.kind {
            SupportKind::Tower => {
                let gz = seat(sp.point, d, 0.915, 0.915);
                let half = 0.915;
                let p0 = [sp.point[0] - d[0] * half, sp.point[1] - d[1] * half, rz];
                let p1 = [sp.point[0] + d[0] * half, sp.point[1] + d[1] * half, rz];
                merge(&mut mesh, beam(p0, p1, 1.83, rz - gz));
                // The crossarm carrying both directions.
                merge(&mut mesh, beam([sp.point[0] - d[0] * 0.3, sp.point[1] - d[1] * 0.3, rz + 0.6], [sp.point[0] + d[0] * 0.3, sp.point[1] + d[1] * 0.3, rz + 0.6], gauge + 1.0, 0.6));
            }
            SupportKind::Terminal | SupportKind::Angle => {
                let gz = seat(sp.point, d, 15.0, 10.0);
                let half = 15.0;
                let p0 = [sp.point[0] - d[0] * half, sp.point[1] - d[1] * half, gz + 10.0];
                let p1 = [sp.point[0] + d[0] * half, sp.point[1] + d[1] * half, gz + 10.0];
                merge(&mut mesh, beam(p0, p1, 20.0, 10.0));
            }
        }
    }
    for (i, w) in supports.windows(2).enumerate() {
        let (sa, sb) = (&w[0], &w[1]);
        let l = sb.station - sa.station;
        let f = r.spans[i].sag;
        let n = ((l / 10.0).ceil() as usize).max(2);
        for side in [-1.0, 1.0] {
            let pts: Vec<V3> = (0..=n)
                .map(|k| {
                    let x = l * k as f64 / n as f64;
                    let (q, d, _, _) = a.at(sa.station + x);
                    let off = side * gauge / 2.0;
                    [q[0] - d[1] * off, q[1] + d[0] * off, rope_at(sa.rope, sb.rope, l, f, x) - z0]
                })
                .collect();
            for seg in pts.windows(2) {
                merge(&mut mesh, beam(seg[0], seg[1], 0.06, 0.06));
            }
        }
    }
    let tris: Vec<[u32; 3]> = mesh.1.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
    let towers = supports.iter().filter(|s| s.kind == SupportKind::Tower).count();
    let name = format!("Ropeway, {:.0} m, {towers} towers", length);
    let ty = crate::mesh_objects::create_generated_type(txn, &name, crate::mesh_objects::Category::Transport, &mesh.0, &tris, [0.35, 0.37, 0.40, 1.0], Vec::new(), Some("USERDEFINED"))?;
    txn.modify(ty, |e| {
        e.params.insert("@ElementType".into(), "ROPEWAY".into());
    })?;
    let id = crate::mesh_objects::place(
        txn,
        crate::mesh_objects::PlaceParams { type_id: ElementRef::Id(ty), point: [0.0, 0.0], level: ElementRef::Id(level_id), offset: 0.0, rotation: 0.0, mirror: false, scale: 1.0 },
    )?;
    let data = serde_json::to_value(&r).expect("serialises");
    let props = vec![
        ("Riviera_Ropeway.SystemType".to_owned(), measure("IfcLabel", "MGD")),
        ("Riviera_Ropeway.LineSpeed".to_owned(), measure("IfcLinearVelocityMeasure", speed)),
        ("Riviera_Ropeway.Capacity".to_owned(), measure("IfcReal", capacity)),
        ("Riviera_Ropeway.Gauge".to_owned(), measure("IfcPositiveLengthMeasure", gauge)),
        ("Riviera_Ropeway.Towers".to_owned(), measure("IfcCountMeasure", towers)),
    ];
    txn.modify(id, |e| {
        e.name = Some("Ropeway".into());
        e.params.insert("@ObjectType".into(), "Ropeway".into());
        e.params.insert(DATA_KEY.into(), data);
        e.params.extend(props);
    })?;
    Ok(Created { ropeway: id, data: r })
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckParams {
    /// Persons an hour each way it must carry.
    #[serde(default)]
    pub demand: Option<f64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Finding {
    pub ropeway: ElementId,
    /// `error`, `warning` or `note`.
    pub severity: String,
    pub rule: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Info {
    pub ropeway: ElementId,
    #[serde(flatten)]
    pub data: Ropeway,
}

/// `ropeway.list`.
pub(crate) fn list(store: &Store, _: Option<crate::inclined_elevators::NoParams>) -> Result<Vec<Info>> {
    Ok(ropeways(store).into_iter().map(|(id, data)| Info { ropeway: id, data }).collect())
}

/// `ropeway.check`: cabin clearance in every span, the highest overflight, and capacity against a
/// demand.
pub(crate) fn check(store: &Store, p: Option<CheckParams>) -> Result<Vec<Finding>> {
    let p = p.unwrap_or_default();
    let mut out = Vec::new();
    for (id, r) in ropeways(store) {
        let mut add = |severity: &str, rule: &str, message: String| out.push(Finding { ropeway: id, severity: severity.into(), rule: rule.into(), message });
        for (k, sp) in r.spans.iter().enumerate() {
            if sp.clearance + 1e-9 < sp.required {
                add("error", "RM2 clearance", format!("span {} ({:.0} m): the loaded cabin comes within {:.1} m of the ground; {:.1} m are needed", k + 1, sp.length, sp.clearance, sp.required));
            }
        }
        let highest = r.spans.iter().map(|s| s.highest).fold(0.0, f64::max);
        add("note", "overflight", format!("the cabins pass at most {highest:.1} m above the ground: evacuation from that height needs a rescue concept"));
        if let Some(d) = p.demand.filter(|d| *d > r.capacity + 1e-9) {
            add("error", "capacity", format!("{:.0} persons an hour each way for {d:.0} needed: larger cabins or a shorter headway", r.capacity));
        }
    }
    Ok(out)
}
