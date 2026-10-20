// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Rebar visibility and presentation per view (STR-RBR-017).
//!
//! Each view keeps, per rebar set or area, path or fabric system (by GlobalId), whether it is
//! drawn **unobscured** (through its host), **as solid** (3D views; drawn as tubes at Fine
//! detail, as centrelines otherwise) and, for sets, which bars are drawn: **all**, **first and
//! last**, **middle**, or **selected** ones. First/Last and Middle add the set's extent line with
//! end ticks so the distribution still shows. Picks past the bar count are dropped; with none
//! left the set shows all. Presentation changes only what is drawn: counts, schedules and the
//! export keep every bar.
//!
//! The states are view data (`rxd:rebar_view` on the view element): nothing is written to the
//! rebar. A set with no state in a view takes the project's default presentation (Reinforcement
//! Settings).
//!
//! Not yet: spliced-set presentation per lap zone, view template defaults, and showing rebar
//! only in its placement and section views by default.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::rebar::{self, Bar, V3, add, mul, sub, unit};
use crate::store::Store;

pub const VIEW_KEY: &str = "rxd:rebar_view";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    All,
    FirstLast,
    Middle,
    Select,
}

/// A rebar element's state in one view.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct State {
    #[serde(default)]
    pub unobscured: bool,
    #[serde(default)]
    pub solid: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<Mode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected: Vec<usize>,
}

/// Whether an element is reinforcement with view states: a set, or an area, path or fabric
/// system or sheet.
pub fn is_reinforcement(e: &Element) -> bool {
    rebar::rebar_recipe(e).is_some() || crate::rebar_systems::system_kind(e).is_some() || crate::precast_fabric::recipe(e).is_some()
}

fn states(view: &Element) -> BTreeMap<String, State> {
    view.param(VIEW_KEY).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

/// A rebar element's state in a view (`None`: the unsaved default view).
pub fn state(store: &Store, view: Option<&Element>, e: &Element) -> State {
    let mut s = view.and_then(|v| states(v).remove(e.guid().as_str())).unwrap_or_default();
    if s.mode.is_none() && rebar::rebar_recipe(e).is_some() {
        s.mode = Some(rebar::settings(store).presentation);
    }
    s
}

fn is_3d(view: Option<&Element>) -> bool {
    view.is_none_or(|v| crate::views::view_data(v).is_some_and(|d| d.kind == crate::views::KIND_3D))
}

fn fine(view: Option<&Element>) -> bool {
    view.is_some_and(|v| crate::views::detail_level(v) == crate::views::DetailLevel::Fine)
}

fn targets(store: &Store, ids: &[ElementRef]) -> Result<Vec<ElementId>> {
    ids.iter()
        .map(|r| {
            let id = store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
            if !store.get(id).is_some_and(is_reinforcement) {
                return Err(Error::new("rebar.not_rebar", format!("element {id} is not reinforcement")));
            }
            Ok(id)
        })
        .collect()
}

fn update(
    txn: &mut crate::txn::Transaction<'_>,
    view_id: &ElementRef,
    ids: &[ElementRef],
    mut f: impl FnMut(&Element, &mut State) -> Result<()>,
) -> Result<()> {
    let store = txn.store().clone();
    let (view, _) = crate::views::resolve_view(&store, view_id)?;
    let v = store.get(view).expect("resolved");
    let mut map = states(v);
    for id in targets(&store, ids)? {
        let e = store.get(id).expect("resolved");
        let s = map.entry(e.guid().to_string()).or_default();
        f(e, s)?;
        if *s == State::default() {
            map.remove(e.guid().as_str());
        }
    }
    txn.set_param(view, VIEW_KEY, serde_json::to_value(&map).expect("serialises"))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct VisibilityParams {
    pub rebar_ids: Vec<ElementRef>,
    pub view_id: ElementRef,
    #[serde(default)]
    pub unobscured: Option<bool>,
    /// Only in 3D views.
    #[serde(default)]
    pub solid: Option<bool>,
}

/// Sets rebar's view visibility states (`rebar.set_view_visibility`).
pub(crate) fn set_view_visibility(txn: &mut crate::txn::Transaction<'_>, p: VisibilityParams) -> Result<()> {
    let store = txn.store().clone();
    let (view, data) = crate::views::resolve_view(&store, &p.view_id)?;
    if p.solid == Some(true) && data.kind != crate::views::KIND_3D {
        return Err(Error::new("rebar.solid_3d", format!("view {view} is not a 3D view: only 3D views show rebar as solid")));
    }
    update(txn, &p.view_id, &p.rebar_ids, |_, s| {
        if let Some(u) = p.unobscured {
            s.unobscured = u;
        }
        if let Some(v) = p.solid {
            s.solid = v;
        }
        Ok(())
    })
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PresentationParams {
    pub rebar_ids: Vec<ElementRef>,
    pub view_id: ElementRef,
    pub mode: Mode,
    /// The bars to show (from 0), for `select`.
    #[serde(default)]
    pub selected_indices: Vec<usize>,
}

/// Sets rebar sets' presentation in a view (`rebar.set_presentation`).
pub(crate) fn set_presentation(txn: &mut crate::txn::Transaction<'_>, p: PresentationParams) -> Result<()> {
    if p.mode == Mode::Select && p.selected_indices.is_empty() {
        return Err(Error::new("rebar.presentation", "Select needs the bars to show"));
    }
    update(txn, &p.view_id, &p.rebar_ids, |e, s| {
        if rebar::rebar_recipe(e).is_none() {
            return Err(Error::new("rebar.presentation", format!("element {} is a system: presentation is for rebar sets", e.id())));
        }
        s.mode = Some(p.mode);
        s.selected = if p.mode == Mode::Select { p.selected_indices.clone() } else { Vec::new() };
        Ok(())
    })
}

/// What a view draws of a rebar element.
#[derive(Debug, Clone)]
pub struct Drawn {
    pub element: ElementId,
    /// Drawn through its host.
    pub unobscured: bool,
    /// Every bar drawn as a tube: the element's own body serves.
    pub own_body: bool,
    /// The bars drawn as tubes, when only some are (with their model diameter).
    pub tubes: Vec<(f64, Bar)>,
    /// The bars drawn as centrelines.
    pub centrelines: Vec<[V3; 2]>,
    /// The set's extent line and its end ticks.
    pub extent: Vec<[V3; 2]>,
    /// The bars shown, by index.
    pub shown: Vec<usize>,
}

/// The bars a presentation shows, of `n`.
pub fn shown(mode: Mode, selected: &[usize], n: usize) -> Vec<usize> {
    match mode {
        _ if n == 0 => Vec::new(),
        Mode::All => (0..n).collect(),
        Mode::FirstLast if n == 1 => vec![0],
        Mode::FirstLast => vec![0, n - 1],
        Mode::Middle => vec![n / 2],
        Mode::Select => {
            let picks: Vec<usize> = selected.iter().copied().filter(|i| *i < n).collect();
            if picks.is_empty() { (0..n).collect() } else { picks }
        }
    }
}

fn mid(b: &Bar) -> V3 {
    let half = b.length() / 2.0;
    let mut run = 0.0;
    for w in b.points.windows(2) {
        let l = rebar::len(sub(w[1], w[0]));
        if run + l >= half && l > 0.0 {
            return add(w[0], mul(sub(w[1], w[0]), (half - run) / l));
        }
        run += l;
    }
    b.points[0]
}

/// How a view draws a rebar element (`None` if it is not reinforcement or cannot be worked out).
pub fn drawn(store: &Store, view: Option<&Element>, e: &Element) -> Option<Drawn> {
    let s = state(store, view, e);
    let solid = s.solid && is_3d(view) && fine(view);
    // Every bar with its drawn diameter.
    let bars: Vec<(f64, Bar)> = if let Some(r) = rebar::rebar_recipe(e) {
        let d = rebar::bar_types(store).into_iter().find(|b| b.id == r.bar_type)?.model_diameter;
        rebar::geometry(store, &r).ok()?.bars.into_iter().map(|b| (d, b)).collect()
    } else {
        let i = crate::rebar_systems::info(store, crate::rebar_systems::InfoParams { system_ids: vec![ElementRef::Id(e.id())] }).ok()?;
        let i = i.into_iter().next()?;
        let (da, db) = i.sheet_type.as_deref().and_then(|t| crate::rebar_systems::wire_diameters(store, t)).unwrap_or((0.006, 0.006));
        let mut v: Vec<(f64, Bar)> = i.groups.into_iter().flat_map(|g| g.bars.into_iter().map(move |b| (g.diameter, b))).collect();
        v.extend(i.sheets.into_iter().flat_map(|s| s.major.into_iter().map(move |b| (da, b)).chain(s.minor.into_iter().map(move |b| (db, b)))));
        v
    };
    let mode = s.mode.unwrap_or_default();
    let shown = shown(mode, &s.selected, bars.len());
    let mut d = Drawn {
        element: e.id(),
        unobscured: s.unobscured,
        own_body: solid && shown.len() == bars.len(),
        tubes: Vec::new(),
        centrelines: Vec::new(),
        extent: Vec::new(),
        shown: shown.clone(),
    };
    if !d.own_body {
        for &i in &shown {
            let (dia, b) = &bars[i];
            if solid {
                d.tubes.push((*dia, b.clone()));
            } else {
                d.centrelines.extend(b.points.windows(2).map(|w| [w[0], w[1]]));
            }
        }
    }
    // The extent line across the set, through the bars' middles, with ticks along the bars.
    if matches!(mode, Mode::FirstLast | Mode::Middle | Mode::Select) && bars.len() > 1 && shown.len() < bars.len() {
        let (a, b) = (mid(&bars[0].1), mid(&bars[bars.len() - 1].1));
        let along = {
            let p = &bars[0].1.points;
            unit(sub(p[p.len() - 1], p[0]))
        };
        let tick = mul(along, 0.1);
        d.extent.push([a, b]);
        for p in [a, b] {
            d.extent.push([sub(p, tick), add(p, tick)]);
        }
    }
    Some(d)
}

/// A rebar element's states in every view (`rebar.view_states`), for the View Visibility States
/// grid: each view with whether it is 3D (only 3D views take Solid).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ViewState {
    pub view_id: ElementId,
    pub name: String,
    pub kind: String,
    pub is_3d: bool,
    #[serde(flatten)]
    pub state: State,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ViewStatesParams {
    pub rebar_id: ElementRef,
}

pub fn view_states(store: &Store, p: ViewStatesParams) -> Result<Vec<ViewState>> {
    let id = targets(store, std::slice::from_ref(&p.rebar_id))?[0];
    let e = store.get(id).expect("resolved");
    Ok(crate::views::list(store)
        .into_iter()
        .filter(|v| !v.kind.starts_with("view.schedule") && !v.kind.starts_with("view.sheet"))
        .filter_map(|v| {
            let view = store.get(v.id)?;
            Some(ViewState { is_3d: v.kind == crate::views::KIND_3D, view_id: v.id, name: v.name, kind: v.kind, state: state(store, Some(view), e) })
        })
        .collect())
}

/// Tubes along bars, each with its diameter (a presentation's shown bars drawn as solids).
pub fn tubes_mesh(tubes: &[(f64, Bar)]) -> crate::bodies::Mesh {
    let mut parts = Vec::new();
    for (d, b) in tubes {
        for w in b.points.windows(2) {
            if rebar::len(sub(w[1], w[0])) > 1e-6 {
                parts.push(crate::steel_fab::cylinder(w[0], w[1], d / 2.0, 8));
            }
        }
    }
    crate::steel_fab::join(parts)
}
