// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Escalators and moving walks (ARC-VT-010, ARC-VT-011): sized from the vertical-transport rule
//! packs ([`crate::elevators`]), made as one custom object (`IfcTransportElement` ESCALATOR or
//! MOVINGWALKWAY) shaped from its rise, with the floor openings its pits and headroom need, its
//! headroom and landing zones, and the checks.
//!
//! Its frame: `point` is the middle of the truss's lower end, on the lower level; local x runs up
//! the escalator (along the walk), local y across it; `rotation` turns that frame
//! counter-clockwise. The step line is level from the truss end to the lower working point, rises
//! at the inclination to the upper working point, and is level again to the upper end.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::clearances::{Clearance, ClearanceKind};
use crate::elevators::{Confidence, Layer, Pack, active, by_guid, measure};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::store::Store;
use crate::txn::Transaction;

type V2 = [f64; 2];

/// An escalator's or walk's data, on it.
pub const DATA_KEY: &str = "rxd:escalator";
/// On each floor opening an escalator makes: its GlobalId.
pub const MEMBER_KEY: &str = "rxd:escalator_of";

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

// ---- Pack blocks ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FlatSteps {
    pub min: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rise_over: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_over: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Measure {
    /// From the handrails' outer edges.
    Outer,
    /// From the handrails' centrelines.
    Centreline,
}

/// Rules for escalators and moving walks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Rules {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub max_inclination: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steep_inclination: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steep_max_rise: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steep_max_speed: Option<f64>,
    pub max_speed: f64,
    pub flat_steps: Vec<FlatSteps>,
    pub headroom: f64,
    pub landing_measure: Measure,
    pub landing_width_add: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub landing_depth: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub landing_depth_factor: Option<f64>,
    pub side_clearance: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_width_min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_width_max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub walk_max_inclination: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub walk_max_speed: Option<f64>,
    pub source: String,
    pub confidence: Confidence,
}

impl Rules {
    /// The least flat steps for a rise and speed.
    pub fn flat_steps_for(&self, rise: f64, speed: f64) -> u32 {
        self.flat_steps
            .iter()
            .filter(|f| f.rise_over.is_none_or(|r| rise > r + 1e-9) && f.speed_over.is_none_or(|v| speed > v + 1e-9))
            .map(|f| f.min)
            .max()
            .unwrap_or(2)
    }

    /// The largest inclination allowed at a rise and speed.
    pub fn inclination_for(&self, rise: f64, speed: f64) -> f64 {
        match self.steep_inclination {
            Some(s) if self.steep_max_rise.is_none_or(|r| rise <= r + 1e-9) && self.steep_max_speed.is_none_or(|v| speed <= v + 1e-9) => s,
            _ => self.max_inclination,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Ends {
    pub flat: u32,
    pub lower: f64,
    pub upper: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WalkEnds {
    pub lower: f64,
    pub upper: f64,
}

/// Generic, maker-style sizes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Sizing {
    pub handrail_centres_add: f64,
    pub handrail_outer_add: f64,
    pub truss_add: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transit_truss_add: Option<f64>,
    pub pit_add: f64,
    pub truss_depth: f64,
    pub pit_depth: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transit_pit_depth: Option<f64>,
    pub ends: Vec<Ends>,
    pub walk_ends: WalkEnds,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub walk_pit_depth: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub walk_deck: Option<f64>,
    pub balustrade: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub support_span: Option<f64>,
    /// Step width, speed and practical capacity (persons per hour).
    pub capacity: Vec<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reaction_per_m: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reaction_upper: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reaction_lower: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_per_m: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_base: Option<f64>,
    pub source: String,
    pub confidence: Confidence,
}

fn rules(packs: &[Pack]) -> Option<(&Pack, &Rules)> {
    packs.iter().find_map(|p| p.escalator.as_ref().map(|r| (p, r)))
}

fn sizing(packs: &[Pack]) -> Result<(&Pack, &Sizing)> {
    packs
        .iter()
        .find_map(|p| p.escalator_sizing.as_ref().map(|s| (p, s)))
        .ok_or_else(|| bad("vt.no_sizing", "no rule pack in use gives escalator sizes (add practice_us or practice_en)"))
}

// ---- Sizing ----

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    Escalator,
    MovingWalk,
}

/// Duty: transit units are wider and deeper (heavy-duty public transport trusses).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Duty {
    #[default]
    Commercial,
    Transit,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SizeParams {
    #[serde(default)]
    pub kind: Kind,
    /// The rise between the levels (m); 0 for a horizontal walk.
    #[schemars(extend("x-riviera-data-type" = "length"))]
    pub rise: Option<f64>,
    /// A horizontal walk's length between its working points (m).
    #[schemars(extend("x-riviera-data-type" = "length"))]
    pub length: Option<f64>,
    /// Step or pallet width (m); default 1.0.
    #[schemars(extend("x-riviera-data-type" = "length"))]
    pub step_width: Option<f64>,
    /// Degrees; default 30 for escalators, 12 for inclined walks.
    pub inclination: Option<f64>,
    /// m/s; default 0.5.
    pub speed: Option<f64>,
    /// Flat steps at each landing; default the least the rules ask.
    pub flat_steps: Option<u32>,
    #[serde(default)]
    pub duty: Duty,
    /// A walk set in a pit, its pallets flush with the floor (default: on the floor).
    #[serde(default)]
    pub pit: bool,
}

/// An escalator's or walk's sizes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Sized {
    pub kind: Kind,
    pub duty: Duty,
    pub rise: f64,
    /// Degrees.
    pub inclination: f64,
    pub step_width: f64,
    pub speed: f64,
    pub flat_steps: u32,
    /// Between the handrails' centrelines and their outer edges.
    pub handrail_centres: f64,
    pub handrail_outer: f64,
    pub truss_width: f64,
    pub truss_depth: f64,
    /// Depth of the pit at each end below the floor (0 for a walk on the floor).
    pub pit_depth: f64,
    /// Lengths in plan: truss end to the lower working point, the incline, and the upper
    /// working point to the truss end.
    pub lower_end: f64,
    pub run: f64,
    pub upper_end: f64,
    /// A walk on the floor: its pallets this far above it.
    #[serde(default)]
    pub deck: f64,
    pub balustrade: f64,
    /// Practical capacity, persons per hour.
    pub capacity: f64,
    /// Planning estimates: motor power (kW) and the support reactions at each end (kN).
    pub motor_kw: f64,
    pub reactions: V2,
    /// Its span between the end supports needs an intermediate support.
    pub intermediate_support: bool,
    pub sizing_pack: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

impl Sized {
    pub fn length(&self) -> f64 {
        self.lower_end + self.run + self.upper_end
    }

    /// The step line's height at `x` along the truss.
    pub fn step_z(&self, x: f64) -> f64 {
        self.deck + ((x - self.lower_end) / self.run.max(1e-9)).clamp(0.0, 1.0) * self.rise
    }
}

/// Sizes an escalator or walk from the packs in use.
pub fn size(packs: &[Pack], p: &SizeParams) -> Result<Sized> {
    let (pack, sz) = sizing(packs)?;
    let r = rules(packs).map(|(_, r)| r);
    let mut notes = Vec::new();
    let rise = p.rise.unwrap_or(0.0).max(0.0);
    let speed = p.speed.unwrap_or(0.5);
    let step = p.step_width.unwrap_or(1.0);
    if speed <= 0.0 || step <= 0.0 {
        return Err(bad("vt.params", "the speed and step width must be positive"));
    }
    let horizontal = p.kind == Kind::MovingWalk && rise < 1e-6;
    let inclination = if horizontal {
        0.0
    } else {
        p.inclination.unwrap_or(match p.kind {
            Kind::Escalator => r.map_or(30.0, |r| r.max_inclination.min(30.0)),
            Kind::MovingWalk => r.and_then(|r| r.walk_max_inclination).unwrap_or(12.0).min(12.0),
        })
    };
    if !horizontal && !(1.0..=45.0).contains(&inclination) {
        return Err(bad("vt.params", "the inclination must be between 1° and 45°"));
    }
    let run = if horizontal {
        p.length.ok_or_else(|| bad("vt.params", "a horizontal walk needs its length"))?
    } else {
        rise / inclination.to_radians().tan()
    };
    if run <= 0.0 {
        return Err(bad("vt.params", "an escalator needs a rise (two levels)"));
    }
    let flat = match p.kind {
        Kind::Escalator => p.flat_steps.unwrap_or_else(|| r.map_or(2, |r| r.flat_steps_for(rise, speed))),
        Kind::MovingWalk => 0,
    };
    let (lower_end, upper_end) = match p.kind {
        Kind::Escalator => {
            let e = sz.ends.iter().filter(|e| e.flat <= flat.max(2)).max_by_key(|e| e.flat).or(sz.ends.first()).expect("schema: ends not empty");
            if e.flat != flat {
                notes.push(format!("{} gives ends for {} flat steps; used for {flat}", pack.id, e.flat));
            }
            (e.lower, e.upper)
        }
        Kind::MovingWalk => (sz.walk_ends.lower, sz.walk_ends.upper),
    };
    let transit = p.duty == Duty::Transit;
    let truss_width = step + if transit { sz.transit_truss_add.unwrap_or(sz.truss_add) } else { sz.truss_add };
    let (truss_depth, pit_depth, deck) = match (p.kind, horizontal, p.pit) {
        (Kind::MovingWalk, true, false) => {
            let d = sz.walk_deck.unwrap_or(0.15);
            (d, 0.0, d)
        }
        (Kind::MovingWalk, true, true) => {
            let d = sz.walk_pit_depth.unwrap_or(0.38);
            (d - 0.02, d, 0.0)
        }
        _ => (sz.truss_depth, if transit { sz.transit_pit_depth.unwrap_or(sz.pit_depth) } else { sz.pit_depth }, 0.0),
    };
    // Practical capacity: the table's nearest width at or below, then its nearest speed.
    let widths: Vec<f64> = sz.capacity.iter().map(|c| c[0]).collect();
    let w_row = widths.iter().copied().filter(|w| *w <= step + 1e-6).fold(f64::NAN, f64::max);
    let w_row = if w_row.is_nan() { widths.iter().copied().fold(f64::MAX, f64::min) } else { w_row };
    let capacity = sz
        .capacity
        .iter()
        .filter(|c| (c[0] - w_row).abs() < 1e-6)
        .min_by(|a, b| (a[1] - speed).abs().total_cmp(&(b[1] - speed).abs()))
        .map_or(0.0, |c| c[2]);
    let length = lower_end + run + upper_end;
    let k = sz.reaction_per_m.unwrap_or(4.75);
    let reactions = [k * length / 2.0 + sz.reaction_lower.unwrap_or(0.0), k * length / 2.0 + sz.reaction_upper.unwrap_or(0.0)];
    let motor_kw = sz.power_base.unwrap_or(3.0) + sz.power_per_m.unwrap_or(1.5) * rise * (step / 1.0) * (speed / 0.5);
    let intermediate_support = sz.support_span.is_some_and(|s| length > s + 1e-9);
    if intermediate_support {
        notes.push(format!("its {length:.1} m span between end supports needs an intermediate support (beyond {:.1} m)", sz.support_span.unwrap_or_default()));
    }
    Ok(Sized {
        kind: p.kind,
        duty: p.duty,
        rise,
        inclination,
        step_width: step,
        speed,
        flat_steps: flat,
        handrail_centres: step + sz.handrail_centres_add,
        handrail_outer: step + sz.handrail_outer_add,
        truss_width,
        truss_depth,
        pit_depth,
        lower_end,
        run,
        upper_end,
        deck,
        balustrade: sz.balustrade,
        capacity,
        motor_kw,
        reactions,
        intermediate_support,
        sizing_pack: pack.id.clone(),
        notes,
    })
}

/// `escalator.size`.
pub(crate) fn size_command(store: &Store, p: SizeParams) -> Result<Sized> {
    size(&active(store), &p)
}

// ---- The object ----

/// An escalator's or walk's data, kept on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Escalator {
    #[serde(flatten)]
    pub sized: Sized,
    pub point: V2,
    pub rotation: f64,
    /// The lower and upper levels (GlobalIds; the same for a horizontal walk).
    pub levels: [String; 2],
    /// Its floor openings (GlobalIds).
    #[serde(default)]
    pub openings: Vec<String>,
}

impl Escalator {
    pub fn at(&self, x: f64, y: f64) -> V2 {
        let (s, c) = self.rotation.sin_cos();
        [self.point[0] + x * c - y * s, self.point[1] + x * s + y * c]
    }
}

pub fn escalator(e: &crate::element::Element) -> Option<Escalator> {
    serde_json::from_value(e.param(DATA_KEY)?.clone()).ok()
}

pub fn escalators(store: &Store) -> Vec<(ElementId, Escalator)> {
    store.iter().filter_map(|e| Some((e.id(), escalator(e)?))).collect()
}

/// A profile in the (x, z) plane, extruded across y from `y0` to `y1`.
fn profile_mesh(profile: &[V2], y0: f64, y1: f64) -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
    let (p, i) = crate::bodies::extrude(profile, &[], y0, y1);
    // The extrusion's z is our y: swap, which mirrors, so turn each triangle round.
    let points = p.iter().map(|q| [q[0], q[2], q[1]]).collect();
    let tris = i.chunks(3).map(|t| [t[0], t[2], t[1]]).collect();
    (points, tris)
}

/// The step line's profile between `below` and `above` it.
fn band(s: &Sized, below: f64, above: f64) -> Vec<V2> {
    let (a, b, c) = (s.lower_end, s.lower_end + s.run, s.length());
    let (z0, z1) = (s.deck, s.deck + s.rise);
    vec![[0.0, z0 - below], [a, z0 - below], [b, z1 - below], [c, z1 - below], [c, z1 + above], [b, z1 + above], [a, z0 + above], [0.0, z0 + above]]
}

/// The truss and the two balustrades.
fn mesh(s: &Sized) -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
    let mut out = profile_mesh(&band(s, s.truss_depth, 0.0), -s.truss_width / 2.0, s.truss_width / 2.0);
    let glass = 0.03;
    for side in [-1.0, 1.0] {
        let edge = side * s.handrail_outer / 2.0;
        let (p, t) = profile_mesh(&band(s, -0.001, s.balustrade), edge.min(edge - side * glass), edge.max(edge - side * glass));
        let base = out.0.len() as u32;
        out.0.extend(p);
        out.1.extend(t.into_iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
    }
    out
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// The middle of the truss's lower end (metres).
    pub point: V2,
    /// Turns it counter-clockwise; at 0 it rises toward +x.
    #[serde(default)]
    #[schemars(extend("x-riviera-data-type" = "angle"))]
    pub rotation: f64,
    /// The lower level (a horizontal walk's level).
    pub base_level: ElementRef,
    /// The upper level; omitted for a horizontal walk.
    pub top_level: Option<ElementRef>,
    #[serde(flatten)]
    pub size: SizeParams,
}

/// What `escalator.create` made.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Created {
    pub escalator: ElementId,
    pub openings: Vec<ElementId>,
    #[serde(flatten)]
    pub sized: Sized,
}

/// Makes an escalator or moving walk (`escalator.create`).
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<Created> {
    let store = txn.store().clone();
    let packs = active(&store);
    let level = |r: &ElementRef| -> Result<crate::levels::LevelInfo> {
        let id = store.resolve(r).ok_or_else(|| bad("vt.no_level", "no such level"))?;
        crate::levels::list(&store).into_iter().find(|l| l.id == id).ok_or_else(|| bad("vt.no_level", format!("element {id} is not a level")))
    };
    let base = level(&p.base_level)?;
    let top = match &p.top_level {
        Some(t) => level(t)?,
        None if p.size.kind == Kind::MovingWalk => base.clone(),
        None => return Err(bad("vt.params", "an escalator needs its upper level")),
    };
    let rise = top.elevation - base.elevation;
    if rise < -1e-6 {
        return Err(bad("vt.params", "the upper level must be above the lower"));
    }
    let sized = size(&packs, &SizeParams { rise: Some(rise), ..p.size.clone() })?;
    let mut el = Escalator { sized: sized.clone(), point: p.point, rotation: p.rotation, levels: [base.guid.clone(), top.guid.clone()], openings: Vec::new() };

    // The object: a type of its own shape.
    let (pts, tris) = mesh(&sized);
    let label = match sized.kind {
        Kind::Escalator => "Escalator",
        Kind::MovingWalk => "Moving Walk",
    };
    let name = format!("{label} {:.0} mm, {:.0}°, rise {:.0} mm, {:.0} mm long", sized.step_width * 1000.0, sized.inclination, sized.rise * 1000.0, sized.length() * 1000.0);
    let predefined = match sized.kind {
        Kind::Escalator => "ESCALATOR",
        Kind::MovingWalk => "MOVINGWALKWAY",
    };
    let existing = txn.store().iter().find(|e| e.name.as_deref() == Some(name.as_str()) && crate::mesh_objects::mesh_object_type(e).is_some()).map(crate::element::Element::id);
    let ty = match existing {
        Some(t) => t,
        None => crate::mesh_objects::create_generated_type(txn, &name, crate::mesh_objects::Category::Transport, &pts, &tris, [0.6, 0.63, 0.68, 1.0], Vec::new(), Some(predefined))?,
    };
    let id = crate::mesh_objects::place(
        txn,
        crate::mesh_objects::PlaceParams { type_id: ElementRef::Id(ty), point: p.point, level: ElementRef::Id(base.id), offset: 0.0, rotation: p.rotation, mirror: false, scale: 1.0 },
    )?;
    let guid = txn.store().get(id).expect("made").guid().to_string();

    // Floor openings: the lower pit, and over the upper pit and as far down the incline as the
    // headroom reaches up through the upper floor; a walk in a pit is open along its length.
    let headroom = rules(&packs).map_or(2.3, |(_, r)| r.headroom);
    let half = sized.truss_width / 2.0 + escalator_pit_add(&packs);
    let mut cuts: Vec<(V2, &crate::levels::LevelInfo)> = Vec::new();
    let length = sized.length();
    if sized.rise > 1e-6 {
        let tan = sized.inclination.to_radians().tan();
        cuts.push(([0.0, sized.lower_end + sized.truss_depth / tan], &base));
        // From where the headroom over the steps meets the underside of the thickest floor there.
        let slab = slab_thickness(&store, &top.guid);
        let from = (sized.lower_end + (sized.rise - headroom - slab) / tan).max(sized.lower_end);
        cuts.push(([from, length], &top));
    } else if sized.pit_depth > 0.0 {
        cuts.push(([0.0, length], &base));
    }
    let mut openings = Vec::new();
    for (x, l) in cuts {
        let ring = vec![el.at(x[0], -half), el.at(x[1], -half), el.at(x[1], half), el.at(x[0], half)];
        let depth = sized.truss_depth.max(sized.pit_depth).max(0.5) + 0.05;
        let shaft = crate::slab_openings::create_shaft(
            txn,
            crate::slab_openings::CreateShaftParams {
                loops: vec![ring],
                curves: Vec::new(),
                base_level: ElementRef::Id(l.id),
                base_offset: -depth,
                top_constraint: crate::walls::TopConstraint::Unconnected(depth + 0.01),
                top_offset: 0.0,
            },
        )?;
        let g = guid.clone();
        txn.modify(shaft, |e| {
            e.name = Some(format!("{label} Opening"));
            e.params.insert(MEMBER_KEY.into(), g.into());
        })?;
        el.openings.push(txn.store().get(shaft).expect("made").guid().to_string());
        openings.push(shaft);
    }

    let data = serde_json::to_value(&el).expect("serialises");
    let props = properties(&el);
    txn.modify(id, |e| {
        e.name = Some(label.into());
        e.params.insert(DATA_KEY.into(), data);
        e.params.extend(props);
    })?;
    Ok(Created { escalator: id, openings, sized })
}

/// The thickest floor on a level (0.3 m where there is none).
fn slab_thickness(store: &Store, level: &str) -> f64 {
    store
        .iter()
        .filter(|e| crate::floors::floor_recipe(e).is_some_and(|r| r.level == level))
        .filter_map(|e| {
            let t = e.param("@type")?.as_str().and_then(crate::id::GlobalId::parse)?;
            crate::floors::type_thickness(store.get(store.find_by_guid(&t)?)?)
        })
        .fold(None, |m: Option<f64>, t| Some(m.map_or(t, |m| m.max(t))))
        .unwrap_or(0.3)
}

fn escalator_pit_add(packs: &[Pack]) -> f64 {
    sizing(packs).map_or(0.06, |(_, s)| s.pit_add) / 2.0
}

/// Its property sets: capacity in persons per hour, and `Riviera_Escalator`.
pub fn properties(el: &Escalator) -> Vec<(String, Value)> {
    let s = &el.sized;
    let len = |v: f64| measure("IfcPositiveLengthMeasure", v);
    vec![
        ("Pset_TransportElementCommon.CapacityPeople".into(), measure("IfcCountMeasure", s.capacity.round() as i64)),
        ("Pset_TransportElementCommon.FireExit".into(), measure("IfcBoolean", false)),
        ("Riviera_Escalator.Rise".into(), measure("IfcLengthMeasure", s.rise)),
        ("Riviera_Escalator.Inclination".into(), measure("IfcPlaneAngleMeasure", s.inclination)),
        ("Riviera_Escalator.StepWidth".into(), len(s.step_width)),
        ("Riviera_Escalator.Speed".into(), measure("IfcLinearVelocityMeasure", s.speed)),
        ("Riviera_Escalator.FlatSteps".into(), measure("IfcCountMeasure", s.flat_steps)),
        ("Riviera_Escalator.Length".into(), len(s.length())),
        ("Riviera_Escalator.TrussWidth".into(), len(s.truss_width)),
        ("Riviera_Escalator.PitDepth".into(), measure("IfcLengthMeasure", s.pit_depth)),
        ("Riviera_Escalator.MotorPower".into(), measure("IfcPowerMeasure", s.motor_kw * 1000.0)),
        ("Riviera_Escalator.SupportReactionLower".into(), measure("IfcForceMeasure", s.reactions[0] * 1000.0)),
        ("Riviera_Escalator.SupportReactionUpper".into(), measure("IfcForceMeasure", s.reactions[1] * 1000.0)),
        ("Riviera_Escalator.SizingPack".into(), measure("IfcLabel", s.sizing_pack.clone())),
    ]
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EscalatorParams {
    pub escalator: ElementRef,
}

fn of(store: &Store, r: &ElementRef) -> Result<(ElementId, Escalator)> {
    let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such element"))?;
    let e = store.get(id).expect("resolved");
    let own = match e.param(MEMBER_KEY).and_then(Value::as_str) {
        Some(g) => by_guid(store, g).ok_or_else(|| bad("vt.not_escalator", "the escalator this belonged to is gone"))?,
        None => id,
    };
    let data = store.get(own).and_then(escalator).ok_or_else(|| bad("vt.not_escalator", format!("element {id} is not part of an escalator or walk")))?;
    Ok((own, data))
}

/// Deletes an escalator or walk with its floor openings (`escalator.delete`).
pub(crate) fn delete(txn: &mut Transaction<'_>, p: EscalatorParams) -> Result<usize> {
    let store = txn.store().clone();
    let (id, el) = of(&store, &p.escalator)?;
    let mut n = 1;
    for g in &el.openings {
        if let Some(o) = by_guid(txn.store(), g) {
            txn.delete(o)?;
            n += 1;
        }
    }
    txn.delete(id)?;
    Ok(n)
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Listed {
    pub escalator: ElementId,
    pub name: String,
    #[serde(flatten)]
    pub data: Escalator,
}

/// `escalator.list`.
pub(crate) fn list(store: &Store, _: Option<crate::elevators::NoParams>) -> Result<Vec<Listed>> {
    Ok(escalators(store).into_iter().map(|(id, data)| Listed { escalator: id, name: store.get(id).and_then(|e| e.name.clone()).unwrap_or_default(), data }).collect())
}

// ---- Clear zones (ARC-VT-011) ----

fn frame_mesh(el: &Escalator, profile: &[V2], y0: f64, y1: f64, z0: f64) -> crate::bodies::Mesh {
    let (p, t) = profile_mesh(profile, y0, y1);
    let pts = p
        .into_iter()
        .map(|q| {
            let w = el.at(q[0], q[1]);
            [w[0], w[1], q[2] + z0]
        })
        .collect();
    (pts, t.into_iter().flatten().collect())
}

/// Every escalator's headroom over its step band (as wide as its handrails' outer edges and the
/// side clearance each side) and the landing zone beyond each end, under the first pack in use
/// with escalator rules.
pub fn zones(store: &Store) -> Vec<Clearance> {
    let packs = active(store);
    let Some((pack, r)) = rules(&packs) else { return Vec::new() };
    let levels = crate::levels::list(store);
    let mut out = Vec::new();
    for (id, el) in escalators(store) {
        let Some(z0) = levels.iter().find(|l| l.guid == el.levels[0]).map(|l| l.elevation) else { continue };
        let s = &el.sized;
        let w = s.handrail_outer / 2.0 + r.side_clearance;
        out.push(Clearance {
            element: id,
            kind: ClearanceKind::EscalatorHeadroom,
            rule: format!("{} {}", pack.id, r.id),
            mesh: frame_mesh(&el, &band(s, 0.0, r.headroom), -w, w, z0),
            door_swing_forbidden: false,
        });
        let measure = match r.landing_measure {
            Measure::Outer => s.handrail_outer,
            Measure::Centreline => s.handrail_centres,
        };
        let width = measure + r.landing_width_add;
        let depth = r.landing_depth.or(r.landing_depth_factor.map(|f| f * measure)).unwrap_or(2.5);
        let length = s.length();
        for (x, z) in [([-depth, 0.0], z0 + s.deck), ([length, length + depth], z0 + s.deck + s.rise)] {
            let ring = [el.at(x[0], -width / 2.0), el.at(x[1], -width / 2.0), el.at(x[1], width / 2.0), el.at(x[0], width / 2.0)];
            out.push(Clearance {
                element: id,
                kind: ClearanceKind::EscalatorLanding,
                rule: format!("{} {}", pack.id, r.id),
                mesh: crate::bodies::extrude(&ring, &[], z, z + r.headroom),
                door_swing_forbidden: false,
            });
        }
    }
    out
}

/// What may stand in an escalator's zones besides itself: its floor openings. A neighbour's
/// balustrade inside the side clearance is an intrusion, which keeps adjacent units apart.
pub fn may_share(store: &Store, other: ElementId) -> bool {
    store.get(other).is_some_and(|e| e.param(MEMBER_KEY).is_some())
}

// ---- Checks ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    Inclination,
    Speed,
    FlatSteps,
    StepWidth,
}

/// One finding of `escalator.check`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Finding {
    pub severity: crate::elevators::Severity,
    pub kind: FindingKind,
    pub escalator: ElementId,
    pub pack: String,
    pub edition: String,
    pub layer: Layer,
    pub rule: String,
    pub description: String,
    pub confidence: Confidence,
    pub source: String,
    pub message: String,
    pub measured: f64,
    pub required: f64,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckParams {
    /// The escalators and walks to check; omitted, all.
    #[serde(default)]
    pub escalators: Vec<ElementRef>,
}

/// Checks escalators and walks against the packs in use (`escalator.check`). Headroom, landing
/// and side-clearance intrusions are reported by `clearance.check`.
pub(crate) fn check(store: &Store, p: Option<CheckParams>) -> Result<Vec<Finding>> {
    let p = p.unwrap_or_default();
    let packs = active(store);
    let chosen: Vec<(ElementId, Escalator)> = if p.escalators.is_empty() { escalators(store) } else { p.escalators.iter().map(|r| of(store, r)).collect::<Result<_>>()? };
    let mut out = Vec::new();
    for pack in &packs {
        let Some(r) = &pack.escalator else { continue };
        let note = pack.layer == Layer::Recommended || r.confidence == Confidence::Advisory;
        let mut push = |id: ElementId, kind: FindingKind, message: String, measured: f64, required: f64| {
            out.push(Finding {
                severity: if note { crate::elevators::Severity::Note } else { crate::elevators::Severity::Error },
                kind,
                escalator: id,
                pack: pack.id.clone(),
                edition: pack.edition.clone(),
                layer: pack.layer,
                rule: r.id.clone(),
                description: r.description.clone().unwrap_or_default(),
                confidence: r.confidence,
                source: r.source.clone(),
                message,
                measured,
                required,
            });
        };
        for (id, el) in &chosen {
            let s = &el.sized;
            let walk = s.kind == Kind::MovingWalk;
            let max_incl = if walk { r.walk_max_inclination.unwrap_or(r.max_inclination) } else { r.inclination_for(s.rise, s.speed) };
            if s.inclination > max_incl + 1e-6 {
                push(*id, FindingKind::Inclination, format!("{:.1}° is steeper than the {max_incl:.1}° allowed at {:.2} m rise and {:.2} m/s", s.inclination, s.rise, s.speed), s.inclination, max_incl);
            }
            let max_speed = if walk { r.walk_max_speed.unwrap_or(r.max_speed) } else { r.max_speed };
            if s.speed > max_speed + 1e-6 {
                push(*id, FindingKind::Speed, format!("{:.2} m/s is faster than {max_speed:.2} m/s", s.speed), s.speed, max_speed);
            }
            if !walk {
                let need = r.flat_steps_for(s.rise, s.speed);
                if s.flat_steps < need {
                    push(*id, FindingKind::FlatSteps, format!("{} flat steps; {need} are needed at {:.2} m rise and {:.2} m/s", s.flat_steps, s.rise, s.speed), f64::from(s.flat_steps), f64::from(need));
                }
            }
            // Step-width limits are for escalators and walks steeper than 6°; horizontal walks may
            // be wider (EN 115-1 via KONE, research/16 E1).
            let limited = !walk || s.inclination > 6.0;
            if let Some(max) = r.step_width_max.filter(|m| limited && s.step_width > m + 1e-6) {
                push(*id, FindingKind::StepWidth, format!("a {:.0} mm step or pallet is wider than {:.0} mm", s.step_width * 1000.0, max * 1000.0), s.step_width, max);
            }
            if let Some(min) = r.step_width_min.filter(|m| limited && s.step_width < m - 1e-6) {
                push(*id, FindingKind::StepWidth, format!("a {:.0} mm step or pallet is narrower than {:.0} mm", s.step_width * 1000.0, min * 1000.0), s.step_width, min);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packs(ids: &[&str]) -> Vec<Pack> {
        crate::elevators::shipped().iter().filter(|p| ids.contains(&p.id.as_str())).cloned().collect()
    }

    #[test]
    fn a_30_degree_escalator_is_1_732_h_plus_its_ends() {
        let us = packs(&["us_a17", "practice_us"]);
        let s = size(&us, &SizeParams { rise: Some(4.5), ..SizeParams::default() }).unwrap();
        assert!((s.length() - (4.5 * 3f64.sqrt() + 2.21 + 2.49)).abs() < 1e-9, "{}", s.length());
        assert_eq!((s.flat_steps, s.truss_width, s.capacity), (2, 1.54, 6000.0));
        // EN: above 6 m of rise, 3 flat steps and their longer ends.
        let en = packs(&["en115", "practice_en"]);
        let s = size(&en, &SizeParams { rise: Some(7.0), ..SizeParams::default() }).unwrap();
        assert_eq!((s.flat_steps, s.lower_end, s.upper_end), (3, 2.61, 2.95));
        // A horizontal walk on the floor needs its length.
        let w = size(&en, &SizeParams { kind: Kind::MovingWalk, length: Some(30.0), ..SizeParams::default() }).unwrap();
        assert_eq!((w.inclination, w.pit_depth, w.deck), (0.0, 0.0, 0.15));
        assert!(size(&en, &SizeParams { kind: Kind::MovingWalk, ..SizeParams::default() }).is_err());
    }

    #[test]
    fn the_published_definitions_cover_what_is_written() {
        let defs: Value = serde_json::from_str(include_str!("../../../schemas/psets/riviera-psets-1.json")).unwrap();
        let set = defs["property_sets"].as_array().unwrap().iter().find(|s| s["name"] == "Riviera_Escalator").expect("Riviera_Escalator");
        let s = size(&packs(&["us_a17", "practice_us"]), &SizeParams { rise: Some(4.5), ..SizeParams::default() }).unwrap();
        let el = Escalator { sized: s, point: [0.0; 2], rotation: 0.0, levels: [String::new(), String::new()], openings: Vec::new() };
        for (key, value) in properties(&el) {
            let Some(name) = key.strip_prefix("Riviera_Escalator.") else { continue };
            let def = set["properties"].as_array().unwrap().iter().find(|p| p["name"] == name).unwrap_or_else(|| panic!("{name} is not published"));
            assert_eq!(def["measure"], value["type"], "{name}");
        }
    }

    #[test]
    fn steep_escalators_need_a_low_rise_and_slow_speed() {
        let en = packs(&["en115"]);
        let r = en[0].escalator.as_ref().unwrap();
        assert_eq!(r.inclination_for(5.0, 0.5), 35.0);
        assert_eq!(r.inclination_for(6.5, 0.5), 30.0);
        assert_eq!(r.inclination_for(5.0, 0.65), 30.0);
        assert_eq!((r.flat_steps_for(5.0, 0.5), r.flat_steps_for(5.0, 0.6), r.flat_steps_for(5.0, 0.75)), (2, 3, 4));
    }
}
