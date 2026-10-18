// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Analytical model checks (STR-AM-007): the **support check** (members nothing carries, support
//! chains that loop without reaching a boundary condition, a scope with no boundary condition at
//! all) and the **consistency check** (nodes that look connected but are not, parts with no path
//! to a support, analytical elements that have drifted from or left their physical elements,
//! elements with no counterpart, instability hints and missing properties).
//!
//! Checks are read-only queries over the store: they never change the model. Each issue has a
//! stable key (its code and kind, then the sorted GlobalIds of the elements involved). Ignoring
//! an issue is the one edit (`analytical_model.ignore_issue`): the ignore is kept in the project's
//! `rxd:analysis_checks` with a hash of the involved elements' geometry, so it survives re-checks
//! and saves and lapses when any of them changes.

use std::collections::{BTreeMap, HashMap, HashSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::analytical::{self, AnalyzeAs, MemberGeometry, Role};
use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::walls::RECIPE_KEY;

/// The project parameter holding ignored issues.
pub const CHECKS_KEY: &str = "rxd:analysis_checks";

/// The consistency criteria; C1–C7 are on by default.
pub const CRITERIA: [&str; 8] = ["C1", "C2", "C3", "C4", "C5", "C6", "C7", "C8"];

type V3 = [f64; 3];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Hint,
    Info,
}

/// One issue found by a check.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Issue {
    /// `<code>.<kind>:<sorted GlobalIds>`, stable across runs.
    pub key: String,
    /// `supports` or `consistency`.
    pub check: &'static str,
    /// S1–S3, C1–C8.
    pub code: &'static str,
    pub kind: &'static str,
    pub severity: Severity,
    pub title: &'static str,
    pub message: String,
    pub element_ids: Vec<ElementId>,
    /// The measured distance and the tolerance it broke (metres), where relevant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub measured: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<f64>,
    /// Where it is: a box to zoom to (metres).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<V3>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<V3>,
    pub ignored: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Tolerances {
    pub support_distance: f64,
    pub analytical_distance: f64,
    pub node_merge_distance: f64,
}

/// A check's report.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Report {
    pub run_at: String,
    /// `supports`, `consistency` or both.
    pub checks: Vec<&'static str>,
    pub tolerances: Tolerances,
    pub issues: Vec<Issue>,
    /// Issues per severity, ignored ones left out.
    pub counts_by_severity: BTreeMap<Severity, usize>,
    pub ignored: usize,
    /// Criteria that are on but cannot be checked yet, and why.
    pub not_checked: Vec<String>,
}

impl Report {
    /// The errors that are not ignored.
    pub fn errors(&self) -> impl Iterator<Item = &Issue> {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::Error && !i.ignored)
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckParams {
    /// `"model"` (the default) or a list of elements: analytical members and nodes, loads, or
    /// physical elements (their associated members are checked).
    pub scope: Option<Scope>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Scope {
    Named(String),
    Elements(Vec<ElementRef>),
}

// ---- Geometry ----

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn len(a: V3) -> f64 {
    dot(a, a).sqrt()
}
fn dist(a: V3, b: V3) -> f64 {
    len(sub(a, b))
}

/// The distance from `p` to segment `a`–`b`, and the parameter (0–1) of the nearest point.
fn to_segment(p: V3, a: V3, b: V3) -> (f64, f64) {
    let ab = sub(b, a);
    let l2 = dot(ab, ab);
    let t = if l2 < 1e-18 {
        0.0
    } else {
        (dot(sub(p, a), ab) / l2).clamp(0.0, 1.0)
    };
    let q = [a[0] + ab[0] * t, a[1] + ab[1] * t, a[2] + ab[2] * t];
    (dist(p, q), t)
}

fn bbox(points: &[V3]) -> (V3, V3) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in points {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    (lo, hi)
}

/// Nodes bucketed in cubes of the support distance, for neighbour queries.
struct Grid {
    cell: f64,
    cells: HashMap<[i64; 3], Vec<usize>>,
}

impl Grid {
    fn new(points: &[V3], cell: f64) -> Self {
        let mut cells: HashMap<[i64; 3], Vec<usize>> = HashMap::new();
        let cell = cell.max(1e-3);
        for (i, p) in points.iter().enumerate() {
            cells.entry(Self::key(*p, cell)).or_default().push(i);
        }
        Self { cell, cells }
    }
    fn key(p: V3, cell: f64) -> [i64; 3] {
        [
            (p[0] / cell).floor() as i64,
            (p[1] / cell).floor() as i64,
            (p[2] / cell).floor() as i64,
        ]
    }
    /// Indices of points in the cells overlapping the box, grown by one cell.
    fn near_box(&self, lo: V3, hi: V3) -> Vec<usize> {
        let a = Self::key(lo, self.cell);
        let b = Self::key(hi, self.cell);
        let mut out = Vec::new();
        // A long member spans many cells; past a bound, scan every bucket instead.
        let n = (b[0] - a[0] + 3) * (b[1] - a[1] + 3) * (b[2] - a[2] + 3);
        if n > self.cells.len() as i64 * 4 {
            for (k, v) in &self.cells {
                if (0..3).all(|i| k[i] >= a[i] - 1 && k[i] <= b[i] + 1) {
                    out.extend(v);
                }
            }
            return out;
        }
        for x in a[0] - 1..=b[0] + 1 {
            for y in a[1] - 1..=b[1] + 1 {
                for z in a[2] - 1..=b[2] + 1 {
                    if let Some(v) = self.cells.get(&[x, y, z]) {
                        out.extend(v);
                    }
                }
            }
        }
        out
    }
}

// ---- The model as the checks see it ----

struct Node {
    id: ElementId,
    guid: String,
    p: V3,
    support: Option<crate::loads::Support>,
}

struct Member {
    id: ElementId,
    guid: String,
    g: MemberGeometry,
    /// Node indices of its ends.
    ends: [usize; 2],
}

impl Member {
    fn point(&self, end: usize) -> V3 {
        if end == 0 { self.g.start } else { self.g.end }
    }
    /// Carries load only along its axis.
    fn axial(&self) -> bool {
        matches!(self.g.recipe.role, Role::Brace | Role::Member | Role::Tie)
            || self.g.recipe.analyze_as != AnalyzeAs::Releases
    }
    /// A column needs support at its lower end; everything else at both ends.
    fn needs_support(&self) -> Vec<usize> {
        if self.g.recipe.role == Role::Column {
            vec![if self.g.start[2] <= self.g.end[2] {
                0
            } else {
                1
            }]
        } else {
            vec![0, 1]
        }
    }
}

struct Panel {
    id: ElementId,
    g: crate::analytical_panels::PanelGeometry,
    /// Node indices of its corners.
    nodes: Vec<usize>,
}

struct Model<'a> {
    store: &'a Store,
    nodes: Vec<Node>,
    members: Vec<Member>,
    panels: Vec<Panel>,
    /// Node index → (member index, end).
    at_node: Vec<Vec<(usize, usize)>>,
    /// Node index → panel indices.
    panel_at_node: Vec<Vec<usize>>,
    grid: Grid,
    settings: crate::structural::Settings,
}

fn guid_of(store: &Store, id: ElementId) -> String {
    store
        .get(id)
        .map(|e| e.guid().to_string())
        .unwrap_or_default()
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

impl<'a> Model<'a> {
    fn new(store: &'a Store) -> Self {
        let settings = crate::structural::settings(store);
        let mut nodes = Vec::new();
        let mut index: HashMap<ElementId, usize> = HashMap::new();
        for e in store.iter() {
            let Some(r) = analytical::node_recipe(e) else {
                continue;
            };
            let Some(p) = analytical::node_point(store, e) else {
                continue;
            };
            index.insert(e.id(), nodes.len());
            nodes.push(Node {
                id: e.id(),
                guid: e.guid().to_string(),
                p,
                support: r.support,
            });
        }
        let mut members = Vec::new();
        let mut at_node = vec![Vec::new(); nodes.len()];
        for (id, g) in analytical::members(store) {
            let (Some(&a), Some(&b)) = (index.get(&g.nodes[0]), index.get(&g.nodes[1])) else {
                continue;
            };
            at_node[a].push((members.len(), 0));
            at_node[b].push((members.len(), 1));
            members.push(Member {
                id,
                guid: guid_of(store, id),
                g,
                ends: [a, b],
            });
        }
        let mut panels = Vec::new();
        let mut panel_at_node = vec![Vec::new(); nodes.len()];
        for (id, g) in crate::analytical_panels::panels(store) {
            let Some(ns) = g
                .nodes
                .iter()
                .map(|n| index.get(n).copied())
                .collect::<Option<Vec<usize>>>()
            else {
                continue;
            };
            for n in &ns {
                panel_at_node[*n].push(panels.len());
            }
            panels.push(Panel { id, g, nodes: ns });
        }
        let points: Vec<V3> = nodes.iter().map(|n| n.p).collect();
        let grid = Grid::new(&points, settings.support_distance);
        Self {
            store,
            nodes,
            members,
            panels,
            at_node,
            panel_at_node,
            grid,
            settings,
        }
    }

    fn supported(&self, n: usize) -> bool {
        self.nodes[n].support.is_some()
    }

    /// Whether a member or panel uses node `n`.
    fn used(&self, n: usize) -> bool {
        !self.at_node[n].is_empty() || !self.panel_at_node[n].is_empty()
    }

    /// Members and panels as one list of units: members first.
    fn units(&self) -> usize {
        self.members.len() + self.panels.len()
    }
    fn unit_id(&self, u: usize) -> ElementId {
        if u < self.members.len() {
            self.members[u].id
        } else {
            self.panels[u - self.members.len()].id
        }
    }
    fn unit_points(&self, u: usize) -> Vec<V3> {
        if u < self.members.len() {
            vec![self.members[u].g.start, self.members[u].g.end]
        } else {
            self.panels[u - self.members.len()].g.corners.clone()
        }
    }
    /// Panels (as units) that `p` lies on, within `d`.
    fn panels_at(&self, p: V3, d: f64) -> Vec<usize> {
        let m = self.members.len();
        self.panels
            .iter()
            .enumerate()
            .filter(|(_, x)| x.g.contains(p, d))
            .map(|(i, _)| m + i)
            .collect()
    }

    /// Nodes within `d` of `p`.
    fn nodes_near(&self, p: V3, d: f64) -> Vec<usize> {
        self.grid
            .near_box(p, p)
            .into_iter()
            .filter(|&i| dist(self.nodes[i].p, p) <= d)
            .collect()
    }

    /// Members (other than `except`) whose axis passes within `d` of `p`, with the distance and
    /// where along them.
    fn members_near(&self, p: V3, d: f64, except: usize) -> Vec<(usize, f64, f64)> {
        self.members
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != except)
            .filter_map(|(i, m)| {
                let (lo, hi) = bbox(&[m.g.start, m.g.end]);
                if (0..3).any(|k| p[k] < lo[k] - d || p[k] > hi[k] + d) {
                    return None;
                }
                let (s, t) = to_segment(p, m.g.start, m.g.end);
                (s <= d).then_some((i, s, t))
            })
            .collect()
    }
}

// ---- Issues ----

struct Found {
    code: &'static str,
    kind: &'static str,
    severity: Severity,
    title: &'static str,
    message: String,
    elements: Vec<ElementId>,
    measured: Option<f64>,
    tolerance: Option<f64>,
    at: Vec<V3>,
}

fn mm(m: f64) -> String {
    format!("{:.0} mm", m * 1000.0)
}

fn label(store: &Store, id: ElementId) -> String {
    let Some(e) = store.get(id) else {
        return format!("#{id}");
    };
    let what = if analytical::member_recipe(e).is_some() {
        "analytical member"
    } else if crate::analytical_panels::panel_recipe(e).is_some() {
        "analytical panel"
    } else if crate::walls::wall_recipe(e).is_some() {
        "wall"
    } else if crate::floors::slab_recipe(e).is_some() {
        "floor"
    } else if analytical::node_recipe(e).is_some() {
        "node"
    } else if crate::loads::point_load(e).is_some() || crate::loads::line_load(e).is_some() {
        "load"
    } else if crate::columns::column_recipe(e).is_some() {
        "column"
    } else if crate::beams::beam_recipe(e).is_some_and(|r| r.brace) {
        "brace"
    } else if crate::beams::beam_recipe(e).is_some() {
        "beam"
    } else {
        e.class.trim_start_matches("Ifc")
    };
    match e.name.as_deref().filter(|n| !n.is_empty()) {
        Some(n) => format!("{what} {id} ({n})"),
        None => format!("{what} {id}"),
    }
}

// ---- The support check ----

fn check_supports(m: &Model<'_>, out: &mut Vec<Found>) {
    let d = m.settings.support_distance;
    if m.units() > 0 && !m.nodes.iter().any(|n| n.support.is_some()) {
        out.push(Found {
            code: "S3",
            kind: "no_supports",
            severity: Severity::Error,
            title: "No supports in scope",
            message: "the analytical model has no boundary condition: add supports (for example \
                      at the column bases)"
                .into(),
            elements: Vec::new(),
            measured: None,
            tolerance: None,
            at: (0..m.units()).flat_map(|u| m.unit_points(u)).collect(),
        });
    }
    // What carries each end that needs support: a boundary condition, or other members (sharing
    // its node, with a node or axis within the support distance).
    let nm = m.members.len();
    let mut carriers: Vec<Vec<(usize, bool, Vec<usize>)>> = Vec::new();
    for (i, mem) in m.members.iter().enumerate() {
        let mut ends = Vec::new();
        for end in mem.needs_support() {
            let n = mem.ends[end];
            let p = mem.point(end);
            let bc = m.supported(n)
                || m.nodes_near(p, d)
                    .into_iter()
                    .any(|k| m.supported(k) && m.at_node[k].iter().all(|(x, _)| *x != i));
            let mut by: HashSet<usize> = m.at_node[n]
                .iter()
                .map(|(x, _)| *x)
                .filter(|x| *x != i)
                .collect();
            for (x, _, _) in m.members_near(p, d, i) {
                by.insert(x);
            }
            by.extend(m.panel_at_node[n].iter().map(|x| nm + x));
            by.extend(m.panels_at(p, d));
            let mut by: Vec<usize> = by.into_iter().collect();
            by.sort();
            ends.push((end, bc, by));
        }
        carriers.push(ends);
    }
    // A panel is carried at its corners, or by members lying on it.
    for (i, pan) in m.panels.iter().enumerate() {
        let me = nm + i;
        let bc = pan.nodes.iter().any(|n| m.supported(*n));
        let mut by: HashSet<usize> = HashSet::new();
        for n in &pan.nodes {
            by.extend(m.at_node[*n].iter().map(|(x, _)| *x));
            by.extend(
                m.panel_at_node[*n]
                    .iter()
                    .map(|x| nm + x)
                    .filter(|x| *x != me),
            );
        }
        for (x, mem) in m.members.iter().enumerate() {
            let mid = [0, 1, 2].map(|k| (mem.g.start[k] + mem.g.end[k]) / 2.0);
            if pan.g.contains(mid, d) {
                by.insert(x);
            }
        }
        let mut by: Vec<usize> = by.into_iter().collect();
        by.sort();
        carriers.push(vec![(0, bc, by)]);
    }
    for (i, ends) in carriers.iter().enumerate() {
        let loose: Vec<usize> = ends
            .iter()
            .filter(|(_, bc, by)| !bc && by.is_empty())
            .map(|(end, _, _)| *end)
            .collect();
        if loose.is_empty() {
            continue;
        }
        if i >= nm {
            let pan = &m.panels[i - nm];
            out.push(Found {
                code: "S1",
                kind: "unsupported_panel",
                severity: Severity::Warning,
                title: "Unsupported panel",
                message: format!(
                    "{} is not supported: no member, panel or boundary condition at its corners or \
                     on it",
                    label(m.store, pan.id)
                ),
                elements: vec![pan.id],
                measured: None,
                tolerance: Some(d),
                at: pan.g.corners.clone(),
            });
            continue;
        }
        let mem = &m.members[i];
        let which = match (mem.g.recipe.role == Role::Column, loose.as_slice()) {
            (true, _) => "its lower end",
            (false, [0]) => "its start",
            (false, [1]) => "its end",
            _ => "either end",
        };
        out.push(Found {
            code: "S1",
            kind: "unsupported_member",
            severity: Severity::Warning,
            title: "Unsupported member",
            message: format!(
                "{} is not supported at {which}: nothing lies within the support distance ({})",
                label(m.store, mem.id),
                mm(d)
            ),
            elements: vec![mem.id],
            measured: None,
            tolerance: Some(d),
            at: loose.iter().map(|e| mem.point(*e)).collect(),
        });
    }
    // Circular support (S2): members whose ends are all carried, but whose chain of carriers
    // never reaches a boundary condition.
    if !m.settings.circular_references_check || !m.nodes.iter().any(|n| n.support.is_some()) {
        return;
    }
    let mut grounded = vec![false; m.units()];
    loop {
        let mut changed = false;
        for (i, ends) in carriers.iter().enumerate() {
            if grounded[i] {
                continue;
            }
            if ends
                .iter()
                .any(|(_, bc, by)| *bc || by.iter().any(|x| grounded[*x]))
            {
                grounded[i] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let looping: Vec<usize> = (0..m.units())
        .filter(|i| !grounded[*i] && carriers[*i].iter().all(|(_, _, by)| !by.is_empty()))
        .collect();
    // One issue per group of members carrying one another.
    let mut seen = HashSet::new();
    for &start in &looping {
        if !seen.insert(start) {
            continue;
        }
        let mut group = vec![start];
        let mut k = 0;
        while k < group.len() {
            let i = group[k];
            for (_, _, by) in &carriers[i] {
                for &x in by {
                    if looping.contains(&x) && seen.insert(x) {
                        group.push(x);
                    }
                }
            }
            k += 1;
        }
        group.sort();
        let names: Vec<String> = group
            .iter()
            .take(4)
            .map(|i| label(m.store, m.unit_id(*i)))
            .collect();
        out.push(Found {
            code: "S2",
            kind: "circular_support",
            severity: Severity::Warning,
            title: "Circular support",
            message: format!(
                "{}{} carry one another without reaching a boundary condition",
                names.join(", "),
                if group.len() > 4 {
                    format!(" and {} more", group.len() - 4)
                } else {
                    String::new()
                }
            ),
            elements: group.iter().map(|i| m.unit_id(*i)).collect(),
            measured: None,
            tolerance: None,
            at: group.iter().flat_map(|i| m.unit_points(*i)).collect(),
        });
    }
}

// ---- The consistency check ----

fn criterion_on(s: &crate::structural::Settings, code: &str) -> bool {
    s.consistency_criteria.iter().any(|c| c == code)
}

fn check_consistency(m: &Model<'_>, out: &mut Vec<Found>, not_checked: &mut Vec<String>) {
    let s = &m.settings;
    let (d, merge, a_tol) = (
        s.support_distance,
        s.node_merge_distance,
        s.analytical_distance,
    );
    if criterion_on(s, "C1") {
        disconnected(m, d, merge, out);
        loads_off_host(m, out);
    }
    if criterion_on(s, "C2") {
        isolated_parts(m, out);
    }
    if criterion_on(s, "C3") || criterion_on(s, "C4") || criterion_on(s, "C5") {
        counterparts(m, a_tol, out);
        panel_counterparts(m, a_tol, out);
    }
    if criterion_on(s, "C6") {
        overlaps(m, d, out);
    }
    let _ = not_checked;
    if criterion_on(s, "C7") {
        instability(m, out);
    }
    if criterion_on(s, "C8") {
        missing_properties(m, out);
    }
}

/// C1: end nodes closer than the support distance but farther than the merge distance; an end
/// on another member's span with no node there; free ends.
fn disconnected(m: &Model<'_>, d: f64, merge: f64, out: &mut Vec<Found>) {
    let mut pairs = HashSet::new();
    for (i, n) in m.nodes.iter().enumerate() {
        if !m.used(i) {
            continue;
        }
        let mine: HashSet<usize> = m.at_node[i].iter().map(|(x, _)| *x).collect();
        for k in m.nodes_near(n.p, d) {
            if k == i || !m.used(k) {
                continue;
            }
            // The two ends of one member, or two corners of one panel, are not a gap.
            if m.at_node[k].iter().any(|(x, _)| mine.contains(x))
                || m.panel_at_node[k]
                    .iter()
                    .any(|x| m.panel_at_node[i].contains(x))
            {
                continue;
            }
            let gap = dist(n.p, m.nodes[k].p);
            if gap <= merge || !pairs.insert((i.min(k), i.max(k))) {
                continue;
            }
            out.push(Found {
                code: "C1",
                kind: "disconnected_nodes",
                severity: Severity::Warning,
                title: "Disconnected nodes",
                message: format!(
                    "{} and {} look connected but are not: node gap {} > merge distance {}",
                    label(m.store, n.id),
                    label(m.store, m.nodes[k].id),
                    mm(gap),
                    mm(merge)
                ),
                elements: vec![n.id, m.nodes[k].id],
                measured: Some(gap),
                tolerance: Some(merge),
                at: vec![n.p, m.nodes[k].p],
            });
        }
    }
    for (i, n) in m.nodes.iter().enumerate() {
        let using = &m.at_node[i];
        if using.is_empty() {
            continue;
        }
        let mine: HashSet<usize> = using.iter().map(|(x, _)| *x).collect();
        let mut on_span = false;
        for (x, gap, t) in m.members_near(n.p, d, usize::MAX) {
            if mine.contains(&x) {
                continue;
            }
            let other = &m.members[x];
            // Near the other member's end node, the node pair above covers it.
            let l = dist(other.g.start, other.g.end);
            if t * l <= d || (1.0 - t) * l <= d {
                continue;
            }
            on_span = true;
            out.push(Found {
                code: "C1",
                kind: "end_on_span",
                severity: Severity::Warning,
                title: "Disconnected nodes",
                message: format!(
                    "{} lies on the span of {} ({} away) but is not connected: it has no node there",
                    label(m.store, n.id),
                    label(m.store, other.id),
                    mm(gap)
                ),
                elements: vec![n.id, other.id],
                measured: Some(gap),
                tolerance: Some(d),
                at: vec![n.p],
            });
        }
        if using.len() == 1 && m.panel_at_node[i].is_empty() && n.support.is_none() && !on_span {
            let alone = m
                .nodes_near(n.p, d)
                .into_iter()
                .all(|k| k == i || !m.used(k));
            if alone {
                let mem = &m.members[using[0].0];
                out.push(Found {
                    code: "C1",
                    kind: "free_end",
                    severity: Severity::Warning,
                    title: "Free member end",
                    message: format!(
                        "{} ends at {} which touches nothing and has no boundary condition",
                        label(m.store, mem.id),
                        label(m.store, n.id)
                    ),
                    elements: vec![mem.id, n.id],
                    measured: None,
                    tolerance: None,
                    at: vec![n.p],
                });
            }
        }
    }
}

/// C1, *Load off host*: a load whose host is gone or not part of the model, or a point load
/// placed beyond its member's ends.
fn loads_off_host(m: &Model<'_>, out: &mut Vec<Found>) {
    let used: HashSet<&str> = m
        .members
        .iter()
        .flat_map(|x| x.ends.iter().map(|n| m.nodes[*n].guid.as_str()))
        .chain(
            m.panels
                .iter()
                .flat_map(|x| x.nodes.iter().map(|n| m.nodes[*n].guid.as_str())),
        )
        .collect();
    let members: HashMap<&str, &Member> = m.members.iter().map(|x| (x.guid.as_str(), x)).collect();
    for e in m.store.iter() {
        let (host, at) = if let Some(l) = crate::loads::point_load(e) {
            (l.host, Some(l.at))
        } else if let Some(l) = crate::loads::line_load(e) {
            (l.host, None)
        } else {
            continue;
        };
        let why = if let Some(mem) = members.get(host.as_str()) {
            match at {
                Some(t) if !(-1e-9..=1.0 + 1e-9).contains(&t) => Some((
                    format!(
                        "it sits at {:.0}% along {}, beyond its ends",
                        t * 100.0,
                        label(m.store, mem.id)
                    ),
                    vec![mem.g.start, mem.g.end],
                )),
                _ => None,
            }
        } else if used.contains(host.as_str()) {
            None
        } else if let Some(nid) = find(m.store, &host) {
            Some((
                format!("its host {} belongs to no member", label(m.store, nid)),
                m.nodes
                    .iter()
                    .find(|n| n.id == nid)
                    .map(|n| vec![n.p])
                    .unwrap_or_default(),
            ))
        } else {
            Some(("its host is gone".to_owned(), Vec::new()))
        };
        if let Some((why, at)) = why {
            out.push(Found {
                code: "C1",
                kind: "load_off_host",
                severity: Severity::Warning,
                title: "Load off host",
                message: format!("{}: {why}", label(m.store, e.id())),
                elements: vec![e.id()],
                measured: None,
                tolerance: None,
                at,
            });
        }
    }
}

/// C2: groups of members and panels connected through shared nodes with no boundary condition.
fn isolated_parts(m: &Model<'_>, out: &mut Vec<Found>) {
    let nm = m.members.len();
    let n = m.units();
    let unit_nodes = |u: usize| -> Vec<usize> {
        if u < nm {
            m.members[u].ends.to_vec()
        } else {
            m.panels[u - nm].nodes.clone()
        }
    };
    let mut part = vec![usize::MAX; n];
    let mut parts = 0;
    for start in 0..n {
        if part[start] != usize::MAX {
            continue;
        }
        let mut stack = vec![start];
        part[start] = parts;
        while let Some(u) = stack.pop() {
            for k in unit_nodes(u) {
                let next = m.at_node[k]
                    .iter()
                    .map(|(x, _)| *x)
                    .chain(m.panel_at_node[k].iter().map(|x| nm + x));
                for x in next {
                    if part[x] == usize::MAX {
                        part[x] = parts;
                        stack.push(x);
                    }
                }
            }
        }
        parts += 1;
    }
    for p in 0..parts {
        let group: Vec<usize> = (0..n).filter(|i| part[*i] == p).collect();
        let held = group
            .iter()
            .any(|u| unit_nodes(*u).iter().any(|k| m.supported(*k)));
        if held {
            continue;
        }
        let what = match (
            group.iter().filter(|u| **u < nm).count(),
            group.iter().filter(|u| **u >= nm).count(),
        ) {
            (a, 0) => format!("{a} connected member(s)"),
            (0, b) => format!("{b} connected panel(s)"),
            (a, b) => format!("{a} member(s) and {b} panel(s), connected,"),
        };
        out.push(Found {
            code: "C2",
            kind: "isolated_part",
            severity: Severity::Error,
            title: "Isolated part",
            message: format!(
                "{what} from {}, have no boundary condition: the solver would see a mechanism",
                label(m.store, m.unit_id(group[0]))
            ),
            elements: group.iter().map(|i| m.unit_id(*i)).collect(),
            measured: None,
            tolerance: None,
            at: group.iter().flat_map(|i| m.unit_points(*i)).collect(),
        });
    }
}

/// C6: a physical beam on a structural floor, or a wall under one, whose analytical member or
/// panel neither shares a node with the floor's panel nor lies within the support distance of it.
fn overlaps(m: &Model<'_>, d: f64, out: &mut Vec<Found>) {
    let floors: Vec<&Panel> = m
        .panels
        .iter()
        .filter(|p| {
            p.g.recipe.role != crate::analytical_panels::PanelRole::Wall && p.g.normal.is_some()
        })
        .filter(|p| !p.g.recipe.physical.is_empty())
        .collect();
    if floors.is_empty() {
        return;
    }
    // In plan, inside the floor's outline, and within a metre of its height.
    let over = |f: &Panel, q: V3| -> bool {
        let ring: Vec<[f64; 2]> = f.g.corners.iter().map(|c| [c[0], c[1]]).collect();
        let z = f.g.corners.iter().map(|c| c[2]).sum::<f64>() / f.g.corners.len() as f64;
        let near_edge = f.g.corners.iter().enumerate().any(|(i, a)| {
            let b = f.g.corners[(i + 1) % f.g.corners.len()];
            to_segment([q[0], q[1], 0.0], [a[0], a[1], 0.0], [b[0], b[1], 0.0]).0 <= d
        });
        (q[2] - z).abs() <= 1.0 && (in_ring([q[0], q[1]], &ring) || near_edge)
    };
    for f in &floors {
        for mem in m.members.iter().filter(|x| !x.g.recipe.physical.is_empty()) {
            if mem.g.recipe.role == Role::Column {
                continue;
            }
            let mid = [0, 1, 2].map(|k| (mem.g.start[k] + mem.g.end[k]) / 2.0);
            if !over(f, mid) {
                continue;
            }
            let shares = mem.ends.iter().any(|n| f.nodes.contains(n));
            let gap = f.g.surface_distance(mid);
            if shares || gap <= d {
                continue;
            }
            out.push(Found {
                code: "C6",
                kind: "beam_slab",
                severity: Severity::Warning,
                title: "Overlap not reflected",
                message: format!(
                    "{} runs under {} but lies {} from its panel {}: they are not connected",
                    label(m.store, mem.id),
                    label(m.store, f.id),
                    mm(gap),
                    format_args!("(support distance {})", mm(d))
                ),
                elements: vec![mem.id, f.id],
                measured: Some(gap),
                tolerance: Some(d),
                at: vec![mem.g.start, mem.g.end],
            });
        }
        for w in m.panels.iter().filter(|p| {
            p.g.recipe.role == crate::analytical_panels::PanelRole::Wall
                && !p.g.recipe.physical.is_empty()
        }) {
            // The wall's top edge: its third and fourth corners.
            let Some(top) = w.g.corners.get(2..4) else {
                continue;
            };
            let mid = [0, 1, 2].map(|k| (top[0][k] + top[1][k]) / 2.0);
            if !over(f, mid) {
                continue;
            }
            let shares = w.nodes.iter().any(|n| f.nodes.contains(n));
            let gap = f.g.surface_distance(mid);
            if shares || gap <= d {
                continue;
            }
            out.push(Found {
                code: "C6",
                kind: "wall_slab",
                severity: Severity::Warning,
                title: "Overlap not reflected",
                message: format!(
                    "{}'s top lies {} from {}: the wall and slab panels are not connected",
                    label(m.store, w.id),
                    mm(gap),
                    label(m.store, f.id)
                ),
                elements: vec![w.id, f.id],
                measured: Some(gap),
                tolerance: Some(d),
                at: top.to_vec(),
            });
        }
    }
}

fn in_ring(q: [f64; 2], ring: &[[f64; 2]]) -> bool {
    let mut c = false;
    let n = ring.len();
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + n - 1) % n]);
        if (a[1] > q[1]) != (b[1] > q[1])
            && q[0] < (b[0] - a[0]) * (q[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            c = !c;
        }
    }
    c
}

/// C3, C4 and C5 for panels: against the panel their wall or floor would have, and walls and
/// floors without one.
fn panel_counterparts(m: &Model<'_>, a_tol: f64, out: &mut Vec<Found>) {
    let s = &m.settings;
    let (c3, c4, c5) = (
        criterion_on(s, "C3"),
        criterion_on(s, "C4"),
        criterion_on(s, "C5"),
    );
    let mut associated: HashSet<String> = HashSet::new();
    for pan in &m.panels {
        let r = &pan.g.recipe;
        associated.extend(r.physical.iter().cloned());
        let physical: Vec<ElementId> = r.physical.iter().filter_map(|g| find(m.store, g)).collect();
        if c5 && r.physical.is_empty() {
            out.push(Found {
                code: "C5",
                kind: "no_physical",
                severity: Severity::Info,
                title: "No counterpart",
                message: format!(
                    "{} has no physical element (allowed; analytical-only)",
                    label(m.store, pan.id)
                ),
                elements: vec![pan.id],
                measured: None,
                tolerance: None,
                at: pan.g.corners.clone(),
            });
        } else if c5 && physical.is_empty() {
            out.push(Found {
                code: "C5",
                kind: "lost_association",
                severity: Severity::Warning,
                title: "No counterpart",
                message: format!(
                    "{} lost its physical element: it was deleted",
                    label(m.store, pan.id)
                ),
                elements: vec![pan.id],
                measured: None,
                tolerance: None,
                at: pan.g.corners.clone(),
            });
        }
        for pid in physical {
            let pe = m.store.get(pid).expect("found");
            let Ok((pts, defaults)) = crate::analytical_panels::physical_panel(m.store, pe) else {
                continue;
            };
            // The default panel's surface, from its corners (the arc's circle when curved).
            let (outline, circle) = (
                pts.clone(),
                defaults.arc.as_ref().and_then(|a| {
                    crate::walls::curved::circle(
                        [pts[0][0], pts[0][1]],
                        a.through,
                        [pts[1][0], pts[1][1]],
                    )
                }),
            );
            let reference = crate::analytical_panels::PanelGeometry {
                nodes: Vec::new(),
                normal: if circle.is_none() {
                    crate::analytical_panels::normal(&pts)
                } else {
                    None
                },
                corners: pts,
                outline,
                circle,
                recipe: defaults.clone(),
            };
            let gap = pan
                .g
                .corners
                .iter()
                .map(|c| reference.surface_distance(*c))
                .fold(0.0, f64::max);
            if c3 && gap > a_tol {
                out.push(Found {
                    code: "C3",
                    kind: "position_mismatch",
                    severity: Severity::Warning,
                    title: "Physical/analytical mismatch",
                    message: format!(
                        "{} lies {} from {}'s {} > analytical-to-physical distance {}",
                        label(m.store, pan.id),
                        mm(gap),
                        label(m.store, pid),
                        if r.role == crate::analytical_panels::PanelRole::Wall {
                            "centre plane"
                        } else {
                            "top"
                        },
                        mm(a_tol)
                    ),
                    elements: vec![pan.id, pid],
                    measured: Some(gap),
                    tolerance: Some(a_tol),
                    at: pan.g.corners.clone(),
                });
            }
            if c3 && (r.thickness - defaults.thickness).abs() > 1e-4 {
                out.push(Found {
                    code: "C3",
                    kind: "thickness_mismatch",
                    severity: Severity::Warning,
                    title: "Physical/analytical mismatch",
                    message: format!(
                        "{} is {} thick but {} is {}",
                        label(m.store, pan.id),
                        mm(r.thickness),
                        label(m.store, pid),
                        mm(defaults.thickness)
                    ),
                    elements: vec![pan.id, pid],
                    measured: None,
                    tolerance: None,
                    at: pan.g.corners.clone(),
                });
            }
            if c3
                && r.material.is_some()
                && defaults.material.is_some()
                && r.material != defaults.material
            {
                out.push(Found {
                    code: "C3",
                    kind: "material_mismatch",
                    severity: Severity::Warning,
                    title: "Physical/analytical mismatch",
                    message: format!(
                        "{} is {} but {} is {}",
                        label(m.store, pan.id),
                        r.material.as_deref().unwrap_or_default(),
                        label(m.store, pid),
                        defaults.material.as_deref().unwrap_or_default()
                    ),
                    elements: vec![pan.id, pid],
                    measured: None,
                    tolerance: None,
                    at: pan.g.corners.clone(),
                });
            }
            // C4: a wall's panel outside the wall's thickness; a floor's below its underside or
            // above its top.
            let outside = if r.role == crate::analytical_panels::PanelRole::Wall {
                (gap - defaults.thickness / 2.0).max(0.0)
            } else {
                let top = reference.corners[0][2];
                pan.g
                    .corners
                    .iter()
                    .map(|c| (c[2] - top).max(top - defaults.thickness - c[2]).max(0.0))
                    .fold(0.0, f64::max)
            };
            if c4 && outside > a_tol {
                out.push(Found {
                    code: "C4",
                    kind: "outside_physical",
                    severity: Severity::Warning,
                    title: "Analytical outside physical",
                    message: format!(
                        "{} leaves {}'s body by {} > analytical-to-physical distance {}",
                        label(m.store, pan.id),
                        label(m.store, pid),
                        mm(outside),
                        mm(a_tol)
                    ),
                    elements: vec![pan.id, pid],
                    measured: Some(outside),
                    tolerance: Some(a_tol),
                    at: pan.g.corners.clone(),
                });
            }
        }
    }
    if c5 {
        for e in m.store.iter() {
            let bearing_wall = crate::walls::wall_recipe(e).is_some()
                && crate::structural::wall_usage(e) != crate::structural::WallUsage::NonBearing;
            if !(bearing_wall || crate::structural::is_structural_floor(e))
                || associated.contains(&e.guid().to_string())
            {
                continue;
            }
            out.push(Found {
                code: "C5",
                kind: "no_analytical",
                severity: Severity::Info,
                title: "No counterpart",
                message: format!("{} has no analytical panel", label(m.store, e.id())),
                elements: vec![e.id()],
                measured: None,
                tolerance: None,
                at: Vec::new(),
            });
        }
    }
}

/// Whether a physical element counts as load-bearing framing that should have an analytical
/// member.
fn structural_framing(store: &Store, e: &Element) -> bool {
    if crate::beams::beam_recipe(e).is_some() {
        return true;
    }
    crate::columns::column_recipe(e).is_some()
        && crate::columns::regenerate(store, e).is_ok_and(|g| g.ty.structural)
}

/// How far `p` lies outside a physical element's body (metres; 0 inside), or `None` when its
/// body is unknown.
fn outside_body(store: &Store, e: &Element, p: V3) -> Option<f64> {
    if crate::beams::beam_recipe(e).is_some() {
        // Along the location line rather than the body, which is cut back at joins: analytical
        // ends at the supports' centrelines are where they should be.
        let g = crate::beams::regenerate(store, e).ok()?;
        let l = len(sub(g.b, g.a));
        if l < 1e-9 {
            return None;
        }
        let r = sub(p, g.a);
        let t = dot(r, g.u);
        let (u, v) = (dot(r, g.left), dot(r, g.up));
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for q in &g.outline {
            for k in 0..2 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
        }
        let out = |x: f64, a: f64, b: f64| (a - x).max(x - b).max(0.0);
        let along = out(t, 0.0, l);
        let across = out(u, lo[0], hi[0]).hypot(out(v, lo[1], hi[1]));
        return Some(along.max(across));
    }
    if crate::columns::column_recipe(e).is_some() {
        let g = crate::columns::regenerate(store, e).ok()?;
        let (a, b) = g.axis();
        let (r, t) = to_segment(p, a, b);
        let l = dist(a, b);
        let ab = sub(b, a);
        // Beyond the ends: how far along; beside: past the section's half-diagonal.
        let s = if l > 1e-9 {
            dot(sub(p, a), ab) / l
        } else {
            0.0
        };
        let along = (-s).max(s - l).max(0.0);
        let half = 0.5 * g.ty.width.hypot(g.ty.depth);
        let across = if t > 0.0 && t < 1.0 {
            (r - half).max(0.0)
        } else {
            0.0
        };
        return Some(along.max(across));
    }
    None
}

/// C3, C4 and C5: associated members against their physical elements, and elements without a
/// counterpart.
fn counterparts(m: &Model<'_>, a_tol: f64, out: &mut Vec<Found>) {
    let s = &m.settings;
    let (c3, c4, c5) = (
        criterion_on(s, "C3"),
        criterion_on(s, "C4"),
        criterion_on(s, "C5"),
    );
    let mut associated: HashSet<String> = HashSet::new();
    for mem in &m.members {
        let r = &mem.g.recipe;
        associated.extend(r.physical.iter().cloned());
        let physical: Vec<ElementId> = r.physical.iter().filter_map(|g| find(m.store, g)).collect();
        if c5 && r.physical.is_empty() {
            out.push(Found {
                code: "C5",
                kind: "no_physical",
                severity: Severity::Info,
                title: "No counterpart",
                message: format!(
                    "{} has no physical element (allowed; analytical-only)",
                    label(m.store, mem.id)
                ),
                elements: vec![mem.id],
                measured: None,
                tolerance: None,
                at: vec![mem.g.start, mem.g.end],
            });
        } else if c5 && physical.is_empty() {
            out.push(Found {
                code: "C5",
                kind: "lost_association",
                severity: Severity::Warning,
                title: "No counterpart",
                message: format!(
                    "{} lost its physical element: it was deleted",
                    label(m.store, mem.id)
                ),
                elements: vec![mem.id],
                measured: None,
                tolerance: None,
                at: vec![mem.g.start, mem.g.end],
            });
        }
        for pid in physical {
            let pe = m.store.get(pid).expect("found");
            if c3 && let Ok((a, b, defaults)) = analytical::physical_member(m.store, pe) {
                // The member's ends against the physical axis (a member split where framing
                // meets it covers only part of it).
                let gap = [
                    to_segment(mem.g.start, a, b).0,
                    to_segment(mem.g.end, a, b).0,
                ]
                .into_iter()
                .fold(0.0, f64::max);
                if gap > a_tol {
                    out.push(Found {
                        code: "C3",
                        kind: "position_mismatch",
                        severity: Severity::Warning,
                        title: "Physical/analytical mismatch",
                        message: format!(
                            "{} lies {} from {}'s axis > analytical-to-physical distance {}",
                            label(m.store, mem.id),
                            mm(gap),
                            label(m.store, pid),
                            mm(a_tol)
                        ),
                        elements: vec![mem.id, pid],
                        measured: Some(gap),
                        tolerance: Some(a_tol),
                        at: vec![mem.g.start, mem.g.end, a, b],
                    });
                }
                let differs =
                    |x: &Option<String>, y: &Option<String>| x.is_some() && y.is_some() && x != y;
                if differs(&r.section_type, &defaults.section_type) {
                    out.push(Found {
                        code: "C3",
                        kind: "section_mismatch",
                        severity: Severity::Warning,
                        title: "Physical/analytical mismatch",
                        message: format!(
                            "{} has a different section type from {}",
                            label(m.store, mem.id),
                            label(m.store, pid)
                        ),
                        elements: vec![mem.id, pid],
                        measured: None,
                        tolerance: None,
                        at: vec![mem.g.start, mem.g.end],
                    });
                }
                if differs(&r.material, &defaults.material) {
                    out.push(Found {
                        code: "C3",
                        kind: "material_mismatch",
                        severity: Severity::Warning,
                        title: "Physical/analytical mismatch",
                        message: format!(
                            "{} is {} but {} is {}",
                            label(m.store, mem.id),
                            r.material.as_deref().unwrap_or_default(),
                            label(m.store, pid),
                            defaults.material.as_deref().unwrap_or_default()
                        ),
                        elements: vec![mem.id, pid],
                        measured: None,
                        tolerance: None,
                        at: vec![mem.g.start, mem.g.end],
                    });
                }
            }
            if c4 {
                let worst = [mem.g.start, mem.g.end]
                    .into_iter()
                    .filter_map(|p| outside_body(m.store, pe, p))
                    .fold(0.0, f64::max);
                if worst > a_tol {
                    out.push(Found {
                        code: "C4",
                        kind: "outside_physical",
                        severity: Severity::Warning,
                        title: "Analytical outside physical",
                        message: format!(
                            "{} leaves {}'s body by {} > analytical-to-physical distance {}",
                            label(m.store, mem.id),
                            label(m.store, pid),
                            mm(worst),
                            mm(a_tol)
                        ),
                        elements: vec![mem.id, pid],
                        measured: Some(worst),
                        tolerance: Some(a_tol),
                        at: vec![mem.g.start, mem.g.end],
                    });
                }
            }
        }
    }
    if c5 {
        for e in m.store.iter() {
            if associated.contains(&e.guid().to_string()) || !structural_framing(m.store, e) {
                continue;
            }
            out.push(Found {
                code: "C5",
                kind: "no_analytical",
                severity: Severity::Info,
                title: "No counterpart",
                message: format!("{} has no analytical member", label(m.store, e.id())),
                elements: vec![e.id()],
                measured: None,
                tolerance: None,
                at: Vec::new(),
            });
        }
    }
}

/// C7: instability hints.
fn instability(m: &Model<'_>, out: &mut Vec<Found>) {
    use crate::loads::Dof;
    let free_rotations = |r: &[bool; 6]| !r[3] && !r[4] && !r[5];
    for (i, n) in m.nodes.iter().enumerate() {
        let using = &m.at_node[i];
        if using.is_empty() || using.iter().all(|(x, _)| m.members[*x].axial()) {
            continue;
        }
        let all_released = using
            .iter()
            .all(|(x, end)| free_rotations(&m.members[*x].g.recipe.releases[*end]));
        let held = n
            .support
            .as_ref()
            .is_some_and(|s| s.dof[3..].iter().any(|d| *d != Dof::Released));
        if all_released && !held {
            out.push(Found {
                code: "C7",
                kind: "rotation_unrestrained",
                severity: Severity::Hint,
                title: "Instability hint",
                message: format!(
                    "every member at {} releases its rotations and no boundary condition fixes \
                     them: the node can spin freely",
                    label(m.store, n.id)
                ),
                elements: vec![n.id],
                measured: None,
                tolerance: None,
                at: vec![n.p],
            });
        }
    }
    for mem in &m.members {
        let r = &mem.g.recipe.releases;
        if !r[0][3] && !r[1][3] && !mem.axial() && !(free_rotations(&r[0]) && free_rotations(&r[1]))
        {
            out.push(Found {
                code: "C7",
                kind: "torsion_released",
                severity: Severity::Hint,
                title: "Instability hint",
                message: format!(
                    "{} releases torsion (Mx) at both ends: it can twist freely",
                    label(m.store, mem.id)
                ),
                elements: vec![mem.id],
                measured: None,
                tolerance: None,
                at: vec![mem.g.start, mem.g.end],
            });
        }
        if free_rotations(&r[0]) && free_rotations(&r[1]) && !mem.axial() {
            out.push(Found {
                code: "C7",
                kind: "pinned_both_ends",
                severity: Severity::Hint,
                title: "Instability hint",
                message: format!(
                    "{} releases every rotation at both ends but is not a truss member or brace",
                    label(m.store, mem.id)
                ),
                elements: vec![mem.id],
                measured: None,
                tolerance: None,
                at: vec![mem.g.start, mem.g.end],
            });
        }
    }
}

/// C8: members without a section type or a structural material.
fn missing_properties(m: &Model<'_>, out: &mut Vec<Found>) {
    for pan in &m.panels {
        if pan.g.recipe.material.is_none() {
            out.push(Found {
                code: "C8",
                kind: "missing_properties",
                severity: Severity::Warning,
                title: "Missing properties",
                message: format!("{} has no structural material", label(m.store, pan.id)),
                elements: vec![pan.id],
                measured: None,
                tolerance: None,
                at: pan.g.corners.clone(),
            });
        }
    }
    for mem in &m.members {
        let r = &mem.g.recipe;
        let ty = r
            .section_type
            .as_deref()
            .and_then(|g| find(m.store, g))
            .and_then(|t| m.store.get(t));
        let material = r.material.clone().or_else(|| {
            ty.and_then(|t| {
                crate::columns::column_type(t)
                    .or_else(|| crate::beams::beam_type(t))
                    .and_then(|c| c.material)
            })
        });
        let mut missing = Vec::new();
        if ty.is_none() {
            missing.push("a section type");
        }
        if material.is_none() {
            missing.push("a structural material");
        }
        if missing.is_empty() {
            continue;
        }
        out.push(Found {
            code: "C8",
            kind: "missing_properties",
            severity: Severity::Warning,
            title: "Missing properties",
            message: format!(
                "{} has no {}",
                label(m.store, mem.id),
                missing.join(" and ")
            ),
            elements: vec![mem.id],
            measured: None,
            tolerance: None,
            at: vec![mem.g.start, mem.g.end],
        });
    }
}

// ---- Ignoring ----

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Ignored {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub by: String,
    pub at: String,
    /// A hash of the involved elements' geometry when it was ignored.
    pub hash: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
struct Stored {
    #[serde(default)]
    ignored: Vec<Ignored>,
}

fn project(store: &Store) -> Option<&Element> {
    store.iter().find(|e| e.class == "IfcProject")
}

fn stored(store: &Store) -> Stored {
    project(store)
        .and_then(|p| p.param(CHECKS_KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// The GlobalIds an issue key names.
fn key_guids(key: &str) -> Vec<String> {
    key.split_once(':')
        .map(|(_, g)| {
            g.split(',')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// A hash of the elements' geometry: their recipes and types, and members' end points.
fn geometry_hash(store: &Store, guids: &[String]) -> String {
    let mut text = String::new();
    for g in guids {
        let Some(e) = find(store, g).and_then(|i| store.get(i)) else {
            text.push_str("gone;");
            continue;
        };
        if let Some(r) = e.param(RECIPE_KEY) {
            text.push_str(&r.to_string());
        }
        if let Some(t) = e.param("@type") {
            text.push_str(&t.to_string());
        }
        if analytical::member_recipe(e).is_some() {
            if let Some((_, mg)) = analytical::members(store)
                .into_iter()
                .find(|(i, _)| *i == e.id())
            {
                text.push_str(&format!("{:?}{:?}", mg.start, mg.end));
            }
        } else if let Some(pg) = crate::analytical_panels::geometry(store, e) {
            text.push_str(&format!("{:?}", pg.corners));
        } else if let Some(p) = analytical::node_point(store, e) {
            text.push_str(&format!("{p:?}"));
        }
        text.push(';');
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IgnoreParams {
    pub issue_key: String,
    /// False un-ignores it.
    pub ignore: bool,
    pub reason: Option<String>,
}

/// Ignores or un-ignores an issue (`analytical_model.ignore_issue`): kept against its key until
/// any element it involves changes geometry.
pub(crate) fn ignore_issue(txn: &mut crate::txn::Transaction<'_>, p: IgnoreParams) -> Result<()> {
    let store = txn.store().clone();
    let id = project(&store)
        .map(Element::id)
        .ok_or_else(|| Error::new("project.missing", "the model has no project"))?;
    if !p.issue_key.contains(':') {
        return Err(Error::new(
            "analytical_check.bad_key",
            format!("`{}` is not an issue key", p.issue_key),
        ));
    }
    let mut s = stored(&store);
    s.ignored.retain(|i| i.key != p.issue_key);
    if p.ignore {
        s.ignored.push(Ignored {
            key: p.issue_key.clone(),
            reason: p.reason.filter(|r| !r.trim().is_empty()),
            by: txn.origin().label(),
            at: crate::overlap_accept::now_rfc3339(),
            hash: geometry_hash(&store, &key_guids(&p.issue_key)),
        });
    }
    let v = serde_json::to_value(&s).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(CHECKS_KEY.into(), v);
    })
}

// ---- Running ----

/// The elements a scope covers, expanded to associated members and members' nodes; `None` is
/// the whole model.
fn scope_set(store: &Store, scope: &Option<Scope>) -> Result<Option<HashSet<ElementId>>> {
    let refs = match scope {
        None => return Ok(None),
        Some(Scope::Named(s)) if s == "model" => return Ok(None),
        Some(Scope::Named(s)) => {
            return Err(Error::new(
                "analytical_check.scope",
                format!("unknown scope `{s}`: pass \"model\" or a list of elements"),
            ));
        }
        Some(Scope::Elements(refs)) => refs,
    };
    let mut set = HashSet::new();
    for r in refs {
        let id = store
            .resolve(r)
            .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
        set.insert(id);
    }
    let guids: HashSet<String> = set.iter().map(|i| guid_of(store, *i)).collect();
    for (id, g) in analytical::members(store) {
        if set.contains(&id) || g.recipe.physical.iter().any(|p| guids.contains(p)) {
            set.insert(id);
            set.extend(g.nodes);
            set.extend(g.recipe.physical.iter().filter_map(|p| find(store, p)));
        }
    }
    for (id, g) in crate::analytical_panels::panels(store) {
        if set.contains(&id) || g.recipe.physical.iter().any(|p| guids.contains(p)) {
            set.insert(id);
            set.extend(g.nodes.iter().copied());
            set.extend(g.recipe.physical.iter().filter_map(|p| find(store, p)));
        }
    }
    Ok(Some(set))
}

/// Runs the support check, the consistency check, or both.
pub fn run(
    store: &Store,
    supports: bool,
    consistency: bool,
    scope: &Option<Scope>,
) -> Result<Report> {
    let set = scope_set(store, scope)?;
    let model = Model::new(store);
    let mut found = Vec::new();
    let mut not_checked = Vec::new();
    let mut checks = Vec::new();
    if supports {
        checks.push("supports");
        check_supports(&model, &mut found);
    }
    if consistency {
        checks.push("consistency");
        check_consistency(&model, &mut found, &mut not_checked);
    }
    let ignores = stored(store);
    let mut issues: Vec<Issue> = Vec::new();
    let mut keys = HashSet::new();
    for f in found {
        if let Some(set) = &set {
            // S3 is about the scope as a whole; everything else must touch it.
            if f.code != "S3" && !f.elements.iter().any(|e| set.contains(e)) {
                continue;
            }
        }
        let mut guids: Vec<String> = f.elements.iter().map(|i| guid_of(store, *i)).collect();
        guids.sort();
        guids.dedup();
        let key = format!("{}.{}:{}", f.code, f.kind, guids.join(","));
        if !keys.insert(key.clone()) {
            continue;
        }
        let hash = geometry_hash(store, &guids);
        let ignore = ignores
            .ignored
            .iter()
            .find(|i| i.key == key && i.hash == hash);
        let (min, max) = if f.at.is_empty() {
            (None, None)
        } else {
            let (lo, hi) = bbox(&f.at);
            (Some(lo), Some(hi))
        };
        issues.push(Issue {
            check: if f.code.starts_with('S') {
                "supports"
            } else {
                "consistency"
            },
            ignored: ignore.is_some(),
            ignore_reason: ignore.and_then(|i| i.reason.clone()),
            key,
            code: f.code,
            kind: f.kind,
            severity: f.severity,
            title: f.title,
            message: f.message,
            element_ids: f.elements,
            measured: f.measured,
            tolerance: f.tolerance,
            min,
            max,
        });
    }
    issues.sort_by(|a, b| (a.severity, a.code, &a.key).cmp(&(b.severity, b.code, &b.key)));
    let mut counts = BTreeMap::new();
    for i in issues.iter().filter(|i| !i.ignored) {
        *counts.entry(i.severity).or_insert(0) += 1;
    }
    let s = &model.settings;
    Ok(Report {
        run_at: crate::overlap_accept::now_rfc3339(),
        checks,
        tolerances: Tolerances {
            support_distance: s.support_distance,
            analytical_distance: s.analytical_distance,
            node_merge_distance: s.node_merge_distance,
        },
        ignored: issues.iter().filter(|i| i.ignored).count(),
        issues,
        counts_by_severity: counts,
        not_checked,
    })
}

/// The ignore list as stored (for reports and the palette's *Show ignored*).
pub fn ignored(store: &Store) -> Vec<Ignored> {
    stored(store).ignored
}

/// The keys of a report's issues, for comparing runs.
pub fn keys(r: &Report) -> Vec<String> {
    r.issues.iter().map(|i| i.key.clone()).collect()
}
