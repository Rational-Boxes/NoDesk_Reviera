// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Colour schemes (spec Ch 3 §3.9.4, ARC-CLR-001): rooms (and areas) coloured in plans by the
//! value of one of their parameters.
//!
//! The schemes are kept on the project (`rxd:color_schemes`): each has a category, a title, the
//! parameter it colours by, *by value* or *by range*, and its rows. By value, every value found
//! among the category's elements has a row: stored rows keep their colour and visibility, and a
//! value with no row yet gets the next colour of a pastel palette (golden-ratio hue steps at
//! saturation 0.35 and value 0.95), so a new value shows at once. By range, an element falls in
//! the row with the largest *At least* not above its value. A plan shows a scheme when its view
//! names it (`view.set_color_scheme`).
//!
//! Parameters: `Name`, `Number`, `Area` (m²), `Perimeter` (m), or any parameter of the
//! category (project parameters too), by its name.
//!
//! Not built yet: fill patterns, the legend as a placed annotation, Spaces, HVAC zones, ducts and
//! pipes, sections, linked models, colour fills in IFC.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;

pub const KEY: &str = "rxd:color_schemes";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Value,
    Range,
}

/// A stored row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Row {
    /// By value: the value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// By range: the row's lower bound (SI: m², m, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_least: Option<f64>,
    /// `#rrggbb`.
    pub color: String,
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ColorScheme {
    /// A GlobalId-like key.
    pub id: String,
    pub name: String,
    /// `cat.rooms` or `cat.areas`.
    pub category: String,
    #[serde(default)]
    pub title: String,
    pub parameter: String,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default)]
    pub rows: Vec<Row>,
}

/// A row as it applies now.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct RowNow {
    pub value: Option<String>,
    pub at_least: Option<f64>,
    /// Up to the next row's `at_least` (ranges).
    pub less_than: Option<f64>,
    pub color: String,
    pub visible: bool,
    pub caption: String,
    /// An element of the category has this value (or falls in this range).
    pub in_use: bool,
    /// Stored (false: found among the elements, coloured from the palette).
    pub stored: bool,
}

fn project(store: &Store) -> Option<&Element> {
    store.iter().find(|e| e.class == "IfcProject")
}

/// Every colour scheme of the model.
pub fn all(store: &Store) -> Vec<ColorScheme> {
    project(store)
        .and_then(|p| p.param(KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

pub fn find(store: &Store, id: &str) -> Option<ColorScheme> {
    all(store).into_iter().find(|s| s.id == id || s.name == id)
}

fn write(txn: &mut Transaction<'_>, list: &[ColorScheme]) -> Result<()> {
    let p = project(txn.store())
        .map(Element::id)
        .ok_or_else(|| Error::new("project.missing", "the model has no project"))?;
    let v = serde_json::to_value(list).expect("serialises");
    txn.modify(p, |e| {
        e.params.insert(KEY.into(), v);
    })
}

/// The `k`th palette colour.
pub fn palette(k: usize) -> String {
    const GOLDEN: f64 = 0.618_033_988_749_895;
    let h = (k as f64 * GOLDEN + 0.1).fract() * 6.0;
    let (s, v) = (0.35, 0.95);
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let to = |u: f64| ((u + m) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", to(r), to(g), to(b))
}

/// `#rrggbb` as linear-ish RGBA (0–1).
pub fn rgba(hex: &str) -> Option<[f32; 4]> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let c = |i: usize| {
        u8::from_str_radix(&h[i..i + 2], 16)
            .ok()
            .map(|v| f32::from(v) / 255.0)
    };
    Some([c(0)?, c(2)?, c(4)?, 1.0])
}

/// The elements a scheme colours: rooms (or areas) that regenerate.
fn elements(store: &Store, category: &str) -> Vec<ElementId> {
    match category {
        "cat.rooms" => crate::rooms::all(store).into_iter().map(|r| r.id).collect(),
        "cat.areas" => crate::areas::all(store).into_iter().map(|a| a.id).collect(),
        _ => Vec::new(),
    }
}

/// An element's value of a parameter: its text, and its number for ranges.
pub fn value_of(store: &Store, id: ElementId, parameter: &str) -> Option<(String, Option<f64>)> {
    let e = store.get(id)?;
    let region_area = |id: ElementId| -> Option<(f64, f64)> {
        if let Some(r) = crate::rooms::all(store).into_iter().find(|r| r.id == id) {
            return Some((r.geometry.area(), r.geometry.perimeter()));
        }
        crate::areas::all(store)
            .into_iter()
            .find(|a| a.id == id)
            .map(|a| (a.area, a.perimeter))
    };
    match parameter {
        "Name" | "Number" => {
            let (number, name) = crate::rooms::number_and_name(e);
            let v = if parameter == "Name" { name } else { number };
            Some((v, None))
        }
        "Area" | "Perimeter" => {
            let (a, p) = region_area(id)?;
            let v = if parameter == "Area" { a } else { p };
            let unit = if parameter == "Area" { "m²" } else { "m" };
            Some((format!("{v:.2} {unit}"), Some(v)))
        }
        _ => {
            let p = crate::parameters::get(store, id, parameter).ok()?;
            let number = p.value.as_f64();
            let text = p.display.or_else(|| p.value.as_str().map(str::to_owned))?;
            Some((text, number))
        }
    }
}

/// A scheme's rows as they apply now: stored rows, then values found with no row yet.
pub fn rows_now(store: &Store, s: &ColorScheme) -> Vec<RowNow> {
    let values: Vec<(String, Option<f64>)> = elements(store, &s.category)
        .into_iter()
        .filter_map(|id| value_of(store, id, &s.parameter))
        .collect();
    match s.mode {
        Mode::Value => {
            let mut out: Vec<RowNow> = s
                .rows
                .iter()
                .filter_map(|r| {
                    let v = r.value.clone()?;
                    Some(RowNow {
                        in_use: values.iter().any(|x| x.0 == v),
                        caption: r.caption.clone().unwrap_or_else(|| v.clone()),
                        value: Some(v),
                        at_least: None,
                        less_than: None,
                        color: r.color.clone(),
                        visible: r.visible,
                        stored: true,
                    })
                })
                .collect();
            for (v, _) in values {
                if !out.iter().any(|r| r.value.as_deref() == Some(v.as_str())) {
                    out.push(RowNow {
                        caption: v.clone(),
                        value: Some(v),
                        at_least: None,
                        less_than: None,
                        color: palette(out.len()),
                        visible: true,
                        in_use: true,
                        stored: false,
                    });
                }
            }
            out
        }
        Mode::Range => {
            let mut rows: Vec<&Row> = s.rows.iter().filter(|r| r.at_least.is_some()).collect();
            rows.sort_by(|a, b| {
                a.at_least
                    .unwrap_or(0.0)
                    .total_cmp(&b.at_least.unwrap_or(0.0))
            });
            (0..rows.len())
                .map(|k| {
                    let lo = rows[k].at_least.expect("filtered");
                    let hi = rows.get(k + 1).and_then(|r| r.at_least);
                    let inside = |v: f64| v >= lo - 1e-9 && hi.is_none_or(|h| v < h - 1e-9);
                    RowNow {
                        value: None,
                        at_least: Some(lo),
                        less_than: hi,
                        color: rows[k].color.clone(),
                        visible: rows[k].visible,
                        caption: rows[k].caption.clone().unwrap_or_else(|| match hi {
                            Some(h) => format!("{lo} to {h}"),
                            None => format!("{lo} or more"),
                        }),
                        in_use: values.iter().any(|x| x.1.is_some_and(inside)),
                        stored: true,
                    }
                })
                .collect()
        }
    }
}

/// The colour an element takes in a scheme, if it is in a visible row.
pub fn color_of(
    store: &Store,
    s: &ColorScheme,
    rows: &[RowNow],
    id: ElementId,
) -> Option<[f32; 4]> {
    let (text, number) = value_of(store, id, &s.parameter)?;
    let row = match s.mode {
        Mode::Value => rows
            .iter()
            .find(|r| r.value.as_deref() == Some(text.as_str()))?,
        Mode::Range => {
            let v = number?;
            rows.iter()
                .rfind(|r| r.at_least.is_some_and(|lo| v >= lo - 1e-9))?
        }
    };
    if !row.visible {
        return None;
    }
    rgba(&row.color)
}

/// A coloured element: its outline rings (outer first) and its colour.
pub type Fill = (ElementId, Vec<Vec<[f64; 2]>>, [f32; 4]);

/// The fills of a view's scheme on a level: each coloured room's (or area's) outline rings and
/// its colour.
pub fn fills(store: &Store, view: &Element) -> Vec<Fill> {
    let Some(data) = crate::views::view_data(view) else {
        return Vec::new();
    };
    let Some(s) = data.color_scheme.as_deref().and_then(|id| find(store, id)) else {
        return Vec::new();
    };
    let level = data.level.unwrap_or_default();
    let rows = rows_now(store, &s);
    let ring = |r: &crate::arrangement::Region| -> Vec<Vec<[f64; 2]>> {
        std::iter::once(&r.outer)
            .chain(&r.holes)
            .map(|lp| lp.iter().map(|e| e.line.start).collect())
            .collect()
    };
    match s.category.as_str() {
        "cat.rooms" => crate::rooms::all(store)
            .into_iter()
            .filter(|r| r.level == level && !r.redundant)
            .filter_map(|r| {
                let region = r.geometry.region.as_ref()?;
                Some((r.id, ring(region), color_of(store, &s, &rows, r.id)?))
            })
            .collect(),
        "cat.areas" => crate::areas::all(store)
            .into_iter()
            .filter(|a| a.level == level && data.area_scheme.as_deref() == Some(a.scheme.as_str()))
            .filter_map(|a| {
                let region = a.region.as_ref()?;
                Some((a.id, ring(region), color_of(store, &s, &rows, a.id)?))
            })
            .collect(),
        _ => Vec::new(),
    }
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub name: String,
    /// `cat.rooms` (default) or `cat.areas`.
    pub category: Option<String>,
    /// The parameter to colour by: Name, Number, Area, Perimeter or a parameter's name.
    pub parameter: String,
    #[serde(default)]
    pub mode: Mode,
    /// By range: the rows' lower bounds (SI).
    #[serde(default)]
    pub ranges: Vec<f64>,
    pub title: Option<String>,
}

/// `colorscheme.create`: its key.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<String> {
    let mut list = all(txn.store());
    let name = p.name.trim().to_owned();
    if name.is_empty() || list.iter().any(|s| s.name == name) {
        return Err(Error::new(
            "colorscheme.name",
            "a colour scheme needs a name of its own",
        ));
    }
    let category = p.category.unwrap_or_else(|| "cat.rooms".into());
    if category != "cat.rooms" && category != "cat.areas" {
        return Err(Error::new(
            "colorscheme.category",
            "colour schemes colour rooms (cat.rooms) or areas (cat.areas) for now",
        ));
    }
    let mut ranges = p.ranges.clone();
    ranges.sort_by(f64::total_cmp);
    let id = GlobalId::new().to_string();
    list.push(ColorScheme {
        id: id.clone(),
        title: p.title.unwrap_or_else(|| format!("{} Legend", name)),
        name,
        category,
        parameter: p.parameter,
        mode: p.mode,
        rows: ranges
            .iter()
            .enumerate()
            .map(|(k, lo)| Row {
                value: None,
                at_least: Some(*lo),
                color: palette(k),
                visible: true,
                caption: None,
            })
            .collect(),
    });
    write(txn, &list)?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetRowsParams {
    /// The scheme's key or name.
    pub scheme_id: String,
    pub rows: Vec<Row>,
    pub title: Option<String>,
    pub parameter: Option<String>,
    pub mode: Option<Mode>,
}

/// `colorscheme.set_rows`.
pub(crate) fn set_rows(txn: &mut Transaction<'_>, p: SetRowsParams) -> Result<()> {
    let mut list = all(txn.store());
    let s = list
        .iter_mut()
        .find(|s| s.id == p.scheme_id || s.name == p.scheme_id)
        .ok_or_else(|| Error::new("colorscheme.not_found", "no such colour scheme"))?;
    for r in &p.rows {
        if rgba(&r.color).is_none() {
            return Err(Error::new(
                "colorscheme.color",
                format!("`{}` is not a #rrggbb colour", r.color),
            ));
        }
    }
    s.rows = p.rows;
    if let Some(t) = p.title {
        s.title = t;
    }
    if let Some(v) = p.parameter {
        s.parameter = v;
    }
    if let Some(m) = p.mode {
        s.mode = m;
    }
    write(txn, &list)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DuplicateParams {
    pub scheme_id: String,
    pub name: String,
}

/// `colorscheme.duplicate`: its key.
pub(crate) fn duplicate(txn: &mut Transaction<'_>, p: DuplicateParams) -> Result<String> {
    let mut list = all(txn.store());
    let mut s = list
        .iter()
        .find(|s| s.id == p.scheme_id || s.name == p.scheme_id)
        .cloned()
        .ok_or_else(|| Error::new("colorscheme.not_found", "no such colour scheme"))?;
    if list.iter().any(|x| x.name == p.name.trim()) {
        return Err(Error::new("colorscheme.name", "that name is taken"));
    }
    s.id = GlobalId::new().to_string();
    s.name = p.name.trim().to_owned();
    let id = s.id.clone();
    list.push(s);
    write(txn, &list)?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteParams {
    pub scheme_id: String,
}

/// `colorscheme.delete`; views showing it show none.
pub(crate) fn delete(txn: &mut Transaction<'_>, p: DeleteParams) -> Result<()> {
    let mut list = all(txn.store());
    let before = list.len();
    let gone: Vec<String> = list
        .iter()
        .filter(|s| s.id == p.scheme_id || s.name == p.scheme_id)
        .map(|s| s.id.clone())
        .collect();
    list.retain(|s| !gone.contains(&s.id));
    if list.len() == before {
        return Err(Error::new("colorscheme.not_found", "no such colour scheme"));
    }
    write(txn, &list)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LegendParams {
    pub scheme_id: String,
}

/// `colorscheme.legend`: the rows as they apply now.
pub(crate) fn legend(store: &Store, p: LegendParams) -> Result<Vec<RowNow>> {
    let s = find(store, &p.scheme_id)
        .ok_or_else(|| Error::new("colorscheme.not_found", "no such colour scheme"))?;
    Ok(rows_now(store, &s))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetViewParams {
    pub view_id: ElementRef,
    /// The scheme's key or name; omitted or null clears it.
    pub scheme_id: Option<String>,
}

/// `view.set_color_scheme`.
pub(crate) fn set_view(txn: &mut Transaction<'_>, p: SetViewParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut data) = crate::views::resolve_view(&store, &p.view_id)?;
    if data.level.is_none() {
        return Err(Error::new(
            "view.not_plan",
            "colour schemes show in plan views",
        ));
    }
    data.color_scheme = match p.scheme_id {
        Some(s) => Some(
            find(&store, &s)
                .ok_or_else(|| Error::new("colorscheme.not_found", "no such colour scheme"))?
                .id,
        ),
        None => None,
    };
    crate::views::write_data(txn, id, &data)
}
