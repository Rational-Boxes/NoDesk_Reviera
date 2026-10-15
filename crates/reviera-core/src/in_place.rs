// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Model In-Place (spec Ch 3 §3.10.1, ARC-INPL-001): unique, free-form elements modelled in the
//! project.
//!
//! An in-place element has a category (its IFC class: Walls → `IfcWall`, Casework →
//! `IfcFurniture`, Generic Models → `IfcBuildingElementProxy`, …), a name, and its forms
//! (`recipe.in_place`): each an **extrusion** (a profile on a work plane between two offsets), a
//! **blend** (a base and a top profile with as many points, at two offsets), a **revolve** (a
//! profile turned about an axis in its plane between two angles) or a **sweep** (a profile
//! carried along a level path, mitred at its corners), solid or **void**. The solids are its
//! body; the voids are a companion element (an opening, `recipe.in_place_void`) subtracted from
//! it through the model's boolean relations (ARC-MOD-028), so the 3D view, plans and IFC show the
//! cut. Copying an in-place element makes another, independent one.
//!
//! Not built yet: the in-place editor session with half-tone, swept blends, sweeps on sloping or
//! picked paths, Cut and Join Geometry with project elements, room bounding and hosting by
//! in-place walls, attaching walls to in-place roofs, CSG bodies in IFC (they are B-reps).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bodies::Mesh;
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::RECIPE_KEY;
use crate::work_planes::{Plane, PlaneArgs, WorkPlaneRef};

pub const RECIPE: &str = "recipe.in_place";
pub const VOID_RECIPE: &str = "recipe.in_place_void";

/// Categories an in-place element can be, and their IFC classes.
pub const CATEGORIES: &[(&str, &str)] = &[
    ("Generic Models", "IfcBuildingElementProxy"),
    ("Walls", "IfcWall"),
    ("Floors", "IfcSlab"),
    ("Roofs", "IfcRoof"),
    ("Ceilings", "IfcCovering"),
    ("Columns", "IfcColumn"),
    ("Casework", "IfcFurniture"),
    ("Furniture", "IfcFurniture"),
    ("Stairs", "IfcStair"),
    ("Railings", "IfcRailing"),
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Shape {
    /// The profile pushed out of its plane from `start` to `end`.
    Extrusion { start: f64, end: f64 },
    /// The profile at `start`, `top` (as many points) at `end`.
    Blend {
        start: f64,
        end: f64,
        top: Vec<[f64; 2]>,
    },
    /// The profile turned about the axis (two points in its plane) from `start` to `end` (radians).
    Revolve {
        axis: [[f64; 2]; 2],
        start: f64,
        end: f64,
    },
    /// The profile (x across, y up) carried along a path in plan at a height.
    Sweep {
        path: Vec<[f64; 2]>,
        z: f64,
        closed: bool,
    },
    /// Stacked closed profiles (as many points each), each at its offset out of the plane; the
    /// form's own profile is the first (masses, ARC-MASS-001).
    Loft { profiles: Vec<LoftProfile> },
}

/// One profile of a loft.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LoftProfile {
    pub offset: f64,
    pub points: Vec<[f64; 2]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Form {
    #[serde(flatten)]
    pub shape: Shape,
    pub plane: WorkPlaneRef,
    /// The profile, in the plane's coordinates (a sweep's: x across the path, y up).
    pub profile: Vec<[f64; 2]>,
    #[serde(default)]
    pub void: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InPlaceRecipe {
    pub kind: String,
    pub category: String,
    pub forms: Vec<Form>,
    /// The mass face a roof by face was made from (ARC-ROOF-003).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by_face: Option<crate::masses::FaceRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VoidRecipe {
    pub kind: String,
    /// The in-place element it cuts (GlobalId).
    pub of: String,
}

pub fn recipe(e: &Element) -> Option<InPlaceRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

pub fn void_recipe(e: &Element) -> Option<VoidRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == VOID_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

pub(crate) fn lift(p: &Plane, u: f64, v: f64, w: f64) -> [f64; 3] {
    std::array::from_fn(|k| p.origin[k] + p.x[k] * u + p.y[k] * v + p.normal[k] * w)
}

/// The Newell normal of a ring (not unit).
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

fn centroid(r: &[[f64; 3]]) -> [f64; 3] {
    let n = r.len().max(1) as f64;
    let s = r
        .iter()
        .fold([0.0; 3], |s, p| [s[0] + p[0], s[1] + p[1], s[2] + p[2]]);
    [s[0] / n, s[1] / n, s[2] / n]
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Rings of corresponding points into a closed band, capped at both ends when asked (the caps
/// triangulated like `outline`), facing out.
fn loft(rings: &[Vec<[f64; 3]>], cap_ends: bool, outline: &[[f64; 2]]) -> Mesh {
    let n = rings.first().map_or(0, Vec::len);
    let pts: Vec<[f64; 3]> = rings.iter().flatten().copied().collect();
    let mut idx: Vec<u32> = Vec::new();
    let at = |r: usize, k: usize| (r * n + k % n) as u32;
    if rings.len() < 2 || n < 3 {
        return (pts, idx);
    }
    // The band faces out when the rings run the way the first ring's normal points.
    let ahead = |r: usize| {
        let d = centroid(&rings[r + 1]);
        let c = centroid(&rings[r]);
        dot3(newell(&rings[r]), [d[0] - c[0], d[1] - c[1], d[2] - c[2]]) > 0.0
    };
    for r in 0..rings.len() - 1 {
        let out = ahead(r);
        for k in 0..n {
            let (a, b, c, d) = (at(r, k), at(r, k + 1), at(r + 1, k + 1), at(r + 1, k));
            if out {
                idx.extend([a, b, c, a, c, d]);
            } else {
                idx.extend([a, c, b, a, d, c]);
            }
        }
    }
    if cap_ends {
        let mut tris: Vec<u32> = Vec::new();
        earcut::Earcut::new().earcut(outline.iter().copied(), &[], &mut tris);
        let last = rings.len() - 1;
        for (ring, other) in [(0, 1), (last, last - 1)] {
            let away = {
                let (c, o) = (centroid(&rings[ring]), centroid(&rings[other]));
                [c[0] - o[0], c[1] - o[1], c[2] - o[2]]
            };
            for t in tris.chunks(3) {
                let (i, j, k) = (t[0] as usize, t[1] as usize, t[2] as usize);
                let tri = [rings[ring][i], rings[ring][j], rings[ring][k]];
                if dot3(newell(&tri), away) >= 0.0 {
                    idx.extend([at(ring, i), at(ring, j), at(ring, k)]);
                } else {
                    idx.extend([at(ring, i), at(ring, k), at(ring, j)]);
                }
            }
        }
    }
    (pts, idx)
}

/// A form's mesh.
pub fn form_mesh(store: &Store, f: &Form) -> Result<Mesh> {
    if f.profile.len() < 3 {
        return Err(Error::new(
            "inplace.profile",
            "a profile needs at least three points",
        ));
    }
    let mut profile = f.profile.clone();
    if crate::polygons::area(&profile) < 0.0 {
        profile.reverse();
    }
    let plane = crate::work_planes::resolve(store, &f.plane)?;
    Ok(match &f.shape {
        Shape::Extrusion { start, end } => {
            let (pts, idx) = crate::bodies::extrude(&profile, &[], *start, *end);
            (
                pts.iter().map(|p| lift(&plane, p[0], p[1], p[2])).collect(),
                idx,
            )
        }
        Shape::Blend { start, end, top } => {
            if top.len() != f.profile.len() {
                return Err(Error::new(
                    "inplace.blend",
                    "a blend's base and top need as many points",
                ));
            }
            let mut top = top.clone();
            if crate::polygons::area(&top) < 0.0 {
                top.reverse();
            }
            let a: Vec<[f64; 3]> = profile
                .iter()
                .map(|p| lift(&plane, p[0], p[1], *start))
                .collect();
            let b: Vec<[f64; 3]> = top.iter().map(|p| lift(&plane, p[0], p[1], *end)).collect();
            // The top cap is the top profile; earcut on the base outline gives the same
            // triangles for both when they correspond point by point.
            loft(&[a, b], true, &profile)
        }
        Shape::Revolve { axis, start, end } => {
            let (o, d) = (axis[0], axis[1]);
            let l = ((d[0] - o[0]).powi(2) + (d[1] - o[1]).powi(2)).sqrt();
            if l < 1e-9 {
                return Err(Error::new("inplace.axis", "the axis needs two points"));
            }
            let u = [(d[0] - o[0]) / l, (d[1] - o[1]) / l];
            let sweep = end - start;
            let full = (sweep.abs() - std::f64::consts::TAU).abs() < 1e-9;
            let steps = ((sweep.abs() / std::f64::consts::TAU * 48.0).ceil() as usize).max(2);
            let rings: Vec<Vec<[f64; 3]>> = (0..=steps)
                .filter(|s| !(full && *s == steps))
                .map(|s| {
                    let a = start + sweep * s as f64 / steps as f64;
                    profile
                        .iter()
                        .map(|p| {
                            // Along the axis and out from it, in the plane.
                            let q = [p[0] - o[0], p[1] - o[1]];
                            let along = q[0] * u[0] + q[1] * u[1];
                            let out = -q[0] * u[1] + q[1] * u[0];
                            let (c, sn) = (a.cos(), a.sin());
                            // Out from the axis turns between the plane's normal-to-axis and its
                            // normal.
                            let inplane = out * c;
                            let normal = out * sn;
                            lift(
                                &plane,
                                o[0] + u[0] * along - u[1] * inplane,
                                o[1] + u[1] * along + u[0] * inplane,
                                normal,
                            )
                        })
                        .collect()
                })
                .collect();
            if full {
                let mut r = rings.clone();
                r.push(rings[0].clone());
                loft(&r, false, &profile)
            } else {
                loft(&rings, true, &profile)
            }
        }
        Shape::Sweep { path, z, closed } => {
            if path.len() < 2 {
                return Err(Error::new(
                    "inplace.path",
                    "a sweep's path needs two points",
                ));
            }
            crate::wall_sweeps::sweep_mitred(path, *closed, *z, &profile)
        }
        Shape::Loft { profiles } => {
            if profiles.len() < 2 || profiles.iter().any(|p| p.points.len() != profile.len()) {
                return Err(Error::new(
                    "inplace.loft",
                    "a loft needs two or more profiles with as many points as its first",
                ));
            }
            let mut sorted = profiles.clone();
            sorted.sort_by(|a, b| a.offset.total_cmp(&b.offset));
            let rings: Vec<Vec<[f64; 3]>> = sorted
                .iter()
                .map(|lp| {
                    let mut pts = lp.points.clone();
                    if crate::polygons::area(&pts) < 0.0 {
                        pts.reverse();
                    }
                    pts.iter()
                        .map(|p| lift(&plane, p[0], p[1], lp.offset))
                        .collect()
                })
                .collect();
            loft(&rings, true, &profile)
        }
    })
}

/// An in-place element's solid forms.
pub fn regenerate(store: &Store, e: &Element) -> Result<Vec<Mesh>> {
    let r = recipe(e).ok_or_else(|| Error::new("inplace.not", "not an in-place element"))?;
    r.forms
        .iter()
        .filter(|f| !f.void)
        .map(|f| form_mesh(store, f))
        .collect()
}

/// An in-place void element's forms (its in-place element's voids).
pub fn void_meshes(store: &Store, e: &Element) -> Result<Vec<Mesh>> {
    let v = void_recipe(e).ok_or_else(|| Error::new("inplace.not", "not an in-place void"))?;
    let owner = GlobalId::parse(&v.of)
        .and_then(|g| store.find_by_guid(&g))
        .and_then(|id| store.get(id))
        .and_then(recipe)
        .ok_or_else(|| Error::new("inplace.not", "its in-place element is gone"))?;
    owner
        .forms
        .iter()
        .filter(|f| f.void)
        .map(|f| form_mesh(store, f))
        .collect()
}

/// Every in-place element and void that regenerates, with whether it is a void.
pub fn all(store: &Store) -> Vec<(ElementId, bool, Vec<Mesh>)> {
    store
        .iter()
        .filter_map(|e| {
            if recipe(e).is_some() {
                return Some((e.id(), false, regenerate(store, e).ok()?));
            }
            if void_recipe(e).is_some() {
                return Some((e.id(), true, void_meshes(store, e).ok()?));
            }
            None
        })
        .collect()
}

/// Keeps the companion void element and its boolean relation in step with the voids.
fn sync_voids(txn: &mut Transaction<'_>, id: ElementId) -> Result<()> {
    let store = txn.store().clone();
    let e = store.get(id).expect("exists");
    let r = recipe(e).expect("an in-place element");
    let guid = e.guid().to_string();
    let existing = store
        .iter()
        .find(|v| void_recipe(v).is_some_and(|x| x.of == guid))
        .map(Element::id);
    let wants = r.forms.iter().any(|f| f.void);
    match (existing, wants) {
        (None, true) => {
            let v = VoidRecipe {
                kind: VOID_RECIPE.into(),
                of: guid.clone(),
            };
            let vid = txn.create(
                NewElement::new("IfcOpeningElement")
                    .name("In-Place Void")
                    .param(RECIPE_KEY, serde_json::to_value(&v).expect("serialises")),
            )?;
            let vg = txn.store().get(vid).expect("created").guid().to_string();
            crate::csg::set_tool_targets(txn, &vg, &[guid], crate::csg::CsgOp::Subtract, true)?;
        }
        (Some(v), false) => {
            let vg = store.get(v).expect("exists").guid().to_string();
            crate::csg::set_tool_targets(txn, &vg, &[], crate::csg::CsgOp::Subtract, true)?;
            txn.delete(v)?;
        }
        _ => {}
    }
    Ok(())
}

fn class_of(category: &str) -> Result<&'static str> {
    CATEGORIES
        .iter()
        .find(|c| c.0.eq_ignore_ascii_case(category))
        .map(|c| c.1)
        .ok_or_else(|| {
            Error::new(
                "inplace.category",
                format!(
                    "`{category}` is not a category for in-place elements ({})",
                    CATEGORIES
                        .iter()
                        .map(|c| c.0)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )
        })
}

// ---- Commands ----

// Not `deny_unknown_fields`: serde does not combine it with `flatten`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct FormArg {
    /// `extrusion`, `blend`, `revolve` or `sweep`, with its values.
    #[serde(flatten)]
    pub shape: Shape,
    /// The profile in the work plane's coordinates.
    pub profile: Vec<[f64; 2]>,
    /// The work plane (`level` with `offset`, `grid`, `ref_plane` or `plane`); omitted, the lowest
    /// level.
    #[serde(flatten)]
    pub plane: PlaneArgs,
    #[serde(default)]
    pub void: bool,
}

pub(crate) fn form(store: &Store, a: FormArg) -> Result<Form> {
    let plane = if a.plane.given() {
        a.plane.to_ref(store)?
    } else {
        let lowest = crate::levels::list(store)
            .into_iter()
            .next()
            .ok_or_else(|| Error::new("level.none", "the model has no level"))?;
        WorkPlaneRef::Level {
            level: lowest.guid,
            offset: a.plane.offset,
        }
    };
    let f = Form {
        shape: a.shape,
        plane,
        profile: a.profile,
        void: a.void,
    };
    form_mesh(store, &f)?;
    Ok(f)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub name: String,
    /// Generic Models (default), Walls, Floors, Roofs, Ceilings, Columns, Casework, Furniture,
    /// Stairs or Railings.
    pub category: Option<String>,
    /// Its first forms.
    #[serde(default)]
    pub forms: Vec<FormArg>,
}

/// `inplace.create`.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let category = p.category.unwrap_or_else(|| "Generic Models".into());
    let class = class_of(&category)?;
    let forms = p
        .forms
        .into_iter()
        .map(|a| form(&store, a))
        .collect::<Result<Vec<_>>>()?;
    let r = InPlaceRecipe {
        kind: RECIPE.into(),
        category,
        forms,
        by_face: None,
    };
    let id = txn.create(
        NewElement::new(class)
            .name(p.name.trim())
            .param("@ObjectType", "InPlace")
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )?;
    sync_voids(txn, id)?;
    Ok(id)
}

fn write(txn: &mut Transaction<'_>, id: ElementId, r: &InPlaceRecipe) -> Result<()> {
    let v = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })?;
    sync_voids(txn, id)
}

fn element(store: &Store, r: &ElementRef) -> Result<(ElementId, InPlaceRecipe)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such in-place element"))?;
    let rec = store
        .get(id)
        .and_then(recipe)
        .ok_or_else(|| Error::new("inplace.not", "not an in-place element"))?;
    Ok((id, rec))
}

// Not `deny_unknown_fields`: serde does not combine it with `flatten`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddFormParams {
    pub element_id: ElementRef,
    #[serde(flatten)]
    pub form: FormArg,
}

/// `inplace.add_form`: its index.
pub(crate) fn add_form(txn: &mut Transaction<'_>, p: AddFormParams) -> Result<usize> {
    let store = txn.store().clone();
    let (id, mut r) = element(&store, &p.element_id)?;
    crate::modify::check_unpinned(&store, id)?;
    r.forms.push(form(&store, p.form)?);
    write(txn, id, &r)?;
    Ok(r.forms.len() - 1)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveFormParams {
    pub element_id: ElementRef,
    pub index: usize,
}

/// `inplace.remove_form`.
pub(crate) fn remove_form(txn: &mut Transaction<'_>, p: RemoveFormParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = element(&store, &p.element_id)?;
    crate::modify::check_unpinned(&store, id)?;
    if p.index >= r.forms.len() {
        return Err(Error::new(
            "inplace.no_form",
            format!("there is no form {}", p.index),
        ));
    }
    r.forms.remove(p.index);
    write(txn, id, &r)
}

/// Moves, turns or mirrors an in-place element's forms (their planes).
pub(crate) fn transform(
    txn: &mut Transaction<'_>,
    id: ElementId,
    point: &dyn Fn([f64; 2]) -> [f64; 2],
    dz: f64,
) -> Result<()> {
    let store = txn.store().clone();
    let mut r = recipe(store.get(id).expect("exists")).expect("an in-place element");
    transform_forms(&store, &mut r.forms, point, dz)?;
    write(txn, id, &r)
}

/// Carries forms by a plan transformation (each plane made explicit).
pub(crate) fn transform_forms(
    store: &Store,
    forms: &mut [Form],
    point: &dyn Fn([f64; 2]) -> [f64; 2],
    dz: f64,
) -> Result<()> {
    for f in forms.iter_mut() {
        // Each form's plane becomes an explicit plane carried by the transformation.
        let plane = crate::work_planes::resolve(store, &f.plane)?;
        let carried = crate::work_planes::WorkPlaneRef::Explicit {
            origin: plane.origin,
            x_dir: plane.x,
            normal: plane.normal,
        };
        f.plane = crate::work_planes::transform_frame(store, &carried, point, dz)?;
        if let Shape::Sweep { path, z, .. } = &mut f.shape {
            for p in path.iter_mut() {
                *p = point(*p);
            }
            *z += dz;
        }
    }
    Ok(())
}
