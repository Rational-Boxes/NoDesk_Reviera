// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Grids (spec Ch 3 §3.2.2, ARC-GRID-001; IFC mapping DM-IFC-006): vertical datum planes that
//! organise the design.
//!
//! Each grid line is its own element, selectable and named: an RXD-only element of class
//! `rxd:model/datums/<axisGuid>` (stored as the RXD document `model/datums/<axisGuid>`), with its
//! data in the `rxd:datum` parameter:
//!
//! ```json
//! { "kind": "datum.grid", "system": "<IfcGrid GlobalId>",
//!   "curve": { "start": [0, 0], "end": [0, 20] }, "bubbles": [false, true] }
//! ```
//!
//! The lines belong to a grid system: an `IfcGrid` element (one per project for now) marked by
//! its `rxd:grid_system` data. Saving writes each line as an `IfcGridAxis` of that `IfcGrid`
//! (`AxisTag` = the grid name). The axis has no GlobalId of its own; the RXD record keeps its
//! identity, so a renamed grid keeps its GUID and the dimensions to it.
//!
//! Built so far: straight grids with `grid.create`, `grid.rename` and `grid.set_curve`. Arcs,
//! multi-segment grids, grid types, scope boxes and datum extents come later.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::levels::next_name;
use crate::store::Store;
use crate::walls::{CurveArg, Line, TOL_SHORT};

/// Element class prefix of grid lines (the RXD document path after `rxd:`).
pub const CLASS_PREFIX: &str = "rxd:model/datums/";
/// Parameter holding a grid line's data.
pub const DATUM_KEY: &str = "rxd:datum";
pub const KIND_GRID: &str = "datum.grid";
/// Parameter (RXD element-data section) marking the `IfcGrid` that Reviera writes.
pub const SYSTEM_KEY: &str = "rxd:grid_system";
pub const SYSTEM_CLASS: &str = "IfcGrid";
/// The name of the first grid of a project.
pub const FIRST_NAME: &str = "1";

/// Stored data of a grid line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GridData {
    pub kind: String,
    /// The grid system (`IfcGrid` GlobalId) the line belongs to.
    pub system: String,
    pub curve: Line,
    /// Bubble shown at the start and at the end of the line.
    #[serde(default = "end_bubble")]
    pub bubbles: [bool; 2],
}

fn end_bubble() -> [bool; 2] {
    [false, true]
}

pub fn is_grid(e: &Element) -> bool {
    e.class.starts_with(CLASS_PREFIX)
}

pub fn grid_data(e: &Element) -> Option<GridData> {
    if !is_grid(e) {
        return None;
    }
    let d: GridData = serde_json::from_value(e.param(DATUM_KEY)?.clone()).ok()?;
    (d.kind == KIND_GRID).then_some(d)
}

/// Whether an element is a grid system written by Reviera.
pub fn is_system(e: &Element) -> bool {
    e.class == SYSTEM_CLASS && e.param(SYSTEM_KEY).is_some()
}

/// One grid line, for listings and drawing.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct GridInfo {
    pub id: ElementId,
    pub guid: String,
    pub name: String,
    pub curve: Line,
    pub bubbles: [bool; 2],
    /// The grid system's GlobalId.
    pub system: String,
}

/// The grid lines of a model, in creation order.
pub fn list(store: &Store) -> Vec<GridInfo> {
    let mut out: Vec<GridInfo> = store
        .iter()
        .filter_map(|e| {
            let d = grid_data(e)?;
            Some(GridInfo {
                id: e.id(),
                guid: e.guid().to_string(),
                name: e.name.clone().unwrap_or_default(),
                curve: d.curve,
                bubbles: d.bubbles,
                system: d.system,
            })
        })
        .collect();
    out.sort_by_key(|g| g.id);
    out
}

/// The grid lines of system `guid`.
pub fn lines_of(store: &Store, system: &str) -> Vec<ElementId> {
    store
        .iter()
        .filter(|e| grid_data(e).is_some_and(|d| d.system == system))
        .map(Element::id)
        .collect()
}

/// The automatic name of a new grid: the next name after the last grid drawn (`1` → `2`,
/// `A` → `B`), not used by another grid.
pub fn auto_name(store: &Store) -> String {
    let all = list(store);
    let mut name = match all.last() {
        Some(g) if !g.name.is_empty() => next_name(&g.name),
        _ => FIRST_NAME.to_owned(),
    };
    while all.iter().any(|g| g.name == name) {
        name = next_name(&name);
    }
    name
}

fn check_name(store: &Store, name: &str, except: Option<ElementId>) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::new("grid.bad_name", "a grid needs a name"));
    }
    if store
        .iter()
        .any(|e| is_grid(e) && Some(e.id()) != except && e.name.as_deref() == Some(name))
    {
        return Err(Error::new(
            "grid.name_taken",
            format!("another grid is already named {name}"),
        ));
    }
    Ok(name.to_owned())
}

fn check_curve(line: &Line) -> Result<()> {
    if !(line.start.iter().chain(&line.end)).all(|v| v.is_finite()) {
        return Err(Error::new(
            "grid.bad_curve",
            "the grid's points must be numbers",
        ));
    }
    if line.length() < TOL_SHORT {
        return Err(Error::new("grid.too_short", "Grid is too short"));
    }
    Ok(())
}

fn resolve_grid(store: &Store, r: &ElementRef) -> Result<(ElementId, GridData)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such grid"))?;
    let e = store.get(id).expect("resolved");
    let d = grid_data(e).ok_or_else(|| {
        Error::new(
            "grid.not_a_grid",
            format!("element {id} ({}) is not a grid", e.class),
        )
    })?;
    Ok((id, d))
}

/// The project's grid system, created when first needed.
fn system(txn: &mut crate::txn::Transaction<'_>) -> Result<String> {
    if let Some(e) = txn.store().iter().find(|e| is_system(e)) {
        return Ok(e.guid().to_string());
    }
    let id = txn.create(
        NewElement::new(SYSTEM_CLASS)
            .name("Grid")
            .param(SYSTEM_KEY, json!({ "kind": "grid.system" })),
    )?;
    Ok(txn.store().get(id).expect("created").guid().to_string())
}

fn write(txn: &mut crate::txn::Transaction<'_>, id: ElementId, d: &GridData) -> Result<()> {
    let value = serde_json::to_value(d).expect("grid data serialises");
    txn.modify(id, |e| {
        e.params.insert(DATUM_KEY.into(), value);
    })
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// A straight line in plan, metres.
    pub curve: CurveArg,
    /// Omitted, the name continues the series of the last grid (`1` → `2`, `A` → `B`).
    pub name: Option<String>,
}

pub(crate) fn create(txn: &mut crate::txn::Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    check_curve(&p.curve.line)?;
    let store = txn.store().clone();
    let name = match &p.name {
        Some(n) => check_name(&store, n, None)?,
        None => auto_name(&store),
    };
    let system = system(txn)?;
    let data = GridData {
        kind: KIND_GRID.into(),
        system,
        curve: p.curve.line,
        bubbles: end_bubble(),
    };
    let guid = GlobalId::new();
    txn.create(
        NewElement::new(format!("{CLASS_PREFIX}{}", guid.as_str()))
            .guid(guid)
            .name(name)
            .param(
                DATUM_KEY,
                serde_json::to_value(&data).expect("grid data serialises"),
            ),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenameParams {
    pub grid_id: ElementRef,
    pub name: String,
}

pub(crate) fn rename(txn: &mut crate::txn::Transaction<'_>, p: RenameParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, _) = resolve_grid(&store, &p.grid_id)?;
    let name = check_name(&store, &p.name, Some(id))?;
    txn.modify(id, |e| e.name = Some(name))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetCurveParams {
    pub grid_id: ElementRef,
    pub curve: CurveArg,
}

pub(crate) fn set_curve(txn: &mut crate::txn::Transaction<'_>, p: SetCurveParams) -> Result<()> {
    check_curve(&p.curve.line)?;
    let (id, mut d) = resolve_grid(txn.store(), &p.grid_id)?;
    d.curve = p.curve.line;
    write(txn, id, &d)
}

/// Grid systems left without lines once `deleted` are gone (they go with their last line).
pub fn emptied_systems(
    store: &Store,
    deleted: &std::collections::BTreeSet<ElementId>,
) -> Vec<ElementId> {
    store
        .iter()
        .filter(|e| is_system(e) && !deleted.contains(&e.id()))
        .filter(|e| {
            let lines = lines_of(store, e.guid().as_str());
            !lines.is_empty() && lines.iter().all(|l| deleted.contains(l))
        })
        .map(Element::id)
        .collect()
}
