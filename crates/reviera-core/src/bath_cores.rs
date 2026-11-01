// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Toilet cores (ARC-BATH-008): a run of toilet compartments, urinals and lavatories along a wall
//! face, made of ordinary elements: partitions are walls of a *Toilet Partition* type standing
//! off the floor, each compartment's door an ordinary door in its front panel, and the fixtures
//! the generic components of [`crate::bath_components`]. Compartments are sized by the rule
//! packs in use, and the numbers may come from the fixture count ([`crate::bath_rules`]).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::txn::Transaction;

/// The tag on a core's elements: the wall it is on.
pub const CORE_KEY: &str = "rxd:bath_core";
pub const PARTITION_TYPE: &str = "Toilet Partition";
/// Partition thickness, height above the floor and height (IPC 405.3.4 privacy; the usual US
/// partition).
const T: f64 = 0.025;
const RAISE: f64 = 0.305;
const PANEL: f64 = 1.475;
/// Urinal screens: out from the wall, from 12 in to 72 in above the floor (IPC 405.3.5).
const SCREEN_OUT: f64 = 0.455;
const SCREEN_HEIGHT: f64 = 1.525;
/// Urinals and lavatories 30 in apart, 15 in from a partition (IPC 405.3.1).
const PITCH: f64 = 0.762;
const SIDE: f64 = 0.381;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Sex {
    #[default]
    Male,
    Female,
    /// An all-gender room.
    All,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreParams {
    /// The wall the core stands against.
    pub wall_id: ElementRef,
    /// The run's ends in plan, on or near the wall's face on the room's side.
    pub start: [f64; 2],
    pub end: [f64; 2],
    pub level: ElementRef,
    /// WCs (compartments, the accessible and ambulatory ones included), urinals and lavatories;
    /// from the count when omitted and an occupancy is given.
    pub wcs: Option<u32>,
    pub urinals: Option<u32>,
    pub lavatories: Option<u32>,
    /// An accessible compartment first (default true), an ambulatory one last (default false;
    /// ADA asks for one where there are six or more compartments, or WCs and urinals together).
    pub accessible: Option<bool>,
    pub ambulatory: Option<bool>,
    /// Count the fixtures for this occupancy and load (`bath.fixture_count`), for one sex's room.
    pub occupancy: Option<String>,
    pub occupant_load: Option<f64>,
    #[serde(default)]
    pub sex: Sex,
    /// Let urinals take the share of the males' WCs the count allows.
    #[serde(default)]
    pub urinals_for_wcs: bool,
    /// Remove an earlier core on this wall first.
    #[serde(default)]
    pub replace: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CoreFit {
    pub length: f64,
    pub needed: f64,
    pub short: f64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Core {
    /// Fixtures placed.
    pub placed: Vec<ElementId>,
    pub partitions: Vec<ElementId>,
    pub doors: Vec<ElementId>,
    pub grab_bars: Vec<ElementId>,
    pub wcs: u32,
    pub urinals: u32,
    pub lavatories: u32,
    pub fit: CoreFit,
}

/// A compartment: clear width and depth.
#[derive(Debug, Clone, Copy)]
struct Compartment {
    width: f64,
    depth: f64,
    kind: &'static str,
}

/// The partition wall type, made the first time.
fn partition_type(txn: &mut Transaction<'_>) -> Result<ElementId> {
    if let Some(e) = txn.store().iter().find(|e| e.class == "IfcWallType" && e.name.as_deref() == Some(PARTITION_TYPE)) {
        return Ok(e.id());
    }
    txn.create(
        crate::element::NewElement::new("IfcWallType")
            .name(PARTITION_TYPE)
            .param("@PredefinedType", "PARTITIONING")
            .param(crate::walls::RECIPE_KEY, json!({ "kind": crate::walls::WALL_TYPE_RECIPE, "width": T })),
    )
}

/// Lays out a toilet core (`bath.toilet_core`).
pub(crate) fn toilet_core(txn: &mut Transaction<'_>, p: CoreParams) -> Result<Core> {
    let store = txn.store().clone();
    let wall_id = store.resolve(&p.wall_id).filter(|id| store.get(*id).is_some_and(|e| crate::walls::wall_recipe(e).is_some())).ok_or_else(|| bad("bath.no_wall", "no such Reviera wall"))?;
    let wall = store.get(wall_id).expect("resolved");
    let g = crate::walls::regenerate(&store, wall)?;
    let level = store.resolve(&p.level).and_then(|l| store.get(l)).filter(|l| l.class == crate::levels::CLASS).ok_or_else(|| bad("bath.no_level", "no such level"))?;
    let level_ref = ElementRef::Id(level.id());
    let (d, n) = (g.direction(), g.normal());
    let rel = |q: [f64; 2]| [q[0] - g.start[0], q[1] - g.start[1]];
    let side = if rel(p.start)[0] * n[0] + rel(p.start)[1] * n[1] >= 0.0 { 1.0 } else { -1.0 };
    let along = |q: [f64; 2]| rel(q)[0] * d[0] + rel(q)[1] * d[1];
    let (u0, u1) = (along(p.start), along(p.end));
    let dir = if u1 >= u0 { 1.0 } else { -1.0 };
    let length = (u1 - u0).abs();
    // A point `a` along the run from its start, `out` from the wall face into the room.
    let face = g.width / 2.0;
    let at = |a: f64, out: f64| {
        let u = u0 + dir * a;
        let o = side * (face + out);
        [g.start[0] + d[0] * u + n[0] * o, g.start[1] + d[1] * u + n[1] * o]
    };

    // How many: given, else counted.
    let packs = crate::bath_rules::active(&store);
    let england = packs.iter().any(|x| x.id == "england_adm");
    let (mut wcs, mut urinals, mut lavatories) = (p.wcs, p.urinals, p.lavatories);
    if let (Some(occ), Some(load)) = (&p.occupancy, p.occupant_load) {
        let c = crate::bath_rules::fixture_count(&store, crate::bath_rules::CountParams { occupancy: occ.clone(), occupant_load: load, male_share: None, units: None, all_gender: p.sex == Sex::All, pack: None, level_id: None })?;
        let need = |f: &str| c.needs.iter().find(|x| x.fixture == f);
        let pick = |x: Option<&crate::bath_rules::Need>| x.map_or(0, |x| match p.sex {
            Sex::Male => x.male.unwrap_or(x.required / 2),
            Sex::Female => x.female.unwrap_or(x.required.div_ceil(2)),
            Sex::All => x.required,
        });
        let mut w = pick(need("water_closet"));
        let mut u = 0;
        if p.sex == Sex::Male && p.urinals_for_wcs {
            u = c.urinals_may_replace.min(w.saturating_sub(1));
            w -= u;
        }
        wcs = wcs.or(Some(w));
        urinals = urinals.or(Some(u + pick(need("urinal"))));
        lavatories = lavatories.or(Some(pick(need("lavatory"))));
    }
    let (wcs, urinals, lavatories) = (wcs.unwrap_or(0), urinals.unwrap_or(0), lavatories.unwrap_or(0));
    if wcs == 0 && urinals == 0 && lavatories == 0 {
        return Err(bad("bath.invalid_input", "give the numbers of WCs, urinals and lavatories, or an occupancy and its load"));
    }
    let accessible = p.accessible.unwrap_or(true) && wcs > 0;
    let ambulatory = p.ambulatory.unwrap_or(false) && wcs > u32::from(accessible);

    // The compartments, in order.
    let (std_c, amb_c) = if england {
        (Compartment { width: 0.91, depth: 1.74, kind: "standard" }, Compartment { width: 0.92, depth: 1.465, kind: "ambulatory" })
    } else {
        (Compartment { width: 0.915, depth: 1.525, kind: "standard" }, Compartment { width: 0.915, depth: 1.525, kind: "ambulatory" })
    };
    let mut comps = Vec::new();
    if accessible {
        // 60 × 59 in at least (ADA 604.8.1.1); in the US as deep as the standard ones, so the
        // fronts line up.
        comps.push(Compartment { width: 1.525, depth: if england { 1.5 } else { std_c.depth.max(1.5) }, kind: "accessible" });
    }
    let n_std = wcs - u32::from(accessible) - u32::from(ambulatory);
    for _ in 0..n_std {
        comps.push(std_c);
    }
    if ambulatory {
        comps.push(amb_c);
    }
    let comp_len: f64 = comps.iter().map(|c| c.width + T).sum::<f64>() + if comps.is_empty() { 0.0 } else { T };
    // Urinals 30 in apart plus the screen between them, so each is 15 in clear of it.
    let u_pitch = PITCH + T;
    let needed = comp_len + if urinals > 0 { SIDE + u_pitch * f64::from(urinals - 1) + SIDE } else { 0.0 } + if lavatories > 0 { SIDE + PITCH * f64::from(lavatories - 1) + SIDE } else { 0.0 };
    let fit = CoreFit { length, needed, short: (needed - length).max(0.0) };
    if fit.short > 0.0 {
        return Err(bad("bath.no_fit", format!("the core does not fit: {:.0} mm needed along the wall, {:.0} mm there ({:.0} mm short)", needed * 1000.0, length * 1000.0, fit.short * 1000.0)).with_details(serde_json::to_value(&fit).expect("serialises")));
    }

    if p.replace {
        let key = wall.guid().to_string();
        let old: Vec<ElementId> = txn.store().iter().filter(|e| e.param(CORE_KEY).and_then(|v| v.get("wall")).and_then(|v| v.as_str()) == Some(key.as_str())).map(crate::element::Element::id).collect();
        for id in &old {
            for (bar, _) in crate::grab_bars::of_fixture(txn.store(), *id) {
                if txn.store().get(bar).is_some() {
                    txn.delete(bar)?;
                }
            }
        }
        for id in old {
            if txn.store().get(id).is_some() {
                txn.delete(id)?;
            }
        }
    }

    let tag = json!({ "wall": wall.guid().to_string() });
    let mark = |txn: &mut Transaction<'_>, id: ElementId| -> Result<()> {
        let tag = tag.clone();
        txn.modify(id, |e| {
            e.params.insert(CORE_KEY.into(), tag);
        })
    };
    let ptype = partition_type(txn)?;
    let mut partitions = Vec::new();
    let mut doors = Vec::new();
    let mut placed = Vec::new();
    let partition = |txn: &mut Transaction<'_>, a: [f64; 2], b: [f64; 2], raise: f64, height: f64| -> Result<ElementId> {
        let args = json!({ "curve": { "line": { "start": a, "end": b } }, "base_level": level.id().0, "base_offset": raise, "top_constraint": { "unconnected": height }, "type_id": ptype.0 });
        crate::walls::create(txn, serde_json::from_value(args).map_err(|e| bad("bath.params", e.to_string()))?)
    };
    let types = crate::bath_components::load(txn, crate::bath_components::LoadParams { names: vec!["WC, close-coupled".into(), "WC, accessible height".into(), "Urinal, wall-hung".into(), "Lavatory, wall-hung".into()] })?;
    let fixture = |txn: &mut Transaction<'_>, name: &str, q: [f64; 2]| -> Result<ElementId> {
        crate::bath_components::place(
            txn,
            crate::mep_controls::PlaceParams {
                type_id: ElementRef::Id(types[name]),
                point: q,
                level: level_ref.clone(),
                offset: None,
                rotation: 0.0,
                host_wall: Some(ElementRef::Id(wall_id)),
                side: None,
                work_plane: false,
                controlled_ids: Vec::new(),
            },
        )
    };

    // Compartments: a side panel at each boundary, a front panel with a door, a WC.
    let mut a = 0.0;
    let mut accessible_wc = None;
    // Fixtures the accessibility packs do not apply to: all but the accessible compartment, one
    // urinal and one lavatory (ADA 213.3).
    let mut not_accessible = Vec::new();
    for (k, c) in comps.iter().enumerate() {
        if k == 0 {
            let id = partition(txn, at(a + T / 2.0, 0.0), at(a + T / 2.0, c.depth + T), RAISE, PANEL)?;
            partitions.push(id);
        }
        let (x0, x1) = (a + T, a + T + c.width);
        let next_depth = comps.get(k + 1).map_or(c.depth, |n| n.depth.max(c.depth));
        partitions.push(partition(txn, at(x1 + T / 2.0, 0.0), at(x1 + T / 2.0, next_depth + T), RAISE, PANEL)?);
        let front = partition(txn, at(x0, c.depth + T / 2.0), at(x1, c.depth + T / 2.0), RAISE, PANEL)?;
        partitions.push(front);
        // The WC: an accessible one 455 mm (18 in) from the side partition, the others centred.
        let (name, centre) = if c.kind == "accessible" { ("WC, accessible height", x0 + 0.455) } else { ("WC, close-coupled", (x0 + x1) / 2.0) };
        let wc = fixture(txn, name, at(centre, 0.1))?;
        if c.kind == "accessible" {
            accessible_wc = Some(wc);
        } else {
            not_accessible.push(wc);
        }
        placed.push(wc);
        // The door: centred, or beyond the WC in the accessible compartment (ADA 604.8.1.2);
        // opening outward.
        let width = if c.kind == "standard" { 0.61 } else { 0.915 };
        // The accessible door hard by the far partition, its push side clear of the WC.
        let door_at = if c.kind == "accessible" { x1 - width / 2.0 - 0.01 } else { (x0 + x1) / 2.0 };
        let gf = crate::walls::regenerate(txn.store(), txn.store().get(front).expect("made"))?;
        let along_front = ((at(door_at, c.depth + T / 2.0)[0] - gf.start[0]) * gf.direction()[0] + (at(door_at, c.depth + T / 2.0)[1] - gf.start[1]) * gf.direction()[1]).clamp(width / 2.0, gf.length() - width / 2.0);
        let args = json!({ "host_wall_id": front.0, "point_along": along_front, "sill_height": RAISE, "width": width, "height": PANEL - 0.05, "fit": false });
        let door = crate::inserts::place(txn, crate::inserts::Kind::Door, serde_json::from_value(args).map_err(|e| bad("bath.params", e.to_string()))?)?;
        // Swinging out of the compartment: away from the wall.
        if let Some(sw) = crate::clearances::all(txn.store()).into_iter().find(|v| v.element == door && v.kind == crate::clearances::ClearanceKind::DoorSwing) {
            let (lo, hi) = crate::clearances::Clearance::body(&sw).bounds().unwrap_or(([0.0; 3], [0.0; 3]));
            let mid = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
            let out = (mid[0] - (g.start[0] + n[0] * side * face)) * n[0] * side + (mid[1] - (g.start[1] + n[1] * side * face)) * n[1] * side;
            if out < c.depth {
                crate::inserts::flip(txn, serde_json::from_value(json!({ "element_ids": [door.0] })).map_err(|e| bad("bath.params", e.to_string()))?, false)?;
            }
        }
        if c.kind == "standard" {
            not_accessible.push(door);
        }
        doors.push(door);
        a = x1;
    }
    if !comps.is_empty() {
        a += T;
    }
    // Urinals, a screen between each pair.
    for k in 0..urinals {
        let c = a + SIDE + u_pitch * f64::from(k);
        let u = fixture(txn, "Urinal, wall-hung", at(c, 0.1))?;
        if k > 0 {
            not_accessible.push(u);
        }
        placed.push(u);
        if k + 1 < urinals {
            let s = c + u_pitch / 2.0;
            partitions.push(partition(txn, at(s, 0.0), at(s, SCREEN_OUT), RAISE, SCREEN_HEIGHT)?);
        }
    }
    if urinals > 0 {
        a += SIDE + u_pitch * f64::from(urinals - 1) + SIDE;
    }
    // Lavatories.
    for k in 0..lavatories {
        let c = a + SIDE + PITCH * f64::from(k);
        let l = fixture(txn, "Lavatory, wall-hung", at(c, 0.1))?;
        if k > 0 {
            not_accessible.push(l);
        }
        placed.push(l);
    }
    if !not_accessible.is_empty() {
        crate::bath_rules::set_accessible(txn, crate::bath_rules::SetAccessibleParams { element_ids: not_accessible.iter().map(|id| ElementRef::Id(*id)).collect(), accessible: false })?;
    }
    // The accessible WC's grab bars.
    let mut grab_bars = Vec::new();
    if let Some(wc) = accessible_wc {
        let out = crate::bath_rules::place_grab_bars(txn, crate::bath_rules::PlaceBarsParams { element_ids: Some(vec![ElementRef::Id(wc)]) })?;
        grab_bars = out.placed;
    }
    for id in partitions.iter().chain(&doors).chain(&placed).chain(&grab_bars).copied().collect::<Vec<_>>() {
        mark(txn, id)?;
    }
    Ok(Core { placed, partitions, doors, grab_bars, wcs, urinals, lavatories, fit })
}
