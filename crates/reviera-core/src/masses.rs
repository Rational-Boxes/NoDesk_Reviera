// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Masses, mass floors and model by face (spec Ch 3 §3.11, ARC-MASS-001 … 003, ARC-ROOF-003,
//! ARC-CW-007).
//!
//! - A **mass** is an `IfcBuildingElementProxy` (`ObjectType = "Mass"`) with `recipe.mass`: its
//!   forms (the in-place forms, ARC-INPL-001, plus lofts of stacked profiles) and the levels its
//!   mass floors are wanted on.
//! - **Mass floors** are worked out from the mass: at each ticked level that crosses it (not at
//!   its top), the section's area and perimeter, the volume up to the next such level (or the
//!   top), and the exterior surface of that band (with the roof faces for the topmost floor).
//!   They follow the mass at once.
//! - **Faces** are named from the forms (`form0.side2`, `form0.top`, `form1.side0_3` for a loft's
//!   band 0, side 3) and stay while the forms keep their points.
//! - **By face**: walls (along a face's bottom edge, its height), floors (on mass floors), roofs
//!   (a slab on the face, in-place) and curtain systems (a curtain grid on faces, as sloped
//!   glazing is) remember their mass and face, and follow it only with Update to Face.
//!
//! Not built yet: the conceptual design environment (X-Ray, gizmos, Add Edge/Profile,
//! Dissolve), voids in masses, mass families (Place Mass), Show Mass modes, joined masses,
//! curved and freeform faces by face, masses in IFC export (they are written as B-reps).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::bodies::Mesh;
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::in_place::{Form, FormArg, Shape};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::RECIPE_KEY;

pub const RECIPE: &str = "recipe.mass";
pub const SYSTEM_RECIPE: &str = "recipe.curtain_system";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MassRecipe {
    pub kind: String,
    pub forms: Vec<Form>,
    /// Levels (GlobalIds) mass floors are wanted on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub floors: Vec<String>,
    /// Every level, now and later.
    #[serde(default)]
    pub all_levels: bool,
}

pub fn recipe(e: &Element) -> Option<MassRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// A face an element was made from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FaceRef {
    /// The mass (GlobalId).
    pub mass: String,
    pub face: String,
}

/// A mass's solids.
pub fn regenerate(store: &Store, e: &Element) -> Result<Vec<Mesh>> {
    let r = recipe(e).ok_or_else(|| Error::new("mass.not", "not a mass"))?;
    r.forms
        .iter()
        .filter(|f| !f.void)
        .map(|f| crate::in_place::form_mesh(store, f))
        .collect()
}

pub fn all(store: &Store) -> Vec<(ElementId, Vec<Mesh>)> {
    store
        .iter()
        .filter(|e| recipe(e).is_some())
        .filter_map(|e| Some((e.id(), regenerate(store, e).ok()?)))
        .collect()
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn newell(r: &[[f64; 3]]) -> [f64; 3] {
    let mut n = [0.0; 3];
    for k in 0..r.len() {
        let (a, b) = (r[k], r[(k + 1) % r.len()]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    n
}

fn norm(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// The section of meshes at height `z`: closed loops, counter-clockwise round solid.
pub fn section(meshes: &[Mesh], z: f64) -> Vec<Vec<[f64; 2]>> {
    let mut segs: Vec<([f64; 2], [f64; 2])> = Vec::new();
    for (pts, idx) in meshes {
        for t in idx.chunks(3) {
            let v = [pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]];
            let n = cross(sub(v[1], v[0]), sub(v[2], v[0]));
            let mut hits = Vec::new();
            for k in 0..3 {
                let (a, b) = (v[k], v[(k + 1) % 3]);
                let (da, db) = (a[2] - z, b[2] - z);
                if (da > 0.0) != (db > 0.0) {
                    let s = da / (da - db);
                    hits.push([a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s]);
                }
            }
            if hits.len() == 2 {
                let (p, q) = (hits[0], hits[1]);
                let d = [q[0] - p[0], q[1] - p[1]];
                // The solid on the left: the outward normal to the right of the direction.
                if d[1] * n[0] - d[0] * n[1] >= 0.0 {
                    segs.push((p, q));
                } else {
                    segs.push((q, p));
                }
            }
        }
    }
    let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-7;
    let mut loops = Vec::new();
    while let Some((a, b)) = segs.pop() {
        let mut ring = vec![a, b];
        while let Some(k) = segs
            .iter()
            .position(|s| near(s.0, *ring.last().expect("points")))
        {
            let (_, q) = segs.remove(k);
            ring.push(q);
        }
        if near(ring[0], *ring.last().expect("points")) {
            ring.pop();
        }
        let ring = tidy(ring);
        if ring.len() >= 3 {
            loops.push(ring);
        }
    }
    loops
}

/// A loop without points closer than 0.1 mm to the last kept, nor points in line with their
/// neighbours (triangle diagonals crossing near a corner leave both).
fn tidy(ring: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    let mut out: Vec<[f64; 2]> = Vec::new();
    for p in ring {
        if out
            .last()
            .is_none_or(|q| (q[0] - p[0]).hypot(q[1] - p[1]) > 1e-4)
        {
            out.push(p);
        }
    }
    while out.len() > 3
        && (out[0][0] - out[out.len() - 1][0]).hypot(out[0][1] - out[out.len() - 1][1]) <= 1e-4
    {
        out.pop();
    }
    loop {
        let n = out.len();
        if n <= 3 {
            return out;
        }
        let k = (0..n).find(|&k| {
            let (a, b, c) = (out[(k + n - 1) % n], out[k], out[(k + 1) % n]);
            let cr = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            let l = (c[0] - a[0]).hypot(c[1] - a[1]).max(1e-12);
            cr.abs() / l < 1e-6
        });
        match k {
            Some(k) => {
                out.remove(k);
            }
            None => return out,
        }
    }
}

/// The section's area (holes, clockwise, take away).
fn area(loops: &[Vec<[f64; 2]>]) -> f64 {
    loops
        .iter()
        .map(|l| crate::polygons::area(l))
        .sum::<f64>()
        .max(0.0)
}

fn perimeter(loops: &[Vec<[f64; 2]>]) -> f64 {
    loops
        .iter()
        .map(|l| {
            (0..l.len())
                .map(|k| {
                    let (a, b) = (l[k], l[(k + 1) % l.len()]);
                    (b[0] - a[0]).hypot(b[1] - a[1])
                })
                .sum::<f64>()
        })
        .sum()
}

/// A polygon clipped to `z0 ≤ z ≤ z1`.
fn clip_slab(poly: &[[f64; 3]], z0: f64, z1: f64) -> Vec<[f64; 3]> {
    let clip = |poly: Vec<[f64; 3]>, keep: &dyn Fn(f64) -> f64| -> Vec<[f64; 3]> {
        let mut out = Vec::new();
        for k in 0..poly.len() {
            let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
            let (da, db) = (keep(a[2]), keep(b[2]));
            if da >= 0.0 {
                out.push(a);
            }
            if (da >= 0.0) != (db >= 0.0) {
                let s = da / (da - db);
                out.push([
                    a[0] + (b[0] - a[0]) * s,
                    a[1] + (b[1] - a[1]) * s,
                    a[2] + (b[2] - a[2]) * s,
                ]);
            }
        }
        out
    };
    let p = clip(poly.to_vec(), &|z| z - z0);
    clip(p, &|z| z1 - z)
}

/// A mass floor as it is now.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct MassFloor {
    /// The level (GlobalId) and its elevation.
    pub level: String,
    pub elevation: f64,
    pub area: f64,
    pub perimeter: f64,
    /// Up to the next mass floor or the top.
    pub volume: f64,
    pub exterior_area: f64,
    /// The section, counter-clockwise loops round solid.
    pub loops: Vec<Vec<[f64; 2]>>,
}

/// A mass's floors at its ticked levels that cross it (ARC-MASS-002).
pub fn floors(store: &Store, e: &Element) -> Vec<MassFloor> {
    let (Some(r), Ok(meshes)) = (recipe(e), regenerate(store, e)) else {
        return Vec::new();
    };
    let (zmin, zmax) = meshes
        .iter()
        .flat_map(|m| m.0.iter())
        .fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p[2]), a.1.max(p[2])));
    if zmin > zmax {
        return Vec::new();
    }
    let mut levels: Vec<(String, f64)> = crate::levels::list(store)
        .into_iter()
        .filter(|l| r.all_levels || r.floors.contains(&l.guid))
        .map(|l| (l.guid, l.elevation))
        .collect();
    levels.sort_by(|a, b| a.1.total_cmp(&b.1));
    // A floor where the mass is just above the level (not at its top).
    let eps = 1e-9;
    let crossing: Vec<(String, f64)> = levels
        .into_iter()
        .filter(|(_, z)| *z >= zmin - eps && *z < zmax - eps)
        .collect();
    let area_at = |z: f64| area(&section(&meshes, z));
    let mut out = Vec::new();
    for (k, (level, z)) in crossing.iter().enumerate() {
        let at = (*z).max(zmin + eps);
        let loops = section(&meshes, at);
        let a = area(&loops);
        if a < 1e-9 {
            continue;
        }
        let top = crossing.get(k + 1).map_or(zmax, |n| n.1);
        let topmost = k + 1 == crossing.len();
        // Simpson's rule on the section's area: exact for prisms, lofts and pyramids.
        let n = 8;
        let h = (top - z) / f64::from(n);
        let volume = (0..=n)
            .map(|i| {
                let zz = (z + h * f64::from(i)).clamp(zmin + eps, zmax - eps);
                let w = if i == 0 || i == n {
                    1.0
                } else if i % 2 == 1 {
                    4.0
                } else {
                    2.0
                };
                w * area_at(zz)
            })
            .sum::<f64>()
            * h
            / 3.0;
        let mut exterior = 0.0;
        for (pts, idx) in &meshes {
            for t in idx.chunks(3) {
                let tri = [pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]];
                let nn = newell(&tri);
                let nz = nn[2] / norm(nn).max(1e-12);
                if nz < -0.999 || (nz > 0.999 && !topmost) {
                    continue;
                }
                let (lo, hi) = if topmost { (*z, zmax + 1.0) } else { (*z, top) };
                let part = clip_slab(&tri, lo, hi);
                if part.len() >= 3 {
                    exterior += norm(newell(&part)) / 2.0;
                }
            }
        }
        out.push(MassFloor {
            level: level.clone(),
            elevation: *z,
            area: a,
            perimeter: perimeter(&loops),
            volume,
            exterior_area: exterior,
            loops,
        });
    }
    out
}

/// A named face of a mass: its polygon (outward, counter-clockwise seen from outside).
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct MassFace {
    pub face: String,
    pub points: Vec<[f64; 3]>,
    pub normal: [f64; 3],
}

/// A mass's planar faces, by name.
pub fn faces(store: &Store, e: &Element) -> Vec<MassFace> {
    let Some(r) = recipe(e) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (i, f) in r.forms.iter().enumerate() {
        if f.void {
            continue;
        }
        let Ok(plane) = crate::work_planes::resolve(store, &f.plane) else {
            continue;
        };
        let lift = |p: [f64; 2], w: f64| crate::in_place::lift(&plane, p[0], p[1], w);
        let ccw = |pts: &[[f64; 2]]| {
            let mut p = pts.to_vec();
            if crate::polygons::area(&p) < 0.0 {
                p.reverse();
            }
            p
        };
        let mut push = |name: String, pts: Vec<[f64; 3]>| {
            let n = newell(&pts);
            let l = norm(n).max(1e-12);
            out.push(MassFace {
                face: name,
                points: pts,
                normal: [n[0] / l, n[1] / l, n[2] / l],
            });
        };
        // Rings at offsets, bottom to top.
        let rings: Vec<(f64, Vec<[f64; 2]>)> = match &f.shape {
            Shape::Extrusion { start, end } => {
                let p = ccw(&f.profile);
                let (a, b) = (start.min(*end), start.max(*end));
                vec![(a, p.clone()), (b, p)]
            }
            Shape::Loft { profiles } => {
                let mut ps: Vec<(f64, Vec<[f64; 2]>)> = profiles
                    .iter()
                    .map(|p| (p.offset, ccw(&p.points)))
                    .collect();
                ps.sort_by(|a, b| a.0.total_cmp(&b.0));
                ps
            }
            _ => continue,
        };
        let n = rings[0].1.len();
        for (j, w) in rings.windows(2).enumerate() {
            for k in 0..n {
                let (a, b) = (w[0].1[k], w[0].1[(k + 1) % n]);
                let (c, d) = (w[1].1[(k + 1) % n], w[1].1[k]);
                let name = if rings.len() == 2 {
                    format!("form{i}.side{k}")
                } else {
                    format!("form{i}.side{j}_{k}")
                };
                push(
                    name,
                    vec![
                        lift(a, w[0].0),
                        lift(b, w[0].0),
                        lift(c, w[1].0),
                        lift(d, w[1].0),
                    ],
                );
            }
        }
        let (bz, bottom) = &rings[0];
        let (tz, top) = &rings[rings.len() - 1];
        push(
            format!("form{i}.top"),
            top.iter().map(|p| lift(*p, *tz)).collect(),
        );
        push(
            format!("form{i}.bottom"),
            bottom.iter().rev().map(|p| lift(*p, *bz)).collect(),
        );
    }
    out
}

fn mass(store: &Store, r: &ElementRef) -> Result<(ElementId, MassRecipe)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such mass"))?;
    let m = store
        .get(id)
        .and_then(recipe)
        .ok_or_else(|| Error::new("mass.not", "not a mass"))?;
    Ok((id, m))
}

fn face_of(store: &Store, f: &FaceRef) -> Result<MassFace> {
    let e = GlobalId::parse(&f.mass)
        .and_then(|g| store.find_by_guid(&g))
        .and_then(|id| store.get(id))
        .ok_or_else(|| Error::new("mass.gone", "the mass no longer exists"))?;
    faces(store, e)
        .into_iter()
        .find(|x| x.face == f.face)
        .ok_or_else(|| Error::new("mass.no_face", "Face no longer exists"))
}

fn write(txn: &mut Transaction<'_>, id: ElementId, r: &MassRecipe) -> Result<()> {
    let v = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub name: String,
    /// Forms as for `inplace.create`, plus `loft` (`profiles: [{offset, points}]`).
    pub forms: Vec<FormArg>,
}

/// `mass.create`: an in-place mass from its forms.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    if p.forms.is_empty() {
        return Err(Error::new("mass.no_forms", "a mass needs a form"));
    }
    let forms = p
        .forms
        .into_iter()
        .map(|a| crate::in_place::form(&store, a))
        .collect::<Result<Vec<_>>>()?;
    let r = MassRecipe {
        kind: RECIPE.into(),
        forms,
        floors: Vec::new(),
        all_levels: false,
    };
    txn.create(
        NewElement::new("IfcBuildingElementProxy")
            .name(p.name.trim())
            .param("@ObjectType", "Mass")
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddFormParams {
    pub mass_id: ElementRef,
    #[serde(flatten)]
    pub form: FormArg,
}

/// `mass.add_form`: its index.
pub(crate) fn add_form(txn: &mut Transaction<'_>, p: AddFormParams) -> Result<usize> {
    let store = txn.store().clone();
    let (id, mut r) = mass(&store, &p.mass_id)?;
    r.forms.push(crate::in_place::form(&store, p.form)?);
    write(txn, id, &r)?;
    Ok(r.forms.len() - 1)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetFormParams {
    pub mass_id: ElementRef,
    pub index: usize,
    /// A form's new start and end offsets (extrusions).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub start: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub end: Option<f64>,
}

/// `mass.set_form`: stretches an extrusion form.
pub(crate) fn set_form(txn: &mut Transaction<'_>, p: SetFormParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = mass(&store, &p.mass_id)?;
    let f = r
        .forms
        .get_mut(p.index)
        .ok_or_else(|| Error::new("mass.no_form", format!("there is no form {}", p.index)))?;
    match &mut f.shape {
        Shape::Extrusion { start, end } => {
            if let Some(v) = p.start {
                *start = v;
            }
            if let Some(v) = p.end {
                *end = v;
            }
        }
        _ => {
            return Err(Error::new(
                "mass.form_kind",
                "only extrusions are stretched so far",
            ));
        }
    }
    write(txn, id, &r)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetFloorsParams {
    pub mass_id: ElementRef,
    /// The levels; omitted with `all`, every level.
    #[serde(default)]
    pub level_ids: Vec<ElementRef>,
    #[serde(default)]
    pub all: bool,
}

/// `mass.set_floors`.
pub(crate) fn set_floors(txn: &mut Transaction<'_>, p: SetFloorsParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = mass(&store, &p.mass_id)?;
    r.all_levels = p.all;
    r.floors = p
        .level_ids
        .iter()
        .map(|l| {
            store
                .resolve(l)
                .and_then(|id| store.get(id))
                .filter(|e| e.class == crate::levels::CLASS)
                .map(|e| e.guid().to_string())
                .ok_or_else(|| Error::new("element.not_found", "no such level"))
        })
        .collect::<Result<_>>()?;
    write(txn, id, &r)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MassParams {
    pub mass_id: ElementRef,
}

/// `mass.floors`.
pub(crate) fn floors_of(store: &Store, p: MassParams) -> Result<Vec<MassFloor>> {
    let (id, _) = mass(store, &p.mass_id)?;
    Ok(floors(store, store.get(id).expect("resolved")))
}

/// `mass.faces`.
pub(crate) fn faces_of(store: &Store, p: MassParams) -> Result<Vec<MassFace>> {
    let (id, _) = mass(store, &p.mass_id)?;
    Ok(faces(store, store.get(id).expect("resolved")))
}

/// `mass.select_dependents`: the elements made by face from the mass.
pub(crate) fn dependents(store: &Store, p: MassParams) -> Result<Vec<ElementId>> {
    let (id, _) = mass(store, &p.mass_id)?;
    let guid = store.get(id).expect("resolved").guid().to_string();
    Ok(store
        .iter()
        .filter(|e| by_face(e).is_some_and(|f| f.mass == guid))
        .map(Element::id)
        .collect())
}

/// The face an element was made from, if any.
pub fn by_face(e: &Element) -> Option<FaceRef> {
    if let Some(r) = crate::walls::wall_recipe(e) {
        return r.by_face;
    }
    if let Some(r) = crate::in_place::recipe(e) {
        return r.by_face;
    }
    if let Some(r) = system_recipe(e) {
        return r.faces.into_iter().next();
    }
    None
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ByFaceParams {
    pub mass_id: ElementRef,
    pub faces: Vec<String>,
    /// Walls: the type, the level they stand on (default the highest at or below the face's
    /// bottom) and their location line on the face (default the exterior finish face).
    pub type_id: Option<ElementRef>,
    pub level_id: Option<ElementRef>,
    pub location_line: Option<crate::walls::LocationLine>,
    /// Roofs: the slab's thickness (default 300 mm).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub thickness: Option<f64>,
}

/// The wall line, base and height for a face; `None` for a face that is not upright.
fn wall_on(face: &MassFace) -> Option<([f64; 2], [f64; 2], f64, f64)> {
    if face.normal[2].abs() > 0.05 {
        return None;
    }
    let zmin = face.points.iter().map(|p| p[2]).fold(f64::MAX, f64::min);
    let zmax = face.points.iter().map(|p| p[2]).fold(f64::MIN, f64::max);
    // Its bottom edge, run so that the face's outside is on the wall's left (its exterior).
    let bottom: Vec<[f64; 2]> = face
        .points
        .iter()
        .filter(|p| (p[2] - zmin).abs() < 1e-6)
        .map(|p| [p[0], p[1]])
        .collect();
    let (&a, &b) = (bottom.first()?, bottom.last()?);
    let d = [b[0] - a[0], b[1] - a[1]];
    let left = [-d[1], d[0]];
    let (s, e) = if left[0] * face.normal[0] + left[1] * face.normal[1] >= 0.0 {
        (a, b)
    } else {
        (b, a)
    };
    Some((s, e, zmin, zmax - zmin))
}

/// `wall.create_by_face`.
pub(crate) fn walls_by_face(txn: &mut Transaction<'_>, p: ByFaceParams) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let (id, _) = mass(&store, &p.mass_id)?;
    let mass_guid = store.get(id).expect("resolved").guid().to_string();
    let mut out = Vec::new();
    for name in &p.faces {
        let fr = FaceRef {
            mass: mass_guid.clone(),
            face: name.clone(),
        };
        let face = face_of(&store, &fr)?;
        let (s, e, z0, h) = wall_on(&face).ok_or_else(|| {
            Error::new(
                "mass.face_not_upright",
                format!("{name} is not an upright face"),
            )
        })?;
        let level = match &p.level_id {
            Some(l) => store
                .resolve(l)
                .ok_or_else(|| Error::new("element.not_found", "no such level"))?,
            None => crate::levels::list(&store)
                .into_iter()
                .filter(|l| l.elevation <= z0 + 1e-6)
                .max_by(|a, b| a.elevation.total_cmp(&b.elevation))
                .or_else(|| crate::levels::list(&store).into_iter().next())
                .map(|l| l.id)
                .ok_or_else(|| Error::new("level.none", "the model has no level"))?,
        };
        let level_z = crate::levels::elevation(store.get(level).expect("resolved"));
        let line = p
            .location_line
            .unwrap_or(crate::walls::LocationLine::FinishFaceExterior);
        let args: crate::walls::CreateParams = serde_json::from_value(json!({
            "type_id": p.type_id,
            "curve": { "line": { "start": s, "end": e } },
            "base_level": level.0,
            "base_offset": z0 - level_z,
            "top_constraint": { "unconnected": h },
            "location_line": line,
        }))
        .map_err(|e| Error::new("args.invalid", e.to_string()))?;
        let w = crate::walls::create(txn, args)?;
        let mut r =
            crate::walls::wall_recipe(txn.store().get(w).expect("created")).expect("a wall");
        r.by_face = Some(fr);
        let v = serde_json::to_value(&r).expect("serialises");
        txn.modify(w, |el| {
            el.params.insert(RECIPE_KEY.into(), v);
        })?;
        out.push(w);
    }
    Ok(out)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FloorsByMassParams {
    pub mass_id: ElementRef,
    /// The levels of the mass floors to make floors on; omitted, all of them.
    #[serde(default)]
    pub level_ids: Vec<ElementRef>,
    pub type_id: Option<ElementRef>,
}

/// `floor.create_by_mass_floor`.
pub(crate) fn floors_by_mass(
    txn: &mut Transaction<'_>,
    p: FloorsByMassParams,
) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let (id, _) = mass(&store, &p.mass_id)?;
    let wanted: Vec<String> = p
        .level_ids
        .iter()
        .filter_map(|l| {
            store
                .resolve(l)
                .and_then(|i| store.get(i))
                .map(|e| e.guid().to_string())
        })
        .collect();
    let mut out = Vec::new();
    for mf in floors(&store, store.get(id).expect("resolved")) {
        if !wanted.is_empty() && !wanted.contains(&mf.level) {
            continue;
        }
        let level = GlobalId::parse(&mf.level)
            .and_then(|g| store.find_by_guid(&g))
            .ok_or_else(|| Error::new("level.missing", "the level is gone"))?;
        let loops: Vec<Value> = mf
            .loops
            .iter()
            .map(|l| {
                Value::Array(
                    (0..l.len())
                        .map(|k| json!({ "line": { "start": l[k], "end": l[(k + 1) % l.len()] } }))
                        .collect(),
                )
            })
            .collect();
        let args: crate::floors::CreateParams = serde_json::from_value(json!({
            "type_id": p.type_id, "loops": loops, "level_id": level.0,
        }))
        .map_err(|e| Error::new("args.invalid", e.to_string()))?;
        out.push(crate::floors::create(txn, args)?);
    }
    if out.is_empty() {
        return Err(Error::new(
            "mass.no_floors",
            "the mass has no mass floors there (mass.set_floors)",
        ));
    }
    Ok(out)
}

/// The in-place roof form on a face: the face pushed out along its normal.
fn roof_form(face: &MassFace, thickness: f64) -> Form {
    let o = face.points[0];
    let x = {
        let d = sub(face.points[1], o);
        let l = norm(d).max(1e-12);
        [d[0] / l, d[1] / l, d[2] / l]
    };
    let n = face.normal;
    let y = cross(n, x);
    let local: Vec<[f64; 2]> = face
        .points
        .iter()
        .map(|p| {
            let d = sub(*p, o);
            [
                d[0] * x[0] + d[1] * x[1] + d[2] * x[2],
                d[0] * y[0] + d[1] * y[1] + d[2] * y[2],
            ]
        })
        .collect();
    Form {
        shape: Shape::Extrusion {
            start: 0.0,
            end: thickness,
        },
        plane: crate::work_planes::WorkPlaneRef::Explicit {
            origin: o,
            x_dir: x,
            normal: n,
        },
        profile: local,
        void: false,
    }
}

/// `roof.create_by_face` (ARC-ROOF-003): a roof slab on each face (an in-place roof).
pub(crate) fn roofs_by_face(txn: &mut Transaction<'_>, p: ByFaceParams) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let (id, _) = mass(&store, &p.mass_id)?;
    let mass_guid = store.get(id).expect("resolved").guid().to_string();
    let t = p.thickness.unwrap_or(0.3);
    let mut out = Vec::new();
    for name in &p.faces {
        let fr = FaceRef {
            mass: mass_guid.clone(),
            face: name.clone(),
        };
        let face = face_of(&store, &fr)?;
        if face.normal[2] < 0.05 {
            return Err(Error::new(
                "mass.face_not_roof",
                format!("{name} does not face up"),
            ));
        }
        let r = crate::in_place::InPlaceRecipe {
            kind: crate::in_place::RECIPE.into(),
            category: "Roofs".into(),
            forms: vec![roof_form(&face, t)],
            by_face: Some(fr),
        };
        out.push(
            txn.create(
                NewElement::new("IfcRoof")
                    .name(format!("Roof by Face {name}"))
                    .param("@ObjectType", "InPlace")
                    .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
            )?,
        );
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SystemRecipe {
    pub kind: String,
    pub faces: Vec<FaceRef>,
    /// The faces' polygons as they were when made or last updated.
    pub polygons: Vec<Vec<[f64; 3]>>,
    pub layout: crate::curtain::CurtainType,
}

pub fn system_recipe(e: &Element) -> Option<SystemRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == SYSTEM_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// A curtain system's panels and mullions (a grid on each face, as sloped glazing).
pub fn system_parts(r: &SystemRecipe) -> crate::roofs::glazing::Glazing {
    let faces = r
        .polygons
        .iter()
        .map(|p| {
            let n = newell(p);
            let l = norm(n).max(1e-12);
            crate::roofs::RoofFace {
                key: None,
                bottom: p.clone(),
                normal: [n[0] / l, n[1] / l, n[2] / l],
                slab: (Vec::new(), Vec::new()),
                holes: Vec::new(),
                faceted: false,
            }
        })
        .collect();
    let g = crate::roofs::RoofGeometry {
        faces,
        plate: 0.0,
        thickness: 0.0,
        footprint: Vec::new(),
        keys: Vec::new(),
        predefined: "NOTDEFINED",
        trims: Vec::new(),
        uncut: 0,
    };
    crate::roofs::glazing::regenerate(&g, &r.layout)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SystemParams {
    pub mass_id: ElementRef,
    pub faces: Vec<String>,
    /// Grid 1 (horizontal lines on upright faces) and Grid 2 rules; default 1.5 m each way.
    pub grid1: Option<crate::curtain::Rule>,
    pub grid2: Option<crate::curtain::Rule>,
}

/// `curtainsystem.create_by_face` (ARC-CW-007).
pub(crate) fn system_by_face(txn: &mut Transaction<'_>, p: SystemParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let (id, _) = mass(&store, &p.mass_id)?;
    let mass_guid = store.get(id).expect("resolved").guid().to_string();
    let faces: Vec<FaceRef> = p
        .faces
        .iter()
        .map(|f| FaceRef {
            mass: mass_guid.clone(),
            face: f.clone(),
        })
        .collect();
    let polygons = faces
        .iter()
        .map(|f| face_of(&store, f).map(|x| x.points))
        .collect::<Result<Vec<_>>>()?;
    let fixed = crate::curtain::Rule {
        layout: crate::curtain::Layout::FixedDistance,
        spacing: 1.5,
        number: 0,
    };
    let layout = crate::curtain::CurtainType {
        u: p.grid2.unwrap_or(fixed),
        v: p.grid1.unwrap_or(fixed),
        ..crate::curtain::CurtainType::default()
    };
    let r = SystemRecipe {
        kind: SYSTEM_RECIPE.into(),
        faces,
        polygons,
        layout,
    };
    txn.create(
        NewElement::new("IfcCurtainWall")
            .name("Curtain System")
            .param("@ObjectType", "CurtainSystem")
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateParams {
    pub element_ids: Vec<ElementRef>,
}

/// `element.update_to_face`: walls, roofs and curtain systems made by face follow their faces.
pub(crate) fn update_to_face(txn: &mut Transaction<'_>, p: UpdateParams) -> Result<()> {
    for r in &p.element_ids {
        let store = txn.store().clone();
        let id = store
            .resolve(r)
            .ok_or_else(|| Error::new("element.not_found", "no such element"))?;
        let e = store.get(id).expect("resolved");
        if let Some(mut w) = crate::walls::wall_recipe(e) {
            let Some(fr) = w.by_face.clone() else {
                continue;
            };
            // Walls with a level for a top keep it.
            if w.top.get("level").is_some() {
                continue;
            }
            let face = face_of(&store, &fr)?;
            let (s, en, z0, h) = wall_on(&face).ok_or_else(|| {
                Error::new("mass.face_not_upright", "the face is no longer upright")
            })?;
            let level_z = crate::levels::elevation_of(&store, &w.base_level)?;
            w.curve = crate::walls::Line { start: s, end: en };
            w.base_offset = z0 - level_z;
            w.top = json!({ "unconnected": h });
            let v = serde_json::to_value(&w).expect("serialises");
            txn.modify(id, |el| {
                el.params.insert(RECIPE_KEY.into(), v);
            })?;
        } else if let Some(mut ip) = crate::in_place::recipe(e) {
            let Some(fr) = ip.by_face.clone() else {
                continue;
            };
            let face = face_of(&store, &fr)?;
            let t = match ip.forms.first().map(|f| &f.shape) {
                Some(Shape::Extrusion { start, end }) => (end - start).abs(),
                _ => 0.3,
            };
            ip.forms = vec![roof_form(&face, t)];
            let v = serde_json::to_value(&ip).expect("serialises");
            txn.modify(id, |el| {
                el.params.insert(RECIPE_KEY.into(), v);
            })?;
        } else if let Some(mut s) = system_recipe(e) {
            s.polygons = s
                .faces
                .iter()
                .map(|f| face_of(&store, f).map(|x| x.points))
                .collect::<Result<Vec<_>>>()?;
            let v = serde_json::to_value(&s).expect("serialises");
            txn.modify(id, |el| {
                el.params.insert(RECIPE_KEY.into(), v);
            })?;
        }
    }
    Ok(())
}

/// Moves, turns or mirrors a mass.
pub(crate) fn transform(
    txn: &mut Transaction<'_>,
    id: ElementId,
    point: &dyn Fn([f64; 2]) -> [f64; 2],
    dz: f64,
) -> Result<()> {
    let store = txn.store().clone();
    let mut r = recipe(store.get(id).expect("exists")).expect("a mass");
    crate::in_place::transform_forms(&store, &mut r.forms, point, dz)?;
    write(txn, id, &r)
}
