// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Results Explorer and Results Manager (STR-RES-001): how a structural analysis's results are
//! shown in model views and managed.
//!
//! A view shows the results of one load case or combination as an overlay, never changing the
//! model's geometry: member force diagrams (N, Vy, Vz, T, My, Mz) drawn off each member in its
//! local plane, the deformed shape (deflections along members from their moment diagrams, so a
//! loaded span sags between its supports), or reaction arrows at the supports. The view keeps
//! what it shows (`rxd:results_display`). Presets name common displays (column axial forces,
//! support reactions, beam moments with deformation …); users save their own on the project
//! (`rxd:results_presets`). Extremes ("Insights") find the largest and smallest values and
//! where they are. The Results Manager lists the kept result set and deletes it.

use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::structural_model::{RESULTS_KEY, Stored};

/// A view's results display.
pub const DISPLAY_KEY: &str = "rxd:results_display";
/// The project's user presets.
pub const PRESETS_KEY: &str = "rxd:results_presets";

type V3 = [f64; 3];

/// What is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResultType {
    /// N, tension positive.
    Axial,
    ShearY,
    ShearZ,
    /// T (Mx).
    Torsion,
    MomentY,
    MomentZ,
    /// Displaced members.
    Deformed,
    /// Arrows at the supports.
    Reactions,
}

impl ResultType {
    /// The member force index (N, Vy, Vz, T, My, Mz), for force diagrams.
    fn force(self) -> Option<usize> {
        match self {
            ResultType::Axial => Some(0),
            ResultType::ShearY => Some(1),
            ResultType::ShearZ => Some(2),
            ResultType::Torsion => Some(3),
            ResultType::MomentY => Some(4),
            ResultType::MomentZ => Some(5),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            ResultType::Axial => "Axial force N",
            ResultType::ShearY => "Shear Vy",
            ResultType::ShearZ => "Shear Vz",
            ResultType::Torsion => "Torsion T",
            ResultType::MomentY => "Moment My",
            ResultType::MomentZ => "Moment Mz",
            ResultType::Deformed => "Deformed shape",
            ResultType::Reactions => "Reactions",
        }
    }
    /// Values shown in kN or kN·m; displacements in mm.
    fn unit(self) -> (&'static str, f64) {
        match self {
            ResultType::Axial | ResultType::ShearY | ResultType::ShearZ | ResultType::Reactions => {
                ("kN", 1e-3)
            }
            ResultType::Torsion | ResultType::MomentY | ResultType::MomentZ => ("kN·m", 1e-3),
            ResultType::Deformed => ("mm", 1e3),
        }
    }
}

/// Which members are shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Filter {
    #[default]
    All,
    Columns,
    Beams,
    Braces,
    /// Beams that a column stands on.
    TransferBeams,
}

fn one() -> f64 {
    1.0
}

/// A results display.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Display {
    /// A load case's or combination's name; empty: the first.
    #[serde(default)]
    pub load_group: String,
    pub result_type: ResultType,
    /// A multiplier on the automatic scale: the largest value drawn 8% of the model's size long
    /// (1.5 to 6 m), the largest displacement 5% (1 to 4 m).
    #[serde(default = "one")]
    pub scale: f64,
    /// Draws the deformed shape too, under a force diagram.
    #[serde(default)]
    pub deformation: bool,
    #[serde(default)]
    pub filter: Filter,
    /// Values at each member's extremes.
    #[serde(default = "yes")]
    pub labels: bool,
    /// The preset it came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
}

fn yes() -> bool {
    true
}

// ---- Presets ----

/// A preset: its id, name, display, and whether it can be shown (panel results cannot yet).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub display: Option<Display>,
    #[serde(default)]
    pub user: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<String>,
}

fn builtin(id: &str, name: &str, t: ResultType, filter: Filter, deformation: bool) -> Preset {
    Preset {
        id: id.into(),
        name: name.into(),
        display: Some(Display {
            load_group: String::new(),
            result_type: t,
            scale: 1.0,
            deformation,
            filter,
            labels: true,
            preset: Some(id.into()),
        }),
        user: false,
        unavailable: None,
    }
}

fn panel_only(id: &str, name: &str) -> Preset {
    Preset {
        id: id.into(),
        name: name.into(),
        display: None,
        user: false,
        unavailable: Some(
            "needs panel results: the built-in solver analyses members only (STR-AM-002 panels \
             go to providers that analyse surfaces)"
                .into(),
        ),
    }
}

/// The built-in presets.
pub fn builtin_presets() -> Vec<Preset> {
    use Filter::*;
    use ResultType::*;
    vec![
        builtin("column_axial", "Column axial forces", Axial, Columns, false),
        builtin(
            "column_moments",
            "Column bending moments",
            MomentY,
            Columns,
            false,
        ),
        builtin("column_shear", "Column shear", ShearZ, Columns, false),
        builtin(
            "support_reactions",
            "Support reactions",
            Reactions,
            All,
            false,
        ),
        builtin(
            "beam_moments_deformation",
            "Beam moments with deformation",
            MomentY,
            Beams,
            true,
        ),
        builtin("beam_shear", "Beam shear", ShearZ, Beams, false),
        builtin("beam_torsion", "Beam torsion", Torsion, Beams, false),
        builtin(
            "transfer_beam_forces",
            "Transfer beam forces",
            MomentY,
            TransferBeams,
            false,
        ),
        builtin("brace_axial", "Brace axial forces", Axial, Braces, false),
        builtin("deformed_shape", "Deformed shape", Deformed, All, false),
        panel_only("wall_in_plane_x", "Wall in-plane forces X"),
        panel_only("wall_in_plane_y", "Wall in-plane forces Y"),
        panel_only("wall_deflection", "Wall deflection"),
        panel_only("floor_moments_x", "Floor moments with deformation X"),
        panel_only("floor_moments_y", "Floor moments with deformation Y"),
        panel_only("floor_shear", "Floor shear"),
        panel_only("slab_twisting", "Slab twisting moments"),
    ]
}

fn project(store: &Store) -> Option<&Element> {
    store.iter().find(|e| e.class == "IfcProject")
}

fn user_presets(store: &Store) -> Vec<Preset> {
    project(store)
        .and_then(|p| p.param(PRESETS_KEY))
        .and_then(|v| serde_json::from_value::<Vec<Preset>>(v.clone()).ok())
        .unwrap_or_default()
}

/// Every preset: the built-in ones, then the project's (`results.presets`).
pub fn presets(store: &Store) -> Vec<Preset> {
    let mut out = builtin_presets();
    out.extend(user_presets(store));
    out
}

// ---- The model's results, joined to its analytical members ----

/// The kept results, or an error saying how to get some.
fn stored(store: &Store) -> Result<Stored> {
    crate::structural_model::stored(store).ok_or_else(|| {
        Error::new(
            "results.none",
            "no analysis results are kept with the model: run Analyze › Run Analysis first",
        )
    })
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

/// The case or combination by name (the first when empty).
fn group<'a>(s: &'a Stored, name: &str) -> Result<&'a crate::frame::CaseResult> {
    if name.is_empty() {
        return s
            .results
            .results
            .first()
            .ok_or_else(|| Error::new("results.none", "the result set has no load cases"));
    }
    s.results
        .results
        .iter()
        .find(|r| r.name == name)
        .ok_or_else(|| {
            let names: Vec<&str> = s.results.results.iter().map(|r| r.name.as_str()).collect();
            Error::new(
                "results.no_load_group",
                format!(
                    "no load case or combination \"{name}\" (there are: {})",
                    names.join(", ")
                ),
            )
        })
}

/// A member's local axes as the solver takes them (STR-AM-001: z toward up for members that run
/// across, toward x for vertical ones).
pub(crate) fn axes(a: V3, b: V3) -> (f64, [V3; 3]) {
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-12);
    let x = d.map(|v| v / l);
    let hint = if d[0].hypot(d[1]) < 1e-6 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    };
    let cross = |u: V3, v: V3| {
        [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ]
    };
    let y = cross(hint, x);
    let ly = (y[0] * y[0] + y[1] * y[1] + y[2] * y[2]).sqrt().max(1e-12);
    let y = y.map(|v| v / ly);
    let z = cross(x, y);
    (l, [x, y, z])
}

fn add(a: V3, b: V3, s: f64) -> V3 {
    [a[0] + b[0] * s, a[1] + b[1] * s, a[2] + b[2] * s]
}

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// One member's results joined to the model.
struct Shown {
    id: ElementId,
    physical: Vec<ElementId>,
    start: V3,
    end: V3,
    stations: Vec<crate::frame::Station>,
    nodes: [String; 2],
}

/// Whether a member passes the filter.
fn passes(store: &Store, filter: Filter, role: crate::analytical::Role, id: ElementId) -> bool {
    use crate::analytical::Role;
    match filter {
        Filter::All => true,
        Filter::Columns => role == Role::Column,
        Filter::Beams => role == Role::Beam,
        Filter::Braces => matches!(role, Role::Brace | Role::Tie),
        Filter::TransferBeams => {
            if role != Role::Beam {
                return false;
            }
            // A column's lower end on the beam's span.
            let ms = crate::analytical::members(store);
            let Some((_, beam)) = ms.iter().find(|(m, _)| *m == id) else {
                return false;
            };
            ms.iter().any(|(_, c)| {
                c.recipe.role == Role::Column && {
                    let foot = if c.start[2] <= c.end[2] {
                        c.nodes[0]
                    } else {
                        c.nodes[1]
                    };
                    foot != beam.nodes[0] && foot != beam.nodes[1] && {
                        let p = if c.start[2] <= c.end[2] {
                            c.start
                        } else {
                            c.end
                        };
                        let (l, ax) = axes(beam.start, beam.end);
                        let r = [
                            p[0] - beam.start[0],
                            p[1] - beam.start[1],
                            p[2] - beam.start[2],
                        ];
                        let t = dot(r, ax[0]);
                        let off = add(r, ax[0], -t);
                        t > 0.0 && t < l && dot(off, off).sqrt() < 0.05
                    }
                }
            })
        }
    }
}

fn shown(store: &Store, r: &crate::frame::CaseResult, filter: Filter) -> Vec<Shown> {
    let ms: HashMap<ElementId, crate::analytical::MemberGeometry> =
        crate::analytical::members(store).into_iter().collect();
    let guid_of = |id: ElementId| {
        store
            .get(id)
            .map(|e| e.guid().to_string())
            .unwrap_or_default()
    };
    r.members
        .iter()
        .filter_map(|m| {
            let id = find(store, &m.key)?;
            let g = ms.get(&id)?;
            if !passes(store, filter, g.recipe.role, id) {
                return None;
            }
            Some(Shown {
                id,
                physical: g
                    .recipe
                    .physical
                    .iter()
                    .filter_map(|p| find(store, p))
                    .collect(),
                start: g.start,
                end: g.end,
                stations: m.stations.clone(),
                nodes: [guid_of(g.nodes[0]), guid_of(g.nodes[1])],
            })
        })
        .collect()
}

/// Members' stiffness (E·Iy, E·Iz) by GlobalId, from the model as it is now.
fn stiffness(store: &Store) -> HashMap<String, (f64, f64)> {
    let Ok(d) = crate::structural_model::distill(store) else {
        return HashMap::new();
    };
    d.model
        .members
        .iter()
        .filter_map(|m| {
            let guid = d.key_map.get(&m.key)?.clone();
            Some((guid, (m.e * m.iy, m.e * m.iz)))
        })
        .collect()
}

/// Displacements along a member (global, metres) at its stations: the ends' translations, and
/// between them the deflection from integrating its moment diagram (curvature M / EI) twice to
/// meet the ends, so a loaded span sags between supports that do not move.
fn deflections(s: &Shown, ends: [[f64; 6]; 2], ei: Option<(f64, f64)>) -> Vec<(f64, V3)> {
    let (l, ax) = axes(s.start, s.end);
    let local = |d: &[f64; 6]| {
        [
            dot([d[0], d[1], d[2]], ax[0]),
            dot([d[0], d[1], d[2]], ax[1]),
            dot([d[0], d[1], d[2]], ax[2]),
        ]
    };
    let (ua, ub) = (local(&ends[0]), local(&ends[1]));
    let xs: Vec<f64> = s.stations.iter().map(|st| st.x).collect();
    // Transverse deflection in local y (from Mz) and z (from My): w'' = κ, with w at the ends
    // given. κz = My / EIy bends in z; κy = −Mz / EIy bends in y (right-handed axes).
    let integrate = |kappa: &dyn Fn(usize) -> f64, w0: f64, w1: f64| -> Vec<f64> {
        let n = xs.len();
        if n < 2 {
            return vec![w0; n];
        }
        let mut slope = vec![0.0; n];
        let mut w = vec![0.0; n];
        for i in 1..n {
            let h = xs[i] - xs[i - 1];
            slope[i] = slope[i - 1] + 0.5 * (kappa(i - 1) + kappa(i)) * h;
            // Exact for a linearly varying curvature over the interval.
            w[i] = w[i - 1] + slope[i - 1] * h + (2.0 * kappa(i - 1) + kappa(i)) * h * h / 6.0;
        }
        let lx = xs[n - 1] - xs[0];
        let c = if lx > 1e-12 {
            (w1 - w0 - w[n - 1]) / lx
        } else {
            0.0
        };
        (0..n).map(|i| w0 + w[i] + c * (xs[i] - xs[0])).collect()
    };
    let (wz, wy) = match ei {
        Some((eiy, eiz)) if eiy > 0.0 && eiz > 0.0 => (
            integrate(&|i| -s.stations[i].forces[4] / eiy, ua[2], ub[2]),
            integrate(&|i| s.stations[i].forces[5] / eiz, ua[1], ub[1]),
        ),
        _ => {
            let lin = |a: f64, b: f64| {
                xs.iter()
                    .map(|x| a + (b - a) * x / l.max(1e-12))
                    .collect::<Vec<_>>()
            };
            (lin(ua[2], ub[2]), lin(ua[1], ub[1]))
        }
    };
    xs.iter()
        .enumerate()
        .map(|(i, x)| {
            let t = x / l.max(1e-12);
            let u = ua[0] + (ub[0] - ua[0]) * t;
            let d = add(add(ax[0].map(|v| v * u), ax[1], wy[i]), ax[2], wz[i]);
            (*x, d)
        })
        .collect()
}

// ---- Extremes (Insights) ----

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Extreme {
    /// In the result type's unit (kN, kN·m or mm).
    pub value: f64,
    /// The analytical member (or node, for reactions and node displacements).
    pub element_id: ElementId,
    /// Its physical elements.
    pub physical: Vec<ElementId>,
    /// Where along the member (metres from its start), and the point.
    pub at: Option<f64>,
    pub point: V3,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Extremes {
    pub load_group: String,
    pub result_type: ResultType,
    pub unit: String,
    pub min: Option<Extreme>,
    pub max: Option<Extreme>,
    /// The largest absolute values, one per member, largest first (at most 20).
    pub top: Vec<Extreme>,
    /// The model changed since the analysis.
    pub stale: bool,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtremesParams {
    pub result_type: Option<ResultType>,
    /// Default the first case.
    pub load_group: Option<String>,
    pub filter: Option<Filter>,
}

/// The extremes of a result type in a load case or combination (`results.extremes`).
pub fn extremes(store: &Store, p: &ExtremesParams) -> Result<Extremes> {
    let s = stored(store)?;
    let t = p.result_type.unwrap_or(ResultType::MomentY);
    let r = group(&s, p.load_group.as_deref().unwrap_or(""))?;
    let (unit, k) = t.unit();
    let stale = crate::structural_model::snapshot(store) != s.snapshot;
    let mut samples: Vec<Extreme> = Vec::new();
    let mut per_member: Vec<Extreme> = Vec::new();
    match t {
        ResultType::Reactions => {
            for (key, f) in &r.reactions {
                let Some(id) = find(store, key) else { continue };
                let point = store
                    .get(id)
                    .and_then(|e| crate::analytical::node_point(store, e))
                    .unwrap_or_default();
                // The vertical reaction.
                let e = Extreme {
                    value: f[2] * k,
                    element_id: id,
                    physical: Vec::new(),
                    at: None,
                    point,
                };
                samples.push(e.clone());
                per_member.push(e);
            }
        }
        ResultType::Deformed => {
            let ei = stiffness(store);
            let disp: HashMap<&str, [f64; 6]> = r
                .displacements
                .iter()
                .map(|(k, d)| (k.as_str(), *d))
                .collect();
            for m in shown(store, r, p.filter.unwrap_or_default()) {
                let ends = [
                    disp.get(m.nodes[0].as_str()).copied().unwrap_or([0.0; 6]),
                    disp.get(m.nodes[1].as_str()).copied().unwrap_or([0.0; 6]),
                ];
                let guid = store
                    .get(m.id)
                    .map(|e| e.guid().to_string())
                    .unwrap_or_default();
                let (_, ax) = axes(m.start, m.end);
                let mut best: Option<Extreme> = None;
                for (x, d) in deflections(&m, ends, ei.get(&guid).copied()) {
                    let v = dot(d, d).sqrt() * k;
                    let e = Extreme {
                        value: v,
                        element_id: m.id,
                        physical: m.physical.clone(),
                        at: Some(x),
                        point: add(m.start, ax[0], x),
                    };
                    if best.as_ref().is_none_or(|b| v > b.value) {
                        best = Some(e.clone());
                    }
                    samples.push(e);
                }
                per_member.extend(best);
            }
        }
        _ => {
            let f = t.force().expect("a force");
            for m in shown(store, r, p.filter.unwrap_or_default()) {
                let (_, ax) = axes(m.start, m.end);
                let mut best: Option<Extreme> = None;
                for st in &m.stations {
                    let v = st.forces[f] * k;
                    let e = Extreme {
                        value: v,
                        element_id: m.id,
                        physical: m.physical.clone(),
                        at: Some(st.x),
                        point: add(m.start, ax[0], st.x),
                    };
                    if best.as_ref().is_none_or(|b| v.abs() > b.value.abs()) {
                        best = Some(e.clone());
                    }
                    samples.push(e);
                }
                per_member.extend(best);
            }
        }
    }
    let min = samples
        .iter()
        .min_by(|a, b| a.value.total_cmp(&b.value))
        .cloned();
    let max = samples
        .iter()
        .max_by(|a, b| a.value.total_cmp(&b.value))
        .cloned();
    per_member.sort_by(|a, b| b.value.abs().total_cmp(&a.value.abs()));
    per_member.truncate(20);
    Ok(Extremes {
        load_group: r.name.clone(),
        result_type: t,
        unit: unit.into(),
        min,
        max,
        top: per_member,
        stale,
    })
}

// ---- The overlay ----

/// A text label: its baseline start, reading and up directions, and height (metres).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Label {
    pub origin: V3,
    pub right: V3,
    pub up: V3,
    pub height: f64,
    pub text: String,
}

/// What a view draws for a results display: the diagram's outline and its hatching (lines from
/// the member to the outline at each station), or the deformed members, or reaction arrows;
/// and the values.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct Overlay {
    pub positive: Vec<[V3; 2]>,
    pub negative: Vec<[V3; 2]>,
    pub deformed: Vec<[V3; 2]>,
    pub labels: Vec<Label>,
    pub load_group: String,
    pub title: String,
    pub stale: bool,
    /// The values' extremes, in the unit shown.
    pub min: f64,
    pub max: f64,
    pub unit: String,
}

fn fmt(v: f64) -> String {
    if v.abs() >= 100.0 {
        format!("{v:.0}")
    } else if v.abs() >= 10.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.2}")
    }
}

/// The text's reading direction for a member: along it, from left to right as seen from the
/// front (−y), or across for vertical members.
fn reading(x: V3) -> V3 {
    if x[0].hypot(x[1]) < 1e-6 {
        return [1.0, 0.0, 0.0];
    }
    if x[0] < -1e-9 || (x[0].abs() <= 1e-9 && x[1] < 0.0) {
        x.map(|v| -v)
    } else {
        x
    }
}

/// The overlay for a display (`results.overlay`).
pub fn overlay(store: &Store, d: &Display) -> Result<Overlay> {
    let s = stored(store)?;
    let r = group(&s, &d.load_group)?;
    let (unit, k) = d.result_type.unit();
    let mut out = Overlay {
        load_group: r.name.clone(),
        title: format!("{} — {}", d.result_type.label(), r.name),
        stale: crate::structural_model::snapshot(store) != s.snapshot,
        unit: unit.into(),
        ..Default::default()
    };
    let members = shown(store, r, d.filter);
    let disp: HashMap<&str, [f64; 6]> = r
        .displacements
        .iter()
        .map(|(k, d)| (k.as_str(), *d))
        .collect();
    // Sizes follow the model: its analytical members' extent.
    let extent = {
        let ms = crate::analytical::members(store);
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for (_, m) in &ms {
            for p in [m.start, m.end] {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
        }
        if ms.is_empty() {
            0.0
        } else {
            ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt()
        }
    };
    let force_length = (0.08 * extent).clamp(1.5, 6.0);
    let deformed_length = (0.05 * extent).clamp(1.0, 4.0);
    let label_height = (0.012 * extent).clamp(0.18, 0.6);
    let push_label = |out: &mut Overlay, at: V3, right: V3, text: String| {
        out.labels.push(Label {
            origin: at,
            right,
            up: [0.0, 0.0, 1.0],
            height: label_height,
            text,
        });
    };
    // Deformed shape, alone or under a force diagram.
    if d.result_type == ResultType::Deformed || d.deformation {
        let ei = stiffness(store);
        let shapes: Vec<(&Shown, Vec<(f64, V3)>)> = members
            .iter()
            .map(|m| {
                let ends = [
                    disp.get(m.nodes[0].as_str()).copied().unwrap_or([0.0; 6]),
                    disp.get(m.nodes[1].as_str()).copied().unwrap_or([0.0; 6]),
                ];
                let guid = store
                    .get(m.id)
                    .map(|e| e.guid().to_string())
                    .unwrap_or_default();
                (m, deflections(m, ends, ei.get(&guid).copied()))
            })
            .collect();
        let largest = shapes
            .iter()
            .flat_map(|(_, v)| v.iter().map(|(_, d)| dot(*d, *d).sqrt()))
            .fold(0.0, f64::max);
        let scale = if largest > 1e-12 {
            deformed_length / largest
        } else {
            0.0
        } * d.scale;
        for (m, v) in &shapes {
            let (_, ax) = axes(m.start, m.end);
            let pts: Vec<V3> = v
                .iter()
                .map(|(x, dd)| add(add(m.start, ax[0], *x), *dd, scale))
                .collect();
            for w in pts.windows(2) {
                out.deformed.push([w[0], w[1]]);
            }
            if d.result_type == ResultType::Deformed
                && d.labels
                && let Some((x, dd)) = v
                    .iter()
                    .max_by(|a, b| dot(a.1, a.1).total_cmp(&dot(b.1, b.1)))
            {
                let mag = dot(*dd, *dd).sqrt() * k;
                if mag > 0.05 {
                    let at = add(add(m.start, ax[0], *x), *dd, scale);
                    push_label(
                        &mut out,
                        add(at, [0.0, 0.0, 1.0], 0.08),
                        reading(ax[0]),
                        format!("{} mm", fmt(mag)),
                    );
                }
            }
        }
        if d.result_type == ResultType::Deformed {
            out.max = largest * k;
            out.min = 0.0;
            return Ok(out);
        }
    }
    if d.result_type == ResultType::Reactions {
        let pts: Vec<(V3, [f64; 6])> = r
            .reactions
            .iter()
            .filter_map(|(key, f)| {
                let id = find(store, key)?;
                let p = crate::analytical::node_point(store, store.get(id)?)?;
                Some((p, *f))
            })
            .collect();
        let largest = pts
            .iter()
            .map(|(_, f)| (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt())
            .fold(0.0, f64::max);
        let scale = if largest > 1e-9 {
            force_length / largest
        } else {
            0.0
        } * d.scale;
        out.min = f64::INFINITY;
        out.max = f64::NEG_INFINITY;
        for (p, f) in &pts {
            out.min = out.min.min(f[2] * k);
            out.max = out.max.max(f[2] * k);
            // Each force component as an arrow pointing at the node, in the reaction's direction.
            for axis in 0..3 {
                let v = f[axis];
                if v.abs() < largest * 1e-3 {
                    continue;
                }
                let mut dir = [0.0; 3];
                dir[axis] = v.signum();
                let tail = add(*p, dir, -v.abs() * scale);
                let bucket = if v >= 0.0 {
                    &mut out.positive
                } else {
                    &mut out.negative
                };
                bucket.push([tail, *p]);
                let side = if axis == 2 {
                    [1.0, 0.0, 0.0]
                } else {
                    [0.0, 0.0, 1.0]
                };
                for sgn in [-1.0, 1.0] {
                    let barb = add(add(*p, dir, -0.2), side, 0.1 * sgn);
                    bucket.push([barb, *p]);
                }
            }
            if d.labels {
                push_label(
                    &mut out,
                    add(*p, [0.15, 0.0, 0.0], 1.0),
                    [1.0, 0.0, 0.0],
                    format!("{} kN", fmt(f[2] * k)),
                );
            }
        }
        if pts.is_empty() {
            out.min = 0.0;
            out.max = 0.0;
        }
        return Ok(out);
    }
    // A force diagram: each station's value drawn off the member, in its local y (Vy, Mz) or z
    // (N, Vz, T, My); moments on the tension side.
    let f = d.result_type.force().expect("a force");
    let largest = members
        .iter()
        .flat_map(|m| m.stations.iter().map(|s| s.forces[f].abs()))
        .fold(0.0, f64::max);
    let scale = if largest > 1e-9 {
        force_length / largest
    } else {
        0.0
    } * d.scale;
    out.min = f64::INFINITY;
    out.max = f64::NEG_INFINITY;
    for m in &members {
        let (_, ax) = axes(m.start, m.end);
        let (dir, sign) = match f {
            1 | 5 => (ax[1], 1.0),
            // My: under the solver's axes (z up, y = z × x) a sagging moment is negative, so
            // drawing the value along z puts it below the member, on the tension side.
            4 => (ax[2], 1.0),
            _ => (ax[2], 1.0),
        };
        let mut prev: Option<V3> = None;
        let mut best: Option<(f64, V3)> = None;
        for st in &m.stations {
            let v = st.forces[f];
            out.min = out.min.min(v * k);
            out.max = out.max.max(v * k);
            let base = add(m.start, ax[0], st.x);
            let tip = add(base, dir, sign * v * scale);
            let bucket = if v >= 0.0 {
                &mut out.positive
            } else {
                &mut out.negative
            };
            bucket.push([base, tip]);
            if let Some(q) = prev {
                bucket.push([q, tip]);
            }
            prev = Some(tip);
            if best.is_none_or(|(b, _)| v.abs() > b.abs()) {
                best = Some((v, tip));
            }
        }
        if d.labels
            && let Some((v, tip)) = best
            && v.abs() > largest * 0.02
        {
            push_label(
                &mut out,
                add(tip, dir, sign * v.signum() * 0.1),
                reading(ax[0]),
                fmt(v * k),
            );
        }
    }
    if members.is_empty() {
        out.min = 0.0;
        out.max = 0.0;
    }
    Ok(out)
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShowParams {
    pub view_id: ElementRef,
    pub result_type: ResultType,
    pub load_group: Option<String>,
    pub scale: Option<f64>,
    pub deformation: Option<bool>,
    pub filter: Option<Filter>,
    pub labels: Option<bool>,
}

fn view(store: &Store, r: &ElementRef) -> Result<ElementId> {
    let (id, _) = crate::views::resolve_view(store, r)?;
    Ok(id)
}

fn write_display(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    d: Option<&Display>,
) -> Result<()> {
    let v = d.map(|d| serde_json::to_value(d).expect("serialises"));
    txn.modify(id, |e| match v {
        Some(v) => {
            e.params.insert(DISPLAY_KEY.into(), v);
        }
        None => {
            e.params.remove(DISPLAY_KEY);
        }
    })
}

fn check(store: &Store, d: &Display) -> Result<()> {
    if !(d.scale > 0.0 && d.scale <= 1000.0) {
        return Err(Error::new(
            "results.bad_scale",
            "the scale is above 0 and at most 1000",
        ));
    }
    let s = stored(store)?;
    group(&s, &d.load_group)?;
    Ok(())
}

/// Shows results in a view (`results.show`).
pub(crate) fn show(txn: &mut crate::txn::Transaction<'_>, p: ShowParams) -> Result<Display> {
    let store = txn.store().clone();
    let id = view(&store, &p.view_id)?;
    let before = display(store.get(id).expect("resolved"));
    let d = Display {
        load_group: p
            .load_group
            .or_else(|| before.as_ref().map(|b| b.load_group.clone()))
            .unwrap_or_default(),
        result_type: p.result_type,
        scale: p.scale.unwrap_or(1.0),
        deformation: p.deformation.unwrap_or(false),
        filter: p.filter.unwrap_or_default(),
        labels: p.labels.unwrap_or(true),
        preset: None,
    };
    check(&store, &d)?;
    write_display(txn, id, Some(&d))?;
    Ok(d)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PresetParams {
    pub view_id: ElementRef,
    pub preset_id: String,
    pub load_group: Option<String>,
}

/// Shows a preset in a view (`results.apply_preset`).
pub(crate) fn apply_preset(
    txn: &mut crate::txn::Transaction<'_>,
    p: PresetParams,
) -> Result<Display> {
    let store = txn.store().clone();
    let id = view(&store, &p.view_id)?;
    let preset = presets(&store)
        .into_iter()
        .find(|x| x.id == p.preset_id)
        .ok_or_else(|| Error::new("results.no_preset", format!("no preset {}", p.preset_id)))?;
    let Some(mut d) = preset.display else {
        return Err(Error::new(
            "results.preset_unavailable",
            format!("{} {}", preset.name, preset.unavailable.unwrap_or_default()),
        ));
    };
    if let Some(g) = p.load_group {
        d.load_group = g;
    } else if let Some(b) = display(store.get(id).expect("resolved")) {
        d.load_group = b.load_group;
    }
    d.preset = Some(preset.id);
    check(&store, &d)?;
    write_display(txn, id, Some(&d))?;
    Ok(d)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HideParams {
    pub view_id: ElementRef,
}

/// Stops showing results in a view (`results.hide`).
pub(crate) fn hide(txn: &mut crate::txn::Transaction<'_>, p: HideParams) -> Result<()> {
    let store = txn.store().clone();
    let id = view(&store, &p.view_id)?;
    write_display(txn, id, None)
}

/// A view's results display, if it shows one.
pub fn display(e: &Element) -> Option<Display> {
    serde_json::from_value(e.param(DISPLAY_KEY)?.clone()).ok()
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SavePresetParams {
    pub name: String,
    /// The view whose display to save.
    pub view_id: ElementRef,
}

/// Saves a view's display as a project preset (`results.save_preset`). Returns its id.
pub(crate) fn save_preset(
    txn: &mut crate::txn::Transaction<'_>,
    p: SavePresetParams,
) -> Result<String> {
    let store = txn.store().clone();
    let id = view(&store, &p.view_id)?;
    let mut d = display(store.get(id).expect("resolved"))
        .ok_or_else(|| Error::new("results.not_shown", "the view shows no results to save"))?;
    let name = p.name.trim().to_owned();
    if name.is_empty() {
        return Err(Error::new("results.bad_name", "a preset needs a name"));
    }
    if presets(&store)
        .iter()
        .any(|x| x.name.eq_ignore_ascii_case(&name))
    {
        return Err(Error::new(
            "results.name_taken",
            format!("there is already a preset named \"{name}\""),
        ));
    }
    let pid = format!(
        "user_{}",
        name.to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect::<String>()
    );
    d.preset = Some(pid.clone());
    d.load_group.clear();
    let mut list = user_presets(&store);
    list.push(Preset {
        id: pid.clone(),
        name,
        display: Some(d),
        user: true,
        unavailable: None,
    });
    write_presets(txn, &store, &list)?;
    Ok(pid)
}

fn write_presets(
    txn: &mut crate::txn::Transaction<'_>,
    store: &Store,
    list: &[Preset],
) -> Result<()> {
    let pid = project(store)
        .map(Element::id)
        .ok_or_else(|| Error::new("project.missing", "the model has no project"))?;
    let v = serde_json::to_value(list).expect("serialises");
    txn.modify(pid, |e| {
        e.params.insert(PRESETS_KEY.into(), v);
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeletePresetParams {
    pub preset_id: String,
}

/// Deletes a project preset (`results.delete_preset`); built-in ones cannot be.
pub(crate) fn delete_preset(
    txn: &mut crate::txn::Transaction<'_>,
    p: DeletePresetParams,
) -> Result<()> {
    let store = txn.store().clone();
    let mut list = user_presets(&store);
    let before = list.len();
    list.retain(|x| x.id != p.preset_id);
    if list.len() == before {
        return Err(Error::new(
            "results.no_preset",
            if builtin_presets().iter().any(|x| x.id == p.preset_id) {
                format!("{} is built in and cannot be deleted", p.preset_id)
            } else {
                format!("no project preset {}", p.preset_id)
            },
        ));
    }
    write_presets(txn, &store, &list)
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OverlayParams {
    /// A view showing results …
    pub view_id: Option<ElementRef>,
    /// … or a display.
    pub display: Option<Display>,
}

/// The overlay a view (or a display) draws (`results.overlay`).
pub fn overlay_of(store: &Store, p: &OverlayParams) -> Result<Overlay> {
    let d = match (&p.display, &p.view_id) {
        (Some(d), _) => d.clone(),
        (None, Some(v)) => {
            let id = view(store, v)?;
            display(store.get(id).expect("resolved"))
                .ok_or_else(|| Error::new("results.not_shown", "the view shows no results"))?
        }
        (None, None) => {
            return Err(Error::new(
                "args.invalid",
                "give a view showing results, or a display",
            ));
        }
    };
    overlay(store, &d)
}

/// One result set kept with the model (the Results Manager's list).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Package {
    pub id: String,
    pub at: String,
    pub provider: String,
    pub stale: bool,
    pub load_groups: Vec<String>,
    pub members: usize,
    pub warnings: usize,
}

/// The result sets kept with the model (`results.packages`): the last analysis, if any.
pub fn packages(store: &Store) -> Vec<Package> {
    crate::structural_model::stored(store)
        .map(|s| Package {
            id: s.job_id.clone(),
            at: s.at.clone(),
            provider: s.provider.clone(),
            stale: crate::structural_model::snapshot(store) != s.snapshot,
            load_groups: s.results.results.iter().map(|r| r.name.clone()).collect(),
            members: s.results.results.first().map_or(0, |r| r.members.len()),
            warnings: s.results.warnings.len(),
        })
        .into_iter()
        .collect()
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeletePackageParams {
    /// The result set (its job id); the kept one when omitted.
    pub package_id: Option<String>,
}

/// Deletes the kept results (`results.delete_package`); views stop showing them.
pub(crate) fn delete_package(
    txn: &mut crate::txn::Transaction<'_>,
    p: DeletePackageParams,
) -> Result<()> {
    let store = txn.store().clone();
    let s = stored(&store)?;
    if let Some(id) = &p.package_id
        && *id != s.job_id
    {
        return Err(Error::new(
            "results.no_package",
            format!("no result set {id} (the kept one is {})", s.job_id),
        ));
    }
    let pid = project(&store)
        .map(Element::id)
        .ok_or_else(|| Error::new("project.missing", "the model has no project"))?;
    txn.modify(pid, |e| {
        e.params.remove(RESULTS_KEY);
    })?;
    let views: Vec<ElementId> = store
        .iter()
        .filter(|e| e.param(DISPLAY_KEY).is_some())
        .map(Element::id)
        .collect();
    for v in views {
        write_display(txn, v, None)?;
    }
    Ok(())
}
