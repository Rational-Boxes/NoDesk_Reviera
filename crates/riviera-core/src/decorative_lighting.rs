// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Decorative lighting helpers (ARC-INT-003, `INTERIORS_PLAN.md` §4.5): chandeliers sized from
//! their room or table and hung at their height, pendants spaced over a table or island, and
//! sconces along a wall, all from the generic fixtures ([`crate::furnishings`], research/15 D4).
//! The rule packs check what they make (ARC-RULE-002: headroom, projection, height over).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::rules::{self, Item};
use crate::store::Store;
use crate::txn::Transaction;

const IN: f64 = 0.0254;
const FT: f64 = 0.3048;
/// A chandelier's bottom over a table: the middle of 30–34 in (S1471).
pub const OVER_TABLE: f64 = 32.0 * IN;
/// A pendant's bottom over a table or counter: the middle of 30–36 in (S1472).
pub const PENDANT_OVER: f64 = 33.0 * IN;
/// The clear space between pendants: the middle of 24–30 in (S1472).
pub const PENDANT_GAP: f64 = 27.0 * IN;
/// A sconce's centre above the floor: the middle of 60–66 in (S1473).
pub const SCONCE_HEIGHT: f64 = 63.0 * IN;
/// Sconces along a wall, centre to centre: 8 ft, where the sources' 6–8 and 8–10 ft meet (S1473).
pub const SCONCE_SPACING: f64 = 8.0 * FT;
/// ADA 2010 §307.4's headroom.
pub const HEADROOM: f64 = 80.0 * IN;
/// The chandeliers, by diameter (inches).
const CHANDELIERS: &[(f64, &str)] = &[(24.0, "Chandelier 24 in"), (36.0, "Chandelier 36 in")];

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

fn level_of(store: &Store, r: &ElementRef) -> Result<(ElementId, f64)> {
    let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such level"))?;
    let e = store.get(id).ok_or_else(|| bad("element.not_found", "no such level"))?;
    Ok((id, crate::levels::elevation(e)))
}

fn piece(store: &Store, r: &ElementRef) -> Result<Item> {
    let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such element"))?;
    store.get(id).and_then(|e| rules::item(store, e)).ok_or_else(|| bad("lighting.not_a_piece", format!("element {id} is not a placed component")))
}

fn load(txn: &mut Transaction<'_>, name: &str) -> Result<ElementId> {
    Ok(crate::furnishings::load(txn, crate::furnishings::LoadParams { names: vec![name.to_owned()] })?[name])
}

fn place(txn: &mut Transaction<'_>, ty: ElementId, at: [f64; 2], level: &ElementRef, offset: f64, rotation: f64) -> Result<ElementId> {
    crate::mesh_objects::place(txn, crate::mesh_objects::PlaceParams { type_id: ElementRef::Id(ty), point: at, level: level.clone(), offset, rotation, mirror: false, scale: 1.0 })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChandelierParams {
    /// Where in plan; over `over_id` when given.
    pub point: Option<[f64; 2]>,
    pub level: ElementRef,
    /// The table (or other piece) it hangs over: centred over it, sized from it, its bottom
    /// 32 in above it.
    pub over_id: Option<ElementRef>,
    /// The ceiling's height above the level; default the room's top, else 2.7 m.
    #[schemars(extend("x-riviera-data-type" = "length"))]
    pub ceiling: Option<f64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Hung {
    pub placed: Vec<ElementId>,
    /// The fixture used.
    pub item: String,
    /// The largest diameter the room and table suggest (metres), where one was worked out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diameter_max: Option<f64>,
    /// Its bottom above the level (metres).
    pub bottom: f64,
    pub notes: Vec<String>,
}

/// Hangs a chandelier (`lighting.chandelier`): sized as retailers' guides do (the room's width
/// plus length in feet is its largest diameter in inches; over a table, ½–⅔ of its length), its
/// bottom 32 in over a table, or as high as the ceiling allows elsewhere, kept to 80 in headroom
/// where it can be.
pub(crate) fn chandelier(txn: &mut Transaction<'_>, p: ChandelierParams) -> Result<Hung> {
    let store = txn.store().clone();
    let (level_id, z0) = level_of(&store, &p.level)?;
    let over = p.over_id.as_ref().map(|r| piece(&store, r)).transpose()?;
    let at = match (&over, p.point) {
        (Some(o), _) => [o.frame.origin[0], o.frame.origin[1]],
        (None, Some(pt)) => pt,
        (None, None) => return Err(bad("lighting.where", "give a point or the table it hangs over")),
    };
    let room = crate::rooms::room_at(&store, level_id, at).and_then(|r| store.get(r)).and_then(|r| crate::rooms::regenerate(&store, r).ok());
    let mut notes = Vec::new();
    // The largest diameter: from the room, and from the table.
    let mut max_in: Option<f64> = None;
    if let Some(region) = room.as_ref().and_then(|g| g.region.as_ref()) {
        let pts: Vec<[f64; 2]> = region.outer.iter().map(|e| e.line.start).collect();
        let span = |k: usize| pts.iter().map(|q| q[k]).fold(f64::MIN, f64::max) - pts.iter().map(|q| q[k]).fold(f64::MAX, f64::min);
        max_in = Some((span(0) + span(1)) / FT);
    }
    if let Some(o) = &over {
        let length = (o.x[1] - o.x[0]).max(o.back - o.front);
        let from_table = length * 2.0 / 3.0 / IN;
        max_in = Some(max_in.map_or(from_table, |m| m.min(from_table)));
    }
    let (dia, name) = match max_in {
        Some(m) => CHANDELIERS.iter().rev().find(|(d, _)| *d <= m + 1e-6).copied().unwrap_or_else(|| {
            notes.push(format!("the room or table suggests at most {m:.0} in across; the smallest chandelier, 24 in, is used"));
            CHANDELIERS[0]
        }),
        None => CHANDELIERS[0],
    };
    let ty = load(txn, name)?;
    let size = crate::interior_layouts::sizes()[name];
    let ceiling = p.ceiling.or_else(|| room.as_ref().map(|g| g.z_top - z0)).unwrap_or(2.7);
    let bottom = match &over {
        Some(o) => o.z[1] - z0 + OVER_TABLE,
        // As high as a 150 mm canopy and stem allow, to keep the headroom.
        None => {
            let b = ceiling - size[2] - 0.15;
            if b < HEADROOM {
                notes.push(format!("the ceiling ({ceiling:.2} m) is too low to keep 80 in under a {dia:.0} in chandelier: hang it over furniture"));
            }
            b
        }
    };
    if bottom + size[2] > ceiling + 1e-6 {
        return Err(bad("lighting.too_tall", format!("a {dia:.0} in chandelier {:.0} mm over the table reaches above the {ceiling:.2} m ceiling", OVER_TABLE * 1000.0)));
    }
    let id = place(txn, ty, at, &p.level, bottom, 0.0)?;
    Ok(Hung { placed: vec![id], item: name.into(), diameter_max: max_in.map(|m| m * IN), bottom, notes })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PendantsParams {
    /// The table, island or counter they hang over, along its length.
    pub over_id: ElementRef,
    pub level: ElementRef,
    /// How many; default as many as fit with 27 in between them and 6 in short of each end.
    pub count: Option<usize>,
}

/// Hangs pendants in a row over a table or counter (`lighting.pendants`): 27 in apart (24–30 in,
/// S1472), their bottoms 33 in above its top.
pub(crate) fn pendants(txn: &mut Transaction<'_>, p: PendantsParams) -> Result<Hung> {
    let store = txn.store().clone();
    let (_, z0) = level_of(&store, &p.level)?;
    let o = piece(&store, &p.over_id)?;
    let name = "Pendant 12 in";
    let dia = crate::interior_layouts::sizes()[name][0];
    let along_x = o.x[1] - o.x[0] >= o.back - o.front;
    let length = if along_x { o.x[1] - o.x[0] } else { o.back - o.front };
    let pitch = dia + PENDANT_GAP;
    let fits = ((length - 2.0 * 6.0 * IN - dia) / pitch).floor() as i64 + 1;
    let n = p.count.unwrap_or(fits.max(1) as usize).max(1);
    let ty = load(txn, name)?;
    let bottom = o.z[1] - z0 + PENDANT_OVER;
    let mut placed = Vec::new();
    let mid = [(o.x[0] + o.x[1]) / 2.0, (o.front + o.back) / 2.0];
    for k in 0..n {
        let d = (k as f64 - (n as f64 - 1.0) / 2.0) * pitch;
        let local = if along_x { [mid[0] + d, mid[1], 0.0] } else { [mid[0], mid[1] + d, 0.0] };
        let w = o.frame.apply(local);
        placed.push(place(txn, ty, [w[0], w[1]], &p.level, bottom, 0.0)?);
    }
    let mut notes = Vec::new();
    if (n as i64) > fits.max(1) {
        notes.push(format!("{n} pendants are closer than 27 in apart over {:.2} m", length));
    }
    Ok(Hung { placed, item: name.into(), diameter_max: None, bottom, notes })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SconcesParams {
    pub wall_id: ElementRef,
    pub level: ElementRef,
    /// Which face; default the one with a room on it (else the interior face).
    pub side: Option<crate::mep_controls::Side>,
    /// A point on the side to put them, instead of `side`.
    pub toward: Option<[f64; 2]>,
    /// How many; default one every 8 ft, centred on the wall.
    pub count: Option<usize>,
    /// Their centres above the level; default 63 in.
    #[schemars(extend("x-riviera-data-type" = "length"))]
    pub height: Option<f64>,
}

/// Puts sconces along a wall face (`lighting.sconces`): every 8 ft, centred, at 63 in (S1473).
pub(crate) fn sconces(txn: &mut Transaction<'_>, p: SconcesParams) -> Result<Hung> {
    let store = txn.store().clone();
    let wall_id = store.resolve(&p.wall_id).filter(|w| store.get(*w).is_some_and(|e| crate::walls::wall_recipe(e).is_some())).ok_or_else(|| bad("lighting.no_wall", "no such Riviera wall"))?;
    let g = crate::walls::regenerate(&store, store.get(wall_id).expect("resolved"))?;
    let (d, n) = (g.direction(), g.normal());
    let length = g.length();
    // The face: as given, toward a point, or the one a room is on.
    let (level_id, _) = level_of(&store, &p.level)?;
    let mid = [g.start[0] + d[0] * length / 2.0, g.start[1] + d[1] * length / 2.0];
    let s = match (p.side, p.toward) {
        (Some(crate::mep_controls::Side::Exterior), _) => 1.0,
        (Some(crate::mep_controls::Side::Interior), _) => -1.0,
        (None, Some(q)) => ((q[0] - mid[0]) * n[0] + (q[1] - mid[1]) * n[1]).signum(),
        (None, None) => {
            let off = g.width / 2.0 + 0.3;
            let has_room = |s: f64| crate::rooms::room_at(&store, level_id, [mid[0] + n[0] * s * off, mid[1] + n[1] * s * off]).is_some();
            if has_room(1.0) && !has_room(-1.0) { 1.0 } else { -1.0 }
        }
    };
    let side = if s > 0.0 { crate::mep_controls::Side::Exterior } else { crate::mep_controls::Side::Interior };
    let count = p.count.unwrap_or(((length - 0.6) / SCONCE_SPACING).floor().max(0.0) as usize + 1).max(1);
    let name = "Wall sconce";
    let ty = load(txn, name)?;
    let h = crate::interior_layouts::sizes()[name][2];
    let centre = p.height.unwrap_or(SCONCE_HEIGHT);
    let pitch = if count > 1 { (length / count as f64).min(SCONCE_SPACING) } else { 0.0 };
    let mut placed = Vec::new();
    for k in 0..count {
        let along = length / 2.0 + (k as f64 - (count as f64 - 1.0) / 2.0) * pitch;
        let at = [g.start[0] + d[0] * along + n[0] * s * (g.width / 2.0 + 0.01), g.start[1] + d[1] * along + n[1] * s * (g.width / 2.0 + 0.01)];
        placed.push(crate::mep_controls::place_hosted(
            txn,
            crate::mep_controls::PlaceParams { type_id: ElementRef::Id(ty), point: at, level: p.level.clone(), offset: Some(centre - h / 2.0), rotation: 0.0, host_wall: Some(ElementRef::Id(wall_id)), side: Some(side), work_plane: false, controlled_ids: Vec::new() },
            |c| c == crate::mesh_objects::Category::LightingFixture,
            "lighting.not_a_fixture",
            "a light fixture",
            0.0,
        )?);
    }
    Ok(Hung { placed, item: name.into(), diameter_max: None, bottom: centre - h / 2.0, notes: Vec::new() })
}
