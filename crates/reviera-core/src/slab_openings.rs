// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Vertical openings (ARC-OPEN-002) and stair openings (ARC-STAIR-007).
//!
//! A vertical opening is an `IfcOpeningElement` with a `recipe.vertical_opening`: its host floor
//! or ceiling (GlobalId) and its loops in plan. It cuts that host only, plumb through its whole
//! thickness; a loop partly outside the host notches its edge. Reviera's own display cuts the
//! host's body ([`cut_profiles`]); the IFC keeps the host's body whole and voids it with the
//! opening, as for walls. It moves and is mirrored with its host and goes when the host goes.
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

/// A floor's or ceiling's profiles less its vertical openings (for display and measuring).
pub fn cut_profiles(store: &Store, host: &Element, profiles: &[Profile]) -> Vec<Profile> {
    let loops: Vec<Ring> = of_host(store, host.guid().as_str())
        .into_iter()
        .flat_map(|(_, r)| r.loops)
        .collect();
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
        .ok_or_else(|| Error::new("element.not_found", "no such floor or ceiling"))?;
    if crate::floors::slab_recipe(store.get(id).expect("resolved")).is_none() {
        return Err(Error::new(
            "opening.bad_host",
            format!("element {id} is not a Reviera floor or ceiling"),
        ));
    }
    Ok(id)
}

fn intersects_host(store: &Store, host: ElementId, loops: &[Ring]) -> Result<()> {
    let g = crate::floors::regenerate(store, store.get(host).expect("exists"))?;
    if overlap_area(&g.profiles, loops) < 1e-6 {
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
    /// A Reviera floor or ceiling.
    pub host_id: ElementRef,
    /// Closed loops in plan (metres), each a list of corners.
    pub loops: Vec<Ring>,
}

/// Cuts a floor or ceiling plumb by loops (`opening.create_vertical`).
pub(crate) fn create(txn: &mut crate::txn::Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let host = host_of(&store, &p.host_id)?;
    let loops = check_loops(&p.loops)?;
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
