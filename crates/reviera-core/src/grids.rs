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
//! Grid types (`IfcTypeObject` with a `recipe.grid_type`) set the symbol, the centre and end
//! segments' pens, colours and patterns, and the default bubbles; a grid refers to its type by
//! GlobalId in its record.
//!
//! Built so far: straight grids with `grid.create`, `grid.rename` and `grid.set_curve`, and grid
//! types. Arcs, multi-segment grids, per-view bubbles, scope boxes and datum extents come later.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

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
    /// The grid type (GlobalId); without one the grid draws in the default style.
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_guid: Option<String>,
    /// Bubbles at the start and at the end, overriding the type's defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bubbles: Option<[bool; 2]>,
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
    /// Bubbles at the start and at the end, as drawn (the type's defaults unless overridden;
    /// none when the type has no symbol).
    pub bubbles: [bool; 2],
    /// The grid system's GlobalId.
    pub system: String,
    /// The grid type, if any.
    pub type_id: Option<ElementId>,
    /// How the grid is drawn: its type's values, or the defaults.
    pub style: GridType,
}

/// The grid lines of a model, in creation order.
pub fn list(store: &Store) -> Vec<GridInfo> {
    let mut out: Vec<GridInfo> = store
        .iter()
        .filter_map(|e| {
            let d = grid_data(e)?;
            let type_id = d
                .type_guid
                .as_deref()
                .and_then(GlobalId::parse)
                .and_then(|g| store.find_by_guid(&g));
            let style = type_id
                .and_then(|t| store.get(t))
                .and_then(grid_type)
                .unwrap_or_default();
            let bubbles = match style.symbol {
                Symbol::None => [false, false],
                Symbol::Bubble => d.bubbles.unwrap_or(style.bubbles),
            };
            Some(GridInfo {
                id: e.id(),
                guid: e.guid().to_string(),
                name: e.name.clone().unwrap_or_default(),
                curve: d.curve,
                bubbles,
                system: d.system,
                type_id,
                style,
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

// ---- Grid types ----

/// Recipe kind of grid types (`rxd:recipe` on an `IfcTypeObject`).
pub const TYPE_RECIPE: &str = "recipe.grid_type";
pub const TYPE_CLASS: &str = "IfcTypeObject";
pub const DEFAULT_TYPE_NAME: &str = "Grid - Continuous";

/// The head at a grid's ends.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Symbol {
    /// A circle with the grid's name.
    #[default]
    Bubble,
    None,
}

/// How the middle of a grid line is drawn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CenterSegment {
    /// The whole line in the end segment's style.
    #[default]
    Continuous,
    /// Only the end segments are drawn.
    None,
    /// The middle in the centre segment's own style.
    Custom,
}

/// The pen, colour and pattern of a part of a grid line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SegmentStyle {
    /// Annotation pen, 1 to 16.
    pub pen: u8,
    /// `#RRGGBB`.
    pub colour: String,
    /// A line pattern: `solid`, `dash`, `center`, `dash_dot` or `dot`.
    pub pattern: String,
}

impl SegmentStyle {
    fn new(pen: u8, pattern: &str) -> Self {
        Self {
            pen,
            colour: "#000000".into(),
            pattern: pattern.into(),
        }
    }
}

/// The line patterns grids can use until line patterns are project standards (DOC-GFX-013).
pub const PATTERNS: &[&str] = &["solid", "dash", "center", "dash_dot", "dot"];

/// A grid type's values (ARC-GRID-001 type parameters).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GridType {
    pub kind: String,
    pub symbol: Symbol,
    pub center_segment: CenterSegment,
    /// The middle of the line, when `center_segment` is `custom`.
    pub center: SegmentStyle,
    /// The ends of the line (the whole line when `continuous`).
    pub end: SegmentStyle,
    /// Length of each end segment on paper, millimetres (`none` and `custom`).
    pub end_length_mm: f64,
    /// Bubbles at the start and at the end of new grids, in plan views.
    pub bubbles: [bool; 2],
}

impl Default for GridType {
    fn default() -> Self {
        Self {
            kind: TYPE_RECIPE.into(),
            symbol: Symbol::Bubble,
            center_segment: CenterSegment::Continuous,
            center: SegmentStyle::new(1, "center"),
            end: SegmentStyle::new(1, "solid"),
            end_length_mm: 15.0,
            bubbles: [false, true],
        }
    }
}

/// The values of a grid type element.
pub fn grid_type(e: &Element) -> Option<GridType> {
    let v = e.param(crate::walls::RECIPE_KEY)?;
    (v.get("kind")? == TYPE_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

pub fn is_grid_type(e: &Element) -> bool {
    grid_type(e).is_some()
}

/// One grid type, for listings.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct GridTypeInfo {
    pub id: ElementId,
    pub name: String,
    pub values: GridType,
    /// How many grids use it.
    pub instances: usize,
}

/// The grid types of a model, by name.
pub fn types(store: &Store) -> Vec<GridTypeInfo> {
    let grids = list(store);
    let mut out: Vec<GridTypeInfo> = store
        .iter()
        .filter_map(|e| {
            Some(GridTypeInfo {
                id: e.id(),
                name: e.name.clone().unwrap_or_default(),
                values: grid_type(e)?,
                instances: grids.iter().filter(|g| g.type_id == Some(e.id())).count(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The project's default grid type, created when first needed.
fn default_type(txn: &mut crate::txn::Transaction<'_>) -> Result<ElementId> {
    if let Some(e) = txn
        .store()
        .iter()
        .find(|e| is_grid_type(e) && e.name.as_deref() == Some(DEFAULT_TYPE_NAME))
    {
        return Ok(e.id());
    }
    // Another grid type will do when the default was renamed.
    if let Some(t) = types(txn.store()).first() {
        return Ok(t.id);
    }
    txn.create(NewElement::new(TYPE_CLASS).name(DEFAULT_TYPE_NAME).param(
        crate::walls::RECIPE_KEY,
        serde_json::to_value(GridType::default()).expect("serialises"),
    ))
}

fn resolve_type(store: &Store, r: &ElementRef) -> Result<ElementId> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such grid type"))?;
    if !store.get(id).is_some_and(is_grid_type) {
        return Err(Error::new(
            "grid.not_a_grid_type",
            format!("element {id} is not a grid type"),
        ));
    }
    Ok(id)
}

fn colour(v: &Value) -> Result<String> {
    let s = v.as_str().unwrap_or_default().trim();
    let ok = s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit());
    if !ok {
        return Err(Error::new(
            "type.invalid_value",
            format!("{v} is not a colour (#RRGGBB)"),
        ));
    }
    Ok(s.to_ascii_uppercase())
}

fn pen(v: &Value) -> Result<u8> {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        .filter(|p| (1..=crate::standards::PENS as u64).contains(p))
        .map(|p| p as u8)
        .ok_or_else(|| Error::new("type.invalid_value", format!("pens are 1 to 16, not {v}")))
}

fn pattern(v: &Value) -> Result<String> {
    let s = v.as_str().unwrap_or_default().trim().to_ascii_lowercase();
    if PATTERNS.contains(&s.as_str()) {
        Ok(s)
    } else {
        Err(Error::new(
            "type.invalid_value",
            format!("{v} is not a line pattern ({})", PATTERNS.join(", ")),
        ))
    }
}

fn choice<T: serde::de::DeserializeOwned>(v: &Value, what: &str) -> Result<T> {
    let s = v.as_str().unwrap_or_default().trim().to_ascii_lowercase();
    serde_json::from_value(Value::String(s))
        .map_err(|_| Error::new("type.invalid_value", format!("{v} is not a {what}")))
}

fn flag(v: &Value) -> Result<bool> {
    v.as_bool()
        .or_else(|| match v.as_str()?.trim().to_ascii_lowercase().as_str() {
            "true" | "yes" | "1" => Some(true),
            "false" | "no" | "0" => Some(false),
            _ => None,
        })
        .ok_or_else(|| Error::new("type.invalid_value", format!("{v} is not yes or no")))
}

/// The parameters of a grid type for `type.set_param`.
pub const TYPE_PARAMS: &[&str] = &[
    "symbol",
    "center_segment",
    "center_pen",
    "center_colour",
    "center_pattern",
    "end_pen",
    "end_colour",
    "end_pattern",
    "end_length",
    "bubble_start",
    "bubble_end",
];

/// Sets one value of a grid type (`type.set_param`). `end_length` is a paper length: a number
/// of millimetres, or a length with units.
pub(crate) fn set_type_param(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    param: &str,
    value: &Value,
) -> Result<()> {
    let e = txn.store().get(id).expect("resolved");
    let mut t = grid_type(e).expect("a grid type");
    match param {
        "symbol" => t.symbol = choice(value, "symbol (bubble, none)")?,
        "center_segment" => {
            t.center_segment = choice(value, "centre segment (continuous, none, custom)")?;
        }
        "center_pen" => t.center.pen = pen(value)?,
        "center_colour" => t.center.colour = colour(value)?,
        "center_pattern" => t.center.pattern = pattern(value)?,
        "end_pen" => t.end.pen = pen(value)?,
        "end_colour" => t.end.colour = colour(value)?,
        "end_pattern" => t.end.pattern = pattern(value)?,
        "end_length" => {
            let mm = match value {
                Value::Number(n) => n.as_f64().unwrap_or(-1.0),
                Value::String(s) => match s.trim().parse::<f64>() {
                    Ok(n) => n,
                    Err(_) => {
                        crate::units::parse(s, "length", &crate::units::settings_of(txn.store()))?
                            * 1000.0
                    }
                },
                _ => -1.0,
            };
            if !(mm > 0.0 && mm.is_finite()) {
                return Err(Error::new(
                    "type.invalid_value",
                    "the end segment length must be greater than 0",
                ));
            }
            t.end_length_mm = mm;
        }
        "bubble_start" => t.bubbles[0] = flag(value)?,
        "bubble_end" => t.bubbles[1] = flag(value)?,
        other => {
            return Err(Error::new(
                "type.unknown_param",
                format!(
                    "grid types have no parameter `{other}` ({})",
                    TYPE_PARAMS.join(", ")
                ),
            ));
        }
    }
    let value = serde_json::to_value(&t).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(crate::walls::RECIPE_KEY.into(), value);
    })
}

/// Gives grids another grid type (`element.set_type`); their bubbles follow the new type.
pub(crate) fn set_type(
    txn: &mut crate::txn::Transaction<'_>,
    grids: &[ElementId],
    type_id: ElementId,
) -> Result<()> {
    let store = txn.store().clone();
    let t = resolve_type(&store, &ElementRef::Id(type_id))?;
    let guid = store.get(t).expect("resolved").guid().to_string();
    for id in grids {
        let mut d = grid_data(store.get(*id).expect("checked by the caller")).expect("a grid");
        d.type_guid = Some(guid.clone());
        d.bubbles = None;
        write(txn, *id, &d)?;
    }
    Ok(())
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// A straight line in plan, metres.
    pub curve: CurveArg,
    /// Omitted, the name continues the series of the last grid (`1` → `2`, `A` → `B`).
    pub name: Option<String>,
    /// A grid type; omitted, the project's default grid type (created on first use).
    pub type_id: Option<ElementRef>,
}

pub(crate) fn create(txn: &mut crate::txn::Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    check_curve(&p.curve.line)?;
    let store = txn.store().clone();
    let name = match &p.name {
        Some(n) => check_name(&store, n, None)?,
        None => auto_name(&store),
    };
    let type_id = match &p.type_id {
        Some(r) => resolve_type(&store, r)?,
        None => default_type(txn)?,
    };
    let type_guid = txn.store().get(type_id).expect("exists").guid().to_string();
    let system = system(txn)?;
    let data = GridData {
        kind: KIND_GRID.into(),
        system,
        curve: p.curve.line,
        type_guid: Some(type_guid),
        bubbles: None,
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
