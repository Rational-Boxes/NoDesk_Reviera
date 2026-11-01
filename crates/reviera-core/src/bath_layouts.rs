// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

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
        if z.across.iter().all(|a| a.0 == AcrossFrom::Centre) && z.out.iter().any(|o| o.0 == OutFrom::Wall) {
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
    length: f64,
    depth: f64,
    key: String,
}

fn find_room(store: &Store, p: &LayoutParams, z: f64) -> Result<Room> {
    let wet_given = p
        .wet_wall_id
        .as_ref()
        .map(|r| store.resolve(r).filter(|id| store.get(*id).is_some_and(|e| crate::walls::wall_recipe(e).is_some())).ok_or_else(|| bad("bath.no_wall", "no such Reviera wall")))
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
    let (start, run, start_wall) = match p.start {
        Start::Left => ([p.point[0] + left[0] * t_left, p.point[1] + left[1] * t_left], [-left[0], -left[1]], wall_left),
        Start::Right => ([p.point[0] - left[0] * t_right, p.point[1] - left[1] * t_right], left, wall_right),
    };
    // Onto the wet wall's line: the run's start at the wet face.
    let start = [start[0] + w[0] * tw, start[1] + w[1] * tw];
    let mut walls = [wet, wall_left, wall_right, opposite.1].map(|id| store.get(id).expect("found").guid().to_string());
    walls.sort();
    Ok(Room { wet, w, start, run, start_wall, length: t_left + t_right, depth: tw + opposite.0, key: walls.join("|") })
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
