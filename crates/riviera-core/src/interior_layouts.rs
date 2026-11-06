// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Room layouts (ARC-INT-001, `INTERIORS_PLAN.md` §4.2): a bedroom, living room, dining room,
//! home office, private office or meeting room furnished from a template, fitted to a
//! rectangular room closed by walls.
//!
//! The room's frame has its origin at the middle of the **head wall's** face: `along` runs
//! along that wall (positive to the right as one faces it from the room), `out` into the room.
//! Each template piece is a generic furnishing ([`crate::furnishings`]) set against the head wall,
//! the opposite wall, a side wall, or free in the room, facing a way. The layout fits when every
//! piece, and every use zone the rule packs in use give it ([`crate::rules`]), stays inside the
//! room and clear of the other pieces. When it does not, nothing is placed and the shortfall is
//! reported.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::store::Store;
use crate::txn::Transaction;

/// The tag on a layout's pieces: its template and the room's walls.
pub const LAYOUT_KEY: &str = "rxd:interior_layout";
/// The gap left between a piece and a wall, or between neighbouring pieces.
const GAP: f64 = 0.02;
/// How far a room's walls are looked for.
const REACH: f64 = 30.0;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// Which way a piece's front (its −y side) faces, in the room's frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Facing {
    /// Into the room, away from the head wall.
    Away,
    /// Toward the head wall.
    Toward,
    /// Along the head wall, to the right / left.
    Right,
    Left,
}

/// Where a piece sits across the room (out from the head wall).
#[derive(Debug, Clone, Copy)]
enum Out {
    /// Against the head wall.
    Head,
    /// Against the opposite wall.
    Opposite,
    /// This far from the head wall's face (its centre).
    At(f64),
    /// In the middle of the room, plus this.
    Middle(f64),
}

/// Where a piece sits along the head wall.
#[derive(Debug, Clone, Copy)]
enum Along {
    /// From the middle (its centre).
    Mid(f64),
    /// Against the left / right wall.
    LeftWall,
    RightWall,
}

#[derive(Debug, Clone)]
struct Piece {
    item: &'static str,
    along: Along,
    out: Out,
    facing: Facing,
}

fn p(item: &'static str, along: Along, out: Out, facing: Facing) -> Piece {
    Piece { item, along, out, facing }
}

/// A layout template.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Template {
    pub id: &'static str,
    pub title: &'static str,
    /// The furnishings it places.
    pub items: Vec<&'static str>,
}

/// The catalogue's sizes, by name: width (x), depth (y), height.
pub(crate) fn sizes() -> BTreeMap<String, [f64; 3]> {
    crate::furnishings::list(&Store::default(), None).expect("the catalogue lists").into_iter().map(|i| (i.name, i.size)).collect()
}

type Build = fn(&BTreeMap<String, [f64; 3]>) -> Vec<Piece>;

fn defs() -> Vec<(&'static str, &'static str, Build)> {
    use Along::*;
    use Facing::*;
    use Out::*;
    vec![
        ("bedroom_queen", "Bedroom, queen bed", |s| {
            let (b, n) = (s["Bed, queen 60 × 80 in"][0], s["Nightstand"][0]);
            vec![
                p("Bed, queen 60 × 80 in", Mid(0.0), Head, Away),
                p("Nightstand", Mid(-(b + n) / 2.0 - GAP), Head, Away),
                p("Nightstand", Mid((b + n) / 2.0 + GAP), Head, Away),
                p("Dresser", Mid(0.0), Opposite, Toward),
            ]
        }),
        ("bedroom_king", "Bedroom, king bed", |s| {
            let (b, n) = (s["Bed, king 76 × 80 in"][0], s["Nightstand"][0]);
            vec![
                p("Bed, king 76 × 80 in", Mid(0.0), Head, Away),
                p("Nightstand", Mid(-(b + n) / 2.0 - GAP), Head, Away),
                p("Nightstand", Mid((b + n) / 2.0 + GAP), Head, Away),
                p("Dresser", Mid(0.0), Opposite, Toward),
            ]
        }),
        ("bedroom_twin", "Bedroom, two twin beds", |s| {
            // 0.8 m between the beds: room for the nightstand, and for a 30 in clear space beside
            // each bed (ADA 806.2.3) without the other bed in it.
            let b = s["Bed, twin 38 × 75 in"][0];
            let off = b / 2.0 + 0.4;
            vec![
                p("Bed, twin 38 × 75 in", Mid(-off), Head, Away),
                p("Nightstand", Mid(0.0), Head, Away),
                p("Bed, twin 38 × 75 in", Mid(off), Head, Away),
                p("Dresser", Mid(0.0), Opposite, Toward),
            ]
        }),
        ("bedroom_uk_double", "Bedroom, double bed (UK)", |s| {
            let (b, n) = (s["Bed, principal double 1500 × 2000"][0], s["Nightstand"][0]);
            vec![
                p("Bed, principal double 1500 × 2000", Mid(0.0), Head, Away),
                p("Nightstand", Mid(-(b + n) / 2.0 - GAP), Head, Away),
                p("Nightstand", Mid((b + n) / 2.0 + GAP), Head, Away),
                p("Wardrobe, double 1200 × 600", RightWall, Opposite, Left),
                p("Chest of drawers 750 × 450", Mid(0.0), Opposite, Toward),
            ]
        }),
        ("living_room", "Living room", |s| {
            // The sofa against the head wall, a coffee table 450 mm in front of it, an armchair
            // each side facing in, the storage unit opposite (research/15 D2: 12–18 in).
            let (sd, cd) = (s["Sofa, 3-seat 1850 × 850"][1], s["Coffee table 1050 × 500"][1]);
            let table = GAP + sd + 0.45 + cd / 2.0;
            let (cw, aw) = (s["Coffee table 1050 × 500"][0], s["Armchair 850 × 850"][0]);
            let side = cw / 2.0 + 0.45 + aw / 2.0;
            vec![
                p("Sofa, 3-seat 1850 × 850", Mid(0.0), Head, Away),
                p("Coffee table 1050 × 500", Mid(0.0), At(table), Away),
                p("Armchair 850 × 850", Mid(-side), At(table), Right),
                p("Armchair 850 × 850", Mid(side), At(table), Left),
                p("Storage unit 1500 × 500", Mid(0.0), Opposite, Toward),
            ]
        }),
        ("dining_4", "Dining room, 4 seats", |s| dining(s, "Dining table, 4 seats 48 × 30 in", 2)),
        ("dining_6", "Dining room, 6 seats", |s| dining(s, "Dining table, 6 seats 72 × 36 in", 3)),
        ("home_office", "Home office", |s| {
            let d = s["Desk 60 × 30 in"][1];
            vec![
                p("Desk 60 × 30 in", Mid(0.0), Head, Away),
                p("Task chair", Mid(0.0), At(GAP + d + 0.1 + s["Task chair"][1] / 2.0), Toward),
                p("Bookcase 36 in", LeftWall, Middle(0.0), Right),
            ]
        }),
        ("private_office", "Private office", |s| {
            // The desk against the head wall, its chair, two visitor chairs facing it, a bookcase.
            let d = s["Desk 60 × 30 in"][1];
            let chair = s["Dining chair"];
            let visitors = GAP + d + 0.1 + s["Task chair"][1] + 0.9 + chair[1] / 2.0;
            vec![
                p("Desk 60 × 30 in", Mid(0.0), Head, Away),
                p("Task chair", Mid(0.0), At(GAP + d + 0.1 + s["Task chair"][1] / 2.0), Toward),
                p("Dining chair", Mid(-0.35), At(visitors), Toward),
                p("Dining chair", Mid(0.35), At(visitors), Toward),
                p("Bookcase 36 in", RightWall, Head, Left),
            ]
        }),
        ("meeting_room_8", "Meeting room, 8 seats", |s| {
            // A 96 × 42 in table in the middle, three chairs each side and one at each end.
            let t = s["Conference table, 8 seats 96 × 42 in"];
            let c = s["Task chair"];
            let side = t[1] / 2.0 + 0.1 + c[1] / 2.0;
            let end = t[0] / 2.0 + 0.1 + c[1] / 2.0;
            let mut v = vec![p("Conference table, 8 seats 96 × 42 in", Mid(0.0), Middle(0.0), Away)];
            for a in [-0.8, 0.0, 0.8] {
                v.push(p("Task chair", Mid(a), Middle(-side), Away));
                v.push(p("Task chair", Mid(a), Middle(side), Toward));
            }
            v.push(p("Task chair", Mid(-end), Middle(0.0), Right));
            v.push(p("Task chair", Mid(end), Middle(0.0), Left));
            v
        }),
    ]
}

/// A dining table in the middle of the room with `per_side` chairs along each long side, 50 mm
/// clear of its edge (pieces may not overlap in plan; a chair's pull-back is in the table's
/// zone).
fn dining(s: &BTreeMap<String, [f64; 3]>, table: &'static str, per_side: usize) -> Vec<Piece> {
    let t = s[table];
    let c = s["Dining chair"];
    let off = t[1] / 2.0 + 0.05;
    let pitch = t[0] / per_side as f64;
    let mut v = vec![p(table, Along::Mid(0.0), Out::Middle(0.0), Facing::Away)];
    for k in 0..per_side {
        let a = -t[0] / 2.0 + pitch * (k as f64 + 0.5);
        v.push(p("Dining chair", Along::Mid(a), Out::Middle(-off - c[1] / 2.0), Facing::Away));
        v.push(p("Dining chair", Along::Mid(a), Out::Middle(off + c[1] / 2.0), Facing::Toward));
    }
    v
}

pub fn templates() -> Vec<Template> {
    let s = sizes();
    defs()
        .into_iter()
        .map(|(id, title, build)| {
            let mut items: Vec<&'static str> = build(&s).into_iter().map(|p| p.item).collect();
            items.dedup();
            Template { id, title, items }
        })
        .collect()
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoParams {}

/// `interior.layouts`.
pub(crate) fn list(_: &Store, _: Option<NoParams>) -> Result<Vec<Template>> {
    Ok(templates())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LayoutParams {
    pub template: String,
    /// A point inside the room, in plan.
    pub point: [f64; 2],
    pub level: ElementRef,
    /// The wall the main piece goes against (the bed's head, the sofa's back, the desk); default
    /// one of the room's longer walls.
    pub wall_id: Option<ElementRef>,
    /// Remove the pieces an earlier layout placed in this room first.
    #[serde(default)]
    pub replace: bool,
    /// Report the fit without placing anything.
    #[serde(default)]
    pub fit_only: bool,
}

/// How the template fits (metres).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Fit {
    /// Along the head wall: available, needed, short (0 when it fits).
    pub length: f64,
    pub length_needed: f64,
    pub length_short: f64,
    /// Out from the head wall to the opposite wall.
    pub depth: f64,
    pub depth_needed: f64,
    pub depth_short: f64,
    /// Use zones another piece of the layout stands in: (piece, zone rule, other piece).
    pub conflicts: Vec<(String, String, String)>,
}

impl Fit {
    pub fn fits(&self) -> bool {
        self.length_short <= 1e-6 && self.depth_short <= 1e-6 && self.conflicts.is_empty()
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Layout {
    pub placed: Vec<ElementId>,
    pub fit: Fit,
    /// The rule packs whose zones were fitted.
    pub packs: Vec<String>,
}

/// The room round a point: its head wall's face midpoint, the frame and its size.
struct Room {
    origin: [f64; 2],
    along: [f64; 2],
    out: [f64; 2],
    length: f64,
    depth: f64,
    key: String,
}

fn find_room(store: &Store, p: &LayoutParams, z: f64) -> Result<Room> {
    let ray = |d: [f64; 2]| crate::bath_rules::ray_to_walls(store, p.point, d, z, REACH);
    let given = p
        .wall_id
        .as_ref()
        .map(|r| store.resolve(r).filter(|id| store.get(*id).is_some_and(|e| crate::walls::wall_recipe(e).is_some())).ok_or_else(|| bad("interior.no_wall", "no such Riviera wall")))
        .transpose()?;
    let axis = match given {
        Some(w) => crate::walls::regenerate(store, store.get(w).expect("resolved"))?.direction(),
        None => {
            let near = [[1.0, 0.0], [-1.0, 0.0], [0.0, 1.0], [0.0, -1.0]].iter().filter_map(|d| ray(*d)).min_by(|a, b| a.0.total_cmp(&b.0)).ok_or_else(|| bad("interior.open_room", "no walls around the point"))?;
            crate::walls::regenerate(store, store.get(near.1).expect("found"))?.direction()
        }
    };
    let (u, v) = (axis, [-axis[1], axis[0]]);
    let look = |d: [f64; 2], name: &str| ray(d).ok_or_else(|| bad("interior.open_room", format!("the room is open {name}: no wall that way")));
    let (pu, mu) = (look(u, "one way along its walls")?, look([-u[0], -u[1]], "the other way along its walls")?);
    let (pv, mv) = (look(v, "to one side")?, look([-v[0], -v[1]], "to the other side")?);
    let (len_u, len_v) = (pu.0 + mu.0, pv.0 + mv.0);
    // The head wall and the way into the room from it.
    let (head_t, n) = match given {
        Some(id) if id == pv.1 => (pv.0, [-v[0], -v[1]]),
        Some(id) if id == mv.1 => (mv.0, v),
        Some(id) if id == pu.1 => (pu.0, [-u[0], -u[1]]),
        Some(id) if id == mu.1 => (mu.0, u),
        Some(_) => return Err(bad("interior.no_wall", "the wall is not one of the room's walls")),
        // One of the longer walls.
        None if len_u >= len_v => (mv.0, v),
        None => (mu.0, u),
    };
    let head_face = [p.point[0] - n[0] * head_t, p.point[1] - n[1] * head_t];
    // Along the head wall, positive to the right as one faces it from the room.
    let along = [-n[1], n[0]];
    let (right, left) = (look(along, "to the right")?, look([-along[0], -along[1]], "to the left")?);
    let opposite = look(n, "opposite the head wall")?;
    let mid = (right.0 - left.0) / 2.0;
    let origin = [head_face[0] + along[0] * mid, head_face[1] + along[1] * mid];
    let mut walls: Vec<String> = [pu.1, mu.1, pv.1, mv.1].iter().filter_map(|id| store.get(*id)).map(|e| e.guid().to_string()).collect();
    walls.sort();
    Ok(Room { origin, along, out: n, length: right.0 + left.0, depth: head_t + opposite.0, key: walls.join("|") })
}

/// A piece's footprint in the room's frame and its placement.
struct Placed {
    item: &'static str,
    /// Set against a side wall, or the opposite wall: it reaches that wall whatever the room's
    /// size, so it is checked against the other pieces, not counted in the size needed.
    to_side: bool,
    to_opposite: bool,
    /// along range, out range.
    rect: [[f64; 2]; 2],
    centre: [f64; 2],
    rotation: f64,
}

fn place_piece(piece: &Piece, size: [f64; 3], room: &Room) -> Placed {
    // Its extents along and out, turned by its facing.
    let (ea, eo) = match piece.facing {
        Facing::Away | Facing::Toward => (size[0], size[1]),
        Facing::Left | Facing::Right => (size[1], size[0]),
    };
    let a = match piece.along {
        Along::Mid(x) => x,
        Along::LeftWall => -room.length / 2.0 + GAP + ea / 2.0,
        Along::RightWall => room.length / 2.0 - GAP - ea / 2.0,
    };
    let o = match piece.out {
        Out::Head => GAP + eo / 2.0,
        Out::Opposite => room.depth - GAP - eo / 2.0,
        Out::At(x) => x,
        Out::Middle(x) => room.depth / 2.0 + x,
    };
    // The direction the front faces, in plan.
    let f = match piece.facing {
        Facing::Away => room.out,
        Facing::Toward => [-room.out[0], -room.out[1]],
        Facing::Right => room.along,
        Facing::Left => [-room.along[0], -room.along[1]],
    };
    // A copy's local −y is its front: turned so that (0, −1) maps to f.
    let rotation = f[0].atan2(-f[1]);
    let centre = [room.origin[0] + room.along[0] * a + room.out[0] * o, room.origin[1] + room.along[1] * a + room.out[1] * o];
    Placed {
        item: piece.item,
        to_side: matches!(piece.along, Along::LeftWall | Along::RightWall),
        to_opposite: matches!(piece.out, Out::Opposite),
        rect: [[a - ea / 2.0, a + ea / 2.0], [o - eo / 2.0, o + eo / 2.0]],
        centre,
        rotation,
    }
}

/// A plan point in the room's frame.
fn to_frame(room: &Room, q: [f64; 2]) -> [f64; 2] {
    let d = [q[0] - room.origin[0], q[1] - room.origin[1]];
    [d[0] * room.along[0] + d[1] * room.along[1], d[0] * room.out[0] + d[1] * room.out[1]]
}

fn overlaps(a: [[f64; 2]; 2], b: [[f64; 2]; 2]) -> bool {
    a[0][0] < b[0][1] - 1e-6 && b[0][0] < a[0][1] - 1e-6 && a[1][0] < b[1][1] - 1e-6 && b[1][0] < a[1][1] - 1e-6
}

/// Lays out a room (`interior.layout`).
pub(crate) fn layout(txn: &mut Transaction<'_>, p: LayoutParams) -> Result<Layout> {
    let all = defs();
    let (template_id, _, build) = all.iter().find(|t| t.0 == p.template).copied().ok_or_else(|| {
        let ids: Vec<&str> = all.iter().map(|t| t.0).collect();
        bad("interior.no_template", format!("no layout template {:?}; there are: {}", p.template, ids.join(", ")))
    })?;
    let store = txn.store().clone();
    let level = store.resolve(&p.level).and_then(|l| store.get(l)).ok_or_else(|| bad("element.not_found", "no such level"))?;
    let level_guid = level.guid().to_string();
    let z = crate::levels::elevation_of(&store, &level_guid).unwrap_or(0.0) + 0.6;
    let room = find_room(&store, &p, z)?;
    let s = sizes();
    let pieces: Vec<Placed> = build(&s).iter().map(|pc| place_piece(pc, s[pc.item], &room)).collect();
    // Earlier pieces of a layout in this room.
    let earlier: Vec<ElementId> = store
        .iter()
        .filter(|e| e.param(LAYOUT_KEY).and_then(|v| v.get("room")).and_then(Value::as_str) == Some(room.key.as_str()))
        .map(crate::element::Element::id)
        .collect();
    if p.replace && !p.fit_only {
        for e in &earlier {
            txn.delete(*e)?;
        }
    }
    // Place them, then measure the pieces and their use zones in the room's frame.
    let names: Vec<String> = pieces.iter().map(|x| x.item.to_owned()).collect();
    let types = crate::furnishings::load(txn, crate::furnishings::LoadParams { names })?;
    let mut placed = Vec::new();
    for x in &pieces {
        let id = crate::mesh_objects::place(
            txn,
            crate::mesh_objects::PlaceParams { type_id: ElementRef::Id(types[x.item]), point: x.centre, level: p.level.clone(), offset: 0.0, rotation: x.rotation, mirror: false, scale: 1.0 },
        )?;
        let tag = json!({ "template": template_id, "room": room.key });
        txn.modify(id, |e| {
            e.params.insert(LAYOUT_KEY.into(), tag);
        })?;
        placed.push(id);
    }
    let packs = crate::rules::all_active(txn.store());
    let (mut lo_a, mut hi_a, mut hi_o) = (f64::MAX, f64::MIN, 0.0f64);
    let grow = |r: [[f64; 2]; 2], lo_a: &mut f64, hi_a: &mut f64, hi_o: &mut f64| {
        *lo_a = lo_a.min(r[0][0]);
        *hi_a = hi_a.max(r[0][1]);
        *hi_o = hi_o.max(r[1][1]);
    };
    let mut conflicts = Vec::new();
    for (k, x) in pieces.iter().enumerate() {
        let (a, o) = ([x.rect[0][0] - GAP, x.rect[0][1] + GAP], [0.0, x.rect[1][1] + GAP]);
        let (mut l, mut h, mut d) = (lo_a, hi_a, hi_o);
        grow([a, o], &mut l, &mut h, &mut d);
        if !x.to_side {
            (lo_a, hi_a) = (l, h);
        }
        if !x.to_opposite {
            hi_o = d;
        }
        // Pieces must not stand in each other.
        for other in &pieces[k + 1..] {
            if overlaps(x.rect, other.rect) {
                conflicts.push((x.item.to_owned(), "overlaps".to_owned(), other.item.to_owned()));
            }
        }
    }
    let store_now = txn.store().clone();
    for (k, id) in placed.iter().enumerate() {
        let e = store_now.get(*id).expect("placed");
        for z in crate::rules::zones_of(&store_now, e, &packs) {
            let pts: Vec<[f64; 2]> = z.clearance.mesh.0.iter().map(|q| to_frame(&room, [q[0], q[1]])).collect();
            let r = [
                [pts.iter().map(|q| q[0]).fold(f64::MAX, f64::min), pts.iter().map(|q| q[0]).fold(f64::MIN, f64::max)],
                [pts.iter().map(|q| q[1]).fold(f64::MAX, f64::min), pts.iter().map(|q| q[1]).fold(f64::MIN, f64::max)],
            ];
            grow(r, &mut lo_a, &mut hi_a, &mut hi_o);
            // A zone reaching behind the head wall is short of depth too.
            if r[1][0] < -1e-6 {
                hi_o = hi_o.max(room.depth - r[1][0]);
            }
            for (j, other) in pieces.iter().enumerate() {
                let allowed = store_now.get(placed[j]).is_some_and(|o| z.zone.allows(&store_now, o));
                if j != k && !allowed && overlaps(r, other.rect) {
                    conflicts.push((pieces[k].item.to_owned(), z.zone.id.clone(), other.item.to_owned()));
                }
            }
        }
    }
    let length_needed = 2.0 * lo_a.abs().max(hi_a.abs());
    // Shortfalls under a micrometre are rounding.
    let short = |need: f64, have: f64| if need - have > 1e-6 { need - have } else { 0.0 };
    let fit = Fit {
        length: room.length,
        length_needed,
        length_short: short(length_needed, room.length),
        depth: room.depth,
        depth_needed: hi_o,
        depth_short: short(hi_o, room.depth),
        conflicts,
    };
    let packs: Vec<String> = packs.into_iter().filter(|p| !p.zones.is_empty()).map(|p| p.id).collect();
    if p.fit_only || !fit.fits() {
        for id in &placed {
            txn.delete(*id)?;
        }
        return Ok(Layout { placed: Vec::new(), fit, packs });
    }
    Ok(Layout { placed, fit, packs })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_template_uses_catalogue_items() {
        let names = crate::furnishings::names();
        for t in templates() {
            assert!(t.items.iter().all(|i| names.contains(i)), "{}: {:?}", t.id, t.items);
        }
    }
}
