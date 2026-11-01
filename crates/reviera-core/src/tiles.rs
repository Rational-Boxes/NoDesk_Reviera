// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Tile set-out (ARC-BATH-020): tiles laid on a wall face or a floor from a datum, the full and
//! cut tiles drawn with their joints and counted. A layout is an `IfcCovering` (CLADDING on a
//! wall, FLOORING on a floor) whose recipe Reviera rebuilds: it follows its wall or floor.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bodies::Mesh;
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;

pub const CLASS: &str = "IfcCovering";
pub const RECIPE_KEY: &str = "rxd:tile_layout";
pub const KIND: &str = "reviera.tile_layout";
/// A piece narrower than this is left out (a sliver no one would cut).
const SLIVER: f64 = 0.01;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// Where the first tile goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Datum {
    /// A tile centred on the region's middle each way, cuts equal at both ends.
    #[default]
    Centred,
    /// A full tile at the region's start and bottom (on a floor: its first corner).
    Corner,
    /// A full tile at the region's end and bottom.
    EndCorner,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Pattern {
    /// Joints in line both ways.
    #[default]
    Stack,
    /// Each row moved half a tile along (running bond).
    HalfBond,
}

/// What a layout covers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Host {
    /// A wall's face (1 the exterior, left of the wall's direction; −1 the interior), along its
    /// location line from `from` to `to` and from `bottom` to `top` above its level.
    Wall { wall: String, side: f64, level: String, from: f64, to: f64, bottom: f64, top: f64 },
    /// A floor's top, the whole of it, the tiles square to its first edge.
    Floor { floor: String },
}

/// A tile layout's stored recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TileRecipe {
    pub kind: String,
    pub host: Host,
    /// The tile's width (along) and height (up the wall, or across the floor), metres.
    pub tile: [f64; 2],
    pub joint: f64,
    /// The tile and its bed, standing off the surface.
    pub thickness: f64,
    pub datum: Datum,
    pub pattern: Pattern,
    /// Tiles ordered over those laid, for breakage and cuts that cannot be reused (0.1 is 10 %).
    pub waste: f64,
    /// The tile's name (a product, or a description).
    pub tile_name: String,
}

pub fn recipe(e: &Element) -> Option<TileRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == KIND).then(|| serde_json::from_value(v.clone()).ok())?
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

/// One laid piece: its rectangle in the surface's own axes (along, up/across), and whether it is
/// a full tile.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Piece {
    pub min: [f64; 2],
    pub max: [f64; 2],
    pub full: bool,
}

/// A layout as it is now.
#[derive(Debug, Clone)]
pub struct Layout {
    pub pieces: Vec<Piece>,
    /// Maps the surface's axes (along, up/across) and an offset off it to project coordinates.
    origin: [f64; 3],
    u: [f64; 3],
    v: [f64; 3],
    n: [f64; 3],
    /// Each piece's corners lifted off the surface's plane (a floor's falls), in the order
    /// (min, min), (max, min), (max, max), (min, max); empty when the surface is flat.
    rise: Vec<[f64; 4]>,
    pub thickness: f64,
    pub area: f64,
}

/// The counts a schedule of tiles shows.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Counts {
    pub full: usize,
    pub cut: usize,
    /// Tiles to order: every piece takes a tile, then the waste allowance, rounded up.
    pub to_order: usize,
    /// The area tiled (m²).
    pub area: f64,
}

impl Layout {
    pub fn counts(&self, waste: f64) -> Counts {
        let full = self.pieces.iter().filter(|p| p.full).count();
        let cut = self.pieces.len() - full;
        Counts { full, cut, to_order: ((full + cut) as f64 * (1.0 + waste.max(0.0)) - 1e-9).ceil() as usize, area: self.area }
    }

    fn at(&self, a: f64, b: f64, c: f64) -> [f64; 3] {
        [0, 1, 2].map(|k| self.origin[k] + self.u[k] * a + self.v[k] * b + self.n[k] * c)
    }

    /// Every piece as a thin box off the surface, the joints left open.
    pub fn mesh(&self) -> Mesh {
        let (mut p, mut t): (Vec<[f64; 3]>, Vec<u32>) = (Vec::new(), Vec::new());
        let (u, v, n) = (self.u, self.v, self.n);
        let flip = (u[1] * v[2] - u[2] * v[1]) * n[0] + (u[2] * v[0] - u[0] * v[2]) * n[1] + (u[0] * v[1] - u[1] * v[0]) * n[2] < 0.0;
        for (k, q) in self.pieces.iter().enumerate() {
            let base = p.len() as u32;
            let rise = self.rise.get(k).copied().unwrap_or([0.0; 4]);
            for c in [0.0, self.thickness] {
                for (j, (a, b)) in [(q.min[0], q.min[1]), (q.max[0], q.min[1]), (q.max[0], q.max[1]), (q.min[0], q.max[1])].into_iter().enumerate() {
                    p.push(self.at(a, b, c + rise[j]));
                }
            }
            // Bottom, top and four sides, outward when u × v points along n; reversed otherwise.
            for f in [[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [1, 2, 6], [1, 6, 5], [2, 3, 7], [2, 7, 6], [3, 0, 4], [3, 4, 7]] {
                let f = if flip { [f[0], f[2], f[1]] } else { f };
                t.extend(f.iter().map(|k| base + *k as u32));
            }
        }
        (p, t)
    }
}

/// The pieces over a region from a datum: tiles `w` × `h` with `joint` between them, each kept
/// where its middle falls in `inside` and cut to the region.
fn set_out(region: ([f64; 2], [f64; 2]), w: f64, h: f64, joint: f64, datum: Datum, pattern: Pattern, inside: &dyn Fn([f64; 2]) -> bool) -> Vec<Piece> {
    let (lo, hi) = region;
    let (pu, pv) = (w + joint, h + joint);
    // The first tile's start along and up.
    let (su, sv) = match datum {
        Datum::Centred => ((lo[0] + hi[0]) / 2.0 - w / 2.0, (lo[1] + hi[1]) / 2.0 - h / 2.0),
        Datum::Corner => (lo[0], lo[1]),
        Datum::EndCorner => (hi[0] - w, lo[1]),
    };
    let first = |s: f64, p: f64, from: f64| s - ((s - from) / p).ceil() * p;
    let v0 = first(sv, pv, lo[1]);
    let mut out = Vec::new();
    let mut row = 0;
    let mut v = v0;
    while v < hi[1] - 1e-9 {
        let shift = if pattern == Pattern::HalfBond && row % 2 == 1 { pu / 2.0 } else { 0.0 };
        let mut u = first(su + shift, pu, lo[0]) - pu;
        while u < hi[0] - 1e-9 {
            let (a0, a1, b0, b1) = (u.max(lo[0]), (u + w).min(hi[0]), v.max(lo[1]), (v + h).min(hi[1]));
            if a1 - a0 >= SLIVER && b1 - b0 >= SLIVER && inside([(a0 + a1) / 2.0, (b0 + b1) / 2.0]) {
                let full = (a1 - a0 - w).abs() < 1e-4 && (b1 - b0 - h).abs() < 1e-4;
                out.push(Piece { min: [a0, b0], max: [a1, b1], full });
            }
            u += pu;
        }
        v += pv;
        row += 1;
    }
    out
}

/// Pieces with openings taken out of them: each piece an opening overlaps becomes the parts of
/// it left, below, above and right of the opening, all cut.
fn cut_round(pieces: Vec<Piece>, holes: &[([f64; 2], [f64; 2])]) -> Vec<Piece> {
    let mut out = pieces;
    for (lo, hi) in holes {
        let mut next = Vec::new();
        for p in out {
            let overlaps = p.min[0] < hi[0] && p.max[0] > lo[0] && p.min[1] < hi[1] && p.max[1] > lo[1];
            if !overlaps {
                next.push(p);
                continue;
            }
            let parts = [
                ([p.min[0], p.min[1]], [lo[0].min(p.max[0]), p.max[1]]),
                ([hi[0].max(p.min[0]), p.min[1]], [p.max[0], p.max[1]]),
                ([lo[0].max(p.min[0]), p.min[1]], [hi[0].min(p.max[0]), lo[1].max(p.min[1])]),
                ([lo[0].max(p.min[0]), hi[1].min(p.max[1])], [hi[0].min(p.max[0]), p.max[1]]),
            ];
            for (a, b) in parts {
                if b[0] - a[0] >= SLIVER && b[1] - a[1] >= SLIVER {
                    next.push(Piece { min: a, max: b, full: false });
                }
            }
        }
        out = next;
    }
    out
}

fn inside(poly: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut c = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            c = !c;
        }
    }
    c
}

pub fn regenerate(store: &Store, e: &Element) -> Result<Layout> {
    let r = recipe(e).ok_or_else(|| bad("tile.not_reviera", "not a Reviera tile layout"))?;
    let [w, h] = r.tile;
    match &r.host {
        Host::Wall { wall, side, level, from, to, bottom, top } => {
            let wall = find(store, wall).and_then(|w| store.get(w)).ok_or_else(|| bad("tile.no_host", "the tile layout's wall no longer exists"))?;
            let g = crate::walls::regenerate(store, wall)?;
            let z0 = crate::levels::elevation_of(store, level)?;
            let (d, nrm) = (g.direction(), g.normal());
            let s = side.signum();
            let face = g.width / 2.0;
            // Along the wall's direction, up, and out of the face.
            let origin = [g.start[0] + nrm[0] * s * face, g.start[1] + nrm[1] * s * face, z0];
            let (u, v, n) = ([d[0], d[1], 0.0], [0.0, 0.0, 1.0], [nrm[0] * s, nrm[1] * s, 0.0]);
            let region = ([from.min(*to), *bottom], [from.max(*to), *top]);
            // The wall's doors and windows, in the face's axes: tiles stop at them.
            let holes: Vec<([f64; 2], [f64; 2])> = crate::inserts::placed(store)
                .iter()
                .filter(|i| i.host == wall.id())
                .map(|i| {
                    let (iw, ih) = (i.recipe.width.unwrap_or(i.ty.width), i.recipe.height.unwrap_or(i.ty.height));
                    let b = i.z0 - z0;
                    ([i.recipe.along - iw / 2.0, b], [i.recipe.along + iw / 2.0, b + ih])
                })
                .filter_map(|(lo, hi)| {
                    let (lo, hi) = ([lo[0].max(region.0[0]), lo[1].max(region.0[1])], [hi[0].min(region.1[0]), hi[1].min(region.1[1])]);
                    (hi[0] > lo[0] && hi[1] > lo[1]).then_some((lo, hi))
                })
                .collect();
            let pieces = cut_round(set_out(region, w, h, r.joint, r.datum, r.pattern, &|_| true), &holes);
            let area = (region.1[0] - region.0[0]) * (region.1[1] - region.0[1]) - holes.iter().map(|(lo, hi)| (hi[0] - lo[0]) * (hi[1] - lo[1])).sum::<f64>();
            let layout = Layout { pieces, origin, u, v, n, rise: Vec::new(), thickness: r.thickness, area };
            Ok(layout)
        }
        Host::Floor { floor } => {
            let floor = find(store, floor).and_then(|f| store.get(f)).ok_or_else(|| bad("tile.no_host", "the tile layout's floor no longer exists"))?;
            let g = crate::floors::regenerate(store, floor)?;
            let profile = g.profiles.first().ok_or_else(|| bad("tile.no_host", "the floor has no outline"))?;
            let outer = &profile.outer;
            if outer.len() < 3 {
                return Err(bad("tile.no_host", "the floor has no outline"));
            }
            // Square to the outline's first edge.
            let e0 = [outer[1][0] - outer[0][0], outer[1][1] - outer[0][1]];
            let l = e0[0].hypot(e0[1]).max(1e-12);
            let u2 = [e0[0] / l, e0[1] / l];
            let v2 = [-u2[1], u2[0]];
            let local = |q: [f64; 2]| [(q[0] - outer[0][0]) * u2[0] + (q[1] - outer[0][1]) * u2[1], (q[0] - outer[0][0]) * v2[0] + (q[1] - outer[0][1]) * v2[1]];
            let poly: Vec<[f64; 2]> = outer.iter().map(|q| local(*q)).collect();
            let holes: Vec<Vec<[f64; 2]>> = profile.holes.iter().map(|hh| hh.iter().map(|q| local(*q)).collect()).collect();
            let (lo, hi) = poly.iter().fold(([f64::MAX; 2], [f64::MIN; 2]), |(lo, hi), q| ([lo[0].min(q[0]), lo[1].min(q[1])], [hi[0].max(q[0]), hi[1].max(q[1])]));
            let inside_floor = |p: [f64; 2]| inside(&poly, p) && !holes.iter().any(|hh| inside(hh, p));
            let pieces = set_out((lo, hi), w, h, r.joint, r.datum, r.pattern, &inside_floor);
            let origin = [outer[0][0], outer[0][1], g.top];
            // On a warped floor (its falls), each corner sits on the top there.
            let world = |a: f64, b: f64| [outer[0][0] + u2[0] * a + v2[0] * b, outer[0][1] + u2[1] * a + v2[1] * b];
            let rise = match crate::shape_edit::shape_of(floor).and_then(|shape| crate::shape_edit::top_triangles(profile, &shape).ok()) {
                Some(tris) => pieces
                    .iter()
                    .map(|q| [(q.min[0], q.min[1]), (q.max[0], q.min[1]), (q.max[0], q.max[1]), (q.min[0], q.max[1])].map(|(a, b)| crate::shape_edit::offset_in(&tris, world(a, b))))
                    .collect(),
                None => Vec::new(),
            };
            Ok(Layout { pieces, origin, u: [u2[0], u2[1], 0.0], v: [v2[0], v2[1], 0.0], n: [0.0, 0.0, 1.0], rise, thickness: r.thickness, area: g.area() })
        }
    }
}

/// Every tile layout that regenerates.
pub fn all(store: &Store) -> Vec<(ElementId, Layout)> {
    store.iter().filter(|e| recipe(e).is_some()).filter_map(|e| Some((e.id(), regenerate(store, e).ok()?))).collect()
}

/// A tile layout's level (GlobalId), for its storey.
pub fn level_of(store: &Store, e: &Element) -> Option<String> {
    match recipe(e)?.host {
        Host::Wall { level, .. } => Some(level),
        Host::Floor { floor } => crate::floors::slab_recipe(store.get(find(store, &floor)?)?).map(|(_, r)| r.level),
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// A wall to tile one face of, or a floor to tile.
    pub wall_id: Option<ElementRef>,
    pub floor_id: Option<ElementRef>,
    /// On a wall: a point in plan on the side to tile (default the wall's interior face); the
    /// face is then tiled between the walls meeting it there. With neither a wall nor a floor:
    /// the floor under the point on `level`.
    pub point: Option<[f64; 2]>,
    pub level: Option<ElementRef>,
    /// On a wall: along its location line from its start (default the whole face, or the face
    /// between the walls meeting it by `point`), and the heights above its level (default 0 to
    /// the wall's top).
    pub from: Option<f64>,
    pub to: Option<f64>,
    pub bottom: Option<f64>,
    pub top: Option<f64>,
    /// Tile width and height (metres; default 300 × 600 on walls, 600 × 600 on floors).
    pub tile: Option<[f64; 2]>,
    /// Joint width (default 3 mm on walls, 5 mm on floors).
    pub joint: Option<f64>,
    /// Tile and bed thickness (default 12 mm).
    pub thickness: Option<f64>,
    #[serde(default)]
    pub datum: Datum,
    #[serde(default)]
    pub pattern: Pattern,
    /// Waste allowance (default 0.1).
    pub waste: Option<f64>,
    pub tile_name: Option<String>,
}

/// `tile.create`.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let (host, predefined, wall_like) = match (&p.wall_id, &p.floor_id) {
        (Some(w), None) => {
            let id = store.resolve(w).filter(|id| store.get(*id).and_then(crate::walls::wall_recipe).is_some()).ok_or_else(|| bad("tile.no_host", "no such Reviera wall"))?;
            let e = store.get(id).expect("resolved");
            let r = crate::walls::wall_recipe(e).expect("a wall");
            let g = crate::walls::regenerate(&store, e)?;
            let side = match p.point {
                Some(q) => {
                    let n = g.normal();
                    if (q[0] - g.start[0]) * n[0] + (q[1] - g.start[1]) * n[1] >= 0.0 { 1.0 } else { -1.0 }
                }
                None => -1.0,
            };
            let z0 = crate::levels::elevation_of(&store, &r.base_level)?;
            let top = p.top.unwrap_or(g.base_z + g.height - z0);
            // By a point: the face between the walls that meet it on that side.
            let (d, n) = (g.direction(), g.normal());
            let (mut from, mut to) = (0.0, g.length());
            if let Some(q) = p.point {
                let along = (q[0] - g.start[0]) * d[0] + (q[1] - g.start[1]) * d[1];
                let off = g.width / 2.0 + 0.05;
                let probe = [g.start[0] + d[0] * along + n[0] * side * off, g.start[1] + d[1] * along + n[1] * side * off];
                if let Some((t, _)) = crate::bath_rules::ray_to_walls(&store, probe, [-d[0], -d[1]], z0 + 0.6, 50.0) {
                    from = (along - t).max(0.0);
                }
                if let Some((t, _)) = crate::bath_rules::ray_to_walls(&store, probe, d, z0 + 0.6, 50.0) {
                    to = (along + t).min(g.length());
                }
            }
            let host = Host::Wall { wall: e.guid().to_string(), side, level: r.base_level.clone(), from: p.from.unwrap_or(from), to: p.to.unwrap_or(to), bottom: p.bottom.unwrap_or(0.0), top };
            (host, "CLADDING", true)
        }
        (None, Some(f)) => {
            let id = store.resolve(f).filter(|id| store.get(*id).and_then(crate::floors::floor_recipe).is_some()).ok_or_else(|| bad("tile.no_host", "no such Reviera floor"))?;
            (Host::Floor { floor: store.get(id).expect("resolved").guid().to_string() }, "FLOORING", false)
        }
        (None, None) if p.point.is_some() && p.level.is_some() => {
            let level = p.level.as_ref().and_then(|l| store.resolve(l)).and_then(|l| store.get(l)).filter(|l| l.class == crate::levels::CLASS).ok_or_else(|| bad("tile.no_host", "no such level"))?;
            let id = crate::bath_rules::floor_under(&store, &level.guid().to_string(), p.point.expect("checked")).ok_or_else(|| bad("tile.no_host", "no Reviera floor under the point"))?;
            (Host::Floor { floor: store.get(id).expect("found").guid().to_string() }, "FLOORING", false)
        }
        _ => return Err(bad("tile.no_host", "give a wall_id or a floor_id")),
    };
    let tile = p.tile.unwrap_or(if wall_like { [0.3, 0.6] } else { [0.6, 0.6] });
    if !(tile[0] >= 0.02 && tile[1] >= 0.02 && tile[0] <= 3.0 && tile[1] <= 3.0) {
        return Err(bad("tile.invalid_input", "a tile is 20 mm to 3 m each way"));
    }
    let joint = p.joint.unwrap_or(if wall_like { 0.003 } else { 0.005 });
    if !(0.0..=0.05).contains(&joint) {
        return Err(bad("tile.invalid_input", "a joint is 0 to 50 mm"));
    }
    let r = TileRecipe {
        kind: KIND.into(),
        host,
        tile,
        joint,
        thickness: p.thickness.unwrap_or(0.012).clamp(0.003, 0.1),
        datum: p.datum,
        pattern: p.pattern,
        waste: p.waste.unwrap_or(0.1).clamp(0.0, 1.0),
        tile_name: p.tile_name.clone().unwrap_or_else(|| format!("Tile {:.0} × {:.0}", tile[0] * 1000.0, tile[1] * 1000.0)),
    };
    let name = r.tile_name.clone();
    let id = txn.create(
        NewElement::new(CLASS)
            .name(&name)
            .param("@PredefinedType", predefined)
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )?;
    // It must lay out.
    regenerate(txn.store(), txn.store().get(id).expect("made"))?;
    Ok(id)
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SummaryParams {
    /// These layouts (default every one).
    #[serde(default)]
    pub element_ids: Vec<ElementRef>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SummaryRow {
    pub element: ElementId,
    pub tile: String,
    pub size: [f64; 2],
    #[serde(flatten)]
    pub counts: Counts,
}

/// `tile.summary`: full and cut tiles, tiles to order and area, per layout.
pub(crate) fn summary(store: &Store, p: SummaryParams) -> Result<Vec<SummaryRow>> {
    let only = p.element_ids.iter().map(|r| store.resolve(r).ok_or_else(|| bad("element.not_found", format!("no element {r:?}")))).collect::<Result<Vec<_>>>()?;
    Ok(all(store)
        .into_iter()
        .filter(|(id, _)| only.is_empty() || only.contains(id))
        .filter_map(|(id, l)| {
            let r = recipe(store.get(id)?)?;
            Some(SummaryRow { element: id, tile: r.tile_name.clone(), size: r.tile, counts: l.counts(r.waste) })
        })
        .collect())
}
