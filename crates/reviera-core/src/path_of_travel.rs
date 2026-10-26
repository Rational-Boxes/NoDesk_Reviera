// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Path of travel (ANA-ROUTE-001/002): the shortest walking route between two points of a plan
//! that keeps clear of obstacles, a built-in provider.
//!
//! - **Analysis zone**: a band from a bottom to a top offset above the plan's level. Obstacles
//!   are the walls (their bodies, straight, with a gap where a door's opening reaches the zone's
//!   bottom; windows are not passable) and the placed components whose body reaches into the
//!   band (their plan outline: the hull of their vertices), except the ignored categories.
//! - **Clearance**: each obstacle is grown by the clearance (its outline offset, the corners
//!   rounded by short chords outside the true arc, so a route never cuts a corner).
//! - **Route**: the shortest path through a visibility graph of the grown outlines' corners, from
//!   the start through each waypoint in order to the end. Its length and its time at the walking
//!   speed are kept with it.
//! - Each path is a view annotation (`anno.path_of_travel`) on one level: the view, start, end,
//!   waypoints, and the route as last worked out (`path_of_travel.update` works it out again
//!   after the model changes). Stairs and ramps are not followed (one level).
//! - **Settings** (`rxd:route` on the project): zone offsets, walking speed, clearance and
//!   ignored categories.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::annotations::{ANNO_KEY, CLASS_PREFIX, is_annotation};
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;

pub const KIND: &str = "anno.path_of_travel";
pub const SETTINGS_KEY: &str = "rxd:route";
/// Chords per rounded corner of a grown obstacle.
const ARC_STEPS: usize = 4;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// Route analysis settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Settings {
    /// The analysis zone above the level (metres).
    pub zone_bottom: f64,
    pub zone_top: f64,
    /// m/s.
    pub walking_speed: f64,
    /// Kept from obstacles (metres).
    pub clearance: f64,
    /// Category keys whose elements are not obstacles (`cat.furniture`…).
    pub ignored_categories: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        // Provisional values for expert review (EXPERT_REVIEW E-35).
        Settings { zone_bottom: 0.2, zone_top: 2.0, walking_speed: 1.3, clearance: 0.3, ignored_categories: Vec::new() }
    }
}

pub fn settings(store: &Store) -> Settings {
    store.iter().find(|e| e.class == "IfcProject").and_then(|p| p.param(SETTINGS_KEY)).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

/// A path's stored data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PathData {
    pub kind: String,
    /// The plan view (GlobalId).
    pub view: String,
    pub start: [f64; 2],
    pub end: [f64; 2],
    #[serde(default)]
    pub waypoints: Vec<[f64; 2]>,
    /// The route as last worked out (none: no route was found).
    #[serde(default)]
    pub route: Option<Vec<[f64; 2]>>,
    #[serde(default)]
    pub length: f64,
    /// Seconds.
    #[serde(default)]
    pub time: f64,
}

pub fn path_data(e: &Element) -> Option<PathData> {
    if !is_annotation(e) {
        return None;
    }
    let d: PathData = serde_json::from_value(e.param(ANNO_KEY)?.clone()).ok()?;
    (d.kind == KIND).then_some(d)
}

type Poly = Vec<[f64; 2]>;

/// Grows a convex outline (counter-clockwise) by `r`: each edge moved out by `r`, each corner an
/// arc of chords whose sides touch the true arc from outside.
fn grow(poly: &[[f64; 2]], r: f64) -> Poly {
    let n = poly.len();
    if r <= 0.0 || n < 3 {
        return poly.to_vec();
    }
    // Chords outside the arc: points at radius r / cos(step / 2).
    let mut out = Vec::new();
    for i in 0..n {
        let (p, a, b) = (poly[(i + n - 1) % n], poly[i], poly[(i + 1) % n]);
        let n1 = outward(p, a);
        let n2 = outward(a, b);
        let (a1, mut a2) = (n1[1].atan2(n1[0]), n2[1].atan2(n2[0]));
        while a2 < a1 {
            a2 += std::f64::consts::TAU;
        }
        let sweep = a2 - a1;
        let steps = ((sweep / (std::f64::consts::FRAC_PI_2 / ARC_STEPS as f64)).ceil() as usize).max(1);
        let step = sweep / steps as f64;
        let rr = r / (step / 2.0).cos();
        out.push([a[0] + r * a1.cos(), a[1] + r * a1.sin()]);
        for k in 0..steps {
            let t = a1 + step * (k as f64 + 0.5);
            out.push([a[0] + rr * t.cos(), a[1] + rr * t.sin()]);
        }
        out.push([a[0] + r * a2.cos(), a[1] + r * a2.sin()]);
    }
    out
}

/// The outward unit normal of an edge of a counter-clockwise outline.
fn outward(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l = dx.hypot(dy).max(1e-12);
    [dy / l, -dx / l]
}

fn rect(a: [f64; 2], b: [f64; 2], half: f64) -> Poly {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l = dx.hypot(dy).max(1e-12);
    let n = [-dy / l * half, dx / l * half];
    // Counter-clockwise.
    vec![[a[0] - n[0], a[1] - n[1]], [b[0] - n[0], b[1] - n[1]], [b[0] + n[0], b[1] + n[1]], [a[0] + n[0], a[1] + n[1]]]
}

/// The obstacles of a level's analysis zone, grown by the clearance.
pub fn obstacles(store: &Store, level: ElementId, s: &Settings) -> Vec<Poly> {
    let Some(level_e) = store.get(level) else { return Vec::new() };
    let z = crate::levels::elevation(level_e);
    let (z0, z1) = (z + s.zone_bottom, z + s.zone_top);
    let ignored = |e: &Element| crate::categories::category_of(e).is_some_and(|c| s.ignored_categories.iter().any(|k| k == c.key));
    let mut out = Vec::new();
    // Walls, split at doors whose opening reaches the zone's bottom.
    let doors = crate::inserts::placed(store);
    for e in store.iter().filter(|e| crate::walls::wall_recipe(e).is_some()) {
        if ignored(e) {
            continue;
        }
        let Ok(g) = crate::walls::regenerate(store, e) else { continue };
        if g.base_z > z1 || g.base_z + g.height < z0 {
            continue;
        }
        let len = g.length();
        let mut gaps: Vec<(f64, f64)> = doors
            .iter()
            .filter(|d| d.host == e.id() && d.kind == crate::inserts::Kind::Door && d.z0 <= z0 + 1e-6)
            .map(|d| ((d.recipe.along - d.ty.width / 2.0).max(0.0), (d.recipe.along + d.ty.width / 2.0).min(len)))
            .collect();
        gaps.sort_by(|a, b| a.0.total_cmp(&b.0));
        let u = g.direction();
        let at = |t: f64| [g.start[0] + u[0] * t, g.start[1] + u[1] * t];
        let mut from = 0.0;
        for (a, b) in gaps.into_iter().chain(std::iter::once((len, len))) {
            if a - from > 1e-6 {
                out.push(grow(&rect(at(from), at(a), g.width / 2.0), s.clearance));
            }
            from = from.max(b);
        }
    }
    // Components whose body reaches into the zone.
    for e in store.iter() {
        if crate::mesh_objects::mesh_object_recipe(e).is_none() || ignored(e) {
            continue;
        }
        let Ok(g) = crate::mesh_objects::regenerate(store, e) else { continue };
        let mut pts = Vec::new();
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        for (_, m) in g.slot_meshes() {
            for p in &m.0 {
                lo = lo.min(p[2]);
                hi = hi.max(p[2]);
                pts.push([p[0], p[1]]);
            }
        }
        if pts.is_empty() || hi < z0 || lo > z1 {
            continue;
        }
        let hull = crate::system_zones::hull(&pts);
        if hull.len() >= 3 {
            out.push(grow(&hull, s.clearance));
        }
    }
    out
}

fn cross(o: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

/// Whether a point is strictly inside a convex counter-clockwise outline.
fn inside(p: [f64; 2], poly: &[[f64; 2]]) -> bool {
    (0..poly.len()).all(|i| cross(poly[i], poly[(i + 1) % poly.len()], p) > 1e-9)
}

/// Whether segment a–b passes through a convex outline's interior.
fn blocked(a: [f64; 2], b: [f64; 2], poly: &[[f64; 2]]) -> bool {
    // Clip the segment against each edge's half-plane (Cyrus–Beck); a remaining piece of
    // positive length lies inside.
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let d = [b[0] - a[0], b[1] - a[1]];
    let n = poly.len();
    for i in 0..n {
        let (p, q) = (poly[i], poly[(i + 1) % n]);
        let e = [q[0] - p[0], q[1] - p[1]];
        // Inside is to the left: cross(e, x − p) > 0.
        let num = e[0] * (a[1] - p[1]) - e[1] * (a[0] - p[0]);
        let den = e[0] * d[1] - e[1] * d[0];
        if den.abs() < 1e-15 {
            if num <= 1e-9 {
                return false;
            }
            continue;
        }
        let t = -num / den;
        if den > 0.0 {
            t0 = t0.max(t);
        } else {
            t1 = t1.min(t);
        }
        if t1 - t0 <= 1e-9 {
            return false;
        }
    }
    (t1 - t0) * d[0].hypot(d[1]) > 1e-6
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

/// The shortest route from `a` to `b` round the obstacles, if any.
pub fn shortest(a: [f64; 2], b: [f64; 2], obs: &[Poly]) -> Option<Vec<[f64; 2]>> {
    if obs.iter().any(|p| inside(a, p) || inside(b, p)) {
        return None;
    }
    let free = |p: [f64; 2], q: [f64; 2]| !obs.iter().any(|o| blocked(p, q, o));
    if free(a, b) {
        return Some(vec![a, b]);
    }
    let mut nodes = vec![a, b];
    for o in obs {
        for p in o {
            if !obs.iter().any(|x| inside(*p, x)) {
                nodes.push(*p);
            }
        }
    }
    // Dijkstra over the visibility graph, edges checked as they are reached.
    let n = nodes.len();
    let mut best = vec![f64::INFINITY; n];
    let mut prev = vec![usize::MAX; n];
    let mut done = vec![false; n];
    best[0] = 0.0;
    for _ in 0..n {
        let Some(u) = (0..n).filter(|i| !done[*i] && best[*i].is_finite()).min_by(|x, y| best[*x].total_cmp(&best[*y])) else { break };
        if u == 1 {
            break;
        }
        done[u] = true;
        for v in 0..n {
            if done[v] || v == u {
                continue;
            }
            let c = best[u] + dist(nodes[u], nodes[v]);
            if c < best[v] - 1e-12 && free(nodes[u], nodes[v]) {
                best[v] = c;
                prev[v] = u;
            }
        }
    }
    if !best[1].is_finite() {
        return None;
    }
    let mut route = vec![nodes[1]];
    let mut k = 1;
    while prev[k] != usize::MAX {
        k = prev[k];
        route.push(nodes[k]);
    }
    route.reverse();
    Some(route)
}

fn length(route: &[[f64; 2]]) -> f64 {
    route.windows(2).map(|w| dist(w[0], w[1])).sum()
}

/// Works out a path's route on its view's level.
fn solve(store: &Store, d: &mut PathData) -> Result<()> {
    let view = store.find_by_guid(&GlobalId::parse(&d.view).ok_or_else(|| bad("view.not_found", "the path's view is gone"))?).ok_or_else(|| bad("view.not_found", "the path's view is gone"))?;
    let data = store.get(view).and_then(crate::views::view_data).ok_or_else(|| bad("view.not_found", "the path's view is gone"))?;
    let level = data.level.as_deref().and_then(GlobalId::parse).and_then(|g| store.find_by_guid(&g)).ok_or_else(|| bad("view.not_plan", "paths of travel are drawn in plan views"))?;
    let s = settings(store);
    let obs = obstacles(store, level, &s);
    let mut stops = vec![d.start];
    stops.extend(d.waypoints.iter().copied());
    stops.push(d.end);
    let mut route: Vec<[f64; 2]> = Vec::new();
    for w in stops.windows(2) {
        match shortest(w[0], w[1], &obs) {
            Some(leg) => {
                if !route.is_empty() {
                    route.pop();
                }
                route.extend(leg);
            }
            None => {
                d.route = None;
                d.length = 0.0;
                d.time = 0.0;
                return Ok(());
            }
        }
    }
    d.length = length(&route);
    d.time = d.length / s.walking_speed.max(1e-6);
    d.route = Some(route);
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub view_id: ElementRef,
    pub start: [f64; 2],
    pub end: [f64; 2],
    #[serde(default)]
    pub waypoints: Vec<[f64; 2]>,
}

/// `path_of_travel.create`: refused when no route keeps the clearance.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let (vid, data) = crate::views::resolve_view(&store, &p.view_id)?;
    if data.level.is_none() {
        return Err(bad("view.not_plan", "paths of travel are drawn in plan views"));
    }
    let view = store.get(vid).expect("resolved");
    let mut d = PathData { kind: KIND.into(), view: view.guid().to_string(), start: p.start, end: p.end, waypoints: p.waypoints, route: None, length: 0.0, time: 0.0 };
    solve(&store, &mut d)?;
    if d.route.is_none() {
        return Err(bad("path_of_travel.no_route", "no route keeps clear of the obstacles between those points (a point inside an obstacle or its clearance, or no way through)"));
    }
    let guid = GlobalId::new();
    txn.create(
        NewElement::new(format!("{CLASS_PREFIX}{}/{}", view.guid().as_str(), guid.as_str()))
            .guid(guid)
            .name("Path of Travel")
            .param(ANNO_KEY, serde_json::to_value(&d).expect("serialises")),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateParams {
    /// Paths; every path of `view_id` when empty.
    #[serde(default)]
    pub ids: Vec<ElementRef>,
    pub view_id: Option<ElementRef>,
}

/// `path_of_travel.update`: works the routes out again; returns those that changed.
pub(crate) fn update(txn: &mut Transaction<'_>, p: UpdateParams) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let ids: Vec<ElementId> = if p.ids.is_empty() {
        let view = match &p.view_id {
            Some(v) => Some(store.get(crate::views::resolve_view(&store, v)?.0).expect("resolved").guid().to_string()),
            None => None,
        };
        store.iter().filter(|e| path_data(e).is_some_and(|d| view.as_ref().is_none_or(|v| &d.view == v))).map(Element::id).collect()
    } else {
        p.ids.iter().map(|r| store.resolve(r).filter(|id| store.get(*id).and_then(path_data).is_some()).ok_or_else(|| bad("path_of_travel.not_found", "no such path of travel"))).collect::<Result<_>>()?
    };
    let mut changed = Vec::new();
    for id in ids {
        let mut d = path_data(store.get(id).expect("listed")).expect("a path");
        let before = d.clone();
        solve(&store, &mut d)?;
        if d != before {
            let v = serde_json::to_value(&d).expect("serialises");
            txn.modify(id, |e| {
                e.params.insert(ANNO_KEY.into(), v);
            })?;
            changed.push(id);
        }
    }
    Ok(changed)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WaypointParams {
    pub id: ElementRef,
    pub point: [f64; 2],
    /// Where among the waypoints (0 first); the end when omitted.
    pub index: Option<usize>,
    /// Remove the waypoint at `index` instead.
    #[serde(default)]
    pub remove: bool,
}

/// `path_of_travel.add_waypoint` (or remove one).
pub(crate) fn waypoint(txn: &mut Transaction<'_>, p: WaypointParams) -> Result<f64> {
    let store = txn.store().clone();
    let id = store.resolve(&p.id).ok_or_else(|| bad("path_of_travel.not_found", "no such path of travel"))?;
    let mut d = store.get(id).and_then(path_data).ok_or_else(|| bad("path_of_travel.not_found", "no such path of travel"))?;
    if p.remove {
        let k = p.index.ok_or_else(|| bad("path_of_travel.index", "say which waypoint to remove"))?;
        if k >= d.waypoints.len() {
            return Err(bad("path_of_travel.index", format!("the path has {} waypoint(s)", d.waypoints.len())));
        }
        d.waypoints.remove(k);
    } else {
        let k = p.index.unwrap_or(d.waypoints.len()).min(d.waypoints.len());
        d.waypoints.insert(k, p.point);
    }
    solve(&store, &mut d)?;
    if d.route.is_none() {
        return Err(bad("path_of_travel.no_route", "no route keeps clear of the obstacles through that waypoint"));
    }
    let len = d.length;
    let v = serde_json::to_value(&d).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(ANNO_KEY.into(), v);
    })?;
    Ok(len)
}

/// A path as listed.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PathInfo {
    pub id: ElementId,
    pub view: String,
    pub length: f64,
    pub time: f64,
    pub route: Option<Vec<[f64; 2]>>,
    pub waypoints: Vec<[f64; 2]>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    pub view_id: Option<ElementRef>,
}

/// `path_of_travel.list`.
pub(crate) fn list(store: &Store, p: ListParams) -> Result<Vec<PathInfo>> {
    let view = match &p.view_id {
        Some(v) => Some(store.get(crate::views::resolve_view(store, v)?.0).expect("resolved").guid().to_string()),
        None => None,
    };
    Ok(store
        .iter()
        .filter_map(|e| Some((e.id(), path_data(e)?)))
        .filter(|(_, d)| view.as_ref().is_none_or(|v| &d.view == v))
        .map(|(id, d)| PathInfo { id, view: d.view, length: d.length, time: d.time, route: d.route, waypoints: d.waypoints })
        .collect())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SettingsParams {
    pub zone_bottom: Option<f64>,
    pub zone_top: Option<f64>,
    pub walking_speed: Option<f64>,
    pub clearance: Option<f64>,
    pub ignored_categories: Option<Vec<String>>,
}

/// `route_settings.set`.
pub(crate) fn set_settings(txn: &mut Transaction<'_>, p: SettingsParams) -> Result<Settings> {
    let mut s = settings(txn.store());
    if let Some(v) = p.zone_bottom {
        s.zone_bottom = v;
    }
    if let Some(v) = p.zone_top {
        s.zone_top = v;
    }
    if let Some(v) = p.walking_speed {
        s.walking_speed = v;
    }
    if let Some(v) = p.clearance {
        s.clearance = v;
    }
    if let Some(v) = p.ignored_categories {
        if let Some(bad_key) = v.iter().find(|k| crate::categories::category(k).is_none()) {
            return Err(bad("route.category", format!("{bad_key:?} is not a category key")));
        }
        s.ignored_categories = v;
    }
    if !(s.zone_bottom >= 0.0 && s.zone_top > s.zone_bottom && s.zone_top <= 10.0) {
        return Err(bad("route.settings", "the zone runs from a bottom of 0 m or more to a higher top, at most 10 m"));
    }
    if !(s.walking_speed > 0.0 && s.walking_speed <= 10.0) || !(0.0..=3.0).contains(&s.clearance) {
        return Err(bad("route.settings", "the walking speed runs from above 0 to 10 m/s and the clearance from 0 to 3 m"));
    }
    let pid = txn.store().iter().find(|e| e.class == "IfcProject").map(Element::id).ok_or_else(|| bad("project.missing", "the model has no project"))?;
    let v = serde_json::to_value(&s).expect("serialises");
    txn.modify(pid, |e| {
        e.params.insert(SETTINGS_KEY.into(), v);
    })?;
    Ok(s)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}

/// `route_settings.get`.
pub(crate) fn get_settings(store: &Store, _p: Empty) -> Result<Settings> {
    Ok(settings(store))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_a_square_keeping_clear() {
        // A 2 m square block between the points, a clearance of 0.5 m.
        let block = vec![[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];
        let obs = vec![grow(&block, 0.5)];
        let r = shortest([-5.0, 0.0], [5.0, 0.0], &obs).unwrap();
        let l = length(&r);
        // The true shortest path: tangents to the 0.5 m arc round the corners.
        let (a, b, c) = ([-5.0f64, 0.0f64], [-1.0f64, 1.0f64], 0.5f64);
        let d = (b[0] - a[0]).hypot(b[1] - a[1]);
        let tangent = (d * d - c * c).sqrt();
        let ang = (b[1] - a[1]).atan2(b[0] - a[0]) + (c / d).asin();
        // The radius at the tangent point is square to the tangent; the arc turns from there to
        // the top of the corner, through the tangent's angle.
        let arc = c * ang;
        let truth = 2.0 * (tangent + arc) + 2.0;
        assert!(l >= truth - 1e-9 && l <= truth * 1.01, "{l} against {truth}");
        assert!(shortest([0.0, 0.0], [5.0, 0.0], &obs).is_none(), "a start inside an obstacle");
    }
}
