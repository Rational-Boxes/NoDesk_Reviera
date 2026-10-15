// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Edited wall profiles (spec Ch 3 §3.3.14, ARC-WALL-009): a straight wall's elevation outline
//! changed to gables, notches and holes.
//!
//! The profile is one outer loop and any holes in the wall's own plane: `u` along the wall from
//! its start, `z` above its base. Each point is kept from the nearer end of the wall and the
//! nearer of its base and top, so a point on the top line follows the top when the wall gets
//! taller, and a notch at the top keeps its depth. The body is the profile pushed across the
//! wall between its faces, its ends following the joined ends, less its doors and windows.
//!
//! Not built yet: mid-end faces as join targets, the sketch mode in elevation (the window takes
//! the outline as typed points or a preset), curved walls (Wall Opening instead).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bodies::Mesh;
use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::ElementRef;
use crate::polygons::{self, Shape};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::joins::WallBody;
use crate::walls::openings::Cut;
use crate::walls::{RECIPE_KEY, WallRecipe, wall_recipe};

/// Which end a point's `u` is measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AlongRef {
    #[default]
    Start,
    End,
}

/// Whether a point's `z` is measured up from the base or down from the top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HeightRef {
    #[default]
    Base,
    Top,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProfilePoint {
    /// Distance from the reference end, along the wall (metres).
    pub u: f64,
    /// Distance from the reference height: up from the base, or down from the top (metres).
    pub z: f64,
    #[serde(default)]
    pub u_ref: AlongRef,
    #[serde(default)]
    pub z_ref: HeightRef,
}

/// A wall's edited profile: the outer loop first, then holes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Profile {
    pub loops: Vec<Vec<ProfilePoint>>,
}

impl Profile {
    /// From points in the wall's plane for a wall `length` long and `height` high: the outer
    /// loop's points each keep to their nearer end and nearer of base and top; a hole keeps its
    /// size, the whole loop keeping to the references nearer its centre.
    pub fn from_points(loops: &[Vec<[f64; 2]>], length: f64, height: f64) -> Self {
        let refs = |p: [f64; 2]| {
            (
                if length - p[0] < p[0] {
                    AlongRef::End
                } else {
                    AlongRef::Start
                },
                if height - p[1] < p[1] {
                    HeightRef::Top
                } else {
                    HeightRef::Base
                },
            )
        };
        let point = |p: &[f64; 2], (u_ref, z_ref): (AlongRef, HeightRef)| ProfilePoint {
            u: match u_ref {
                AlongRef::Start => p[0],
                AlongRef::End => length - p[0],
            },
            z: match z_ref {
                HeightRef::Base => p[1],
                HeightRef::Top => height - p[1],
            },
            u_ref,
            z_ref,
        };
        let loops = loops
            .iter()
            .enumerate()
            .map(|(i, l)| {
                if i == 0 {
                    return l.iter().map(|p| point(p, refs(*p))).collect();
                }
                let n = l.len().max(1) as f64;
                let c = [
                    l.iter().map(|p| p[0]).sum::<f64>() / n,
                    l.iter().map(|p| p[1]).sum::<f64>() / n,
                ];
                l.iter().map(|p| point(p, refs(c))).collect()
            })
            .collect();
        Self { loops }
    }

    /// The loops in the wall's plane for a wall `length` long and `height` high.
    pub fn points(&self, length: f64, height: f64) -> Vec<Vec<[f64; 2]>> {
        self.loops
            .iter()
            .map(|l| {
                l.iter()
                    .map(|p| {
                        let u = match p.u_ref {
                            AlongRef::Start => p.u,
                            AlongRef::End => length - p.u,
                        };
                        let z = match p.z_ref {
                            HeightRef::Base => p.z,
                            HeightRef::Top => height - p.z,
                        };
                        [u, z]
                    })
                    .collect()
            })
            .collect()
    }
}

/// The profile as one shape (outer less holes) for a wall `length` long and `height` high.
fn shape(p: &Profile, length: f64, height: f64) -> Result<Shape> {
    let loops = p.points(length, height);
    let Some((outer, holes)) = loops.split_first() else {
        return Err(Error::new(
            "wall.profile_empty",
            "a profile needs an outer loop",
        ));
    };
    if loops.iter().any(|l| l.len() < 3) {
        return Err(Error::new(
            "wall.profile_loop",
            "each loop of a profile needs at least three points",
        ));
    }
    let base = polygons::union(std::slice::from_ref(outer));
    let shapes = polygons::difference(&base, holes);
    match shapes.as_slice() {
        [s] if s.area() > 1e-6 => Ok(s.clone()),
        [] => Err(Error::new("wall.profile_empty", "the profile has no area")),
        _ => Err(Error::new(
            "wall.profile_loops",
            "a profile needs exactly one outer loop; its holes must not split it",
        )),
    }
}

/// The body of a profiled wall: the profile across the wall between its faces, its ends
/// following the joined ends, less `cuts` (absolute elevations, as the wall's openings).
pub fn mesh(body: &WallBody, p: &Profile, cuts: &[Cut]) -> Result<Mesh> {
    let g = &body.geometry;
    let mut s = shape(p, g.length(), g.height)?;
    if !cuts.is_empty() {
        let holes: Vec<Vec<[f64; 2]>> = cuts
            .iter()
            .map(|c| {
                if c.outline.is_empty() {
                    vec![
                        [c.x[0], c.z[0] - g.base_z],
                        [c.x[1], c.z[0] - g.base_z],
                        [c.x[1], c.z[1] - g.base_z],
                        [c.x[0], c.z[1] - g.base_z],
                    ]
                } else {
                    c.outline.iter().map(|q| [q[0], q[1] - g.base_z]).collect()
                }
            })
            .collect();
        let pieces = polygons::difference(&[s.clone()], &holes);
        // A door cut up from the base leaves one piece or more; keep them all.
        return Ok(pieces.iter().map(|piece| extrude_piece(body, piece)).fold(
            (Vec::new(), Vec::new()),
            |mut acc, (p, i)| {
                let base = acc.0.len() as u32;
                acc.0.extend(p);
                acc.1.extend(i.into_iter().map(|k| k + base));
                acc
            },
        ));
    }
    s.outer = polygons::simplify(&s.outer);
    Ok(extrude_piece(body, &s))
}

/// One piece of profile pushed across the wall.
fn extrude_piece(body: &WallBody, s: &Shape) -> Mesh {
    let g = &body.geometry;
    let f = body.local_footprint();
    let (u, n) = (g.direction(), g.normal());
    let (y0, y1) = (f[0][1].min(f[3][1]), f[0][1].max(f[3][1]));
    // The "plan" of the extrusion is the profile (u, z) and its height the depth across.
    let (points, indices) = crate::bodies::extrude(&s.outer, &s.holes, y0, y1);
    let length = g.length();
    // The x of an end line (through two corners) at a given y.
    let at = |a: [f64; 2], b: [f64; 2], y: f64| {
        if (b[1] - a[1]).abs() < 1e-12 {
            a[0]
        } else {
            a[0] + (y - a[1]) * (b[0] - a[0]) / (b[1] - a[1])
        }
    };
    let world: Vec<[f64; 3]> = points
        .iter()
        .map(|p| {
            let (along, z, y) = (p[0], p[1], p[2]);
            // Points on the wall's ends follow its joined ends.
            let x = if along.abs() < 1e-5 {
                at(f[0], f[3], y)
            } else if (along - length).abs() < 1e-5 {
                at(f[1], f[2], y)
            } else {
                along
            };
            [
                g.start[0] + u[0] * x + n[0] * y,
                g.start[1] + u[1] * x + n[1] * y,
                g.base_z + z,
            ]
        })
        .collect();
    // (u, z, y) to (along, normal, up) swaps two axes: reverse the triangles.
    let indices = indices.chunks(3).flat_map(|t| [t[0], t[2], t[1]]).collect();
    (world, indices)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditParams {
    pub wall_id: ElementRef,
    /// The outer loop, then holes: points `[u, z]`, `u` along the wall from its start and `z`
    /// above its base (metres). Each point is kept from the nearer end and the nearer of base
    /// and top.
    pub loops: Vec<Vec<[f64; 2]>>,
}

fn straight_wall(store: &Store, r: &ElementRef) -> Result<(crate::id::ElementId, WallRecipe)> {
    let id = store
        .resolve(r)
        .filter(|id| store.get(*id).and_then(wall_recipe).is_some())
        .ok_or_else(|| Error::new("element.not_found", format!("no Reviera wall {r:?}")))?;
    let recipe = wall_recipe(store.get(id).expect("resolved")).expect("checked");
    if !recipe.through.is_empty() {
        return Err(Error::new(
            "wall.profile_curved",
            "Edit Profile is for straight walls; use Wall Opening on curved walls",
        ));
    }
    Ok((id, recipe))
}

fn set_recipe(txn: &mut Transaction<'_>, id: crate::id::ElementId, r: &WallRecipe) -> Result<()> {
    let value = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })
}

/// `wall.edit_profile`.
pub(crate) fn edit(txn: &mut Transaction<'_>, p: EditParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut recipe) = straight_wall(&store, &p.wall_id)?;
    crate::modify::check_unpinned(&store, id)?;
    if crate::walls::stacked::of_wall(&store, store.get(id).expect("resolved")).is_some() {
        return Err(Error::new(
            "wall.profile_stacked",
            "a stacked wall's profile cannot be edited; Break Up first",
        ));
    }
    let g = crate::walls::regenerate(&store, store.get(id).expect("resolved"))?;
    let profile = Profile::from_points(&p.loops, g.length(), g.height);
    shape(&profile, g.length(), g.height)?;
    recipe.profile = Some(profile);
    set_recipe(txn, id, &recipe)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResetParams {
    pub wall_ids: Vec<ElementRef>,
}

/// `wall.reset_profile`.
pub(crate) fn reset(txn: &mut Transaction<'_>, p: ResetParams) -> Result<()> {
    for r in &p.wall_ids {
        let store = txn.store().clone();
        let (id, mut recipe) = straight_wall(&store, r)?;
        if recipe.profile.take().is_some() {
            set_recipe(txn, id, &recipe)?;
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetParams {
    pub wall_id: ElementRef,
}

/// `wall.get_profile`: the loops as they are now, in the wall's plane; the rectangle when the
/// profile is not edited.
pub(crate) fn get(store: &Store, p: GetParams) -> Result<Vec<Vec<[f64; 2]>>> {
    let (id, recipe) = straight_wall(store, &p.wall_id)?;
    let g = crate::walls::regenerate(store, store.get(id).expect("resolved"))?;
    Ok(match &recipe.profile {
        Some(pr) => pr.points(g.length(), g.height),
        None => vec![vec![
            [0.0, 0.0],
            [g.length(), 0.0],
            [g.length(), g.height],
            [0.0, g.height],
        ]],
    })
}

/// A wall's body when it is profiled or leans (ARC-WALL-009, ARC-WALL-005), else `None`.
pub fn special_mesh(e: &Element, body: &WallBody, cuts: &[Cut]) -> Option<Result<Mesh>> {
    let r = wall_recipe(e)?;
    if r.profile.is_none() && r.cross_section.is_none() {
        return None;
    }
    let mesh = match &r.profile {
        Some(p) => match mesh(body, p, cuts) {
            Ok(m) => m,
            Err(e) => return Some(Err(e)),
        },
        None if cuts.is_empty() => body.mesh(),
        None => crate::walls::openings::cut_mesh(body, cuts),
    };
    Some(Ok(match r.cross_section {
        Some(cs) => crate::walls::cross_section::shear(&mesh, &body.geometry, r.flip, cs),
        None => mesh,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_keep_to_their_nearer_references() {
        let notch = vec![vec![
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 2.0],
            [9.0, 2.0],
            [9.0, 3.0],
            [0.0, 3.0],
        ]];
        let p = Profile::from_points(&notch, 10.0, 3.0);
        // Taller and longer: the notch keeps its 1 m by 1 m.
        let q = p.points(12.0, 4.0);
        assert_eq!(q[0][2], [12.0, 3.0]);
        assert_eq!(q[0][3], [11.0, 3.0]);
        assert_eq!(q[0][4], [11.0, 4.0]);
    }
}
