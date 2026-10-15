// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Ceilings that follow a roof (ARC-CEIL-001): vaulted and cathedral ceilings under pitched,
//! arched and domed roofs.
//!
//! A ceiling whose recipe names a roof (`follow_roof`, GlobalId) is shaped to the roof's
//! underside: its outline is cut into a piece under each roof face (the faces of footprint
//! roofs, and the facets of arched roofs and domes), each piece's top lying on that face's
//! underside and its bottom the ceiling's thickness below; what the roof does not cover stays
//! flat at the ceiling's height. The ceiling's openings are left out.
//!
//! Other measures of the ceiling (its area, the stair headroom check) still use its flat
//! reference.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::bodies::Mesh;
use crate::element::Element;
use crate::error::{Error, Result};
use crate::floors::{self, SlabKind};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::polygons::{self, Shape};
use crate::store::Store;
use crate::walls::RECIPE_KEY;

/// The roof a ceiling follows (GlobalId), if it follows one.
pub fn follow_roof(e: &Element) -> Option<String> {
    let (kind, _) = floors::slab_recipe(e)?;
    if kind != SlabKind::Ceiling {
        return None;
    }
    e.param(RECIPE_KEY)?
        .get("follow_roof")?
        .as_str()
        .map(str::to_owned)
}

fn ccw(mut r: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    if polygons::area(&r) < 0.0 {
        r.reverse();
    }
    r
}

/// A ceiling's body when it follows a roof: one closed mesh per piece. `None` when it does not
/// follow one (or the roof is gone).
pub fn meshes(store: &Store, e: &Element) -> Option<Vec<Mesh>> {
    let roof = follow_roof(e)?;
    let roof = store.get(store.find_by_guid(&GlobalId::parse(&roof)?)?)?;
    let g = crate::roofs::regenerate(store, roof).ok()?;
    let f = floors::regenerate(store, e).ok()?;
    let t = f.thickness;
    let profiles = crate::slab_openings::cut_profiles(store, e, &f.profiles);
    let shapes: Vec<Shape> = profiles
        .iter()
        .map(|p| Shape {
            outer: p.outer.clone(),
            holes: p.holes.clone(),
        })
        .collect();
    let mut out = Vec::new();
    let mut covered: Vec<Vec<[f64; 2]>> = Vec::new();
    for face in &g.faces {
        if face.bottom.len() < 3 || face.normal[2] < 1e-3 {
            continue;
        }
        let ring = ccw(face.bottom.iter().map(|q| [q[0], q[1]]).collect());
        if polygons::area(&ring).abs() < 1e-9 {
            continue;
        }
        let holes: Vec<Vec<[f64; 2]>> = face
            .holes
            .iter()
            .map(|h| h.iter().map(|q| [q[0], q[1]]).collect())
            .collect();
        covered.push(ring.clone());
        let (n, a) = (face.normal, face.bottom[0]);
        let under = move |p: [f64; 2]| a[2] - (n[0] * (p[0] - a[0]) + n[1] * (p[1] - a[1])) / n[2];
        for piece in polygons::intersection(&shapes, &[Shape { outer: ring, holes }]) {
            if piece.area() < 1e-6 {
                continue;
            }
            out.push(crate::bodies::extrude_between(
                &piece.outer,
                &piece.holes,
                &|p| under(p) - t,
                &under,
            ));
        }
    }
    // What the roof does not cover stays flat.
    for piece in polygons::difference(&shapes, &covered) {
        if piece.area() < 1e-6 {
            continue;
        }
        out.push(crate::bodies::extrude(
            &piece.outer,
            &piece.holes,
            f.top - t,
            f.top,
        ));
    }
    Some(out)
}

/// The roof over a ceiling: of the roofs whose underside is above the ceiling's level over its
/// outline, the one covering most of it.
pub fn roof_over(store: &Store, ceiling: ElementId) -> Option<ElementId> {
    let e = store.get(ceiling)?;
    let (_, r) = floors::slab_recipe(e)?;
    let level = store.get(store.find_by_guid(&GlobalId::parse(&r.level)?)?)?;
    let z = crate::levels::elevation(level) + 0.5;
    let f = floors::regenerate(store, e).ok()?;
    let shapes: Vec<Shape> = f
        .profiles
        .iter()
        .map(|p| Shape {
            outer: p.outer.clone(),
            holes: p.holes.clone(),
        })
        .collect();
    crate::roofs::all(store)
        .into_iter()
        .filter_map(|(id, g)| {
            let rings: Vec<Vec<[f64; 2]>> = g
                .faces
                .iter()
                .filter(|f| f.bottom.len() >= 3)
                .map(|f| ccw(f.bottom.iter().map(|q| [q[0], q[1]]).collect()))
                .collect();
            let cover: f64 = polygons::intersection(&shapes, &polygons::union(&rings))
                .iter()
                .map(Shape::area)
                .sum();
            // Its underside over the ceiling's middle must be above the level.
            let low = g
                .faces
                .iter()
                .flat_map(|f| f.bottom.iter().map(|q| q[2]))
                .fold(f64::INFINITY, f64::min);
            (cover > 1e-3 && low > z).then_some((id, cover))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

/// A ceiling's shape.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CeilingShape {
    #[default]
    Flat,
    FollowRoof,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetShapeParams {
    pub ceiling_ids: Vec<ElementRef>,
    pub shape: CeilingShape,
    /// The roof to follow; omitted, the roof over each ceiling.
    pub roof_id: Option<ElementRef>,
}

/// Makes ceilings flat, or follow the roof over them (`ceiling.set_shape`).
pub(crate) fn set_shape(txn: &mut crate::txn::Transaction<'_>, p: SetShapeParams) -> Result<()> {
    let store = txn.store().clone();
    let given = match &p.roof_id {
        Some(r) => {
            let id = store
                .resolve(r)
                .ok_or_else(|| Error::new("element.not_found", "no such roof"))?;
            if crate::roofs::roof_recipe(store.get(id).expect("resolved")).is_none() {
                return Err(Error::new(
                    "ceiling.not_a_roof",
                    format!("element {id} is not a Reviera roof"),
                ));
            }
            Some(id)
        }
        None => None,
    };
    for r in &p.ceiling_ids {
        let id = store
            .resolve(r)
            .ok_or_else(|| Error::new("element.not_found", "no such ceiling"))?;
        let e = store.get(id).expect("resolved");
        if floors::slab_recipe(e).map(|(k, _)| k) != Some(SlabKind::Ceiling)
            || crate::decks::deck_settings(e).is_some()
        {
            return Err(Error::new(
                "ceiling.not_reviera",
                format!("element {id} is not a Reviera ceiling"),
            ));
        }
        crate::modify::check_unpinned(&store, id)?;
        let follow = match p.shape {
            CeilingShape::Flat => Value::Null,
            CeilingShape::FollowRoof => {
                let roof = given.or_else(|| roof_over(&store, id)).ok_or_else(|| {
                    Error::new(
                        "ceiling.no_roof",
                        format!("there is no roof over ceiling {id} to follow"),
                    )
                })?;
                Value::String(store.get(roof).expect("found").guid().to_string())
            }
        };
        let mut recipe = e.param(RECIPE_KEY).cloned().expect("a ceiling recipe");
        match follow {
            Value::Null => {
                if let Some(o) = recipe.as_object_mut() {
                    o.remove("follow_roof");
                }
            }
            v => recipe["follow_roof"] = v,
        }
        txn.modify(id, |e| {
            e.params.insert(RECIPE_KEY.into(), recipe);
        })?;
    }
    Ok(())
}
