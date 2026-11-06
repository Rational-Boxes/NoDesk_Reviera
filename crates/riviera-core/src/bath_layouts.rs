// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Bathroom layouts (ARC-BATH-005): a whole bathroom from a template, fitted to a rectangular
//! room closed by walls. Fixtures run along a wet wall from one end, with a bath across that end
//! when the template has one, spaced so that each fixture's side clear space (from the rule packs
//! in use, [`crate::bath_rules`]) misses its neighbours; when the room is too small, nothing is
//! placed and the shortfall is reported.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::bath_rules::{self, AcrossFrom, FixtureKind, Measure, OutFrom, Pack};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::store::Store;
use crate::txn::Transaction;

/// The tag on a layout's components: its template and room.
pub const LAYOUT_KEY: &str = "rxd:bath_layout";
/// Space between fixtures when no rule pack is in use.
const GAP: f64 = 0.05;
/// How far a room's walls are looked for.
const REACH: f64 = 30.0;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// A layout template.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Template {
    pub id: &'static str,
    pub title: &'static str,
    /// A bath or shower across the start end, against the end wall.
    pub end: Option<&'static str>,
    /// Along the wet wall, from the start end.
    pub run: Vec<&'static str>,
    /// Grab bars placed for the fixtures under the packs in use.
    pub grab_bars: bool,
}

pub fn templates() -> Vec<Template> {
    vec![
        Template { id: "full_bath", title: "Full bath (US, 5 × 8 ft)", end: Some("Bath, 60 × 30 in"), run: vec!["WC, close-coupled", "Lavatory, pedestal"], grab_bars: false },
        Template { id: "three_quarter_bath", title: "Three-quarter bath (shower)", end: None, run: vec!["Shower tray, 900 × 900", "WC, close-coupled", "Lavatory, pedestal"], grab_bars: false },
        Template { id: "powder_room", title: "Powder room", end: None, run: vec!["WC, close-coupled", "Lavatory, wall-hung"], grab_bars: false },
        Template { id: "uk_family_bathroom", title: "Family bathroom (UK)", end: Some("Bath, 1700 × 700"), run: vec!["WC, close-coupled", "Lavatory, pedestal"], grab_bars: false },
        Template { id: "en_suite", title: "En-suite shower room", end: None, run: vec!["Shower tray, 900 × 900", "WC, close-coupled", "Hand-rinse basin"], grab_bars: false },
        Template { id: "cloakroom", title: "Cloakroom WC", end: None, run: vec!["WC, close-coupled", "Hand-rinse basin"], grab_bars: false },
        Template { id: "accessible_single_user", title: "Accessible single-user toilet", end: None, run: vec!["WC, accessible height", "Lavatory, wall-hung"], grab_bars: true },
    ]
}

/// `bath.layouts`.
pub(crate) fn list(_: &Store, _: Option<bath_rules::NoParams>) -> Result<Vec<Template>> {
    Ok(templates())
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Start {
    /// The left end as you face the wet wall.
    #[default]
    Left,
    Right,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LayoutParams {
    pub template: String,
    /// A point inside the room, in plan.
    pub point: [f64; 2],
    pub level: ElementRef,
    /// The wall the fixtures run along; default the longer side.
    pub wet_wall_id: Option<ElementRef>,
    #[serde(default)]
    pub start: Start,
    /// Remove the components an earlier layout placed in this room first.
    #[serde(default)]
    pub replace: bool,
    /// Report the fit without placing anything.
    #[serde(default)]
    pub fit_only: bool,
}

/// How the template fits (metres).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Fit {
    /// Along the wet wall: available, needed, short (0 when it fits).
    pub length: f64,
    pub length_needed: f64,
    pub length_short: f64,
    /// Out from the wet wall to the opposite wall.
    pub depth: f64,
    pub depth_needed: f64,
    pub depth_short: f64,
    /// Across the room for the end item (its length against the room's).
    pub end_short: f64,
}

impl Fit {
    pub fn fits(&self) -> bool {
        self.length_short <= 0.0 && self.depth_short <= 0.0 && self.end_short <= 0.0
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Layout {
    pub placed: Vec<ElementId>,
    pub fit: Fit,
    /// Grab bars placed, and those skipped with the reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grab_bars: Option<bath_rules::Placed>,
}

/// A fixture's clear spaces as the run needs them, under the packs.
struct Spaces {
    /// The half-width its side space needs from its centreline.
    half: Option<f64>,
    /// How far its spaces measured from its side wall reach along the run (an ADA WC's 60 in).
    reach: Option<f64>,
    /// Its front space's depth.
    front: f64,
}

fn spaces(packs: &[Pack], kind: FixtureKind, occupancy: crate::codes::Occupancy) -> Spaces {
    let mut half: Option<f64> = None;
    let mut reach: Option<f64> = None;
    let mut front: f64 = 0.0;
    for z in packs.iter().flat_map(|p| &p.zones).filter(|z| z.fixtures.contains(&kind) && z.occupancy.applies(occupancy)) {
        if z.across.iter().all(|a| a.0 == AcrossFrom::SideWall) {
            let r = z.across.iter().map(|a| a.1).fold(0.0, f64::max);
            reach = Some(reach.map_or(r, |x| x.max(r)));
        }
        // Spaces either side of the centreline, beside the fixture or in front of it (AD M's WC
        // access zone): the fixtures beside it, and an end bath, stay out of both.
        if z.across.iter().all(|a| a.0 == AcrossFrom::Centre) {
            let h = z.across.iter().map(|a| a.1.abs()).fold(0.0, f64::max);
            half = Some(half.map_or(h, |x| x.max(h)));
        }
        if z.out.iter().all(|o| o.0 == OutFrom::Front) {
            front = front.max(z.out.iter().map(|o| o.1).fold(0.0, f64::max));
        }
    }
    Spaces { half, reach, front }
}

/// A distance rule's range for the first fixture's centreline from the end.
fn side_range(packs: &[Pack], kind: FixtureKind, occupancy: crate::codes::Occupancy) -> (Option<f64>, Option<f64>) {
    let mut lo: Option<f64> = None;
    let mut hi: Option<f64> = None;
    for d in packs.iter().flat_map(|p| &p.distances).filter(|d| d.fixtures.contains(&kind) && d.measure == Measure::CentrelineToSideWall && d.occupancy.applies(occupancy)) {
        if let Some(m) = d.min {
            lo = Some(lo.map_or(m, |x| x.max(m)));
        }
        if let Some(m) = d.max {
            hi = Some(hi.map_or(m, |x| x.min(m)));
        }
    }
    (lo, hi)
}

struct Room {
    /// The wet wall's face: a point on it, the unit direction toward it, and its wall.
    wet: ElementId,
    w: [f64; 2],
    /// Along the run, from the start end's face: the start point and unit direction.
    start: [f64; 2],
    run: [f64; 2],
    start_wall: ElementId,
    /// The wall across the room from the wet wall, and the one at the far end of the run.
    opposite: ElementId,
    end_wall: ElementId,
    length: f64,
    depth: f64,
    key: String,
}

fn find_room(store: &Store, p: &LayoutParams, z: f64) -> Result<Room> {
    let wet_given = p
        .wet_wall_id
        .as_ref()
        .map(|r| store.resolve(r).filter(|id| store.get(*id).is_some_and(|e| crate::walls::wall_recipe(e).is_some())).ok_or_else(|| bad("bath.no_wall", "no such Riviera wall")))
        .transpose()?;
    // The room's axes: along the wet wall, else along the nearest wall.
    let axis = match wet_given {
        Some(w) => crate::walls::regenerate(store, store.get(w).expect("resolved"))?.direction(),
        None => {
            let near = [[1.0, 0.0], [-1.0, 0.0], [0.0, 1.0], [0.0, -1.0]]
                .iter()
                .filter_map(|d| bath_rules::ray_to_walls(store, p.point, *d, z, REACH))
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .ok_or_else(|| bad("bath.open_room", "no walls around the point"))?;
            crate::walls::regenerate(store, store.get(near.1).expect("found"))?.direction()
        }
    };
    let u = axis;
    let v = [-u[1], u[0]];
    let look = |d: [f64; 2], name: &str| bath_rules::ray_to_walls(store, p.point, d, z, REACH).ok_or_else(|| bad("bath.open_room", format!("the room is open {name}: no wall that way")));
    let (pu, mu) = (look(u, "ahead along the wall")?, look([-u[0], -u[1]], "behind along the wall")?);
    let (pv, mv) = (look(v, "to one side")?, look([-v[0], -v[1]], "to the other side")?);
    let (len_u, len_v) = (pu.0 + mu.0, pv.0 + mv.0);
    // The wet wall and its distance: the given one, else across the longer dimension.
    let (wet, tw, w) = match wet_given {
        Some(id) if id == pv.1 => (id, pv.0, v),
        Some(id) if id == mv.1 => (id, mv.0, [-v[0], -v[1]]),
        Some(_) => return Err(bad("bath.no_wall", "the wet wall is not one of the room's walls along its length")),
        None if len_u >= len_v => (pv.1, pv.0, v),
        None => (pu.1, pu.0, u),
    };
    // Facing the wet wall: left is w turned a quarter anticlockwise.
    let left = [-w[1], w[0]];
    let (t_left, wall_left, t_right, wall_right) = {
        let a = look(left, "to the left")?;
        let b = look([-left[0], -left[1]], "to the right")?;
        (a.0, a.1, b.0, b.1)
    };
    let opposite = look([-w[0], -w[1]], "opposite the wet wall")?;
    let (start, run, start_wall, end_wall) = match p.start {
        Start::Left => ([p.point[0] + left[0] * t_left, p.point[1] + left[1] * t_left], [-left[0], -left[1]], wall_left, wall_right),
        Start::Right => ([p.point[0] - left[0] * t_right, p.point[1] - left[1] * t_right], left, wall_right, wall_left),
    };
    // Onto the wet wall's line: the run's start at the wet face.
    let start = [start[0] + w[0] * tw, start[1] + w[1] * tw];
    let mut walls = [wet, wall_left, wall_right, opposite.1].map(|id| store.get(id).expect("found").guid().to_string());
    walls.sort();
    Ok(Room { wet, w, start, run, start_wall, opposite: opposite.1, end_wall, length: t_left + t_right, depth: tw + opposite.0, key: walls.join("|") })
}

/// Lays out a bathroom (`bath.layout`).
pub(crate) fn layout(txn: &mut Transaction<'_>, p: LayoutParams) -> Result<Layout> {
    let all = templates();
    let t = all.iter().find(|t| t.id == p.template).ok_or_else(|| {
        let ids: Vec<&str> = all.iter().map(|t| t.id).collect();
        bad("bath.no_template", format!("no layout template {:?}; there are: {}", p.template, ids.join(", ")))
    })?;
    let store = txn.store().clone();
    let level = store.resolve(&p.level).and_then(|l| store.get(l)).filter(|l| l.class == crate::levels::CLASS).ok_or_else(|| bad("bath.no_level", "no such level"))?;
    let z = crate::levels::elevation(level) + 0.3;
    let room = find_room(&store, &p, z)?;
    let packs = bath_rules::active(&store);
    let occupancy = crate::codes::settings(&store).occupancy;

    // The components and their sizes (width along the wall, depth out of it).
    let mut names: Vec<&str> = t.end.into_iter().collect();
    names.extend(&t.run);
    let types = crate::bath_components::load(txn, crate::bath_components::LoadParams { names: names.iter().map(|n| (*n).to_owned()).collect() })?;
    let size = |name: &str| -> [f64; 2] {
        let ty = txn.store().get(types[name]).and_then(crate::mesh_objects::mesh_object_type).expect("loaded");
        [ty.size[0] * ty.scale, ty.size[1] * ty.scale]
    };
    let kind = |name: &str| crate::bath_components::catalogue().into_iter().find(|c| c.name == name).and_then(|c| FixtureKind::from_predefined(c.predefined));

    // The end item, across the start end.
    let (mut edge, mut end_short, mut depth_needed) = (0.0, 0.0f64, 0.0f64);
    if let Some(e) = t.end {
        let [l, d] = size(e);
        end_short = l - room.depth;
        edge = d;
    }
    // The run.
    let mut centres = Vec::new();
    let mut prev: Option<(f64, f64, Option<f64>)> = None; // centre, half width, side space
    // Where spaces measured from the start end's wall reach: nothing after may stand in them.
    let mut barrier = edge;
    for (i, name) in t.run.iter().enumerate() {
        let [wd, dp] = size(name);
        let k = kind(name);
        let Spaces { half, reach, front } = k.map_or(Spaces { half: None, reach: None, front: 0.0 }, |k| spaces(&packs, k, occupancy));
        depth_needed = depth_needed.max(dp + front);
        let c = match prev {
            None => {
                let mut c = edge + half.unwrap_or(wd / 2.0 + GAP).max(wd / 2.0);
                if i == 0
                    && let Some(k) = k
                {
                    let (lo, hi) = side_range(&packs, k, occupancy);
                    if let Some(lo) = lo {
                        c = c.max(edge + lo);
                    }
                    if let Some(hi) = hi {
                        c = c.min(edge + hi);
                    }
                }
                c
            }
            Some((pc, pw, ph)) => {
                let a = ph.unwrap_or(pw + GAP) + wd / 2.0;
                let b = half.unwrap_or(wd / 2.0 + GAP) + pw;
                (pc + a.max(b)).max(barrier + wd / 2.0)
            }
        };
        // The first fixture is against the end: its side-wall spaces reach from there.
        if prev.is_none()
            && let Some(r) = reach
        {
            barrier = barrier.max(edge + r);
        }
        centres.push(c);
        prev = Some((c, wd / 2.0, half));
    }
    let length_needed = prev.map_or(edge, |(c, w, h)| c + h.unwrap_or(w + GAP).max(w)).max(barrier);
    let fit = Fit {
        length: room.length,
        length_needed,
        length_short: (length_needed - room.length).max(0.0),
        depth: room.depth,
        depth_needed,
        depth_short: (depth_needed - room.depth).max(0.0),
        end_short: end_short.max(0.0),
    };
    if p.fit_only {
        return Ok(Layout { placed: Vec::new(), fit, grab_bars: None });
    }
    if !fit.fits() {
        let mut why = Vec::new();
        if fit.length_short > 0.0 {
            why.push(format!("the wet wall is {:.0} mm short ({:.0} mm needed, {:.0} mm there)", fit.length_short * 1000.0, length_needed * 1000.0, room.length * 1000.0));
        }
        if fit.depth_short > 0.0 {
            why.push(format!("the room is {:.0} mm too shallow for the fixtures and their clear spaces", fit.depth_short * 1000.0));
        }
        if fit.end_short > 0.0 {
            why.push(format!("{} is {:.0} mm longer than the room is wide", t.end.unwrap_or_default(), fit.end_short * 1000.0));
        }
        return Err(bad("bath.no_fit", format!("{} does not fit: {}", t.title, why.join("; "))).with_details(serde_json::to_value(&fit).expect("serialises")));
    }

    if p.replace {
        let old: Vec<ElementId> = txn.store().iter().filter(|e| e.param(LAYOUT_KEY).and_then(|v| v.get("room")).and_then(|v| v.as_str()) == Some(room.key.as_str())).map(crate::element::Element::id).collect();
        for id in old {
            for (bar, _) in crate::grab_bars::of_fixture(txn.store(), id) {
                txn.delete(bar)?;
            }
            txn.delete(id)?;
        }
    }
    let tag = json!({ "template": t.id, "room": room.key });
    let into = 0.1;
    let mut placed = Vec::new();
    let mut put = |txn: &mut Transaction<'_>, name: &str, host: ElementId, at: [f64; 2]| -> Result<()> {
        let args = crate::mep_controls::PlaceParams {
            type_id: ElementRef::Id(types[name]),
            point: at,
            level: p.level.clone(),
            offset: None,
            rotation: 0.0,
            host_wall: Some(ElementRef::Id(host)),
            side: None,
            work_plane: false,
            controlled_ids: Vec::new(),
        };
        let id = crate::bath_components::place(txn, args)?;
        let tag = tag.clone();
        txn.modify(id, |e| {
            e.params.insert(LAYOUT_KEY.into(), tag);
        })?;
        placed.push(id);
        Ok(())
    };
    let w = room.w;
    if let Some(e) = t.end {
        // Centred across the room, against the end wall.
        let mid = room.depth / 2.0;
        let at = [room.start[0] - w[0] * mid + room.run[0] * into, room.start[1] - w[1] * mid + room.run[1] * into];
        put(txn, e, room.start_wall, at)?;
    }
    for (name, c) in t.run.iter().zip(&centres) {
        let at = [room.start[0] + room.run[0] * c - w[0] * into, room.start[1] + room.run[1] * c - w[1] * into];
        put(txn, name, room.wet, at)?;
    }
    let grab_bars = if t.grab_bars { Some(bath_rules::place_grab_bars(txn, bath_rules::PlaceBarsParams { element_ids: Some(placed.iter().map(|id| ElementRef::Id(*id)).collect()) })?) } else { None };
    Ok(Layout { placed, fit, grab_bars })
}

// ---- Locker rooms (ARC-BATH-009) ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LockerRoomParams {
    /// A point inside the room, in plan.
    pub point: [f64; 2],
    pub level: ElementRef,
    /// How many lockers (compartments).
    pub lockers: u32,
    /// Compartments per locker column: 1 (full height) or 2 (two tier); default 1.
    pub tiers: Option<u32>,
    /// Gang shower heads along the start end wall (default none).
    #[serde(default)]
    pub showers: u32,
    /// An accessible bench and accessible lockers (default true).
    pub accessible: Option<bool>,
    /// The wall the first locker run is along; default the longer side.
    pub wet_wall_id: Option<ElementRef>,
    #[serde(default)]
    pub start: Start,
    #[serde(default)]
    pub replace: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LockerFit {
    /// Lockers asked for, and how many the runs hold.
    pub lockers: u32,
    pub capacity: u32,
    /// Across the room: needed for two runs, their aisles and the bench, and there.
    pub depth_needed: f64,
    pub depth: f64,
    pub aisle: f64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LockerRoom {
    pub placed: Vec<ElementId>,
    pub lockers: u32,
    pub accessible_lockers: u32,
    pub fit: LockerFit,
}

/// The aisle the packs ask for in front of a locker (the deepest locker front space), if any.
fn locker_aisle(packs: &[Pack], occupancy: crate::codes::Occupancy) -> Option<f64> {
    packs
        .iter()
        .flat_map(|p| &p.zones)
        .filter(|z| z.fixtures.contains(&FixtureKind::Locker) && z.occupancy.applies(occupancy) && z.out.iter().all(|o| o.0 == OutFrom::Front))
        .map(|z| z.out.iter().map(|o| o.1).fold(0.0, f64::max))
        .fold(None, |a: Option<f64>, v| Some(a.map_or(v, |a| a.max(v))))
}

/// Lays out a locker room (`bath.locker_room`).
pub(crate) fn locker_room(txn: &mut Transaction<'_>, p: LockerRoomParams) -> Result<LockerRoom> {
    const LOCKER: [f64; 2] = [0.381, 0.457];
    const BENCH: [f64; 2] = [1.83, 0.305];
    const ACC_BENCH: [f64; 2] = [1.065, 0.61];
    const GANG: f64 = 0.75;
    const DRYING: f64 = 1.2;
    const DEFAULT_AISLE: f64 = 0.9;
    let tiers = p.tiers.unwrap_or(1).clamp(1, 2);
    let accessible = p.accessible.unwrap_or(true);
    let store = txn.store().clone();
    let level = store.resolve(&p.level).and_then(|l| store.get(l)).filter(|l| l.class == crate::levels::CLASS).ok_or_else(|| bad("bath.no_level", "no such level"))?;
    let z = crate::levels::elevation(level) + 0.3;
    let lp = LayoutParams { template: String::new(), point: p.point, level: p.level.clone(), wet_wall_id: p.wet_wall_id.clone(), start: p.start, replace: false, fit_only: false };
    let room = find_room(&store, &lp, z)?;
    let packs = bath_rules::active(&store);
    let occupancy = crate::codes::settings(&store).occupancy;
    let aisle = locker_aisle(&packs, occupancy).unwrap_or(DEFAULT_AISLE);

    // Along the runs: from the showers' drying zone to the far end.
    let s0 = if p.showers > 0 { 0.15 + DRYING } else { 0.0 };
    let per_run = ((room.length - s0) / LOCKER[0] + 1e-9).floor().max(0.0) as u32;
    let capacity = per_run * 2 * tiers;
    let depth_needed = 2.0 * LOCKER[1] + 2.0 * aisle + BENCH[1];
    let fit = LockerFit { lockers: p.lockers, capacity, depth_needed, depth: room.depth, aisle };
    if depth_needed > room.depth + 1e-6 {
        return Err(bad("bath.no_fit", format!("the room is {:.0} mm too narrow for two locker runs with {:.0} mm in front of each and a bench ({:.0} mm needed, {:.0} mm there)", (depth_needed - room.depth) * 1000.0, aisle * 1000.0, depth_needed * 1000.0, room.depth * 1000.0)).with_details(serde_json::to_value(&fit).expect("serialises")));
    }
    if p.lockers > capacity {
        let short = f64::from((p.lockers - capacity).div_ceil(2 * tiers)) * LOCKER[0];
        return Err(bad("bath.no_fit", format!("{capacity} lockers fit the runs, not {}; the room would need to be about {:.0} mm longer", p.lockers, short * 1000.0)).with_details(serde_json::to_value(&fit).expect("serialises")));
    }
    if (f64::from(p.showers) * GANG) > room.depth + 1e-6 {
        return Err(bad("bath.no_fit", format!("{} gang showers at {:.0} mm need {:.0} mm of end wall; there are {:.0} mm", p.showers, GANG * 1000.0, f64::from(p.showers) * GANG * 1000.0, room.depth * 1000.0)));
    }

    if p.replace {
        let old: Vec<ElementId> = txn.store().iter().filter(|e| e.param(LAYOUT_KEY).and_then(|v| v.get("room")).and_then(|v| v.as_str()) == Some(room.key.as_str())).map(crate::element::Element::id).collect();
        for id in old {
            if txn.store().get(id).is_some() {
                txn.delete(id)?;
            }
        }
    }
    let names = vec![
        if tiers == 2 { "Locker, two tier" } else { "Locker, full height" }.to_owned(),
        "Bench, locker room".to_owned(),
        "Bench, accessible".to_owned(),
        "Shower head, gang".to_owned(),
    ];
    let types = crate::bath_components::load(txn, crate::bath_components::LoadParams { names: names.clone() })?;
    let tag = json!({ "template": "locker_room", "room": room.key });
    let mut placed = Vec::new();
    let w = room.w;
    let (run, start) = (room.run, room.start);
    // A point `a` along the run from the start end, `b` out from the wet wall's face.
    let at = |a: f64, b: f64| [start[0] + run[0] * a - w[0] * b, start[1] + run[1] * a - w[1] * b];
    let mut put = |txn: &mut Transaction<'_>, name: &str, host: Option<ElementId>, q: [f64; 2], rotation: f64| -> Result<ElementId> {
        let id = crate::bath_components::place(
            txn,
            crate::mep_controls::PlaceParams {
                type_id: ElementRef::Id(types[name]),
                point: q,
                level: p.level.clone(),
                offset: None,
                rotation,
                host_wall: host.map(ElementRef::Id),
                side: None,
                work_plane: host.is_none(),
                controlled_ids: Vec::new(),
            },
        )?;
        let tag = tag.clone();
        txn.modify(id, |e| {
            e.params.insert(LAYOUT_KEY.into(), tag);
        })?;
        placed.push(id);
        Ok(id)
    };
    // Showers along the start end wall, centred across the room.
    let s_left = (room.depth - f64::from(p.showers) * GANG) / 2.0;
    for k in 0..p.showers {
        put(txn, &names[3], Some(room.start_wall), at(0.1, s_left + GANG * (f64::from(k) + 0.5)), 0.0)?;
    }
    // Locker runs: the wet wall's, then the opposite wall's.
    let columns = p.lockers.div_ceil(tiers);
    let accessible_lockers = if accessible { ((f64::from(p.lockers) * 0.05).ceil() as u32).max(1) } else { 0 };
    let mut not_accessible = Vec::new();
    for c in 0..columns {
        let (side, k) = if c < per_run { (0, c) } else { (1, c - per_run) };
        let a = s0 + LOCKER[0] * (f64::from(k) + 0.5);
        let (host, b) = if side == 0 { (room.wet, 0.1) } else { (room.opposite, room.depth - 0.1) };
        let id = put(txn, &names[0], Some(host), at(a, b), 0.0)?;
        // The first of each run, as many as are needed, are kept accessible.
        let acc_index = if side == 0 { k * 2 } else { k * 2 + 1 };
        if acc_index >= accessible_lockers {
            not_accessible.push(id);
        }
    }
    // Benches down the middle, stopping short of the accessible bench's space.
    let bench_end = room.length - if accessible { ACC_BENCH[1] + 1.22 + 0.3 } else { 0.3 };
    let bench_start = s0 + 0.3;
    let n_bench = (((bench_end - bench_start) / BENCH[0]) + 1e-9).floor().max(0.0) as u32;
    let angle = run[1].atan2(run[0]);
    for k in 0..n_bench {
        let id = put(txn, &names[1], None, at(bench_start + BENCH[0] * (f64::from(k) + 0.5), room.depth / 2.0), angle)?;
        not_accessible.push(id);
    }
    // The accessible bench against the far end wall, centred.
    if accessible {
        put(txn, &names[2], Some(room.end_wall), at(room.length - 0.1, room.depth / 2.0), 0.0)?;
    }
    if !not_accessible.is_empty() {
        bath_rules::set_accessible(txn, bath_rules::SetAccessibleParams { element_ids: not_accessible.iter().map(|id| ElementRef::Id(*id)).collect(), accessible: false })?;
    }
    Ok(LockerRoom { placed, lockers: columns * tiers, accessible_lockers, fit })
}


// ---- Saunas (ARC-BATH-014) ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaunaParams {
    /// A point inside the room, in plan.
    pub point: [f64; 2],
    pub level: ElementRef,
    /// The wall the benches run along; default the longer side.
    pub bench_wall_id: Option<ElementRef>,
    /// The end the heater goes at, as you face the bench wall.
    #[serde(default)]
    pub start: Start,
    /// Bench tiers: 1 or 2 (default 2).
    pub tiers: Option<u32>,
    /// The clear height (metres); default the walls' height.
    pub height: Option<f64>,
    /// Uninsulated surface (glass door, tile, brick) in m², sized as extra volume.
    #[serde(default)]
    pub uninsulated_area: f64,
    /// Log walls (the design volume × 1.5).
    #[serde(default)]
    pub log_walls: bool,
    /// The heater's power (kW); default sized from the room.
    pub heater_kw: Option<f64>,
    /// The lowest bench to ADA §903 (17–19 in high, 24 in deep), as ADA §241 asks where a sauna
    /// has seats (ARC-BATH-016).
    #[serde(default)]
    pub accessible: bool,
    /// Where the vents go (makers disagree): `high_outlet` (default; an inlet 100 mm up beside
    /// the heater, clear of its safety distances, and an outlet 150 mm below the ceiling on the far
    /// wall), `low_outlet` (the inlet beside the heater, an outlet twice its size low on the far
    /// wall) or `none`.
    #[serde(default)]
    pub vents: Vents,
    /// In each door into the room, a 4 × 8 in vent near its top, and outside it, where the
    /// building is not a dwelling, the warning notice (IMC 914); default true.
    pub door_fittings: Option<bool>,
    #[serde(default)]
    pub replace: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Vents {
    #[default]
    HighOutlet,
    LowOutlet,
    None,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Sauna {
    pub placed: Vec<ElementId>,
    pub heater: ElementId,
    pub heater_kw: f64,
    pub sizing: bath_rules::HeaterSizing,
    /// The room: its plan size and clear height (metres).
    pub length: f64,
    pub depth: f64,
    pub height: f64,
    /// Bench modules and the benches' tops.
    pub benches: u32,
    pub bench_tops: Vec<f64>,
    /// The doors into the room that took a vent (and a notice).
    pub doors: Vec<ElementId>,
}

/// Lays out a sauna (`bath.sauna`): benches in tiers along one wall and a heater sized from
/// the room.
pub(crate) fn sauna(txn: &mut Transaction<'_>, p: SaunaParams) -> Result<Sauna> {
    const MODULE: f64 = 0.6;
    const UPPER: f64 = 0.6;
    const LOWER: f64 = 0.45;
    const WALKWAY: f64 = 0.7;
    let tiers = p.tiers.unwrap_or(2).clamp(1, 2);
    let store = txn.store().clone();
    let level = store.resolve(&p.level).and_then(|l| store.get(l)).filter(|l| l.class == crate::levels::CLASS).ok_or_else(|| bad("bath.no_level", "no such level"))?;
    let floor = crate::levels::elevation(level);
    let lp = LayoutParams { template: String::new(), point: p.point, level: p.level.clone(), wet_wall_id: p.bench_wall_id.clone(), start: p.start, replace: false, fit_only: false };
    let room = find_room(&store, &lp, floor + 0.3)?;
    let height = match p.height {
        Some(h) => h,
        None => {
            let g = crate::walls::regenerate(&store, store.get(room.wet).expect("found"))?;
            g.base_z + g.height - floor
        }
    };
    let rules = bath_rules::sauna_rules(&store);
    let sizing = bath_rules::size_heater(&rules, room.length * room.depth * height, p.uninsulated_area.max(0.0), p.log_walls);
    let heater_kw = match (p.heater_kw, sizing.kw) {
        (Some(k), _) if k > 0.0 => k,
        (Some(_), _) => return Err(bad("bath.invalid_input", "a heater's power is above zero")),
        (None, Some(k)) => k,
        (None, None) => {
            let (lo, hi) = rules.heaters.iter().fold((f64::MAX, 0.0f64), |(a, b), h| (a.min(h[1]), b.max(h[2])));
            return Err(bad("bath.no_fit", format!("the design volume, {:.1} m³, is outside the heater table ({lo}–{hi} m³); give heater_kw", sizing.design_volume)).with_details(serde_json::to_value(&sizing).expect("serialises")));
        }
    };
    // The accessible bench is 610 mm deep, the lower tier (or the only one).
    const ACCESSIBLE: f64 = 0.61;
    let lower = if p.accessible { ACCESSIBLE } else { LOWER };
    let benches_deep = if tiers == 2 { UPPER + lower } else if p.accessible { ACCESSIBLE } else { UPPER };
    if room.depth < benches_deep + WALKWAY - 1e-6 {
        return Err(bad("bath.no_fit", format!("the room is {:.0} mm too narrow for {tiers} bench tier(s) ({:.0} mm deep) and a {:.0} mm walkway", (benches_deep + WALKWAY - room.depth) * 1000.0, benches_deep * 1000.0, WALKWAY * 1000.0)));
    }
    let modules = ((room.length / MODULE) + 1e-9).floor() as u32;
    if modules == 0 {
        return Err(bad("bath.no_fit", "the room is shorter than one 600 mm bench module"));
    }
    // The upper bench a headroom below the ceiling, the lower 450 mm below it.
    let upper_top = (height - rules.upper_bench_headroom).clamp(0.45, 1.2);
    let bench_tops = match (tiers, p.accessible) {
        (2, true) => vec![upper_top, 0.46],
        (2, false) => vec![upper_top, (upper_top - 0.45).max(0.4)],
        (_, true) => vec![0.46],
        (_, false) => vec![upper_top.min(0.9)],
    };

    if p.replace {
        let old: Vec<ElementId> = txn.store().iter().filter(|e| e.param(LAYOUT_KEY).and_then(|v| v.get("room")).and_then(|v| v.as_str()) == Some(room.key.as_str())).map(crate::element::Element::id).collect();
        for id in old {
            if txn.store().get(id).is_some() {
                txn.delete(id)?;
            }
        }
    }
    let names = ["Sauna heater, wall", "Sauna bench, upper 600", if p.accessible { "Sauna bench, accessible 600" } else { "Sauna bench, lower 600" }];
    let extras = ["Sauna vent 100", "Sauna vent 200", "Sauna thermostat sensor", "Sauna door vent 4 × 8 in", "Sauna notice"];
    let types = crate::bath_components::load(txn, crate::bath_components::LoadParams { names: names.iter().chain(&extras).map(|n| (*n).to_owned()).collect() })?;
    let tag = json!({ "template": "sauna", "room": room.key });
    let mut placed = Vec::new();
    let (w, run, start) = (room.w, room.run, room.start);
    let at = |a: f64, b: f64| [start[0] + run[0] * a - w[0] * b, start[1] + run[1] * a - w[1] * b];
    // Benches face away from the bench wall: their back (local +y) toward it.
    let facing = (-w[0]).atan2(w[1]);
    let mut put = |txn: &mut Transaction<'_>, name: &str, host: Option<ElementId>, q: [f64; 2], rotation: f64, offset: Option<f64>| -> Result<ElementId> {
        let id = crate::bath_components::place(
            txn,
            crate::mep_controls::PlaceParams {
                type_id: ElementRef::Id(types[name]),
                point: q,
                level: p.level.clone(),
                offset,
                rotation,
                host_wall: host.map(ElementRef::Id),
                side: None,
                work_plane: host.is_none(),
                controlled_ids: Vec::new(),
            },
        )?;
        let tag = tag.clone();
        txn.modify(id, |e| {
            e.params.insert(LAYOUT_KEY.into(), tag);
        })?;
        placed.push(id);
        Ok(id)
    };
    let a0 = (room.length - f64::from(modules) * MODULE) / 2.0;
    for k in 0..modules {
        let a = a0 + MODULE * (f64::from(k) + 0.5);
        // The accessible module's top is 460 mm in its geometry, the others' 450 mm.
        let top_of = |name: &str| if name == "Sauna bench, accessible 600" { 0.46 } else { 0.45 };
        if tiers == 2 {
            put(txn, names[1], None, at(a, UPPER / 2.0), facing, Some(bench_tops[0] - 0.45))?;
            put(txn, names[2], None, at(a, UPPER + lower / 2.0), facing, Some(bench_tops[1] - top_of(names[2])))?;
        } else if p.accessible {
            put(txn, names[2], None, at(a, ACCESSIBLE / 2.0), facing, Some(bench_tops[0] - top_of(names[2])))?;
        } else {
            put(txn, names[1], None, at(a, UPPER / 2.0), facing, Some(bench_tops[0] - 0.45))?;
        }
    }
    // The heater on the start wall, toward the far side from the benches.
    let heater = put(txn, names[0], Some(room.start_wall), at(0.1, room.depth - 0.4), 0.0, None)?;
    // The thermostat's sensor over the heater, 100 mm below the ceiling (IRC M1902: within 6 in).
    put(txn, extras[2], Some(room.start_wall), at(0.1, room.depth - 0.4), 0.0, Some(height - 0.1))?;
    // The vents: one maker's pattern or the other's.
    match p.vents {
        Vents::HighOutlet => {
            put(txn, extras[0], Some(room.start_wall), at(0.1, (room.depth - 0.85).max(0.1)), 0.0, Some(0.1))?;
            put(txn, extras[0], Some(room.end_wall), at(room.length - 0.1, room.depth - 0.4), 0.0, Some(height - 0.15))?;
        }
        Vents::LowOutlet => {
            put(txn, extras[0], Some(room.start_wall), at(0.1, (room.depth - 0.85).max(0.1)), 0.0, Some(0.15))?;
            put(txn, extras[1], Some(room.end_wall), at(room.length - 0.1, room.depth - 0.4), 0.0, Some(0.15))?;
        }
        Vents::None => {}
    }
    // The doors: a vent near each one's top, and outside it the notice where the building is not
    // a dwelling (IMC 914).
    let mut doors = Vec::new();
    if p.door_fittings.unwrap_or(true) {
        let outline = [at(0.0, 0.0), at(room.length, 0.0), at(room.length, room.depth), at(0.0, room.depth)];
        let notice = crate::codes::settings(&store).occupancy != crate::codes::Occupancy::Dwelling;
        for d in bath_rules::doors_into(txn.store(), &outline, &[room.wet, room.start_wall, room.opposite, room.end_wall]) {
            let n = d.wall.normal();
            let top = d.world([0.0, 0.0, d.ty.height - 0.15]);
            let vent = put(txn, extras[3], None, [top[0], top[1]], (-n[0]).atan2(n[1]), Some(top[2] - floor))?;
            let door = txn.store().get(d.id).map(|e| e.guid().to_string());
            txn.modify(vent, |e| {
                e.params.insert(bath_rules::IN_DOOR_KEY.into(), json!(door));
            })?;
            if notice {
                // Beside the door on the face outside the room, 1.5 m up, its back to the wall.
                let s = if bath_rules::plan_distance(&outline, { let c = d.world([0.0, 0.3 + d.wall.width / 2.0, 0.0]); [c[0], c[1]] }) > 0.0 { 1.0 } else { -1.0 };
                let q = d.world([d.ty.width / 2.0 + 0.25, s * d.wall.width / 2.0, 0.0]);
                put(txn, extras[4], None, [q[0], q[1]], (s * n[0]).atan2(-s * n[1]), Some(d.z0 - floor + 1.5))?;
            }
            doors.push(d.id);
        }
    }
    let settings = json!({ "uninsulated": p.uninsulated_area.max(0.0), "log": p.log_walls });
    txn.modify(heater, |e| {
        e.params.insert(bath_rules::HEATER_POWER.into(), json!({ "type": "IfcPowerMeasure", "value": heater_kw * 1000.0 }));
        e.params.insert(bath_rules::SAUNA_KEY.into(), settings);
    })?;
    Ok(Sauna { placed, heater, heater_kw, sizing, length: room.length, depth: room.depth, height, benches: modules * tiers, bench_tops, doors })
}

// ---- Steam rooms (ARC-BATH-015) ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SteamRoomParams {
    /// A point inside the room, in plan.
    pub point: [f64; 2],
    pub level: ElementRef,
    /// The wall the bench runs along; default the longer side. The ceiling is highest over it.
    pub bench_wall_id: Option<ElementRef>,
    /// The end the steam head goes at, as you face the bench wall.
    #[serde(default)]
    pub start: Start,
    /// The ceiling's high side (metres); default the rules' ideal, at most the walls' height.
    pub height: Option<f64>,
    /// The walls' finish, for the generator's design volume: `tile` (default), `acrylic`,
    /// `glass` or `stone` (as the rules name them).
    pub material: Option<String>,
    /// One wall is an outside wall.
    #[serde(default)]
    pub exterior_wall: bool,
    /// Lay the floor's falls to a centre drain when a Riviera floor is under the room (default
    /// true).
    pub floor_falls: Option<bool>,
    /// The bench to ADA §903 (24 in deep, 18 in high), as ADA §612 asks (ARC-BATH-016).
    #[serde(default)]
    pub accessible: bool,
    /// Give the room's four walls the vapour-proof *Steam Room Wall (TCNA SR614)* type (they keep
    /// their location lines).
    #[serde(default)]
    pub wall_assembly: bool,
    /// Place a steam generator outside the room, on the floor behind the head's wall (default
    /// true); move it to a dry, ventilated closet within the rules' run of the head.
    pub generator: Option<bool>,
    #[serde(default)]
    pub replace: bool,
}

/// The vapour-proof steam room wall's type name.
pub const STEAM_WALL: &str = "Steam Room Wall (TCNA SR614)";

/// The steam room wall type, made with its layers on first use: TCNA SR614 as the International
/// Masonry Institute details it, the steam side last.
pub(crate) fn steam_wall_type(txn: &mut Transaction<'_>) -> Result<ElementId> {
    if let Some(e) = txn.store().iter().find(|e| e.class == "IfcWallType" && e.name.as_deref() == Some(STEAM_WALL)) {
        return Ok(e.id());
    }
    let id = txn.create(
        crate::element::NewElement::new("IfcWallType")
            .name(STEAM_WALL)
            .param("@PredefinedType", "PARTITIONING")
            .param(crate::walls::RECIPE_KEY, json!({ "kind": crate::walls::WALL_TYPE_RECIPE, "width": 0.13 })),
    )?;
    let layers = json!({ "type_id": id.0, "layers": [
        { "function": "finish2", "material": "Gypsum board", "thickness": 0.0127 },
        { "function": "structure", "material": "Steel stud, 20 ga at 406 mm", "thickness": 0.092 },
        { "function": "membrane", "material": "Vapour retarder, lapped over the pan", "thickness": 0.0 },
        { "function": "substrate", "material": "Cement backer board", "thickness": 0.0127 },
        { "function": "membrane", "material": "Bonded waterproof membrane", "thickness": 0.0 },
        { "function": "finish1", "material": "Tile and setting bed", "thickness": 0.012 }
    ] });
    crate::compound::set_layers(txn, serde_json::from_value(layers).map_err(|e| bad("bath.params", e.to_string()))?)?;
    Ok(id)
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SteamRoom {
    pub placed: Vec<ElementId>,
    pub ceiling: ElementId,
    pub steam_head: ElementId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drain: Option<ElementId>,
    /// The room's plan size, the ceiling's high and low sides (metres) and its slope.
    pub length: f64,
    pub depth: f64,
    pub ceiling_high: f64,
    pub ceiling_low: f64,
    pub ceiling_slope: f64,
    /// m³: the room, and the design volume a generator is chosen for.
    pub volume: f64,
    pub design_volume: f64,
    pub benches: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generator: Option<ElementId>,
}

/// Lays out a steam room (`bath.steam_room`): a ceiling sloped to shed condensate, a tiled bench,
/// the steam head and its control, and the floor's falls to a drain.
pub(crate) fn steam_room(txn: &mut Transaction<'_>, p: SteamRoomParams) -> Result<SteamRoom> {
    const MODULE: f64 = 0.6;
    let bench_depth = if p.accessible { 0.61 } else { 0.45 };
    let store = txn.store().clone();
    let level = store.resolve(&p.level).and_then(|l| store.get(l)).filter(|l| l.class == crate::levels::CLASS).ok_or_else(|| bad("bath.no_level", "no such level"))?;
    let floor_z = crate::levels::elevation(level);
    let level_guid = level.guid().to_string();
    let lp = LayoutParams { template: String::new(), point: p.point, level: p.level.clone(), wet_wall_id: p.bench_wall_id.clone(), start: p.start, replace: false, fit_only: false };
    let mut room = find_room(&store, &lp, floor_z + 0.3)?;
    // The vapour-proof walls first: the room's faces move with them.
    if p.wall_assembly {
        let t = steam_wall_type(txn)?;
        let walls = [room.wet, room.opposite, room.start_wall, room.end_wall];
        crate::types::set_type(txn, crate::types::SetTypeParams { element_ids: walls.iter().map(|w| ElementRef::Id(*w)).collect(), type_id: ElementRef::Id(t) })?;
        room = find_room(txn.store(), &lp, floor_z + 0.3)?;
    }
    let store = txn.store().clone();
    let rules = bath_rules::steam_rules(&store);
    let walls = {
        let g = crate::walls::regenerate(&store, store.get(room.wet).expect("found"))?;
        g.base_z + g.height - floor_z
    };
    let high = p.height.unwrap_or(rules.ceiling_ideal).min(walls);
    let low = high - rules.ceiling_slope * room.depth;
    if low < 1.8 {
        return Err(bad("bath.no_fit", format!("sloped 1:{:.0} across the room's {:.0} mm, the ceiling would come down to {:.0} mm; a shallower room or a higher ceiling is needed", 1.0 / rules.ceiling_slope, room.depth * 1000.0, low * 1000.0)));
    }
    let material = p.material.clone().unwrap_or_else(|| "tile".to_owned());
    let volume = room.length * room.depth * (high + low) / 2.0;
    let design_volume = bath_rules::steam_design_volume(&rules, volume, high, &material, p.exterior_wall)?;
    let modules = ((room.length / MODULE) + 1e-9).floor() as u32;
    if room.depth < bench_depth + 0.7 {
        return Err(bad("bath.no_fit", format!("the room is {:.0} mm too narrow for a bench and a 700 mm walkway", (bench_depth + 0.7 - room.depth) * 1000.0)));
    }

    if p.replace {
        let old: Vec<ElementId> = txn.store().iter().filter(|e| e.param(LAYOUT_KEY).and_then(|v| v.get("room")).and_then(|v| v.as_str()) == Some(room.key.as_str())).map(crate::element::Element::id).collect();
        for id in old {
            if txn.store().get(id).is_some() {
                txn.delete(id)?;
            }
        }
    }
    let tag = json!({ "template": "steam_room", "room": room.key });
    let (w, run, start) = (room.w, room.run, room.start);
    let at = |a: f64, b: f64| [start[0] + run[0] * a - w[0] * b, start[1] + run[1] * a - w[1] * b];
    let mut placed = Vec::new();
    // The ceiling over the room, its low side on the far wall and rising toward the bench wall;
    // 10 mm in from the walls, as its sloped edges lean out by its thickness's sine.
    let corners = [at(0.0, 0.0), at(room.length, 0.0), at(room.length, room.depth), at(0.0, room.depth)];
    let inset = [at(0.01, 0.01), at(room.length - 0.01, 0.01), at(room.length - 0.01, room.depth - 0.01), at(0.01, room.depth - 0.01)];
    let loop_: Vec<serde_json::Value> = (0..4).map(|k| json!({ "line": { "start": inset[k], "end": inset[(k + 1) % 4] } })).collect();
    let cp: crate::floors::CreateParams = serde_json::from_value(json!({ "loops": [loop_], "level_id": ElementRef::Guid(level.guid().clone()), "offset": low })).map_err(|e| bad("bath.params", e.to_string()))?;
    let ceiling = crate::floors::create_kind(txn, crate::floors::SlabKind::Ceiling, cp)?;
    let out_of = |q: [f64; 2]| -((q[0] - start[0]) * w[0] + (q[1] - start[1]) * w[1]);
    let (slope, depth) = (rules.ceiling_slope, room.depth);
    crate::shape_edit::set_corner_offsets(txn, ceiling, |q| slope * (depth - out_of(q)).clamp(0.0, depth))?;
    let tag2 = tag.clone();
    txn.modify(ceiling, |e| {
        e.params.insert(LAYOUT_KEY.into(), tag2);
    })?;
    placed.push(ceiling);

    let names = [if p.accessible { "Steam room bench, accessible 600" } else { "Steam room bench 600" }, "Steam head", "Steam control", "Steam generator"];
    let types = crate::bath_components::load(txn, crate::bath_components::LoadParams { names: names.iter().map(|n| (*n).to_owned()).collect() })?;
    let facing = (-w[0]).atan2(w[1]);
    let mut put = |txn: &mut Transaction<'_>, name: &str, host: Option<ElementId>, q: [f64; 2], rotation: f64| -> Result<ElementId> {
        let id = crate::bath_components::place(
            txn,
            crate::mep_controls::PlaceParams {
                type_id: ElementRef::Id(types[name]),
                point: q,
                level: p.level.clone(),
                offset: None,
                rotation,
                host_wall: host.map(ElementRef::Id),
                side: None,
                work_plane: host.is_none(),
                controlled_ids: Vec::new(),
            },
        )?;
        let tag = tag.clone();
        txn.modify(id, |e| {
            e.params.insert(LAYOUT_KEY.into(), tag);
        })?;
        placed.push(id);
        Ok(id)
    };
    // The bench along the bench wall, the head on the start wall across the room from it, the
    // control on the far wall by the start end.
    let a0 = (room.length - f64::from(modules) * MODULE) / 2.0;
    for k in 0..modules {
        put(txn, names[0], None, at(a0 + MODULE * (f64::from(k) + 0.5), bench_depth / 2.0), facing)?;
    }
    let head_b = (bench_depth + rules.head_clearance + 0.1).max(room.depth - 0.35).min(room.depth - 0.1);
    let steam_head = put(txn, names[1], Some(room.start_wall), at(0.1, head_b), 0.0)?;
    put(txn, names[2], Some(room.opposite), at(0.4, room.depth - 0.1), 0.0)?;
    // The generator behind the head's wall, outside the room, its back to the wall.
    let mut generator = None;
    if p.generator.unwrap_or(true) {
        let thick = crate::walls::regenerate(txn.store(), txn.store().get(room.start_wall).expect("found"))?.width;
        generator = Some(put(txn, names[3], None, at(-thick - 0.01, head_b), (-run[0]).atan2(run[1]))?);
    }
    // The floor's falls to a centre drain.
    let mut drain = None;
    if p.floor_falls.unwrap_or(true)
        && let Some(floor) = bath_rules::floor_under(txn.store(), &level_guid, [(corners[0][0] + corners[2][0]) / 2.0, (corners[0][1] + corners[2][1]) / 2.0])
    {
        let drain_type = crate::mep::load_starters(txn, crate::mep::LoadStartersParams { names: vec!["Floor Drain".into()] })?["Floor Drain"];
        let serves = txn.store().get(steam_head).map(|e| e.guid().to_string());
        let d = bath_rules::lay_falls(txn, floor, &corners, bath_rules::DrainAt::Centre, rules.floor_fall, None, ElementRef::Guid(level.guid().clone()), drain_type, serves)?;
        drain = Some(d);
    }
    Ok(SteamRoom { placed, ceiling, steam_head, drain, length: room.length, depth: room.depth, ceiling_high: high, ceiling_low: low, ceiling_slope: slope, volume, design_volume, benches: modules, generator })
}

// ---- Bathroom elevations (ARC-BATH-018) ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ElevationsParams {
    /// A point inside the room, in plan.
    pub point: [f64; 2],
    pub level: ElementRef,
    /// The views' names start with it (default `Bathroom`): `Bathroom - North` …
    pub name: Option<String>,
    /// The views' scale, 1:n (default 25).
    pub scale: Option<f64>,
    /// Place the four views on a new A3 sheet (default true).
    pub sheet: Option<bool>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Elevations {
    /// The interior elevations, each looking at one wall: their ids and the walls they face.
    pub views: Vec<ElementId>,
    pub walls: Vec<ElementId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sheet: Option<ElementId>,
}

/// Interior elevations of a room's four walls, cropped to the room, on a sheet
/// (`bath.elevations`).
pub(crate) fn elevations(txn: &mut Transaction<'_>, p: ElevationsParams) -> Result<Elevations> {
    let store = txn.store().clone();
    let level = store.resolve(&p.level).and_then(|l| store.get(l)).filter(|l| l.class == crate::levels::CLASS).ok_or_else(|| bad("bath.no_level", "no such level"))?;
    let floor_z = crate::levels::elevation(level);
    let lp = LayoutParams { template: String::new(), point: p.point, level: p.level.clone(), wet_wall_id: None, start: Start::Left, replace: false, fit_only: false };
    let room = find_room(&store, &lp, floor_z + 0.3)?;
    let scale = p.scale.unwrap_or(25.0);
    if !(scale >= 1.0 && scale.is_finite()) {
        return Err(bad("bath.invalid_input", "a scale is 1:n with n at least 1"));
    }
    let name = p.name.clone().unwrap_or_else(|| "Bathroom".to_owned());
    let top = [room.wet, room.opposite, room.start_wall, room.end_wall]
        .iter()
        .filter_map(|w| crate::walls::regenerate(&store, store.get(*w)?).ok())
        .map(|g| g.base_z + g.height)
        .fold(floor_z + 2.4, f64::max);
    let (w, run, start) = (room.w, room.run, room.start);
    let at = |a: f64, b: f64| [start[0] + run[0] * a - w[0] * b, start[1] + run[1] * a - w[1] * b];
    // Each wall: where the view plane stands (just inside the opposite wall), the way it looks,
    // its width and how far it sees (to just behind the wall's face).
    let faces = [
        (room.wet, at(room.length / 2.0, room.depth - 0.01), w, room.length, room.depth),
        (room.opposite, at(room.length / 2.0, 0.01), [-w[0], -w[1]], room.length, room.depth),
        (room.start_wall, at(room.length - 0.01, room.depth / 2.0), [-run[0], -run[1]], room.depth, room.length),
        (room.end_wall, at(0.01, room.depth / 2.0), run, room.depth, room.length),
    ];
    let compass = |d: [f64; 2]| {
        if d[1].abs() >= d[0].abs() {
            if d[1] > 0.0 { "North" } else { "South" }
        } else if d[0] > 0.0 {
            "East"
        } else {
            "West"
        }
    };
    let mut views = Vec::new();
    let mut walls = Vec::new();
    // Plans draw interior elevations as one marker at the room's middle, each arrow labelled
    // with its direction's initial.
    let middle = at(room.length / 2.0, room.depth / 2.0);
    for (wall, point, look, width, deep) in faces {
        let view = crate::sections::create_elevation(
            txn,
            crate::sections::CreateElevationParams { point, look, width: Some(width), far_offset: Some(json!(deep + 0.05)), name: Some(format!("{name} - {}", compass(look))) },
        )?;
        txn.modify(view, |e| {
            let mut v = e.params.get(crate::views::VIEW_KEY).cloned().unwrap_or_else(|| json!({}));
            v["scale"] = json!(scale);
            v["marker"] = json!({ "at": middle, "label": &compass(look)[..1] });
            e.params.insert(crate::views::VIEW_KEY.into(), v);
        })?;
        crate::crop::set_crop(
            txn,
            crate::crop::SetCropParams { view_id: ElementRef::Id(view), enabled: Some(true), visible: Some(false), shape: Some(crate::crop::Rect { min: [0.0, floor_z - 0.05], max: [width, top + 0.05] }) },
        )?;
        views.push(view);
        walls.push(wall);
    }
    let sheet = if p.sheet.unwrap_or(true) {
        let sheet = crate::sheets::create(txn, crate::sheets::CreateParams { number: None, name: Some(format!("{name} elevations")), size: Some("A3".into()) })?;
        // Two by two on A3 (420 × 297 mm), clear of the title block on the right.
        let spots = [[95.0, 210.0], [255.0, 210.0], [95.0, 90.0], [255.0, 90.0]];
        for (v, at) in views.iter().zip(spots) {
            crate::sheets::place_view(txn, crate::sheets::PlaceViewParams { sheet_id: ElementRef::Id(sheet), view_id: ElementRef::Id(*v), position: Some(at) })?;
        }
        Some(sheet)
    } else {
        None
    };
    Ok(Elevations { views, walls, sheet })
}

// ---- Bathroom schedules (ARC-BATH-018) ----

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SchedulesParams {
    /// Place the schedules on this sheet too, down its left side.
    pub sheet_id: Option<ElementRef>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Schedules {
    /// The plumbing fixture schedule (by room and type, with fixture units and totals) and the
    /// accessories schedule (furniture by room and type).
    pub fixtures: ElementId,
    pub accessories: ElementId,
}

/// A plumbing fixture schedule with fixture units by room, and an accessories schedule
/// (`bath.schedules`).
pub(crate) fn schedules(txn: &mut Transaction<'_>, p: SchedulesParams) -> Result<Schedules> {
    let make = |txn: &mut Transaction<'_>, category: &str, name: &str, fields: &[&str]| -> Result<ElementId> {
        let id = crate::schedules::create(txn, crate::schedules::CreateParams { categories: None, category: category.into(), name: Some(name.into()), fields: Some(fields.iter().map(|f| (*f).to_owned()).collect()) })?;
        crate::schedules::set_sort(
            txn,
            crate::schedules::SetSortParams {
                schedule_id: ElementRef::Id(id),
                levels: ["component.room", "type"].iter().map(|f| crate::schedules::SortLevel { field: (*f).to_owned(), descending: false }).collect(),
                itemize: Some(false),
                grand_totals: Some(true),
            },
        )?;
        Ok(id)
    };
    let fixtures = make(txn, "cat.plumbing_fixtures", "Plumbing Fixture Schedule", &["component.room", "type", "count", "fixture.drainage_units", "fixture.supply_units"])?;
    let accessories = make(txn, "cat.furniture", "Bathroom Accessories", &["component.room", "type", "count"])?;
    if let Some(sheet) = &p.sheet_id {
        for (id, at) in [(fixtures, [20.0, 280.0]), (accessories, [20.0, 150.0])] {
            crate::sheets::place_schedule(txn, crate::sheets::PlaceScheduleParams { sheet_id: sheet.clone(), schedule_id: ElementRef::Id(id), position: Some(at) })?;
        }
    }
    Ok(Schedules { fixtures, accessories })
}
