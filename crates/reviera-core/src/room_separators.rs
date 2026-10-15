// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Room separation lines (spec Ch 3 §3.9.1, ARC-ROOM-001): lines on a level that split space
//! for rooms like walls do, without being built. Each is an `IfcVirtualElement` with
//! `recipe.room_separation` (its level and line, or arc through a point); rooms on the level
//! take them as boundaries at once.
//!
//! Not built yet: Pick Lines, the `Room Separation` line style's graphics, `IfcRelSpaceBoundary`
//! (VIRTUAL) in IFC.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::arrangement::Edge;
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::{Line, RECIPE_KEY};

pub const RECIPE: &str = "recipe.room_separation";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SeparatorRecipe {
    pub kind: String,
    /// The level (GlobalId).
    pub level: String,
    pub line: Line,
    /// A point an arc passes through.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub through: Option<[f64; 2]>,
}

pub fn recipe(e: &Element) -> Option<SeparatorRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// A separator's points (an arc as a polyline).
pub fn points(r: &SeparatorRecipe) -> Vec<[f64; 2]> {
    let mut pts = vec![r.line.start];
    if let Some(t) = r.through {
        pts.extend(crate::sketch::interior(r.line, &[t]));
    }
    pts.push(r.line.end);
    pts
}

/// The separators of the level at elevation `z`, as arrangement edges.
pub fn edges(store: &Store, z: f64) -> Vec<Edge> {
    let mut out = Vec::new();
    for e in store.iter() {
        let Some(r) = recipe(e) else { continue };
        let on_level = GlobalId::parse(&r.level)
            .and_then(|g| store.find_by_guid(&g))
            .and_then(|l| store.get(l))
            .is_some_and(|l| (crate::levels::elevation(l) - z).abs() < 1e-6);
        if !on_level {
            continue;
        }
        for w in points(&r).windows(2) {
            out.push(Edge {
                line: Line {
                    start: w[0],
                    end: w[1],
                },
                lock: Some((e.id(), "line".into())),
            });
        }
    }
    out
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub level_id: ElementRef,
    pub lines: Vec<Line>,
    /// For each line, a point its arc passes through (null for straight).
    #[serde(default)]
    pub through: Vec<Option<[f64; 2]>>,
}

/// `roomseparator.create`.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let level = store
        .resolve(&p.level_id)
        .and_then(|id| store.get(id))
        .filter(|e| e.class == crate::levels::CLASS)
        .ok_or_else(|| Error::new("element.not_found", "no such level"))?
        .guid()
        .to_string();
    let mut out = Vec::new();
    for (k, l) in p.lines.iter().enumerate() {
        if l.length() < 1e-6 {
            return Err(Error::new(
                "roomseparator.short",
                "a room separator needs a length",
            ));
        }
        let r = SeparatorRecipe {
            kind: RECIPE.into(),
            level: level.clone(),
            line: *l,
            through: p.through.get(k).copied().flatten(),
        };
        out.push(
            txn.create(
                NewElement::new("IfcVirtualElement")
                    .name("Room Separation")
                    .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
            )?,
        );
    }
    Ok(out)
}

/// Moves, turns or mirrors a separator.
pub(crate) fn transform(
    txn: &mut Transaction<'_>,
    id: ElementId,
    point: &dyn Fn([f64; 2]) -> [f64; 2],
) -> Result<()> {
    let store = txn.store().clone();
    let mut r = recipe(store.get(id).expect("exists")).expect("a separator");
    r.line = Line {
        start: point(r.line.start),
        end: point(r.line.end),
    };
    r.through = r.through.map(point);
    let v = serde_json::to_value(&r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })
}
