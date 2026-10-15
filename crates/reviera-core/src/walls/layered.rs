// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Layer-by-layer wall bodies: vertically compound walls (regions, ARC-CMPD-003), extendable
//! layers (ARC-CMPD-004) and layer wrapping at inserts and free ends (ARC-WRAP-001).
//!
//! A wall whose type has regions or wrapping, or that has a layer extension, is built as one
//! box per layer *cell*: the layer's band across the wall (cut at the joined ends, as the wall's
//! outline is) between the cell's heights, cut by the wall's openings, in the cell's material.
//!
//! - **Regions** split a layer's column at a distance from the base or top; a short wall clips
//!   the region at its far end.
//! - **Extensions** move the unlocked top or bottom edges of the extendable layers (not while
//!   the top is attached to a roof).
//! - **Wrapping at inserts**: on a wrapping side, the layers between the face and the core lose
//!   the opening grown by the wrapping layers' total thickness; each wrapping layer keeps the
//!   opening grown by the wrapping layers outside it and lines the jambs, head and sill back to
//!   the core.
//! - **Wrapping at ends** (free ends only): each wrapping layer turns round the end as a cap
//!   back to the core's far face; the other layers on that side and the core stop short to make
//!   room.
//!
//! Not built yet: vertical region boundaries (steps in thickness), family closure planes (the
//! core's face is the closure depth), shaped openings wrap as their bounding rectangle, curved,
//! slanted, tapered, profiled and stacked walls (built as one solid), plan layer lines for
//! regions.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bodies::Mesh;
use crate::compound::{Anchor, Compound, Wrap};
use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::joins::{Solved, WallBody};
use crate::walls::openings::Cut;
use crate::walls::{RECIPE_KEY, WallGeometry, WallRecipe, wall_recipe};

/// A wall end's wrapping: the type's, or its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EndWrap {
    #[default]
    ByType,
    Exterior,
    Interior,
    None,
}

pub(crate) fn compound_of(store: &Store, e: &Element) -> Option<Compound> {
    let g = crate::id::GlobalId::parse(e.param("@type")?.as_str()?)?;
    let t = store.get(store.find_by_guid(&g)?)?;
    if crate::walls::stacked::of_type(t).is_some() || !crate::compound::is_layered(t) {
        return None;
    }
    crate::compound::of_wall_type(t)
}

fn end_wrap(c: &Compound, r: &WallRecipe, end: usize) -> Wrap {
    match r.end_wrap[end] {
        EndWrap::ByType => c.wrap_ends,
        EndWrap::Exterior => Wrap::Exterior,
        EndWrap::Interior => Wrap::Interior,
        EndWrap::None => Wrap::None,
    }
}

/// Whether a wall is built layer by layer.
fn needed(c: &Compound, r: &WallRecipe) -> bool {
    !c.regions.is_empty()
        || !c.wrap_inserts.is_none()
        || (0..2).any(|k| !end_wrap(c, r, k).is_none())
        || (r.extension != [0.0, 0.0] && c.layers.iter().any(|l| l.extend_top || l.extend_base))
}

/// A box in the wall's frame: `x` along from its start, `v` in from the exterior face, `z`
/// absolute; `ends` takes x to the joined end lines instead (with the end trims).
struct Cell {
    v: [f64; 2],
    z: [f64; 2],
    x: Option<[f64; 2]>,
    trim: [f64; 2],
    material: String,
    holes: Vec<Cut>,
}

/// Grows a cut's rectangle by `d` on every side.
fn grow(c: &Cut, d: f64) -> Cut {
    Cut::rect([c.x[0] - d, c.x[1] + d], [c.z[0] - d, c.z[1] + d])
}

/// The rings of a wrapping layer round a cut, between growth `d0` and `d1`: jambs, head, sill.
fn ring(c: &Cut, d0: f64, d1: f64) -> Vec<([f64; 2], [f64; 2])> {
    let (x, z) = (c.x, c.z);
    vec![
        ([x[0] - d1, x[0] - d0], [z[0] - d1, z[1] + d1]),
        ([x[1] + d0, x[1] + d1], [z[0] - d1, z[1] + d1]),
        ([x[0] - d0, x[1] + d0], [z[1] + d0, z[1] + d1]),
        ([x[0] - d0, x[1] + d0], [z[0] - d1, z[0] - d0]),
    ]
}

/// A wall's layer meshes with their materials, or `None` when it is built as one solid.
pub fn meshes(
    store: &Store,
    e: &Element,
    body: &WallBody,
    solved: &Solved,
    cuts: &[Cut],
) -> Option<Vec<(String, Mesh)>> {
    let r = wall_recipe(e)?;
    if !r.through.is_empty()
        || r.profile.is_some()
        || r.cross_section.is_some()
        || crate::walls::stacked::of_wall(store, e).is_some()
    {
        return None;
    }
    let c = compound_of(store, e)?;
    if !needed(&c, &r) {
        return None;
    }
    let g = &body.geometry;
    let n = c.layers.len();
    let b = c.boundaries();
    let [cs, ce] = c.core;
    let (z0, z1) = (g.base_z, g.base_z + g.height);
    let attached = r.attach_top.is_some();
    let ext = [r.extension[0], if attached { 0.0 } else { r.extension[1] }];
    let material = |i: usize| {
        let l = &c.layers[i];
        if l.material.trim().is_empty() {
            l.function.label().to_owned()
        } else {
            l.material.clone()
        }
    };
    // Each layer's heights, after extensions.
    let span = |i: usize| {
        let l = &c.layers[i];
        [
            z0 + if l.extend_base { ext[0] } else { 0.0 },
            z1 + if l.extend_top { ext[1] } else { 0.0 },
        ]
    };
    // Wrapping layers on each side, outermost first, with the growth outside each.
    let side_layers = |exterior: bool| -> Vec<usize> {
        if exterior {
            (0..cs).collect()
        } else {
            (ce..n).rev().collect()
        }
    };
    let wraps = |exterior: bool| -> Vec<(usize, f64, f64)> {
        let mut t = 0.0;
        side_layers(exterior)
            .into_iter()
            .filter(|i| c.layers[*i].wraps && c.layers[*i].thickness > 0.0)
            .map(|i| {
                let d0 = t;
                t += c.layers[i].thickness;
                (i, d0, t)
            })
            .collect()
    };
    let total = |w: &[(usize, f64, f64)]| w.last().map_or(0.0, |x| x.2);
    let wrap_ins = c.wrap_inserts;
    let (wx, wi) = (wraps(true), wraps(false));
    // The hole a layer gets for one cut.
    let hole = |i: usize, cut: &Cut| -> Cut {
        for (exterior, w, on) in [
            (true, &wx, wrap_ins.exterior()),
            (false, &wi, wrap_ins.interior()),
        ] {
            if !on || w.is_empty() || !side_layers(exterior).contains(&i) {
                continue;
            }
            return match w.iter().find(|x| x.0 == i) {
                Some(&(_, d0, _)) => grow(cut, d0),
                None => grow(cut, total(w)),
            };
        }
        Cut::rect(cut.x, cut.z)
    };
    // Free ends and how much each layer stops short of them.
    let joined = |end: crate::walls::End| {
        solved.joins.iter().any(|j| {
            (j.wall == e.id() && j.end == end) || (j.other == e.id() && j.other_end == Some(end))
        })
    };
    let ends = [
        if joined(crate::walls::End::Start) {
            Wrap::None
        } else {
            end_wrap(&c, &r, 0)
        },
        if joined(crate::walls::End::End) {
            Wrap::None
        } else {
            end_wrap(&c, &r, 1)
        },
    ];
    let trim_of = |i: usize, w: Wrap| -> f64 {
        if w.is_none() {
            return 0.0;
        }
        let exterior = w.exterior();
        let list = if exterior { &wx } else { &wi };
        if list.is_empty() {
            return 0.0;
        }
        match list.iter().find(|x| x.0 == i) {
            Some(&(_, d0, _)) => d0,
            // Layers on that side and the core stop short by the whole wrap.
            None if side_layers(exterior).contains(&i) || (cs..ce).contains(&i) => total(list),
            None => 0.0,
        }
    };
    let mut cells: Vec<Cell> = Vec::new();
    for i in 0..n {
        let l = &c.layers[i];
        if l.thickness <= 0.0 {
            continue;
        }
        let v = [b[i], b[i + 1]];
        let [lo, hi] = span(i);
        if hi - lo < 1e-6 {
            continue;
        }
        // The layer's column, split by its regions.
        let mut pieces: Vec<([f64; 2], String)> = Vec::new();
        let mut rest = [lo, hi];
        for reg in c.regions.iter().filter(|reg| reg.layer == i) {
            let m = c.region_material(reg);
            let m = if m.trim().is_empty() { material(i) } else { m };
            match reg.from {
                Anchor::Base => {
                    let top = (rest[0] + reg.distance).min(rest[1]);
                    pieces.push(([rest[0], top], m));
                    rest[0] = top;
                }
                Anchor::Top => {
                    let bottom = (rest[1] - reg.distance).max(rest[0]);
                    pieces.push(([bottom, rest[1]], m));
                    rest[1] = bottom;
                }
            }
        }
        if rest[1] - rest[0] > 1e-9 {
            pieces.push((rest, material(i)));
        }
        let holes: Vec<Cut> = cuts.iter().map(|cut| hole(i, cut)).collect();
        for (z, m) in pieces {
            if z[1] - z[0] < 1e-9 {
                continue;
            }
            cells.push(Cell {
                v,
                z,
                x: None,
                trim: [trim_of(i, ends[0]), trim_of(i, ends[1])],
                material: m,
                holes: holes.clone(),
            });
        }
    }
    let length = g.length();
    // The core's faces, from the exterior face.
    let core_v = [b[cs.min(n)], b[ce.min(n)]];
    // Jamb linings: each wrapping layer round each opening, from its inner face to the core.
    for (exterior, w, on) in [
        (true, &wx, wrap_ins.exterior()),
        (false, &wi, wrap_ins.interior()),
    ] {
        if !on {
            continue;
        }
        for &(i, d0, d1) in w.iter() {
            let v = if exterior {
                [b[i + 1], core_v[0]]
            } else {
                [core_v[1], b[i]]
            };
            if v[1] - v[0] < 1e-9 {
                continue;
            }
            let [lo, hi] = span(i);
            for cut in cuts {
                for (x, z) in ring(cut, d0, d1) {
                    let z = [z[0].max(lo), z[1].min(hi)];
                    let x = [x[0].max(0.0), x[1].min(length)];
                    if z[1] - z[0] > 1e-9 && x[1] - x[0] > 1e-9 {
                        cells.push(Cell {
                            v,
                            z,
                            x: Some(x),
                            trim: [0.0, 0.0],
                            material: material(i),
                            holes: Vec::new(),
                        });
                    }
                }
            }
        }
    }
    // End caps: each wrapping layer across the end, back to the core's far face.
    for (k, w) in ends.iter().enumerate() {
        if w.is_none() {
            continue;
        }
        let exterior = w.exterior();
        let list = if exterior { &wx } else { &wi };
        for &(i, d0, d1) in list.iter() {
            // From the layer's inner face (its band runs on along the wall) to the core's far
            // face.
            let v = if exterior {
                [b[i + 1], core_v[1]]
            } else {
                [core_v[0], b[i]]
            };
            let x = if k == 0 {
                [d0, d1]
            } else {
                [length - d1, length - d0]
            };
            cells.push(Cell {
                v,
                z: span(i),
                x: Some(x),
                trim: [0.0, 0.0],
                material: material(i),
                holes: Vec::new(),
            });
        }
    }
    Some(
        cells
            .into_iter()
            .filter_map(|cell| cell_mesh(body, &r, cell))
            .collect(),
    )
}

/// One cell's mesh.
fn cell_mesh(body: &WallBody, r: &WallRecipe, cell: Cell) -> Option<(String, Mesh)> {
    let g = &body.geometry;
    let half = g.width / 2.0;
    let ext = if r.flip { -1.0 } else { 1.0 };
    // From in-from-the-exterior depth to across along the normal.
    let y = |v: f64| ext * (half - v);
    let (ya, yb) = (y(cell.v[0]), y(cell.v[1]));
    let (lo, hi) = (ya.min(yb), ya.max(yb));
    let band = match cell.x {
        None => {
            let mut b = crate::walls::stacked::band(body, lo, hi, cell.z[0], cell.z[1]);
            trim_ends(&mut b, cell.trim);
            b
        }
        Some(x) => {
            let (u, n) = (g.direction(), g.normal());
            let at = |a: f64, c: f64| {
                [
                    g.start[0] + u[0] * a + n[0] * c,
                    g.start[1] + u[1] * a + n[1] * c,
                ]
            };
            WallBody {
                geometry: WallGeometry {
                    base_z: cell.z[0],
                    height: cell.z[1] - cell.z[0],
                    width: hi - lo,
                    ..*g
                },
                footprint: [at(x[0], lo), at(x[1], lo), at(x[1], hi), at(x[0], hi)],
            }
        }
    };
    let (z0, z1) = (cell.z[0], cell.z[1]);
    let span = crate::walls::openings::clear_span(&band);
    let holes: Vec<Cut> = cell
        .holes
        .iter()
        .filter(|h| h.z[1] > z0 + 1e-6 && h.z[0] < z1 - 1e-6)
        .map(|h| Cut {
            x: [h.x[0].max(span[0]), h.x[1].min(span[1])],
            z: [h.z[0].max(z0), h.z[1].min(z1)],
            outline: h.outline.clone(),
        })
        .filter(|h| h.x[1] - h.x[0] > 1e-6)
        .collect();
    let mesh = if holes.is_empty() {
        band.mesh()
    } else {
        crate::walls::openings::cut_mesh(&band, &holes)
    };
    (!mesh.1.is_empty()).then_some((cell.material, mesh))
}

/// Moves a band's start and end corners in along the wall.
fn trim_ends(b: &mut WallBody, trim: [f64; 2]) {
    let u = b.geometry.direction();
    for k in [0, 3] {
        b.footprint[k] = [
            b.footprint[k][0] + u[0] * trim[0],
            b.footprint[k][1] + u[1] * trim[0],
        ];
    }
    for k in [1, 2] {
        b.footprint[k] = [
            b.footprint[k][0] - u[0] * trim[1],
            b.footprint[k][1] - u[1] * trim[1],
        ];
    }
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetExtensionParams {
    pub wall_ids: Vec<ElementRef>,
    /// Top Extension Distance (metres, ±); omitted, unchanged.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub top: Option<f64>,
    /// Base Extension Distance (metres; negative runs below the base).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base: Option<f64>,
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

fn write(txn: &mut Transaction<'_>, id: ElementId, r: &WallRecipe) -> Result<()> {
    let value = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })
}

/// `wall.set_extension`.
pub(crate) fn set_extension(txn: &mut Transaction<'_>, p: SetExtensionParams) -> Result<()> {
    let store = txn.store().clone();
    for w in &p.wall_ids {
        let (id, mut r) = wall(&store, w)?;
        let e = store.get(id).expect("resolved");
        let c = compound_of(&store, e);
        let has = |top: bool| {
            c.as_ref().is_some_and(|c| {
                c.layers
                    .iter()
                    .any(|l| if top { l.extend_top } else { l.extend_base })
            })
        };
        if let Some(v) = p.top {
            if !has(true) && v != 0.0 {
                return Err(Error::new(
                    "wall.no_extendable",
                    "the wall's type has no layer whose top edge is unlocked",
                ));
            }
            if r.attach_top.is_some() && v != 0.0 {
                return Err(Error::new(
                    "wall.extension_attached",
                    "the top is attached, so Top Extension Distance is read-only",
                ));
            }
            r.extension[1] = v;
        }
        if let Some(v) = p.base {
            if !has(false) && v != 0.0 {
                return Err(Error::new(
                    "wall.no_extendable",
                    "the wall's type has no layer whose bottom edge is unlocked",
                ));
            }
            r.extension[0] = v;
        }
        let g = crate::walls::regenerate(&store, e)?;
        if g.height + r.extension[1] - r.extension[0] < crate::walls::TOL_SHORT {
            return Err(Error::new(
                "wall.extension_exceeds",
                "Layer extension exceeds wall height",
            ));
        }
        write(txn, id, &r)?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetEndWrapParams {
    pub wall_id: ElementRef,
    pub end: crate::walls::End,
    pub mode: EndWrap,
}

/// `wall.set_end_wrap`.
pub(crate) fn set_end_wrap(txn: &mut Transaction<'_>, p: SetEndWrapParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = wall(&store, &p.wall_id)?;
    r.end_wrap[p.end.index()] = p.mode;
    write(txn, id, &r)
}
