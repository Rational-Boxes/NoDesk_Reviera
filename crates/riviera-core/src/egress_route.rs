// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Egress routes across levels (ANA-ROUTE-004): the quickest way from a point on one level to a
//! point on another, walking each level round its obstacles (ANA-ROUTE-001's engine) and taking
//! stairs between levels; escalators too where the egress mode counts them (ARC-VT-021).
//!
//! A stair links the point 0.4 m before its first riser on its base level with the point 0.4 m
//! beyond its last riser on its top level, its length along the middle of its flights and
//! landings, at the stair speed. An escalator, where allowed, links the points 0.6 m beyond each
//! end of its truss, walked as a stair.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::store::Store;
use crate::traffic::EgressMode;

type V2 = [f64; 2];

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// A point on a level.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct At {
    pub point: V2,
    pub level: ElementRef,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RouteParams {
    pub start: At,
    pub end: At,
    /// What may be used between levels: stairs always; escalators under `nfpa130` and `scdf`
    /// (default `ibc`: stairs only).
    #[serde(default)]
    pub mode: EgressMode,
}

/// A stretch walked on one level.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Leg {
    pub level: ElementId,
    pub points: Vec<V2>,
    pub length: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    Stair,
    Escalator,
}

/// A stair or escalator taken between levels.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Link {
    pub kind: LinkKind,
    pub element: ElementId,
    pub from: ElementId,
    pub to: ElementId,
    /// Along its flights (m), and the time taken (s).
    pub length: f64,
    pub time: f64,
}

/// The route: legs on levels and links between them, in order.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Route {
    pub legs: Vec<Leg>,
    pub links: Vec<Link>,
    /// Walked and climbed (m), and the time (s).
    pub length: f64,
    pub time: f64,
}

struct Node {
    level: ElementId,
    p: V2,
}

struct Edge {
    a: usize,
    b: usize,
    kind: LinkKind,
    element: ElementId,
    length: f64,
}

fn level_id(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&crate::id::GlobalId::parse(guid)?)
}

fn polyline(p: &[[f64; 3]]) -> f64 {
    p.windows(2).map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2) + (w[1][2] - w[0][2]).powi(2)).sqrt()).sum()
}

/// The links between levels: stairs, and escalators where allowed.
fn links(store: &Store, mode: EgressMode, nodes: &mut Vec<Node>) -> Vec<Edge> {
    let mut out = Vec::new();
    for (id, _) in crate::stairs::all(store) {
        let Some(e) = store.get(id) else { continue };
        let Some(r) = crate::stairs::stair_recipe(e) else { continue };
        let crate::stairs::StairTop::Level(top_guid) = &r.top else { continue };
        let (Some(base), Some(top)) = (level_id(store, &r.base_level), level_id(store, top_guid)) else { continue };
        let Ok(g) = crate::stairs::regenerate(store, e) else { continue };
        let Ok((z_base, _)) = crate::stairs::elevations(store, e) else { continue };
        let r = r.extended(g.risers);
        let (Some(first), Some(last)) = (r.runs.first(), r.runs.last()) else { continue };
        // Along the middle of the flights and landings.
        let middle = crate::stairs::side_path(&r, z_base, g.riser_height, true, first.width / 2.0);
        let d0 = first.direction;
        let d1 = last.end_direction(r.tread_depth);
        let e1 = last.end(r.tread_depth);
        let a = nodes.len();
        nodes.push(Node { level: base, p: [first.start[0] - d0[0] * 0.4, first.start[1] - d0[1] * 0.4] });
        nodes.push(Node { level: top, p: [e1[0] + d1[0] * 0.4, e1[1] + d1[1] * 0.4] });
        out.push(Edge { a, b: a + 1, kind: LinkKind::Stair, element: id, length: polyline(&middle) + 0.8 });
    }
    if mode != EgressMode::Ibc {
        for (id, el) in crate::escalators::escalators(store) {
            let (Some(lo), Some(hi)) = (level_id(store, &el.levels[0]), level_id(store, &el.levels[1])) else { continue };
            if lo == hi {
                continue;
            }
            let s = &el.sized;
            let a = nodes.len();
            nodes.push(Node { level: lo, p: el.at(-0.6, 0.0) });
            nodes.push(Node { level: hi, p: el.at(s.length() + 0.6, 0.0) });
            let length = s.lower_end + s.run.hypot(s.rise) + s.upper_end + 1.2;
            out.push(Edge { a, b: a + 1, kind: LinkKind::Escalator, element: id, length });
        }
    }
    out
}

/// `egress.route`: the quickest route between two points on (possibly) different levels.
pub fn route(store: &Store, p: RouteParams) -> Result<Route> {
    let s = crate::path_of_travel::settings(store);
    let level = |r: &ElementRef| -> Result<ElementId> {
        let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such level"))?;
        crate::levels::list(store).iter().any(|l| l.id == id).then_some(id).ok_or_else(|| bad("route.not_a_level", format!("element {id} is not a level")))
    };
    let (l0, l1) = (level(&p.start.level)?, level(&p.end.level)?);
    let mut nodes = vec![Node { level: l0, p: p.start.point }, Node { level: l1, p: p.end.point }];
    let edges = links(store, p.mode, &mut nodes);
    // Each level's obstacles, once.
    let mut obstacles: std::collections::HashMap<ElementId, Vec<crate::path_of_travel::Poly>> = std::collections::HashMap::new();
    for n in &nodes {
        obstacles.entry(n.level).or_insert_with(|| crate::path_of_travel::obstacles(store, n.level, &s));
    }
    // Walks between the nodes of each level, worked out as needed.
    let mut walks: std::collections::HashMap<(usize, usize), Option<Vec<V2>>> = std::collections::HashMap::new();
    let mut walk = |a: usize, b: usize| -> Option<Vec<V2>> {
        let key = (a.min(b), a.max(b));
        let w = walks.entry(key).or_insert_with(|| crate::path_of_travel::shortest(nodes[key.0].p, nodes[key.1].p, &obstacles[&nodes[key.0].level])).clone();
        w.map(|mut pts| {
            if a > b {
                pts.reverse();
            }
            pts
        })
    };
    let len = |pts: &[V2]| pts.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum::<f64>();
    // Dijkstra over the nodes, by time.
    let n = nodes.len();
    let mut best = vec![f64::INFINITY; n];
    let mut prev: Vec<Option<(usize, Option<usize>)>> = vec![None; n];
    let mut done = vec![false; n];
    best[0] = 0.0;
    while let Some(u) = (0..n).filter(|&i| !done[i] && best[i].is_finite()).min_by(|&a, &b| best[a].total_cmp(&best[b])) {
        if u == 1 {
            break;
        }
        done[u] = true;
        for v in 0..n {
            if done[v] || v == u || nodes[v].level != nodes[u].level {
                continue;
            }
            if let Some(pts) = walk(u, v) {
                let t = best[u] + len(&pts) / s.walking_speed;
                if t < best[v] {
                    best[v] = t;
                    prev[v] = Some((u, None));
                }
            }
        }
        for (k, e) in edges.iter().enumerate() {
            let v = match (e.a == u, e.b == u) {
                (true, _) => e.b,
                (_, true) => e.a,
                _ => continue,
            };
            let t = best[u] + e.length / s.stair_speed;
            if !done[v] && t < best[v] {
                best[v] = t;
                prev[v] = Some((u, Some(k)));
            }
        }
    }
    if !best[1].is_finite() {
        return Err(bad("route.no_route", "no route between the points: a point is inside an obstacle, or no stair links their levels"));
    }
    // Back from the end.
    let mut steps = Vec::new();
    let mut at = 1;
    while let Some((from, via)) = prev[at] {
        steps.push((from, at, via));
        at = from;
    }
    steps.reverse();
    // Walks on one level in a row make one leg; a link ends it.
    let mut legs: Vec<Leg> = Vec::new();
    let mut out_links = Vec::new();
    let mut current: Option<Leg> = None;
    for (a, b, via) in steps {
        match via {
            Some(k) => {
                if let Some(g) = current.take() {
                    legs.push(g);
                }
                let e = &edges[k];
                out_links.push(Link { kind: e.kind, element: e.element, from: nodes[a].level, to: nodes[b].level, length: e.length, time: e.length / s.stair_speed });
            }
            None => {
                let pts = walk(a, b).expect("walked");
                let l = len(&pts);
                match current.as_mut() {
                    Some(g) if g.level == nodes[a].level => {
                        g.points.extend(pts.into_iter().skip(1));
                        g.length += l;
                    }
                    _ => {
                        if let Some(g) = current.take() {
                            legs.push(g);
                        }
                        current = Some(Leg { level: nodes[a].level, points: pts, length: l });
                    }
                }
            }
        }
    }
    if let Some(g) = current {
        legs.push(g);
    }
    let length = legs.iter().map(|g| g.length).sum::<f64>() + out_links.iter().map(|l| l.length).sum::<f64>();
    Ok(Route { legs, links: out_links, length, time: best[1] })
}

/// What `egress.draw_route` drew.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Drawn {
    #[serde(flatten)]
    pub route: Route,
    /// A path of travel for each leg, in a plan of its level (none where the level has no plan).
    pub paths: Vec<ElementId>,
    /// Levels the route crosses that have no floor plan to draw it in.
    pub unplanned: Vec<ElementId>,
}

/// `egress.draw_route`: the route, each leg drawn as a path of travel (ANA-ROUTE-001) in the first
/// floor plan of its level, with its length and time.
pub(crate) fn draw(txn: &mut crate::txn::Transaction<'_>, p: RouteParams) -> Result<Drawn> {
    let store = txn.store().clone();
    let r = route(&store, p)?;
    let (mut paths, mut unplanned) = (Vec::new(), Vec::new());
    for leg in &r.legs {
        let Some(level_guid) = store.get(leg.level).map(|e| e.guid().to_string()) else { continue };
        let mut plans: Vec<(String, ElementId)> = store
            .iter()
            .filter_map(|e| crate::views::view_data(e).filter(|d| d.kind == crate::plans::KIND_PLAN && d.level.as_deref() == Some(level_guid.as_str())).map(|_| (e.name.clone().unwrap_or_default(), e.id())))
            .collect();
        plans.sort();
        let Some((_, view)) = plans.first() else {
            unplanned.push(leg.level);
            continue;
        };
        let (Some(&start), Some(&end)) = (leg.points.first(), leg.points.last()) else { continue };
        let waypoints = leg.points[1..leg.points.len().saturating_sub(1)].to_vec();
        let id = crate::path_of_travel::create(txn, crate::path_of_travel::CreateParams { view_id: ElementRef::Id(*view), start, end, waypoints })?;
        paths.push(id);
    }
    Ok(Drawn { route: r, paths, unplanned })
}
