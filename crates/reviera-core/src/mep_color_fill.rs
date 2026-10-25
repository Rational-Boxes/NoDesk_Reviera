// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Duct and pipe colour fill legends (MEP-DUCT-010): a view shows a duct or pipe scheme
//! (chapter 5's colour schemes, categories `cat.ducts` and `cat.pipes`) by holding its legend,
//! a view annotation (`anno.colorlegend`) with the scheme and where it stands. One duct legend
//! and one pipe legend per view; deleting the legend removes the fill from that view only. The
//! fill is a view setting and changes no element.
//!
//! - **Parameters**: Flow (L/s), Velocity (m/s), Friction (Pa/m), Pressure Drop (Pa) from the
//!   kept network results (MEP-CALC-002), Size, System Type and System Name; for pipes also
//!   Fluid Temperature (°C, the system type's) and Slope (%). A run whose system type
//!   calculates nothing (*Calculations = None*) has no calculated value, nor does one never
//!   calculated: it takes the scheme's *No value* colour (none by default).
//! - Fittings and accessories are filled too unless the scheme says otherwise (*Include
//!   fittings*): with their own value, else the first connected segment's.

use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::annotations::{ANNO_KEY, CLASS_PREFIX, is_annotation};
use crate::color_schemes::{self, ColorScheme};
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::mep::Domain;
use crate::mep_systems::{self, Calculations};
use crate::store::Store;
use crate::txn::Transaction;

pub const KIND: &str = "anno.colorlegend";
/// The parameters offered for ducts, then those pipes add.
pub const DUCT_PARAMETERS: &[&str] = &["Flow", "Velocity", "Friction", "Pressure Drop", "Size", "System Type", "System Name"];
pub const PIPE_PARAMETERS: &[&str] = &["Fluid Temperature", "Slope"];

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// Ducts or pipes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Duct,
    Pipe,
}

impl Kind {
    pub fn category(self) -> &'static str {
        match self {
            Kind::Duct => "cat.ducts",
            Kind::Pipe => "cat.pipes",
        }
    }

    pub fn domain(self) -> Domain {
        match self {
            Kind::Duct => Domain::Duct,
            Kind::Pipe => Domain::Pipe,
        }
    }

    pub fn of_category(c: &str) -> Option<Kind> {
        match c {
            "cat.ducts" => Some(Kind::Duct),
            "cat.pipes" => Some(Kind::Pipe),
            _ => None,
        }
    }

    /// Whether a scheme of this kind may colour by `parameter`.
    pub fn offers(self, parameter: &str) -> bool {
        DUCT_PARAMETERS.contains(&parameter) || (self == Kind::Pipe && PIPE_PARAMETERS.contains(&parameter))
    }
}

/// A legend's stored data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LegendData {
    pub kind: String,
    /// The view (GlobalId).
    pub view: String,
    pub mep: Kind,
    /// The colour scheme's key.
    pub scheme: String,
    /// Where its top-left corner stands in the view (plan metres).
    pub point: [f64; 2],
}

pub fn legend_data(e: &Element) -> Option<LegendData> {
    if !is_annotation(e) {
        return None;
    }
    let d: LegendData = serde_json::from_value(e.param(ANNO_KEY)?.clone()).ok()?;
    (d.kind == KIND).then_some(d)
}

/// A view's legends.
pub fn legends(store: &Store, view: &Element) -> Vec<(ElementId, LegendData)> {
    let guid = view.guid().to_string();
    store.iter().filter_map(|e| Some((e.id(), legend_data(e).filter(|d| d.view == guid)?))).collect()
}

/// The duct or pipe segments (not placeholders) and, with `fittings`, fittings of a kind.
pub fn elements(store: &Store, kind: Kind, fittings: bool) -> Vec<ElementId> {
    let d = kind.domain();
    store
        .iter()
        .filter(|e| {
            crate::mep_runs::segment_recipe(e).is_some_and(|s| s.domain == d && !s.placeholder) || (fittings && crate::mep_runs::fitting_recipe(e).is_some_and(|f| f.domain == d))
        })
        .map(Element::id)
        .collect()
}

/// The system of each element's first port that has one.
fn system_of(store: &Store, e: &Element) -> Option<mep_systems::SystemInfo> {
    let ps = mep_systems::port_systems(store);
    let sys = (0..4).find_map(|k| ps.get(&(e.id(), k)).and_then(|v| v.first().copied()))?;
    mep_systems::systems(store).into_iter().find(|s| s.id == sys)
}

fn calculated(store: &Store, e: &Element) -> bool {
    system_of(store, e).and_then(|s| mep_systems::system_type(store, &s.system_type)).is_none_or(|t| t.calculations != Calculations::None)
}

/// A segment's or fitting's own value of a parameter: its text and number.
fn own_value(store: &Store, e: &Element, parameter: &str) -> Option<(String, Option<f64>)> {
    let kept = |f: &str| crate::mep_network::kept_value(e, f).and_then(Value::as_f64);
    let num = |v: f64, unit: &str, places: usize| Some((format!("{v:.places$} {unit}"), Some(v)));
    match parameter {
        "Flow" | "Velocity" | "Friction" | "Pressure Drop" if !calculated(store, e) => None,
        "Flow" => kept("flow").and_then(|v| num(v * 1000.0, "L/s", 1)),
        "Velocity" => kept("velocity").and_then(|v| num(v, "m/s", 2)),
        "Friction" => kept("friction").and_then(|v| num(v, "Pa/m", 2)),
        "Pressure Drop" => kept("loss").and_then(|v| num(v, "Pa", 1)),
        "Size" => {
            if let Some(s) = crate::mep_runs::segment_recipe(e) {
                return Some((s.size.text(), Some(s.size.nominal() * 1000.0)));
            }
            None
        }
        "System Type" => system_of(store, e).map(|s| (s.type_name, None)),
        "System Name" => system_of(store, e).map(|s| (s.name, None)),
        "Fluid Temperature" => {
            let s = system_of(store, e)?;
            let t = mep_systems::system_type(store, &s.system_type)?.fluid_temperature?;
            num(t, "°C", 1)
        }
        "Slope" => crate::mep_runs::segment_recipe(e).and_then(|s| num(s.slope * 100.0, "%", 2)),
        _ => None,
    }
}

/// A segment's or fitting's value of a parameter: a fitting's own, else its first connected
/// segment's.
pub fn value_of(store: &Store, e: &Element, parameter: &str) -> Option<(String, Option<f64>)> {
    if let Some(v) = own_value(store, e, parameter) {
        return Some(v);
    }
    crate::mep_runs::fitting_recipe(e)?;
    let partners = crate::mep::partners(store);
    let mut ns: Vec<ElementId> = (0..8).filter_map(|k| partners.get(&(e.id(), k))).flatten().map(|p| p.0).collect();
    ns.sort();
    ns.into_iter().filter_map(|n| store.get(n)).filter(|n| crate::mep_runs::segment_recipe(n).is_some()).find_map(|n| own_value(store, n, parameter))
}

/// Whether an element is a duct or pipe segment or fitting.
pub fn is_run_element(e: &Element) -> bool {
    crate::mep_runs::segment_recipe(e).is_some() || crate::mep_runs::fitting_recipe(e).is_some()
}

/// The colour of each filled element in a view: each legend's scheme applied to its kind; the
/// *No value* colour for elements without a value, when the scheme has one.
pub fn fills(store: &Store, view: &Element) -> HashMap<ElementId, [f32; 4]> {
    let mut out = HashMap::new();
    for (_, l) in legends(store, view) {
        let Some(s) = color_schemes::find(store, &l.scheme) else { continue };
        let rows = color_schemes::rows_now(store, &s);
        let none = s.no_value_color.as_deref().and_then(color_schemes::rgba);
        for id in elements(store, l.mep, s.include_fittings) {
            let c = match store.get(id).and_then(|e| value_of(store, e, &s.parameter)) {
                Some(_) => color_schemes::color_of(store, &s, &rows, id),
                None => none,
            };
            if let Some(c) = c {
                out.insert(id, c);
            }
        }
    }
    out
}

/// A scheme's default range rows: five equal bands over the values found now.
pub fn default_bands(store: &Store, s: &ColorScheme) -> Vec<f64> {
    let Some(kind) = Kind::of_category(&s.category) else { return Vec::new() };
    let vals: Vec<f64> = elements(store, kind, s.include_fittings).into_iter().filter_map(|id| store.get(id)).filter_map(|e| value_of(store, e, &s.parameter)?.1).collect();
    let (lo, hi) = vals.iter().fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
    if vals.is_empty() {
        return Vec::new();
    }
    if hi - lo < 1e-9 {
        return vec![lo];
    }
    (0..5).map(|k| lo + (hi - lo) * k as f64 / 5.0).collect()
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceParams {
    pub view_id: ElementRef,
    pub kind: Kind,
    /// A duct (or pipe) colour scheme's key or name.
    pub scheme_id: String,
    /// Where the legend's top-left corner goes (plan metres).
    pub point: [f64; 2],
}

/// `mep_color_legend.place`.
pub(crate) fn place(txn: &mut Transaction<'_>, p: PlaceParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let (vid, data) = crate::views::resolve_view(&store, &p.view_id)?;
    if data.level.is_none() {
        return Err(bad("view.not_plan", "duct and pipe legends go in plan views"));
    }
    let view = store.get(vid).expect("resolved");
    let s = color_schemes::find(&store, &p.scheme_id).ok_or_else(|| bad("colorscheme.not_found", "no such colour scheme"))?;
    if Kind::of_category(&s.category) != Some(p.kind) {
        return Err(bad("colorscheme.category", format!("a {} legend needs a scheme of {}", if p.kind == Kind::Duct { "duct" } else { "pipe" }, p.kind.category())));
    }
    if legends(&store, view).iter().any(|(_, l)| l.mep == p.kind) {
        return Err(bad("mep_color_legend.exists", format!("the view has a {} legend already; one per view", if p.kind == Kind::Duct { "duct" } else { "pipe" })));
    }
    if !p.point.iter().all(|v| v.is_finite()) {
        return Err(bad("mep_color_legend.point", "the point's coordinates are numbers"));
    }
    let d = LegendData { kind: KIND.into(), view: view.guid().to_string(), mep: p.kind, scheme: s.id, point: p.point };
    let guid = GlobalId::new();
    txn.create(
        NewElement::new(format!("{CLASS_PREFIX}{}/{}", view.guid().as_str(), guid.as_str()))
            .guid(guid)
            .param(ANNO_KEY, serde_json::to_value(&d).expect("serialises")),
    )
}

/// Moves a legend (Move tool).
pub(crate) fn transform(txn: &mut Transaction<'_>, id: ElementId, point: &dyn Fn([f64; 2]) -> [f64; 2]) -> Result<()> {
    let mut d = legend_data(txn.store().get(id).expect("exists")).expect("a legend");
    d.point = point(d.point);
    let v = serde_json::to_value(&d).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(ANNO_KEY.into(), v);
    })
}

/// The level (GlobalId) a segment or fitting is drawn from.
pub fn level_of(e: &Element) -> Option<String> {
    crate::mep_runs::segment_recipe(e).map(|s| s.level).or_else(|| crate::mep_runs::fitting_recipe(e).map(|f| f.level))
}

/// A segment's or fitting's plan footprint as quads (a fitting has one per leg); a vertical
/// piece is a square of its size; and the height to draw the fill at (its top).
pub fn footprint(e: &Element) -> (Vec<[[f64; 2]; 4]>, f64) {
    let quad = |a: [f64; 3], b: [f64; 3], w: f64| -> [[f64; 2]; 4] {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l = dx.hypot(dy);
        let h = w / 2.0;
        if l < 1e-6 {
            return [[a[0] - h, a[1] - h], [a[0] + h, a[1] - h], [a[0] + h, a[1] + h], [a[0] - h, a[1] + h]];
        }
        let n = [-dy / l * h, dx / l * h];
        [[a[0] + n[0], a[1] + n[1]], [b[0] + n[0], b[1] + n[1]], [b[0] - n[0], b[1] - n[1]], [a[0] - n[0], a[1] - n[1]]]
    };
    let across = |s: &crate::mep_runs::Size| match s.shape {
        crate::mep::Shape::Round => s.diameter,
        _ => s.width,
    };
    if let Some(s) = crate::mep_runs::segment_recipe(e) {
        let top = s.start[2].max(s.end[2]) + across(&s.size) / 2.0;
        return (vec![quad(s.start, s.end, across(&s.size))], top);
    }
    if let Some(f) = crate::mep_runs::fitting_recipe(e) {
        let c = f.centre;
        let quads = f
            .legs
            .iter()
            .map(|l| quad(c, [c[0] + l.direction[0] * l.length, c[1] + l.direction[1] * l.length, c[2] + l.direction[2] * l.length], across(&l.size)))
            .collect();
        let top = c[2] + f.legs.iter().map(|l| across(&l.size)).fold(0.0, f64::max) / 2.0;
        return (quads, top);
    }
    (Vec::new(), 0.0)
}
