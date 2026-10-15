// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Linings: walls by room and by segment (ARC-WALL-014), Auto Join and Auto Join & Lock
//! (ARC-WALL-015) and hosted walls (ARC-WALL-016).
//!
//! - **Hosted walls** keep `(host, side, offset)` and how far their ends reach past the host's
//!   ends; after every edit their location line is put back against the host's face, so they
//!   follow it when it moves, turns or changes thickness (even when it is pinned).
//! - **Join Geometry** between two parallel walls that touch face to face: doors, windows and
//!   openings in either cut both. Auto Join makes it when a lining is placed against a wall;
//!   Auto Join & Lock also locks the touching faces together (an alignment constraint).
//! - **By Segment** places a lining against a wall's face; **By Room** lines a room's whole
//!   boundary, one wall per straight segment, joined at the corners.
//!
//! Not built yet: columns as targets, room islands, curved targets, linked-model targets, the
//! hover preview and Spacebar flip while placing (the window takes a side or a flip option).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::{RECIPE_KEY, WallGeometry, WallRecipe, regenerate, wall_recipe};

/// The side of a wall a lining is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    #[default]
    Exterior,
    Interior,
}

/// A hosted wall's host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WallHost {
    /// The host wall (GlobalId).
    pub wall: String,
    pub side: Side,
    /// The gap between the host's face and the hosted wall (metres, ≥ 0).
    #[serde(default)]
    pub offset: f64,
    /// How far the hosted wall's start and end reach past the host's start and end, along the
    /// host (metres; negative stops short).
    #[serde(default)]
    pub ends: [f64; 2],
}

/// Whether Auto Join applies to a new wall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AutoJoin {
    #[default]
    Off,
    Join,
    JoinAndLock,
}

fn by_guid<'a>(store: &'a Store, guid: &str) -> Option<&'a Element> {
    store.get(store.find_by_guid(&GlobalId::parse(guid)?)?)
}

fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

/// The unit vector out of a wall's face on `side`.
fn outward(r: &WallRecipe, g: &WallGeometry, side: Side) -> [f64; 2] {
    let ext = if r.flip { -1.0 } else { 1.0 };
    let s = match side {
        Side::Exterior => ext,
        Side::Interior => -ext,
    };
    let n = g.normal();
    [n[0] * s, n[1] * s]
}

/// A hosted wall's location line from its host now.
fn hosted_line(store: &Store, e: &Element, h: &WallHost) -> Option<crate::walls::Line> {
    let host = by_guid(store, &h.wall)?;
    let (hr, hg) = (wall_recipe(host)?, regenerate(store, host).ok()?);
    let width = regenerate(store, e).ok()?.width;
    let o = outward(&hr, &hg, h.side);
    let d = hg.width / 2.0 + h.offset.max(0.0) + width / 2.0;
    let u = hg.direction();
    let at = |p: [f64; 2], along: f64| {
        [
            p[0] + o[0] * d + u[0] * along,
            p[1] + o[1] * d + u[1] * along,
        ]
    };
    Some(crate::walls::Line {
        start: at(hg.start, -h.ends[0]),
        end: at(hg.end, h.ends[1]),
    })
}

/// Puts every hosted wall back against its host (run after each edit).
pub(crate) fn follow_hosts(txn: &mut Transaction<'_>) -> Result<()> {
    let store = txn.store().clone();
    for e in store.iter() {
        let Some(mut r) = wall_recipe(e) else {
            continue;
        };
        let Some(h) = r.host.clone() else { continue };
        let Some(line) = hosted_line(&store, e, &h) else {
            continue;
        };
        let moved = (0..2).any(|k| {
            (line.start[k] - r.curve.start[k]).abs() > 1e-9
                || (line.end[k] - r.curve.end[k]).abs() > 1e-9
        });
        if moved {
            r.curve = line;
            write(txn, e.id(), &r)?;
        }
    }
    Ok(())
}

fn write(txn: &mut Transaction<'_>, id: ElementId, r: &WallRecipe) -> Result<()> {
    let value = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })
}

fn wall(store: &Store, r: &ElementRef) -> Result<(ElementId, WallRecipe)> {
    let id = store
        .resolve(r)
        .filter(|id| store.get(*id).and_then(wall_recipe).is_some())
        .ok_or_else(|| Error::new("element.not_found", format!("no Reviera wall {r:?}")))?;
    Ok((
        id,
        wall_recipe(store.get(id).expect("resolved")).expect("checked"),
    ))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetHostParams {
    pub wall_id: ElementRef,
    pub host_wall_id: ElementRef,
    /// The host's side the wall is on; omitted, the side it is on now.
    pub side: Option<Side>,
    /// The gap from the host's face (metres, ≥ 0); omitted, the gap now.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: Option<f64>,
}

/// `wall.set_host`: the wall follows the host from now on. It must be straight and parallel
/// to it.
pub(crate) fn set_host(txn: &mut Transaction<'_>, p: SetHostParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = wall(&store, &p.wall_id)?;
    let (host_id, hr) = wall(&store, &p.host_wall_id)?;
    if id == host_id {
        return Err(Error::new("wall.host_self", "a wall cannot host itself"));
    }
    if !r.through.is_empty() || !hr.through.is_empty() {
        return Err(Error::new(
            "wall.host_curved",
            "hosted walls and their hosts are straight",
        ));
    }
    // No cycles: the host must not be hosted, through any chain, by this wall.
    let guid = store.get(id).expect("resolved").guid().to_string();
    let mut at = Some(store.get(host_id).expect("resolved"));
    while let Some(w) = at {
        if w.guid().as_str() == guid {
            return Err(Error::new(
                "wall.host_cycle",
                "the host is hosted by this wall; that would be a cycle",
            ));
        }
        at = wall_recipe(w)
            .and_then(|wr| wr.host)
            .and_then(|h| by_guid(&store, &h.wall));
    }
    let (g, hg) = (
        regenerate(&store, store.get(id).expect("resolved"))?,
        regenerate(&store, store.get(host_id).expect("resolved"))?,
    );
    let (u, hu) = (g.direction(), hg.direction());
    if (u[0] * hu[1] - u[1] * hu[0]).abs() > 1e-6 {
        return Err(Error::new(
            "wall.host_not_parallel",
            "a hosted wall must be parallel to its host",
        ));
    }
    // Run the same way as the host, keeping the faces where they are.
    if dot(u, hu) < 0.0 {
        std::mem::swap(&mut r.curve.start, &mut r.curve.end);
        r.flip = !r.flip;
    }
    let hn = hg.normal();
    let across = dot(
        [
            r.curve.start[0] - hg.start[0],
            r.curve.start[1] - hg.start[1],
        ],
        hn,
    );
    let ext = if hr.flip { -1.0 } else { 1.0 };
    let side = p.side.unwrap_or(if across * ext >= 0.0 {
        Side::Exterior
    } else {
        Side::Interior
    });
    let gap = across.abs() - hg.width / 2.0 - g.width / 2.0;
    let offset = p.offset.unwrap_or(gap.max(0.0));
    if offset < 0.0 {
        return Err(Error::new(
            "wall.host_offset",
            "the offset must not be negative",
        ));
    }
    let ends = [
        -dot(
            [
                r.curve.start[0] - hg.start[0],
                r.curve.start[1] - hg.start[1],
            ],
            hu,
        ),
        dot([r.curve.end[0] - hg.end[0], r.curve.end[1] - hg.end[1]], hu),
    ];
    r.host = Some(WallHost {
        wall: store.get(host_id).expect("resolved").guid().to_string(),
        side,
        offset,
        ends,
    });
    write(txn, id, &r)?;
    follow_hosts(txn)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WallIdsParams {
    pub wall_ids: Vec<ElementRef>,
}

/// `wall.remove_host`: the walls stay where they are and no longer follow a host.
pub(crate) fn remove_host(txn: &mut Transaction<'_>, p: WallIdsParams) -> Result<()> {
    let store = txn.store().clone();
    for w in &p.wall_ids {
        let (id, mut r) = wall(&store, w)?;
        if r.host.take().is_some() {
            write(txn, id, &r)?;
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MainHostParams {
    pub wall_id: ElementRef,
}

/// `wall.select_main_host`: the wall's host, if it has one.
pub(crate) fn main_host(store: &Store, p: MainHostParams) -> Result<Option<ElementId>> {
    let (_, r) = wall(store, &p.wall_id)?;
    Ok(r.host
        .and_then(|h| by_guid(store, &h.wall))
        .map(Element::id))
}

// ---- Join Geometry ----

/// Two walls that touch face to face: each one's face reference (`face:ext` or `face:int`)
/// toward the other, or `None`.
fn touching(store: &Store, a: &Element, b: &Element) -> Option<(&'static str, &'static str)> {
    let (ra, rb) = (wall_recipe(a)?, wall_recipe(b)?);
    if !ra.through.is_empty() || !rb.through.is_empty() {
        return None;
    }
    let (ga, gb) = (regenerate(store, a).ok()?, regenerate(store, b).ok()?);
    let (ua, ub) = (ga.direction(), gb.direction());
    if (ua[0] * ub[1] - ua[1] * ub[0]).abs() > 1e-6 {
        return None;
    }
    let na = ga.normal();
    let across = dot([gb.start[0] - ga.start[0], gb.start[1] - ga.start[1]], na);
    if (across.abs() - (ga.width + gb.width) / 2.0).abs() > 1e-4 {
        return None;
    }
    // Overlapping along the walls and in height.
    let along = |p: [f64; 2]| dot([p[0] - ga.start[0], p[1] - ga.start[1]], ua);
    let (b0, b1) = (
        along(gb.start).min(along(gb.end)),
        along(gb.start).max(along(gb.end)),
    );
    if b1.min(ga.length()) - b0.max(0.0) < 1e-6 {
        return None;
    }
    if (ga.base_z + ga.height).min(gb.base_z + gb.height) - ga.base_z.max(gb.base_z) < 1e-6 {
        return None;
    }
    // A's face toward B is on +n when `across` is positive; exterior is +n unless flipped.
    let face = |toward_plus: bool, flip: bool| {
        if toward_plus != flip {
            "face:ext"
        } else {
            "face:int"
        }
    };
    let b_plus_toward_a = dot(gb.normal(), na) * across < 0.0;
    Some((face(across > 0.0, ra.flip), face(b_plus_toward_a, rb.flip)))
}

/// The walls joined to a wall by Join Geometry (both ways).
pub fn geometry_joined(store: &Store, e: &Element) -> Vec<ElementId> {
    let guid = e.guid().as_str();
    let mine: Vec<String> = wall_recipe(e).map(|r| r.geometry_joins).unwrap_or_default();
    store
        .iter()
        .filter(|o| o.id() != e.id())
        .filter(|o| {
            mine.iter().any(|g| g == o.guid().as_str())
                || wall_recipe(o).is_some_and(|r| r.geometry_joins.iter().any(|g| g == guid))
        })
        .map(Element::id)
        .collect()
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JoinParams {
    /// Two elements: walls that touch face to face.
    pub element_ids: Vec<ElementRef>,
    /// Also lock the touching faces together.
    #[serde(default)]
    pub lock: bool,
}

fn join_pair(txn: &mut Transaction<'_>, a: ElementId, b: ElementId, lock: bool) -> Result<()> {
    let store = txn.store().clone();
    let (ea, eb) = (store.get(a).expect("exists"), store.get(b).expect("exists"));
    let Some((fa, fb)) = touching(&store, ea, eb) else {
        return Err(Error::new(
            "geometry.join_not_touching",
            "Join Geometry is for walls that touch face to face (for now)",
        ));
    };
    let mut r = wall_recipe(ea).expect("a wall");
    let gb = eb.guid().to_string();
    if !geometry_joined(&store, ea).contains(&b) {
        r.geometry_joins.push(gb);
        write(txn, a, &r)?;
    }
    let slanted = |e: &Element| {
        wall_recipe(e).is_some_and(|r| {
            matches!(
                r.cross_section,
                Some(crate::walls::cross_section::CrossSection::Slanted { .. })
            )
        })
    };
    if lock && !slanted(ea) && !slanted(eb) {
        let g = regenerate(&store, ea)?;
        let point = |e: &Element, sub: &str| -> Option<[f64; 2]> {
            let r = crate::annotations::DimRef {
                element: Some(e.guid().to_string()),
                sub: sub.to_owned(),
                point: [0.0, 0.0],
                direction: [1.0, 0.0],
            };
            crate::annotations::resolve_ref(&store, &r).map(|(p, _)| p)
        };
        let (Some(pa), Some(pb)) = (point(ea, fa), point(eb, fb)) else {
            return Ok(());
        };
        let _ = g;
        crate::constraints::lock_alignment(
            txn,
            crate::constraints::AlignmentParams {
                ref_a: crate::annotations::DimRefArg {
                    element: Some(ElementRef::Id(a)),
                    sub: fa.to_owned(),
                    point: pa,
                    direction: None,
                },
                ref_b: crate::annotations::DimRefArg {
                    element: Some(ElementRef::Id(b)),
                    sub: fb.to_owned(),
                    point: pb,
                    direction: None,
                },
            },
        )?;
    }
    Ok(())
}

/// `geometry.join`.
pub(crate) fn join(txn: &mut Transaction<'_>, p: JoinParams) -> Result<()> {
    let store = txn.store().clone();
    let [a, b] = p.element_ids.as_slice() else {
        return Err(Error::new(
            "geometry.join_two",
            "Join Geometry takes two elements",
        ));
    };
    let (a, _) = wall(&store, a)?;
    let (b, _) = wall(&store, b)?;
    join_pair(txn, a, b, p.lock)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnjoinParams {
    pub element_ids: Vec<ElementRef>,
}

/// `geometry.unjoin`: removes Join Geometry between the elements (all their joins when one is
/// given).
pub(crate) fn unjoin(txn: &mut Transaction<'_>, p: UnjoinParams) -> Result<()> {
    let store = txn.store().clone();
    let ids: Vec<ElementId> = p
        .element_ids
        .iter()
        .map(|r| wall(&store, r).map(|(id, _)| id))
        .collect::<Result<_>>()?;
    let guids: Vec<String> = ids
        .iter()
        .map(|id| store.get(*id).expect("resolved").guid().to_string())
        .collect();
    for e in store.iter() {
        let Some(mut r) = wall_recipe(e) else {
            continue;
        };
        let me = guids.iter().any(|g| g == e.guid().as_str());
        let before = r.geometry_joins.len();
        r.geometry_joins.retain(|g| {
            let other = guids.contains(g);
            !((me && (other || ids.len() == 1)) || (other && ids.len() == 1 && !me))
        });
        if r.geometry_joins.len() != before {
            write(txn, e.id(), &r)?;
        }
    }
    Ok(())
}

/// Auto Join for a new wall: Join Geometry with each wall it touches face to face (and a lock).
pub(crate) fn auto_join(txn: &mut Transaction<'_>, id: ElementId, mode: AutoJoin) -> Result<()> {
    if mode == AutoJoin::Off {
        return Ok(());
    }
    let store = txn.store().clone();
    let e = store.get(id).expect("exists");
    if crate::walls::stacked::of_wall(&store, e).is_some() {
        return Ok(());
    }
    let others: Vec<ElementId> = store
        .iter()
        .filter(|o| o.id() != id && wall_recipe(o).is_some())
        .filter(|o| crate::walls::stacked::of_wall(&store, o).is_none())
        .filter(|o| touching(&store, e, o).is_some())
        .map(Element::id)
        .collect();
    for o in others {
        join_pair(txn, id, o, mode == AutoJoin::JoinAndLock)?;
    }
    Ok(())
}

/// The cuts of walls joined by Join Geometry: each wall's door, window and opening cuts carried
/// into the walls joined to it, in their own frames.
pub fn joined_cuts(
    store: &Store,
    own: &std::collections::BTreeMap<ElementId, Vec<crate::walls::openings::Cut>>,
) -> Vec<(ElementId, crate::walls::openings::Cut)> {
    let mut out = Vec::new();
    for (host, cuts) in own {
        let Some(h) = store.get(*host) else { continue };
        let Ok(hg) = regenerate(store, h) else {
            continue;
        };
        for other in geometry_joined(store, h) {
            let Ok(og) = regenerate(store, store.get(other).expect("joined")) else {
                continue;
            };
            let (hu, ou) = (hg.direction(), og.direction());
            let along = |x: f64| {
                let p = [hg.start[0] + hu[0] * x, hg.start[1] + hu[1] * x];
                dot([p[0] - og.start[0], p[1] - og.start[1]], ou)
            };
            for c in cuts {
                let (a, b) = (along(c.x[0]), along(c.x[1]));
                let outline = c
                    .outline
                    .iter()
                    .map(|q| [along(q[0]), q[1]])
                    .collect::<Vec<_>>();
                let mut outline = outline;
                if a > b {
                    outline.reverse();
                }
                out.push((
                    other,
                    crate::walls::openings::Cut {
                        x: [a.min(b), a.max(b)],
                        z: c.z,
                        outline,
                    },
                ));
            }
        }
    }
    out
}

// ---- Walls by segment and by room ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BySegmentParams {
    /// The lining's wall type; omitted, the project's default.
    pub type_id: Option<ElementRef>,
    /// The wall to line.
    pub target_id: ElementRef,
    #[serde(default)]
    pub side: Side,
    /// Host the lining on the target (it follows it).
    #[serde(default)]
    pub hosted: bool,
    #[serde(default)]
    pub auto_join: AutoJoin,
}

/// Where a new lining stands: its base level (GlobalId) and offset, and its top as a wall
/// recipe's (`{"level": guid}` or `{"unconnected": h}`) with its offset.
struct Extent<'a> {
    base_level: &'a str,
    base_offset: f64,
    top: Value,
    top_offset: f64,
}

fn new_wall(
    txn: &mut Transaction<'_>,
    type_id: Option<ElementRef>,
    line: crate::walls::Line,
    extent: Extent<'_>,
    flip: bool,
) -> Result<ElementId> {
    let Extent {
        base_level,
        base_offset,
        top,
        top_offset,
    } = extent;
    let store = txn.store().clone();
    let level = by_guid(&store, base_level)
        .map(Element::id)
        .ok_or_else(|| Error::new("level.missing", "the level is missing"))?;
    let top_constraint = match top
        .get("level")
        .and_then(Value::as_str)
        .and_then(|g| by_guid(&store, g))
    {
        Some(l) => json!({ "level": l.id().0 }),
        None => {
            json!({ "unconnected": top.get("unconnected").and_then(Value::as_f64).unwrap_or(3.0) })
        }
    };
    let p: crate::walls::CreateParams = serde_json::from_value(json!({
        "type_id": type_id,
        "curve": { "line": { "start": line.start, "end": line.end } },
        "base_level": level.0,
        "top_constraint": top_constraint,
        "base_offset": base_offset,
        "top_offset": top_offset,
        "flip": flip,
    }))
    .map_err(|e| Error::new("args.invalid", e.to_string()))?;
    crate::walls::create(txn, p)
}

/// `wall.create_by_segment`: a lining against a wall's face, as long as the face (after joins).
pub(crate) fn by_segment(txn: &mut Transaction<'_>, p: BySegmentParams) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let (target, tr) = wall(&store, &p.target_id)?;
    if !tr.through.is_empty() {
        return Err(Error::new(
            "wall.lining_curved",
            "linings go against straight walls (for now)",
        ));
    }
    let solved = crate::walls::solve(&store);
    let body = solved
        .bodies
        .get(&target)
        .ok_or_else(|| Error::new("wall.not_found", "the target does not regenerate"))?;
    let g = &body.geometry;
    let o = outward(&tr, g, p.side);
    let f = body.footprint;
    let d = |q: [f64; 2]| dot([q[0] - g.start[0], q[1] - g.start[1]], o);
    let (a, b) = if d(f[0]) > d(f[3]) {
        (f[0], f[1])
    } else {
        (f[3], f[2])
    };
    // The lining's width, from its type.
    let width = lining_width(txn, &p.type_id)?;
    let shift = |q: [f64; 2]| [q[0] + o[0] * width / 2.0, q[1] + o[1] * width / 2.0];
    // Drawn with the target on its right (−n) and flipped, so its exterior faces the target and
    // its interior the room, as By Room does.
    let (start, end) = (shift(a), shift(b));
    let u = g.direction();
    let toward_left_is_target = {
        let n = [-u[1], u[0]];
        dot(n, o) < 0.0
    };
    let line = if toward_left_is_target {
        crate::walls::Line {
            start: end,
            end: start,
        }
    } else {
        crate::walls::Line { start, end }
    };
    let id = new_wall(
        txn,
        p.type_id.clone(),
        line,
        Extent {
            base_level: &tr.base_level,
            base_offset: tr.base_offset,
            top: tr.top.clone(),
            top_offset: tr.top_offset,
        },
        true,
    )?;
    if p.hosted {
        set_host(
            txn,
            SetHostParams {
                wall_id: ElementRef::Id(id),
                host_wall_id: ElementRef::Id(target),
                side: Some(p.side),
                offset: Some(0.0),
            },
        )?;
    }
    let mode = if p.hosted && p.auto_join == AutoJoin::Off {
        AutoJoin::Join
    } else {
        p.auto_join
    };
    auto_join(txn, id, mode)?;
    Ok(vec![id])
}

fn lining_width(txn: &mut Transaction<'_>, type_id: &Option<ElementRef>) -> Result<f64> {
    let t = match type_id {
        Some(r) => txn
            .store()
            .resolve(r)
            .ok_or_else(|| Error::new("element.not_found", "no such wall type"))?,
        None => crate::walls::default_type(txn)?,
    };
    txn.store()
        .get(t)
        .and_then(crate::walls::type_width)
        .ok_or_else(|| Error::new("wall.no_type_width", "the wall type defines no width"))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ByRoomParams {
    pub type_id: Option<ElementRef>,
    /// The room to line.
    pub room_id: ElementRef,
    /// Put the linings' exterior toward the room instead of toward the walls.
    #[serde(default)]
    pub flip: bool,
    /// Host each lining on the wall it lines.
    #[serde(default)]
    pub hosted: bool,
    #[serde(default)]
    pub auto_join: AutoJoin,
    /// The linings' height; omitted, the room's.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub height: Option<f64>,
}

/// `wall.create_by_room`: linings round a room's boundary, one per straight segment, joined at
/// the corners.
pub(crate) fn by_room(txn: &mut Transaction<'_>, p: ByRoomParams) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let room = store
        .resolve(&p.room_id)
        .and_then(|id| store.get(id))
        .filter(|e| crate::rooms::room_recipe(e).is_some())
        .ok_or_else(|| Error::new("element.not_found", "no such room"))?;
    let rr = crate::rooms::room_recipe(room).expect("checked");
    let rg = crate::rooms::regenerate(&store, room)?;
    let region = rg
        .region
        .ok_or_else(|| Error::new("room.not_enclosed", "the room is not enclosed"))?;
    let mut edges: Vec<crate::arrangement::Edge> = region.outer.clone();
    // Counter-clockwise, so the room is on each edge's left.
    let ring: Vec<[f64; 2]> = edges.iter().map(|e| e.line.start).collect();
    if crate::polygons::area(&ring) < 0.0 {
        edges.reverse();
        for e in &mut edges {
            std::mem::swap(&mut e.line.start, &mut e.line.end);
        }
    }
    let edges: Vec<crate::arrangement::Edge> = edges
        .into_iter()
        .filter(|e| e.line.length() > 1e-6)
        .collect();
    let n = edges.len();
    if n < 3 {
        return Err(Error::new(
            "room.not_enclosed",
            "the room has no boundary to line",
        ));
    }
    let width = lining_width(txn, &p.type_id)?;
    // Each edge moved into the room by half the lining's width; corners where they meet.
    let offset = |e: &crate::arrangement::Edge| {
        let l = e.line.length();
        let u = [
            (e.line.end[0] - e.line.start[0]) / l,
            (e.line.end[1] - e.line.start[1]) / l,
        ];
        let left = [-u[1], u[0]];
        let s = [
            e.line.start[0] + left[0] * width / 2.0,
            e.line.start[1] + left[1] * width / 2.0,
        ];
        (s, u)
    };
    let meet = |a: ([f64; 2], [f64; 2]), b: ([f64; 2], [f64; 2])| -> [f64; 2] {
        let (p, r) = a;
        let (q, s) = b;
        let cross = r[0] * s[1] - r[1] * s[0];
        if cross.abs() < 1e-9 {
            return q;
        }
        let t = ((q[0] - p[0]) * s[1] - (q[1] - p[1]) * s[0]) / cross;
        [p[0] + r[0] * t, p[1] + r[1] * t]
    };
    let lines: Vec<_> = edges.iter().map(offset).collect();
    let corners: Vec<[f64; 2]> = (0..n)
        .map(|k| meet(lines[(k + n - 1) % n], lines[k]))
        .collect();
    let height = p.height.unwrap_or(rg.z_top - rg.z_base);
    let height = if height > 1e-6 { height } else { 3.0 };
    let mut made = Vec::new();
    for k in 0..n {
        let (a, b) = (corners[k], corners[(k + 1) % n]);
        if (b[0] - a[0]).hypot(b[1] - a[1]) < 1e-6 {
            continue;
        }
        // The room is on the left (+n, the exterior unless flipped): flipped, the interior
        // faces the room.
        let id = new_wall(
            txn,
            p.type_id.clone(),
            crate::walls::Line { start: a, end: b },
            Extent {
                base_level: &rr.level,
                base_offset: rr.base_offset,
                top: json!({ "unconnected": height }),
                top_offset: 0.0,
            },
            !p.flip,
        )?;
        // The wall it lines, if the boundary came from one.
        let target = edges[k]
            .lock
            .as_ref()
            .map(|(w, _)| *w)
            .filter(|w| store.get(*w).and_then(wall_recipe).is_some());
        if let (true, Some(t)) = (p.hosted, target) {
            set_host(
                txn,
                SetHostParams {
                    wall_id: ElementRef::Id(id),
                    host_wall_id: ElementRef::Id(t),
                    side: None,
                    offset: Some(0.0),
                },
            )?;
        }
        made.push(id);
    }
    let mode = if p.hosted && p.auto_join == AutoJoin::Off {
        AutoJoin::Join
    } else {
        p.auto_join
    };
    for id in &made {
        auto_join(txn, *id, mode)?;
    }
    Ok(made)
}
