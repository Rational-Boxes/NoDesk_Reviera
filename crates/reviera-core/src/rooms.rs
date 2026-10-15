// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Rooms (spec Ch 3 §3.9.1–3.9.2, ARC-ROOM-001/002).
//!
//! A room is an `IfcSpace` with a `recipe.room` (`rxd:recipe`): its level (GlobalId), the point
//! it was placed at, its upper limit level and limit offset, and its base offset. Its Number is
//! the element's name (`IfcSpace.Name`) and its Name is `@LongName`, the usual IFC convention.
//!
//! The boundary is found every time it is needed:
//! - The walls are cut at the computation plane, 1 mm above the level.
//! - Their finish faces make a planar arrangement ([`crate::arrangement`]).
//! - The room is the face around its point, less the islands inside it (free-standing walls).
//!
//! A point in no closed face is *Not Enclosed*; a point inside a wall is refused. Two rooms in
//! one face are *Redundant*. Area and perimeter come from the face; the volume (for now) is the
//! area times the unbounded height.
//!
//! Built so far: walls bound rooms, at their finish faces. Separation lines, columns, other area
//! rules, clipped volumes, phases and unplaced rooms come later.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::arrangement::{self, Region};
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::walls::RECIPE_KEY;

pub const ROOM_RECIPE: &str = "recipe.room";
pub const CLASS: &str = "IfcSpace";
/// The parameter holding a room's Name.
pub const NAME_KEY: &str = "@LongName";
/// Rooms reach this far above their level unless an upper limit says otherwise (4000 mm).
pub const DEFAULT_LIMIT_OFFSET: f64 = 4.0;
/// Regions smaller than this get no room when placing automatically (0.25 sq ft).
pub const MIN_AREA: f64 = 0.023;
/// How far above the level the walls are cut to find the boundary.
const COMPUTATION_HEIGHT: f64 = 0.001;

/// The stored recipe of a room.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RoomRecipe {
    pub kind: String,
    /// The level (GlobalId).
    pub level: String,
    /// The point the room was placed at: the region around it is the room.
    pub point: [f64; 2],
    /// The upper limit level (GlobalId); the room's own level when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upper_level: Option<String>,
    /// Height of the top above the upper limit, metres.
    pub limit_offset: f64,
    /// Height of the bottom above the level, metres.
    #[serde(default)]
    pub base_offset: f64,
}

/// The recipe of a Reviera room.
pub fn room_recipe(e: &Element) -> Option<RoomRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")? == ROOM_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// A room's Number and Name.
pub fn number_and_name(e: &Element) -> (String, String) {
    (
        e.name.clone().unwrap_or_default(),
        e.param(NAME_KEY)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    )
}

/// A room as it is now.
#[derive(Debug, Clone, PartialEq)]
pub struct RoomGeometry {
    /// The region, or `None` when the room is not enclosed.
    pub region: Option<Region>,
    pub z_base: f64,
    pub z_top: f64,
    /// Where the room's tag goes: the middle of the boundary's extent, or the placement point
    /// when that is outside the room.
    pub tag_point: [f64; 2],
}

impl RoomGeometry {
    pub fn enclosed(&self) -> bool {
        self.region.is_some()
    }

    pub fn area(&self) -> f64 {
        self.region.as_ref().map_or(0.0, Region::area)
    }

    pub fn perimeter(&self) -> f64 {
        self.region.as_ref().map_or(0.0, Region::perimeter)
    }

    /// The height from the room's bottom to its upper limit.
    pub fn unbounded_height(&self) -> f64 {
        self.z_top - self.z_base
    }

    pub fn volume(&self) -> f64 {
        self.area() * self.unbounded_height()
    }
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

fn elevation(store: &Store, guid: &str, what: &str) -> Result<f64> {
    find(store, guid)
        .and_then(|l| store.get(l))
        .map(crate::levels::elevation)
        .ok_or_else(|| {
            Error::new(
                "room.level_missing",
                format!("the room's {what} no longer exists"),
            )
        })
}

/// The arrangement of wall faces where rooms on a level are computed.
fn plane_edges(store: &Store, level_z: f64) -> Vec<arrangement::Edge> {
    let mut edges = arrangement::wall_edges(store, level_z + COMPUTATION_HEIGHT);
    // Room separation lines on the level split space too.
    edges.extend(crate::room_separators::edges(store, level_z));
    edges
}

/// Whether `p` lies inside a wall cut at `z`.
fn in_wall(store: &Store, z: f64, p: [f64; 2]) -> bool {
    crate::walls::solve(store).bodies.values().any(|b| {
        let g = &b.geometry;
        z >= g.base_z - 1e-9 && z <= g.base_z + g.height + 1e-9 && inside(p, &b.footprint)
    })
}

fn inside(p: [f64; 2], ring: &[[f64; 2]]) -> bool {
    let mut c = false;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            c = !c;
        }
    }
    c
}

/// The room's region in an arrangement, if `p` is in a closed face that is not inside a wall.
fn region_at(store: &Store, edges: &[arrangement::Edge], z: f64, p: [f64; 2]) -> Option<Region> {
    if in_wall(store, z, p) {
        return None;
    }
    arrangement::region_around(edges, p)
}

/// Regenerates a room from its recipe, levels and the walls around it.
pub fn regenerate(store: &Store, e: &Element) -> Result<RoomGeometry> {
    let r = room_recipe(e).ok_or_else(|| Error::new("room.not_reviera", "not a Reviera room"))?;
    let level_z = elevation(store, &r.level, "level")?;
    let upper_z = match &r.upper_level {
        Some(g) => elevation(store, g, "upper limit level")?,
        None => level_z,
    };
    let z_base = level_z + r.base_offset;
    let z_top = upper_z + r.limit_offset;
    let edges = plane_edges(store, level_z);
    let region = region_at(store, &edges, level_z + COMPUTATION_HEIGHT, r.point);
    let tag_point = region
        .as_ref()
        .map(|reg| {
            let ring = reg.outer_ring();
            let (lo, hi) = ring.iter().fold(
                ([f64::MAX, f64::MAX], [f64::MIN, f64::MIN]),
                |(lo, hi), p| {
                    (
                        [lo[0].min(p[0]), lo[1].min(p[1])],
                        [hi[0].max(p[0]), hi[1].max(p[1])],
                    )
                },
            );
            let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
            if reg.contains(c) { c } else { r.point }
        })
        .unwrap_or(r.point);
    Ok(RoomGeometry {
        region,
        z_base,
        z_top,
        tag_point,
    })
}

/// A room and what it is now.
#[derive(Debug, Clone, PartialEq)]
pub struct RoomInfo {
    pub id: ElementId,
    pub number: String,
    pub name: String,
    pub level: String,
    pub geometry: RoomGeometry,
    /// Another room placed earlier is in the same region.
    pub redundant: bool,
}

/// Every Reviera room that regenerates, in placement order, with redundant rooms flagged.
pub fn all(store: &Store) -> Vec<RoomInfo> {
    let mut out: Vec<RoomInfo> = store
        .iter()
        .filter_map(|e| {
            let r = room_recipe(e)?;
            let geometry = regenerate(store, e).ok()?;
            let (number, name) = number_and_name(e);
            Some(RoomInfo {
                id: e.id(),
                number,
                name,
                level: r.level,
                geometry,
                redundant: false,
            })
        })
        .collect();
    out.sort_by_key(|r| r.id);
    for i in 0..out.len() {
        let same = (0..i).any(|j| {
            out[j].level == out[i].level
                && out[j].geometry.region.is_some()
                && out[j].geometry.region == out[i].geometry.region
        });
        out[i].redundant = same;
    }
    out
}

/// The room around a plan point on a level, if one is placed there.
pub fn room_at(store: &Store, level: ElementId, p: [f64; 2]) -> Option<ElementId> {
    let guid = store.get(level)?.guid().to_string();
    all(store)
        .into_iter()
        .filter(|r| r.level == guid)
        .find(|r| {
            r.geometry
                .region
                .as_ref()
                .is_some_and(|reg| reg.contains(p))
        })
        .map(|r| r.id)
}

/// The next free room number: one more than the highest number in use.
pub(crate) fn next_number(store: &Store) -> String {
    store
        .iter()
        .filter(|e| room_recipe(e).is_some())
        .filter_map(|e| e.name.as_deref()?.trim().parse::<u64>().ok())
        .max()
        .map_or("1".to_owned(), |n| (n + 1).to_string())
}

/// A point well inside a region (the middle of its largest triangle).
fn interior_point(region: &Region) -> Option<[f64; 2]> {
    let outer = region.outer_ring();
    let holes = region.hole_rings();
    let mut points = outer.clone();
    let mut starts = Vec::new();
    for h in &holes {
        starts.push(points.len() as u32);
        points.extend(h.iter().copied());
    }
    let mut tris: Vec<u32> = Vec::new();
    earcut::Earcut::new().earcut(points.iter().copied(), &starts, &mut tris);
    tris.chunks(3)
        .map(|t| {
            let [a, b, c] = [
                points[t[0] as usize],
                points[t[1] as usize],
                points[t[2] as usize],
            ];
            let area = ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs();
            (
                area,
                [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0],
            )
        })
        .max_by(|x, y| x.0.total_cmp(&y.0))
        .map(|(_, p)| p)
}

fn resolve_level(store: &Store, r: &ElementRef) -> Result<(ElementId, String)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such level"))?;
    let e = store.get(id).expect("resolved");
    if e.class != crate::levels::CLASS {
        return Err(Error::new(
            "room.not_a_level",
            format!("element {id} is not a level"),
        ));
    }
    Ok((id, e.guid().to_string()))
}

fn resolve_room(store: &Store, r: &ElementRef) -> Result<(ElementId, RoomRecipe)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such room"))?;
    let recipe = room_recipe(store.get(id).expect("resolved")).ok_or_else(|| {
        Error::new(
            "room.not_reviera",
            format!("element {id} is not a Reviera room"),
        )
    })?;
    Ok((id, recipe))
}

fn length(store: &Store, v: &Value, what: &str) -> Result<f64> {
    match v {
        Value::Number(n) => Ok(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => crate::units::parse(s, "length", &crate::units::settings_of(store)),
        _ => Err(Error::new(
            "room.invalid_value",
            format!("{what} must be a number or text with units"),
        )),
    }
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceParams {
    pub level_id: ElementRef,
    /// A plan point inside the region to fill (metres).
    pub point: [f64; 2],
    /// The upper limit level; the room's own level when omitted.
    pub upper_limit: Option<ElementRef>,
    /// Height of the top above the upper limit (metres or text with units); default 4000 mm.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub limit_offset: Option<Value>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base_offset: Option<Value>,
    pub name: Option<String>,
    /// Default: one more than the highest number in use.
    pub number: Option<String>,
}

pub(crate) fn place(txn: &mut crate::txn::Transaction<'_>, p: PlaceParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let (level, level_guid) = resolve_level(&store, &p.level_id)?;
    let level_z = crate::levels::elevation(store.get(level).expect("resolved"));
    if in_wall(&store, level_z + COMPUTATION_HEIGHT, p.point) {
        return Err(Error::new(
            "room.inside_element",
            "Room placement point is inside an element",
        ));
    }
    let upper_level = p
        .upper_limit
        .as_ref()
        .map(|u| resolve_level(&store, u).map(|(_, g)| g))
        .transpose()?;
    let num =
        |v: &Option<Value>, what: &str| v.as_ref().map(|v| length(&store, v, what)).transpose();
    let recipe = RoomRecipe {
        kind: ROOM_RECIPE.into(),
        level: level_guid,
        point: p.point,
        upper_level,
        limit_offset: num(&p.limit_offset, "the limit offset")?.unwrap_or(DEFAULT_LIMIT_OFFSET),
        base_offset: num(&p.base_offset, "the base offset")?.unwrap_or(0.0),
    };
    let number = p.number.unwrap_or_else(|| next_number(&store));
    let id = txn.create(
        NewElement::new(CLASS)
            .name(number)
            .param(NAME_KEY, p.name.unwrap_or_else(|| "Room".to_owned()))
            .param("@PredefinedType", "INTERNAL")
            .param(
                RECIPE_KEY,
                serde_json::to_value(&recipe).expect("serialises"),
            ),
    )?;
    let g = regenerate(txn.store(), txn.store().get(id).expect("created"))?;
    if g.unbounded_height() <= 0.0 {
        return Err(Error::new(
            "room.bad_limits",
            "the room's upper limit is at or below its base",
        ));
    }
    let warning = |id: &str, message: &str, elements: Vec<ElementId>| crate::txn::Warning {
        id: id.into(),
        message: message.into(),
        element_ids: elements,
    };
    if !g.enclosed() {
        txn.warn(warning(
            "arc.room.not_enclosed",
            "Room is not in a properly enclosed region",
            vec![id],
        ));
    } else if all(txn.store()).iter().any(|r| r.id == id && r.redundant) {
        txn.warn(warning(
            "arc.room.redundant",
            "Multiple rooms are in the same enclosed region",
            vec![id],
        ));
    }
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceAutoParams {
    pub level_id: ElementRef,
}

/// Places a room in every enclosed region of a level that has none (larger than 0.023 m²),
/// numbered in sequence.
pub(crate) fn place_automatically(
    txn: &mut crate::txn::Transaction<'_>,
    p: PlaceAutoParams,
) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let (level, _) = resolve_level(&store, &p.level_id)?;
    let level_z = crate::levels::elevation(store.get(level).expect("resolved"));
    let z = level_z + COMPUTATION_HEIGHT;
    let edges = plane_edges(&store, level_z);
    // Numbered in reading order: top to bottom, then left to right.
    let mut candidates: Vec<(Region, [f64; 2])> = arrangement::regions(&edges)
        .into_iter()
        .filter(|r| r.area() >= MIN_AREA)
        .filter_map(|r| {
            let p = interior_point(&r)?;
            Some((r, p))
        })
        .collect();
    // Regions whose tops are within half a metre are one row.
    let key = |r: &Region| {
        let ring = r.outer_ring();
        let top = ring.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
        let left = ring.iter().map(|p| p[0]).fold(f64::MAX, f64::min);
        ((-top * 2.0).round(), left)
    };
    candidates.sort_by(|a, b| {
        let (ka, kb) = (key(&a.0), key(&b.0));
        ka.0.total_cmp(&kb.0).then(ka.1.total_cmp(&kb.1))
    });
    let mut out = Vec::new();
    for (_, point) in candidates {
        if in_wall(txn.store(), z, point) || room_at(txn.store(), level, point).is_some() {
            continue;
        }
        out.push(place(
            txn,
            PlaceParams {
                level_id: ElementRef::Id(level),
                point,
                upper_limit: None,
                limit_offset: None,
                base_offset: None,
                name: None,
                number: None,
            },
        )?);
    }
    Ok(out)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub room_ids: Vec<ElementRef>,
    pub number: Option<String>,
    pub name: Option<String>,
    /// The upper limit level; `null` makes the room's own level its upper limit.
    #[serde(default, deserialize_with = "some")]
    pub upper_limit: Option<Option<ElementRef>>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub limit_offset: Option<Value>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base_offset: Option<Value>,
}

fn some<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> std::result::Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

/// Sets rooms' Number, Name, upper limit and offsets.
pub(crate) fn set(txn: &mut crate::txn::Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    if p.number.is_some() && p.room_ids.len() > 1 {
        return Err(Error::new(
            "room.number_not_unique",
            "several rooms cannot take one number",
        ));
    }
    let upper = match &p.upper_limit {
        Some(Some(u)) => Some(Some(resolve_level(&store, u)?.1)),
        Some(None) => Some(None),
        None => None,
    };
    let limit = p
        .limit_offset
        .as_ref()
        .map(|v| length(&store, v, "the limit offset"))
        .transpose()?;
    let base = p
        .base_offset
        .as_ref()
        .map(|v| length(&store, v, "the base offset"))
        .transpose()?;
    for r in &p.room_ids {
        let (id, mut recipe) = resolve_room(&store, r)?;
        if let Some(u) = &upper {
            recipe.upper_level = u.clone();
        }
        if let Some(l) = limit {
            recipe.limit_offset = l;
        }
        if let Some(b) = base {
            recipe.base_offset = b;
        }
        let value = serde_json::to_value(&recipe).expect("serialises");
        let (number, name) = (p.number.clone(), p.name.clone());
        txn.modify(id, |e| {
            e.params.insert(RECIPE_KEY.into(), value);
            if let Some(n) = number {
                e.name = Some(n);
            }
            if let Some(n) = name {
                e.params.insert(NAME_KEY.into(), json!(n));
            }
        })?;
        let g = regenerate(txn.store(), txn.store().get(id).expect("exists"))?;
        if g.unbounded_height() <= 0.0 {
            return Err(Error::new(
                "room.bad_limits",
                "the room's upper limit is at or below its base",
            ));
        }
    }
    Ok(())
}

/// Moves a room's point by a plan transform (`element.move`, copy, rotate, mirror).
pub(crate) fn translate(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    x: &dyn Fn([f64; 2]) -> [f64; 2],
) -> Result<()> {
    let mut r = room_recipe(txn.store().get(id).expect("exists")).expect("a room");
    r.point = x(r.point);
    let value = serde_json::to_value(&r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })
}
