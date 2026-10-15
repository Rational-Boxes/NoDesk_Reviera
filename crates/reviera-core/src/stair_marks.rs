// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Stair annotation in plans (spec Ch 3 §3.7, ARC-STAIR-006): the stair path, the cut mark and
//! tread numbers.
//!
//! - The **path** runs through the middles of the treads from the bottom to the top, ending in an
//!   arrow, with UP at its start (DN in a plan of the stair's top level).
//! - The **cut mark** is a zig-zag across the first tread above the plan's cut plane.
//! - **Tread numbers** are an annotation of one view (`IfcAnnotation`, `recipe.stair_numbers`):
//!   each tread numbered from a start by its index along the stair, so the numbers stay when
//!   the stair moves and change only when the number of treads does.
//!
//! Not built yet: stair path and cut mark types (symbols, double zig-zag), number styles,
//! riser numbers, Tag All Not Tagged, path display above and below the cut line.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::RECIPE_KEY;

pub const NUMBERS_RECIPE: &str = "recipe.stair_numbers";

/// A stair's marks in a plan.
#[derive(Debug, Clone, PartialEq)]
pub struct Marks {
    /// Through the treads' middles, bottom to top.
    pub path: Vec<[f64; 2]>,
    /// The arrowhead at the path's end: two short lines.
    pub arrow: [[[f64; 2]; 2]; 2],
    /// The label and where it goes (at the path's start).
    pub label: (&'static str, [f64; 2]),
    /// The zig-zag across the first tread above the cut plane, if the stair reaches it.
    pub cut: Option<Vec<[f64; 2]>>,
    /// Each tread's middle, bottom first.
    pub treads: Vec<[f64; 2]>,
}

fn mid(l: &[[f64; 2]; 2]) -> [f64; 2] {
    [(l[0][0] + l[1][0]) / 2.0, (l[0][1] + l[1][1]) / 2.0]
}

/// The marks of a stair for a plan cut at `cut_z` on a level at `level_z`.
pub fn marks(store: &Store, e: &Element, level_z: f64, cut_z: f64) -> Option<Marks> {
    let g = crate::stairs::regenerate(store, e).ok()?;
    if g.treads.is_empty() {
        return None;
    }
    let treads: Vec<[f64; 2]> = g.treads.iter().map(mid).collect();
    let top = g.z_base + g.height;
    // The first tread above the cut plane: tread k is at z_base + (k + 1) · riser.
    let cut_at = (g.z_base < cut_z && top > cut_z)
        .then(|| {
            (0..g.treads.len()).find(|k| g.z_base + (*k as f64 + 1.0) * g.riser_height > cut_z)
        })
        .flatten();
    // Up to the cut, where the stair leaves the plan.
    let shown = cut_at.map_or(treads.len(), |k| k + 1);
    let mut path = treads[..shown].to_vec();
    // Run on half a tread past the last nosing, toward the top.
    if path.len() >= 2 {
        let (a, b) = (path[path.len() - 2], path[path.len() - 1]);
        path.push([b[0] + (b[0] - a[0]) / 2.0, b[1] + (b[1] - a[1]) / 2.0]);
    }
    let n = path.len();
    let (a, b) = (path[n.saturating_sub(2)], path[n - 1]);
    let l = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-9);
    let u = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
    let s = g.tread_depth.max(0.15) * 0.6;
    let arrow = [
        [
            b,
            [
                b[0] - u[0] * s + u[1] * s * 0.5,
                b[1] - u[1] * s - u[0] * s * 0.5,
            ],
        ],
        [
            b,
            [
                b[0] - u[0] * s - u[1] * s * 0.5,
                b[1] - u[1] * s + u[0] * s * 0.5,
            ],
        ],
    ];
    // UP from the level the stair starts on; DN in a plan of the level it reaches.
    let label = if (level_z - top).abs() < 0.05 {
        "DN"
    } else {
        "UP"
    };
    let label_at = {
        let (p, q) = (treads[0], *treads.get(1).unwrap_or(&treads[0]));
        let l = (q[0] - p[0]).hypot(q[1] - p[1]).max(1e-9);
        [
            p[0] - (q[0] - p[0]) / l * s * 1.5,
            p[1] - (q[1] - p[1]) / l * s * 1.5,
        ]
    };
    let cut = cut_at.map(|k| &g.treads[k]).map(|l| {
        let (p, q) = (l[0], l[1]);
        let d = [q[0] - p[0], q[1] - p[1]];
        let len = d[0].hypot(d[1]).max(1e-9);
        let (ux, uy) = (d[0] / len, d[1] / len);
        let z = g.tread_depth.max(0.15) * 0.4;
        // Out a little past each side, with a zig-zag in the middle.
        let at = |t: f64, off: f64| [p[0] + ux * t - uy * off, p[1] + uy * t + ux * off];
        vec![
            at(-0.1, 0.0),
            at(len * 0.45, 0.0),
            at(len * 0.48, z),
            at(len * 0.52, -z),
            at(len * 0.55, 0.0),
            at(len + 0.1, 0.0),
        ]
    });
    Some(Marks {
        path,
        arrow,
        label: (label, label_at),
        cut,
        treads,
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NumbersRecipe {
    pub kind: String,
    /// The view (GlobalId) and stair (GlobalId).
    pub view: String,
    pub stair: String,
    pub start: u32,
}

pub fn numbers_recipe(e: &Element) -> Option<NumbersRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == NUMBERS_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// A view's tread numbers: each number and where it goes.
pub fn numbers_in(store: &Store, view: &Element) -> Vec<(u32, [f64; 2])> {
    let guid = view.guid().as_str();
    let mut out = Vec::new();
    for e in store.iter() {
        let Some(n) = numbers_recipe(e) else { continue };
        if n.view != guid {
            continue;
        }
        let Some(stair) = GlobalId::parse(&n.stair)
            .and_then(|g| store.find_by_guid(&g))
            .and_then(|id| store.get(id))
        else {
            continue;
        };
        let Ok(g) = crate::stairs::regenerate(store, stair) else {
            continue;
        };
        for (k, l) in g.treads.iter().enumerate() {
            out.push((n.start + k as u32, mid(l)));
        }
    }
    out
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NumberParams {
    pub view_id: ElementRef,
    pub stair_id: ElementRef,
    /// The first tread's number (default 1).
    pub start_number: Option<u32>,
}

/// `stair.number_treads`.
pub(crate) fn number_treads(txn: &mut Transaction<'_>, p: NumberParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let (view, _) = crate::views::resolve_view(&store, &p.view_id)?;
    let stair = store
        .resolve(&p.stair_id)
        .filter(|id| {
            store
                .get(*id)
                .and_then(crate::stairs::stair_recipe)
                .is_some()
        })
        .ok_or_else(|| Error::new("element.not_found", "no such stair"))?;
    let r = NumbersRecipe {
        kind: NUMBERS_RECIPE.into(),
        view: store.get(view).expect("resolved").guid().to_string(),
        stair: store.get(stair).expect("resolved").guid().to_string(),
        start: p.start_number.unwrap_or(1),
    };
    txn.create(
        NewElement::new("IfcAnnotation")
            .name("Tread Numbers")
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )
}
