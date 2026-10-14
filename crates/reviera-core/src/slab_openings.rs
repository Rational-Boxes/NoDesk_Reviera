// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Vertical openings (ARC-OPEN-002), shafts (ARC-OPEN-004) and stair openings (ARC-STAIR-007).
//!
//! A vertical opening is an `IfcOpeningElement` with a `recipe.vertical_opening`: its host floor,
//! ceiling or roof (GlobalId) and its loops in plan. It cuts that host only, plumb through its
//! whole thickness; a loop partly outside a floor or ceiling notches its edge. Reviera's own
//! display cuts the host's body ([`cut_profiles`]; a roof's face slabs, [`roof_holes`]); the IFC
//! keeps a floor's body whole and voids it with the opening, as for walls. It moves and is
//! mirrored with its host and goes when the host goes.
//!
//! A shaft is an `IfcBuildingElementProxy` (`PROVISIONFORVOID`) with a `recipe.shaft`: loops in
//! plan and a vertical extent between levels. It cuts every floor, ceiling and roof whose body
//! its extent reaches into, plumb, including ones added later; walls are not cut. In IFC its
//! body is the whole prism, and each host it cuts gets its own `IfcOpeningElement` (the part of
//! the prism through that host) voiding it, which records the shaft.
//!
//! A stair opening is a vertical opening made for a stair: where a floor or ceiling over the
//! stair leaves less than the headroom of the building code in force (`codes`), measured from
//! the line of the nosings (a landing's floor), the floor is opened over the stair's full width.
//! It records the stair, the headroom and the clause it follows; its grips edit it afterwards
//! like any opening.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::polygons::{self, Ring, Shape};
use crate::sketch::Profile;
use crate::store::Store;
use crate::walls::RECIPE_KEY;

pub const OPENING_RECIPE: &str = "recipe.vertical_opening";
pub const SHAFT_RECIPE: &str = "recipe.shaft";

/// The stored recipe of a vertical opening.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OpeningRecipe {
    pub kind: String,
    /// The floor or ceiling it cuts (GlobalId).
    pub host: String,
    /// Closed loops in plan (metres).
    pub loops: Vec<Ring>,
    /// The stair it was made for (GlobalId), its headroom (metres) and the clause.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stair: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headroom: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clause: Option<String>,
}

/// The stored recipe of a shaft.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ShaftRecipe {
    pub kind: String,
    /// Closed loops in plan (metres).
    pub loops: Vec<Ring>,
    #[serde(flatten)]
    pub extent: crate::levels::Extent,
}

pub fn shaft_recipe(e: &Element) -> Option<ShaftRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == SHAFT_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// Every shaft that resolves, with its bottom and top elevations.
pub fn shafts(store: &Store) -> Vec<(ElementId, ShaftRecipe, (f64, f64))> {
    store
        .iter()
        .filter_map(|e| {
            let r = shaft_recipe(e)?;
            let z = r.extent.z(store).ok()?;
            Some((e.id(), r, z))
        })
        .collect()
}

/// The shafts whose extent reaches into a body between elevations `lo` and `hi`.
pub fn shafts_through(store: &Store, lo: f64, hi: f64) -> Vec<(ElementId, ShaftRecipe)> {
    shafts(store)
        .into_iter()
        .filter(|(_, _, (z0, z1))| *z0 < hi - 1e-6 && *z1 > lo + 1e-6)
        .map(|(id, r, _)| (id, r))
        .collect()
}

/// The lowest and highest points of a roof's body.
pub fn roof_span(g: &crate::roofs::RoofGeometry) -> (f64, f64) {
    g.faces
        .iter()
        .flat_map(|f| f.slab.0.iter())
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(p[2]), hi.max(p[2]))
        })
}

/// The plan holes a roof's own vertical openings and the shafts through it cut, for
/// [`crate::roofs::cut_holes`].
pub fn roof_holes(store: &Store, roof: &Element, g: &crate::roofs::RoofGeometry) -> Vec<Ring> {
    let mut out: Vec<Ring> = of_host(store, roof.guid().as_str())
        .into_iter()
        .flat_map(|(_, r)| r.loops)
        .collect();
    let (lo, hi) = roof_span(g);
    if lo <= hi {
        out.extend(
            shafts_through(store, lo, hi)
                .into_iter()
                .flat_map(|(_, r)| r.loops),
        );
    }
    out
}

pub fn opening_recipe(e: &Element) -> Option<OpeningRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == OPENING_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

/// The vertical openings cutting `host` (GlobalId), with their loops.
pub fn of_host(store: &Store, host: &str) -> Vec<(ElementId, OpeningRecipe)> {
    store
        .iter()
        .filter_map(|e| Some((e.id(), opening_recipe(e)?)))
        .filter(|(_, r)| r.host == host)
        .collect()
}

fn to_shapes(profiles: &[Profile]) -> Vec<Shape> {
    profiles
        .iter()
        .map(|p| Shape {
            outer: p.outer.clone(),
            holes: p.holes.clone(),
        })
        .collect()
}

/// A floor's or ceiling's profiles less its vertical openings and the shafts through it (for
/// display and measuring).
pub fn cut_profiles(store: &Store, host: &Element, profiles: &[Profile]) -> Vec<Profile> {
    let mut loops: Vec<Ring> = of_host(store, host.guid().as_str())
        .into_iter()
        .flat_map(|(_, r)| r.loops)
        .collect();
    if store.iter().any(|e| shaft_recipe(e).is_some())
        && let Ok(f) = crate::floors::regenerate(store, host)
    {
        loops.extend(
            shafts_through(store, f.top - f.thickness, f.top)
                .into_iter()
                .flat_map(|(_, r)| r.loops),
        );
    }
    if loops.is_empty() {
        return profiles.to_vec();
    }
    polygons::difference(&to_shapes(profiles), &loops)
        .into_iter()
        .map(|s| Profile {
            outer: s.outer,
            holes: s.holes,
        })
        .collect()
}

/// How much of `loops` lies on the host's profiles (square metres).
fn overlap_area(profiles: &[Profile], loops: &[Ring]) -> f64 {
    let cut = polygons::union(loops);
    polygons::intersection(&to_shapes(profiles), &cut)
        .iter()
        .map(Shape::area)
        .sum()
}

fn check_loops(loops: &[Ring]) -> Result<Vec<Ring>> {
    let out: Vec<Ring> = loops.iter().map(|l| polygons::simplify(l)).collect();
    if out.is_empty()
        || out
            .iter()
            .any(|l| l.len() < 3 || polygons::area(l).abs() < 1e-6)
    {
        return Err(Error::new(
            "opening.bad_loop",
            "each loop needs at least three corners around some area",
        ));
    }
    Ok(out)
}

fn host_of(store: &Store, r: &ElementRef) -> Result<ElementId> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such floor, ceiling or roof"))?;
    let e = store.get(id).expect("resolved");
    if let Some(roof) = crate::roofs::roof_recipe(e) {
        if roof.arch.is_some() || roof.dome.is_some() {
            return Err(Error::new(
                "opening.bad_host",
                "openings in arched roofs and domes are not supported yet",
            ));
        }
        return Ok(id);
    }
    if crate::floors::slab_recipe(e).is_none() {
        return Err(Error::new(
            "opening.bad_host",
            format!("element {id} is not a Reviera floor, ceiling or roof"),
        ));
    }
    Ok(id)
}

/// A roof's faces seen from above, as profiles.
fn roof_profiles(g: &crate::roofs::RoofGeometry) -> Vec<Profile> {
    g.faces
        .iter()
        .map(|f| Profile {
            outer: f.bottom.iter().map(|p| [p[0], p[1]]).collect(),
            holes: Vec::new(),
        })
        .collect()
}

/// A floor's, ceiling's or roof's plan profiles.
fn host_profiles(store: &Store, host: ElementId) -> Result<Vec<Profile>> {
    let e = store.get(host).expect("exists");
    if crate::roofs::roof_recipe(e).is_some() {
        return Ok(roof_profiles(&crate::roofs::regenerate(store, e)?));
    }
    Ok(crate::floors::regenerate(store, e)?.profiles)
}

fn intersects_host(store: &Store, host: ElementId, loops: &[Ring]) -> Result<()> {
    let profiles = host_profiles(store, host)?;
    if overlap_area(&profiles, loops) < 1e-6 {
        return Err(Error::new(
            "opening.misses_host",
            "Opening does not intersect host",
        ));
    }
    Ok(())
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// A Reviera floor, ceiling or footprint roof.
    pub host_id: ElementRef,
    /// Closed loops in plan (metres), each a list of corners.
    #[serde(default)]
    pub loops: Vec<Ring>,
    /// Or sketch curves (lines, arcs through a point) forming closed loops, as floors take them.
    #[serde(default)]
    pub curves: Vec<crate::sketch::CurveArg>,
}

/// The loops of a command: given as corners, or as sketch curves (validated like a floor's
/// sketch; each profile's outer loop and holes).
fn loops_of(
    store: &Store,
    loops: &[Ring],
    curves: &[crate::sketch::CurveArg],
) -> Result<Vec<Ring>> {
    if curves.is_empty() {
        return check_loops(loops);
    }
    if !loops.is_empty() {
        return Err(Error::new(
            "opening.loops_and_curves",
            "give either loops or curves, not both",
        ));
    }
    let sketch = crate::sketch::from_args(store, curves)?;
    let rings: Vec<Ring> = crate::sketch::profiles(store, &sketch)
        .into_iter()
        .flat_map(|p| std::iter::once(p.outer).chain(p.holes))
        .collect();
    check_loops(&rings)
}

/// Cuts a floor, ceiling or roof plumb by loops (`opening.create_vertical`).
pub(crate) fn create(txn: &mut crate::txn::Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let host = host_of(&store, &p.host_id)?;
    let loops = loops_of(&store, &p.loops, &p.curves)?;
    intersects_host(&store, host, &loops)?;
    let recipe = OpeningRecipe {
        kind: OPENING_RECIPE.into(),
        host: store.get(host).expect("exists").guid().to_string(),
        loops,
        stair: None,
        headroom: None,
        clause: None,
    };
    make(txn, &recipe, "Opening")
}

fn make(txn: &mut crate::txn::Transaction<'_>, r: &OpeningRecipe, name: &str) -> Result<ElementId> {
    txn.create(
        NewElement::new("IfcOpeningElement")
            .name(name)
            .param("@PredefinedType", "OPENING")
            .param(RECIPE_KEY, serde_json::to_value(r).expect("serialises")),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetLoopsParams {
    pub opening_id: ElementRef,
    pub loops: Vec<Ring>,
}

fn resolve_opening(store: &Store, r: &ElementRef) -> Result<(ElementId, OpeningRecipe)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such opening"))?;
    let recipe = opening_recipe(store.get(id).expect("resolved")).ok_or_else(|| {
        Error::new(
            "opening.not_vertical",
            format!("element {id} is not a vertical opening"),
        )
    })?;
    Ok((id, recipe))
}

/// Gives an opening new loops (`opening.set_loops`; its grips).
pub(crate) fn set_loops(txn: &mut crate::txn::Transaction<'_>, p: SetLoopsParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut recipe) = resolve_opening(&store, &p.opening_id)?;
    crate::modify::check_unpinned(&store, id)?;
    recipe.loops = check_loops(&p.loops)?;
    let host = find(&store, &recipe.host)
        .ok_or_else(|| Error::new("opening.no_host", "the opening's host no longer exists"))?;
    intersects_host(&store, host, &recipe.loops)?;
    let value = serde_json::to_value(&recipe).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })
}

/// Moves (or mirrors) the openings of `host` with it.
pub(crate) fn follow_host(
    txn: &mut crate::txn::Transaction<'_>,
    host: &str,
    x: &dyn Fn([f64; 2]) -> [f64; 2],
) -> Result<()> {
    for (id, mut r) in of_host(txn.store(), host) {
        for l in &mut r.loops {
            for p in l.iter_mut() {
                *p = x(*p);
            }
        }
        let value = serde_json::to_value(&r).expect("serialises");
        txn.modify(id, |e| {
            e.params.insert(RECIPE_KEY.into(), value);
        })?;
    }
    Ok(())
}

/// The host a vertical opening sketched on level `level` (GlobalId) cuts: of the floors (or,
/// with `ceiling`, the ceilings) on that level, the one the loops overlap most; else the roof
/// they overlap most.
pub fn host_for(store: &Store, level: &str, loops: &[Ring], ceiling: bool) -> Option<ElementId> {
    let best = |c: Vec<(ElementId, f64)>| {
        c.into_iter()
            .filter(|(_, a)| *a > 1e-6)
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    };
    let slabs: Vec<(ElementId, f64)> = crate::floors::all(store)
        .into_iter()
        .filter(|(id, _)| {
            let e = store.get(*id).expect("listed");
            crate::decks::deck_settings(e).is_none()
                && crate::floors::slab_recipe(e).is_some_and(|(k, r)| {
                    r.level == level && (k == crate::floors::SlabKind::Ceiling) == ceiling
                })
        })
        .map(|(id, f)| (id, overlap_area(&f.profiles, loops)))
        .collect();
    best(slabs).or_else(|| {
        best(
            crate::roofs::all(store)
                .into_iter()
                .filter(|(id, _)| {
                    crate::roofs::roof_recipe(store.get(*id).expect("listed"))
                        .is_some_and(|r| r.arch.is_none() && r.dome.is_none())
                })
                .map(|(id, g)| (id, overlap_area(&roof_profiles(&g), loops)))
                .collect(),
        )
    })
}

/// The shafts drawn in a plan of the level at elevation `z`: those whose extent covers it.
pub fn shafts_at(store: &Store, z: f64) -> Vec<(ElementId, ShaftRecipe)> {
    shafts(store)
        .into_iter()
        .filter(|(_, _, (z0, z1))| *z0 <= z + 1e-6 && *z1 > z + 1e-6)
        .map(|(id, r, _)| (id, r))
        .collect()
}

// ---- Shafts (ARC-OPEN-004) ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateShaftParams {
    /// Closed loops in plan (metres), each a list of corners.
    #[serde(default)]
    pub loops: Vec<Ring>,
    /// Or sketch curves forming closed loops.
    #[serde(default)]
    pub curves: Vec<crate::sketch::CurveArg>,
    pub base_level: ElementRef,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base_offset: f64,
    /// A level, or a height above the base.
    pub top_constraint: crate::walls::TopConstraint,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub top_offset: f64,
}

/// Makes a shaft (`opening.create_shaft`).
pub(crate) fn create_shaft(
    txn: &mut crate::txn::Transaction<'_>,
    p: CreateShaftParams,
) -> Result<ElementId> {
    let store = txn.store().clone();
    let loops = loops_of(&store, &p.loops, &p.curves)?;
    let extent = crate::levels::Extent::from_args(
        &store,
        &p.base_level,
        p.base_offset,
        p.top_constraint,
        p.top_offset,
    )?;
    let recipe = ShaftRecipe {
        kind: SHAFT_RECIPE.into(),
        loops,
        extent,
    };
    txn.create(
        NewElement::new("IfcBuildingElementProxy")
            .name("Shaft Opening")
            .param("@PredefinedType", "PROVISIONFORVOID")
            .param(
                RECIPE_KEY,
                serde_json::to_value(&recipe).expect("serialises"),
            ),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditShaftParams {
    pub shaft_id: ElementRef,
    /// New loops (corners), kept when omitted.
    pub loops: Option<Vec<Ring>>,
    pub base_level: Option<ElementRef>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base_offset: Option<f64>,
    pub top_constraint: Option<crate::walls::TopConstraint>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub top_offset: Option<f64>,
}

/// Changes a shaft's loops or extent (`opening.edit_shaft`); omitted values are kept.
pub(crate) fn edit_shaft(txn: &mut crate::txn::Transaction<'_>, p: EditShaftParams) -> Result<()> {
    let store = txn.store().clone();
    let id = store
        .resolve(&p.shaft_id)
        .ok_or_else(|| Error::new("element.not_found", "no such shaft"))?;
    let mut r = shaft_recipe(store.get(id).expect("resolved"))
        .ok_or_else(|| Error::new("opening.not_shaft", format!("element {id} is not a shaft")))?;
    crate::modify::check_unpinned(&store, id)?;
    if let Some(l) = &p.loops {
        r.loops = check_loops(l)?;
    }
    let base = match &p.base_level {
        Some(b) => b.clone(),
        None => ElementRef::Id(
            find(&store, &r.extent.base_level)
                .ok_or_else(|| Error::new("level.missing", "the shaft's base level is gone"))?,
        ),
    };
    let top = match p.top_constraint {
        Some(t) => t,
        None => match r.extent.top_level().and_then(|g| find(&store, g)) {
            Some(l) => crate::walls::TopConstraint::Level(crate::walls::ElementRefValue::Id(l)),
            None => crate::walls::TopConstraint::Unconnected(
                r.extent.top["unconnected"].as_f64().unwrap_or(3.0),
            ),
        },
    };
    r.extent = crate::levels::Extent::from_args(
        &store,
        &base,
        p.base_offset.unwrap_or(r.extent.base_offset),
        top,
        p.top_offset.unwrap_or(r.extent.top_offset),
    )?;
    let value = serde_json::to_value(&r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })
}

/// Moves (or mirrors) a shaft's loops by `x` (`element.move` and the other modify tools).
pub(crate) fn transform_shaft(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    x: &dyn Fn([f64; 2]) -> [f64; 2],
    dz: f64,
) -> Result<()> {
    let store = txn.store().clone();
    let mut r = shaft_recipe(store.get(id).expect("exists")).expect("a shaft");
    for l in &mut r.loops {
        for p in l.iter_mut() {
            *p = x(*p);
        }
    }
    r.extent.base_offset += dz;
    if r.extent.top_level().is_some() {
        r.extent.top_offset += dz;
    }
    let value = serde_json::to_value(&r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })
}

/// The hosts a shaft cuts now: every floor, ceiling (not decks) and roof its extent reaches
/// into whose plan it overlaps, with the elevations of the host's body there.
pub fn shaft_hosts(store: &Store, shaft: &ShaftRecipe) -> Vec<(ElementId, f64, f64)> {
    let Ok((z0, z1)) = shaft.extent.z(store) else {
        return Vec::new();
    };
    let reaches = |lo: f64, hi: f64| z0 < hi - 1e-6 && z1 > lo + 1e-6;
    let mut out = Vec::new();
    for (id, f) in crate::floors::all(store) {
        let e = store.get(id).expect("listed");
        if crate::decks::deck_settings(e).is_some() {
            continue;
        }
        let (lo, hi) = (f.top - f.thickness, f.top);
        if reaches(lo, hi) && overlap_area(&f.profiles, &shaft.loops) > 1e-6 {
            out.push((id, lo, hi));
        }
    }
    for (id, g) in crate::roofs::all(store) {
        let (lo, hi) = roof_span(&g);
        if lo <= hi && reaches(lo, hi) && overlap_area(&roof_profiles(&g), &shaft.loops) > 1e-6 {
            out.push((id, lo, hi));
        }
    }
    out
}

// ---- Stair openings (ARC-STAIR-007) ----

/// A floor or ceiling over a stair that leaves less than the headroom, and the opening it needs.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Obstruction {
    pub host: ElementId,
    pub host_name: String,
    /// The loops the opening needs.
    pub loops: Vec<Ring>,
    /// Its area on the host (square metres).
    pub area: f64,
    /// The least headroom over the stair under the host now (metres).
    pub clearance: f64,
    /// An opening already made for this stair in it.
    pub existing: Option<ElementId>,
    /// How much of the region the host still covers (square metres): none once opened.
    pub uncovered: f64,
}

/// The headroom check of a stair under `headroom` (metres; `spiral` over spiral runs): every
/// floor or ceiling that leaves less over a tread (from the nosing line, approximated by the
/// next nosing's height) or a landing, or that the stair passes through, and the plan region
/// it must open: the union of those treads and landings, over the stair's full width.
pub fn obstructions(
    store: &Store,
    stair: ElementId,
    headroom: f64,
    spiral: f64,
) -> Result<Vec<Obstruction>> {
    let e = store
        .get(stair)
        .ok_or_else(|| Error::new("element.not_found", "no such stair"))?;
    let recipe = crate::stairs::stair_recipe(e).ok_or_else(|| {
        Error::new(
            "stair.not_reviera",
            format!("element {stair} is not a Reviera stair"),
        )
    })?;
    let g = crate::stairs::regenerate(store, e)?;
    let need = if recipe.runs.iter().any(|r| r.spiral.is_some()) {
        spiral
    } else {
        headroom
    };
    // The walking surfaces: upward faces of the runs (treads, lifted to the next nosing) and of
    // the landings.
    // Each: the triangle, its height and the height headroom is measured from.
    let mut surfaces: Vec<([[f64; 2]; 3], f64, f64)> = Vec::new();
    let mut collect = |meshes: &[crate::stairs::Mesh], lift: f64| {
        for (pts, idx) in meshes {
            for t in idx.chunks(3) {
                let [a, b, c] = [pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]];
                let n = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
                let flat = (a[2] - b[2]).abs() < 1e-6 && (a[2] - c[2]).abs() < 1e-6;
                if flat && n > 1e-9 {
                    surfaces.push((
                        [[a[0], a[1]], [b[0], b[1]], [c[0], c[1]]],
                        a[2],
                        a[2] + lift,
                    ));
                }
            }
        }
    };
    collect(&g.runs, g.riser_height);
    collect(&g.landings, 0.0);
    let top = g.z_base + g.height;
    // How far the stair's body reaches below a walking surface: a riser, and its structure.
    let t = e
        .param("@type")
        .and_then(Value::as_str)
        .and_then(|guid| find(store, guid))
        .and_then(|t| store.get(t))
        .and_then(crate::stairs::stair_type)
        .unwrap_or_default();
    let body = g.riser_height + t.structural_depth.max(t.landing_thickness);
    let mut out = Vec::new();
    for (id, f) in crate::floors::all(store) {
        let host = store.get(id).expect("listed");
        if crate::decks::deck_settings(host).is_some() {
            continue;
        }
        let underside = f.top - f.thickness;
        // Floors the stair stands on, or above its top by more than the headroom, leave room.
        if underside <= g.z_base + 1e-6 || underside > top + need {
            continue;
        }
        let low: Vec<&([[f64; 2]; 3], f64, f64)> = surfaces
            .iter()
            // Under the slab and short of headroom, or passing through it.
            .filter(|(_, z, from)| *z < f.top + body - 1e-6 && underside - from < need - 1e-6)
            .collect();
        if low.is_empty() {
            continue;
        }
        let rings: Vec<Ring> = low.iter().map(|(t, _, _)| t.to_vec()).collect();
        let region = polygons::union(&rings);
        let on_host: f64 = polygons::intersection(&to_shapes(&f.profiles), &region)
            .iter()
            .map(Shape::area)
            .sum();
        if on_host < 1e-4 {
            continue;
        }
        let clearance = low
            .iter()
            .filter(|(t, _, _)| {
                let ring = vec![t.to_vec()];
                overlap_area(&f.profiles, &ring) > 1e-9
            })
            .map(|(_, _, from)| underside - from)
            .fold(f64::INFINITY, f64::min);
        let guid = e.guid().to_string();
        let existing = of_host(store, host.guid().as_str())
            .into_iter()
            .find(|(_, r)| r.stair.as_deref() == Some(guid.as_str()))
            .map(|(i, _)| i);
        // Openings already there count: the floor is only short of headroom where it is whole.
        let cut = cut_profiles(store, host, &f.profiles);
        let still: f64 = polygons::intersection(&to_shapes(&cut), &region)
            .iter()
            .map(Shape::area)
            .sum();
        if existing.is_none() && still < 1e-4 {
            continue;
        }
        out.push(Obstruction {
            host: id,
            host_name: host.name.clone().unwrap_or_else(|| {
                // "the floor on Level 2"
                let (kind, r) = crate::floors::slab_recipe(host).expect("a floor");
                let level = find(store, &r.level)
                    .and_then(|l| store.get(l)?.name.clone())
                    .unwrap_or_default();
                format!("the {} on {level}", kind.noun())
            }),
            loops: region
                .into_iter()
                .map(|s| polygons::simplify(&s.outer))
                .collect(),
            area: on_host,
            clearance: if clearance.is_finite() {
                clearance
            } else {
                0.0
            },
            existing,
            uncovered: still,
        });
    }
    Ok(out)
}

/// Which headroom a stair command uses: another code's, a length, else the model's.
fn headroom_for(
    store: &Store,
    code: Option<&str>,
    headroom: Option<&Value>,
) -> Result<crate::codes::Headroom> {
    let settings = crate::codes::settings(store);
    if let Some(v) = headroom {
        let h = match v {
            Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
            Value::String(s) => {
                crate::units::parse(s, "length", &crate::units::settings_of(store))?
            }
            _ => f64::NAN,
        };
        if !(1.5..=4.0).contains(&h) {
            return Err(Error::new(
                "code.bad_headroom",
                "the headroom must be between 1.5 m and 4 m",
            ));
        }
        return Ok(crate::codes::Headroom {
            stair: h,
            spiral: h,
            clause: "a headroom given for this stair",
        });
    }
    let code = code
        .map(|c| {
            crate::codes::Code::parse(c)
                .ok_or_else(|| Error::new("code.unknown", format!("no code `{c}`")))
        })
        .transpose()?;
    Ok(settings.headroom(code))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeadroomParams {
    pub stair_id: ElementRef,
    /// Another code than the model's (`code.get` lists them).
    pub code: Option<String>,
    /// Or a headroom of its own (accepts units).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub headroom: Option<Value>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct HeadroomReport {
    /// The headroom checked (metres) and its clause.
    pub headroom: f64,
    pub clause: String,
    pub obstructions: Vec<Obstruction>,
}

/// Which floors and ceilings leave a stair short of headroom (`stair.headroom_check`).
pub(crate) fn check(store: &Store, p: HeadroomParams) -> Result<HeadroomReport> {
    let id = store
        .resolve(&p.stair_id)
        .ok_or_else(|| Error::new("element.not_found", "no such stair"))?;
    let h = headroom_for(store, p.code.as_deref(), p.headroom.as_ref())?;
    let spiral = store
        .get(id)
        .and_then(crate::stairs::stair_recipe)
        .is_some_and(|r| r.runs.iter().any(|r| r.spiral.is_some()));
    Ok(HeadroomReport {
        headroom: if spiral { h.spiral } else { h.stair },
        clause: h.clause.to_owned(),
        obstructions: obstructions(store, id, h.stair, h.spiral)?,
    })
}

/// Opens every floor and ceiling over a stair that leaves it short of headroom
/// (`stair.create_openings`); an opening made for the stair before takes the new loops.
pub(crate) fn create_for_stair(
    txn: &mut crate::txn::Transaction<'_>,
    p: HeadroomParams,
) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let report = check(&store, p.clone_params())?;
    let stair = store
        .resolve(&p.stair_id)
        .and_then(|i| store.get(i))
        .expect("checked")
        .guid()
        .to_string();
    let mut out = Vec::new();
    for o in report.obstructions {
        let host = store.get(o.host).expect("listed").guid().to_string();
        let recipe = OpeningRecipe {
            kind: OPENING_RECIPE.into(),
            host,
            loops: o.loops,
            stair: Some(stair.clone()),
            headroom: Some(report.headroom),
            clause: Some(report.clause.clone()),
        };
        match o.existing {
            Some(id) => {
                let value = serde_json::to_value(&recipe).expect("serialises");
                txn.modify(id, |e| {
                    e.params.insert(RECIPE_KEY.into(), value);
                })?;
                out.push(id);
            }
            None => out.push(make(txn, &recipe, "Stair Opening")?),
        }
    }
    Ok(out)
}

impl HeadroomParams {
    fn clone_params(&self) -> HeadroomParams {
        HeadroomParams {
            stair_id: self.stair_id.clone(),
            code: self.code.clone(),
            headroom: self.headroom.clone(),
        }
    }
}
