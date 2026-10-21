// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Pressure drop, analytical segments and the pressure loss report (MEP-CALC-002, MEP-CALC-004).
//!
//! A system's network is walked from its equipment, as sizing walks it (MEP-CALC-001): each
//! segment and fitting carries the design flow of the terminals beyond it.
//!
//! - **Segments** lose Darcy–Weisbach friction over their length (Colebrook or Haaland, the
//!   project's setting; air at 20 °C, water by the system type's temperature).
//! - **Fittings** lose a coefficient times the velocity pressure ρv²/2 at the leg the flow leaves
//!   by: elbows by their deflection, tees, taps and crosses by whether the flow goes straight
//!   through or turns into the branch, transitions by their area ratio (Reviera's provisional
//!   coefficients, EXPERT_REVIEW E-24). A segment's taps lose the branch coefficient.
//! - **Terminals** lose their connector's loss: a coefficient of the velocity pressure at the
//!   connector, or a specific loss in Pa.
//!
//! The **critical path** runs from the equipment to the terminal with the highest path loss;
//! the equipment's pressure is taken as that loss, so each element's static pressure is it minus
//! the losses upstream, and a terminal's excess pressure is it minus the terminal's path loss.
//!
//! Analytical segments are computed: one per segment and fitting, with flow, velocity,
//! friction, loss, network id and the critical-path flag. `mep_network.calculate` keeps the
//! results on the system (`rxd:mep_network`, with a fingerprint of the network's inputs) and
//! each element (`rxd:mep_analysis`, beside its overrides); when the inputs change the kept
//! results are stale until the network is calculated again.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::mep::{self, Classification, Domain, LossMethod, Shape};
use crate::mep_runs::{self, Part, Size};
use crate::mep_sizing::{self, Equation, flow_in, section};
use crate::mep_systems::{self, Calculations, SystemInfo};
use crate::store::Store;
use crate::txn::{Transaction, Warning};

/// On segments and fittings: `{result, override}`.
pub const KEY: &str = "rxd:mep_analysis";
/// On systems: the kept network result and the fingerprint of its inputs.
pub const NETWORK_KEY: &str = "rxd:mep_network";

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

// ---- Coefficients ----

/// A mitred duct elbow's coefficient by its deflection (degrees).
const DUCT_ELBOW: [(f64, f64); 5] = [(0.0, 0.0), (30.0, 0.16), (45.0, 0.34), (60.0, 0.55), (90.0, 1.2)];
/// A pipe elbow's coefficient by its deflection (degrees).
const PIPE_ELBOW: [(f64, f64); 3] = [(0.0, 0.0), (45.0, 0.4), (90.0, 0.75)];
/// Tees, taps and crosses: (straight through, into the branch), ducts and pipes.
const DUCT_JUNCTION: (f64, f64) = (0.2, 1.0);
const PIPE_JUNCTION: (f64, f64) = (0.4, 1.5);
/// An open isolating valve's coefficient (check and globe valves: see `valve_coefficient`).
const VALVE: f64 = 0.2;

/// A valve's coefficient by its kind.
pub fn valve_coefficient(kind: Option<&str>) -> f64 {
    match kind {
        Some("check_valve") => 2.0,
        Some("globe_valve") | Some("pressure_reducing_valve") => 6.0,
        Some("butterfly_valve") => 0.5,
        _ => VALVE,
    }
}

/// A contraction's coefficient (of the outlet's velocity pressure).
const CONTRACTION: f64 = 0.05;

fn interpolate(t: &[(f64, f64)], x: f64) -> f64 {
    let n = t.len();
    if x <= t[0].0 {
        return t[0].1;
    }
    // Beyond the last row: in proportion to the deflection.
    if x >= t[n - 1].0 {
        return t[n - 1].1 * x / t[n - 1].0;
    }
    let k = t.windows(2).position(|w| x <= w[1].0).unwrap_or(n - 2);
    let (a, b) = (t[k], t[k + 1]);
    a.1 + (b.1 - a.1) * (x - a.0) / (b.0 - a.0)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

pub(crate) fn size_of(c: &mep::Connector) -> Size {
    Size { shape: c.shape, diameter: c.diameter, width: c.width, height: c.height }
}

/// A fitting's coefficient for flow entering by port `kin` and leaving by `kout` (of the
/// velocity pressure at `kout`).
pub fn coefficient(domain: Domain, part: Part, kin: &mep::Port, kout: &mep::Port) -> f64 {
    let duct = domain == Domain::Duct;
    // The legs point outward: straight through when they are opposite.
    let straight = dot(kin.direction, kout.direction) < -0.99;
    match part {
        Part::Elbow => {
            let deflection = 180.0 - dot(kin.direction, kout.direction).clamp(-1.0, 1.0).acos().to_degrees();
            interpolate(if duct { &DUCT_ELBOW } else { &PIPE_ELBOW }, deflection)
        }
        Part::Tee | Part::Tap | Part::Cross => {
            let (run, branch) = if duct { DUCT_JUNCTION } else { PIPE_JUNCTION };
            if straight { run } else { branch }
        }
        Part::Transition => {
            let (ai, ao) = (section(&size_of(&kin.connector)).0, section(&size_of(&kout.connector)).0);
            if ai <= 0.0 || ao <= ai {
                CONTRACTION
            } else {
                // Half the sudden expansion's loss, as a coefficient of the outlet's velocity.
                0.5 * (ao / ai - 1.0).powi(2)
            }
        }
        Part::Cap | Part::Coupling | Part::Box | Part::Flange => 0.0,
        // Open gate or ball valve, meter, strainer (E-24); a valve's kind refines it.
        Part::Valve => VALVE,
        Part::Meter => 1.0,
        Part::Strainer => 2.0,
    }
}

// ---- Results ----

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Override {
    /// A measured or given loss (Pa), in place of the calculated one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loss: Option<f64>,
    /// A fitting's coefficient, in place of Reviera's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coefficient: Option<f64>,
}

impl Override {
    fn is_empty(&self) -> bool {
        self.loss.is_none() && self.coefficient.is_none()
    }
}

/// Flow leaving a fitting (or a segment's tap) toward one element.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Branch {
    pub to: ElementId,
    pub flow: f64,
    pub coefficient: f64,
    /// Pa.
    pub loss: f64,
}

/// An analytical segment: the flow path through one segment or fitting, with its results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AnalyticalSegment {
    pub element: ElementId,
    /// `segment` or `fitting`.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<String>,
    /// 1 for the network reached from the equipment; parts not joined to it are numbered on.
    pub network_id: u32,
    pub size: String,
    /// m³/s.
    pub flow: f64,
    /// m/s.
    pub velocity: f64,
    /// Segments (m).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<f64>,
    /// Segments (Pa/m).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub friction: Option<f64>,
    /// Fittings: the coefficient on the path that counts (the critical one, else the largest).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coefficient: Option<f64>,
    /// Pa; none when the system calculates flow only or the element is not joined to the
    /// equipment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loss: Option<f64>,
    /// Static pressure at its inlet (Pa): the equipment's pressure (the critical-path loss)
    /// less the losses upstream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure: Option<f64>,
    pub critical: bool,
    pub overridden: bool,
    pub connected: bool,
    /// Flow leaving by more than one way (junctions, taps).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub branches: Vec<Branch>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TerminalResult {
    pub element: ElementId,
    pub flow: f64,
    /// Its own connector's loss (Pa).
    pub own_loss: f64,
    /// From the equipment through it (Pa).
    pub path_loss: f64,
    /// The critical-path loss less its path loss (Pa).
    pub excess: f64,
    pub critical: bool,
}

/// A stretch of the network with constant flow between two junctions (or a junction and a
/// terminal or open end).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Section {
    /// 1, 2, … from the equipment outward.
    pub id: usize,
    /// Its segments and fittings in flow order.
    pub elements: Vec<ElementId>,
    /// m³/s.
    pub flow: f64,
    /// m/s, in its first element.
    pub velocity: f64,
    /// Pa: the loss entering it from the junction before, along it, and into the terminal after.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loss: Option<f64>,
    /// Static pressure where it starts and ends (Pa).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure_in: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure_out: Option<f64>,
    pub critical: bool,
    /// Its centreline in flow order (metres), for flow arrows.
    pub points: Vec<[f64; 3]>,
    /// The terminal it feeds, if it ends at one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<ElementId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NetworkResult {
    pub system: ElementId,
    pub name: String,
    pub classification: Classification,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equipment: Option<ElementId>,
    pub calculations: Calculations,
    pub equation: Equation,
    /// m³/s, at the equipment.
    pub total_flow: f64,
    /// Pa; none when flow only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub critical_loss: Option<f64>,
    /// From the equipment to the critical terminal.
    #[serde(default)]
    pub critical_path: Vec<ElementId>,
    pub segments: Vec<AnalyticalSegment>,
    pub terminals: Vec<TerminalResult>,
    /// Stretches of constant flow between junctions, from the equipment outward (MEP-CALC-003).
    #[serde(default)]
    pub sections: Vec<Section>,
    #[serde(default)]
    pub warnings: Vec<String>,
    /// The inputs have changed since it was calculated.
    #[serde(default)]
    pub stale: bool,
}

// ---- The calculation ----

fn hash(text: &str, mut h: u64) -> u64 {
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

pub(crate) fn override_of(e: &crate::element::Element) -> Override {
    e.param(KEY).and_then(|v| v.get("override")).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

/// A fingerprint of everything a system's results depend on.
pub fn fingerprint(store: &Store, sys: &SystemInfo, equation: Equation) -> String {
    let mut h = 0xcbf2_9ce4_8422_2325;
    let partners = mep::partners(store);
    h = hash(&format!("{equation:?}"), h);
    if let Some(e) = store.get(sys.id) {
        h = hash(&format!("{:?}", e.param(crate::walls::RECIPE_KEY)), h);
    }
    let st = mep_systems::system_type(store, &sys.system_type).unwrap_or_else(|| mep_systems::builtin(sys.classification));
    h = hash(&format!("{st:?}"), h);
    h = hash(&format!("{:?}", mep_systems::settings(store).fixture_unit_tables), h);
    let mut ids: Vec<ElementId> = sys.elements.iter().copied().chain(sys.equipment).collect();
    ids.sort();
    ids.dedup();
    for id in ids {
        let Some(e) = store.get(id) else { continue };
        h = hash(&format!("{id:?}{:?}{:?}", e.param(crate::walls::RECIPE_KEY), override_of(e)), h);
        for p in mep::ports(store, e) {
            let mut with: Vec<_> = partners.get(&(id, p.index)).cloned().unwrap_or_default();
            with.sort();
            h = hash(&format!("{:?}{:?}{:?}{with:?}", p.connector, p.position, p.direction), h);
        }
    }
    format!("{h:016x}")
}

/// Calculates a system's network as it is now (nothing is kept).
pub fn compute(store: &Store, sys: &SystemInfo, equation: Equation) -> Result<NetworkResult> {
    if matches!(sys.classification, Classification::Sanitary | Classification::Vent) {
        return Err(bad("mep_network.sanitary", format!("{}: sanitary and vent systems are not calculated by flow", sys.name)));
    }
    let st = mep_systems::system_type(store, &sys.system_type).unwrap_or_else(|| mep_systems::builtin(sys.classification));
    if st.calculations == Calculations::None {
        return Err(bad("mep_network.calculations_off", format!("{}: its system type's Calculations is None", sys.name)));
    }
    let losses = matches!(st.calculations, Calculations::All | Calculations::Performance);
    let fluid = mep_sizing::fluid_of(sys, &st)?;
    let tables = mep_systems::settings(store).fixture_unit_tables;
    let table = mep_sizing::table_of(sys, &st, &tables);
    let fl = mep_sizing::flows(store, sys)?;
    let q = |id: ElementId| fl.design(id, table, false);
    let root = sys.equipment.ok_or_else(|| bad("mep_size.no_equipment", format!("{} has no equipment: Select Equipment first, so flow has a source", sys.name)))?;

    // The tree from the equipment: each element's parent, the parent's port and its own.
    let partners = mep::partners(store);
    let allowed: HashSet<ElementId> = sys.elements.iter().copied().chain([root]).collect();
    let ports_of = |id: ElementId| store.get(id).map(|e| mep::ports(store, e)).unwrap_or_default();
    let mut parent: HashMap<ElementId, (ElementId, usize, usize)> = HashMap::new();
    let mut order = vec![root];
    let mut queue = VecDeque::from([root]);
    while let Some(x) = queue.pop_front() {
        for p in ports_of(x) {
            for (o, ko) in partners.get(&(x, p.index)).into_iter().flatten() {
                if *o != root && allowed.contains(o) && !parent.contains_key(o) {
                    parent.insert(*o, (x, p.index, *ko));
                    order.push(*o);
                    queue.push_back(*o);
                }
            }
        }
    }
    let vp = |q: f64, s: &Size| {
        let a = section(s).0;
        if a <= 0.0 { 0.0 } else { fluid.0 * (q / a).powi(2) / 2.0 }
    };
    let is_run = |id: ElementId| store.get(id).is_some_and(mep_runs::is_run_element);

    // Each element's own loss (segments: friction), and the loss of leaving it toward a child.
    let mut own: HashMap<ElementId, f64> = HashMap::new();
    let mut overridden: HashSet<ElementId> = HashSet::new();
    let mut leaving: HashMap<(ElementId, ElementId), (f64, f64)> = HashMap::new();
    let mut rows: BTreeMap<ElementId, AnalyticalSegment> = BTreeMap::new();
    for &x in &order {
        let Some(e) = store.get(x) else { continue };
        let ov = override_of(e);
        if !ov.is_empty() {
            overridden.insert(x);
        }
        let ports = mep::ports(store, e);
        let children: Vec<(ElementId, usize)> = order.iter().filter_map(|c| parent.get(c).filter(|p| p.0 == x).map(|p| (*c, p.1))).collect();
        if let Some(s) = mep_runs::segment_recipe(e) {
            let flow = q(x);
            let roughness = GlobalId::parse(&s.segment_type)
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|t| store.get(t))
                .and_then(mep_runs::segment_type)
                .map_or(0.0, |t| t.roughness);
            let len = match s.corner {
                Some(c) => length(s.start, c) + length(c, s.end),
                None => length(s.start, s.end),
            };
            let (v, f) = flow_in(flow, &s.size, roughness, fluid, equation);
            let loss = ov.loss.unwrap_or(f * len);
            own.insert(x, loss);
            let (_, branch) = if s.domain == Domain::Duct { DUCT_JUNCTION } else { PIPE_JUNCTION };
            let mut branches = Vec::new();
            for (c, k) in &children {
                // Leaving by a tap: the branch coefficient at the tap's size.
                if *k >= 2
                    && let Some(tp) = ports.iter().find(|p| p.index == *k)
                {
                    let cq = q(*c);
                    let l = branch * vp(cq, &size_of(&tp.connector));
                    leaving.insert((x, *c), (branch, l));
                    branches.push(Branch { to: *c, flow: cq, coefficient: branch, loss: l });
                }
            }
            rows.insert(
                x,
                AnalyticalSegment {
                    element: x,
                    kind: "segment".into(),
                    part: None,
                    network_id: 1,
                    size: s.size.text(),
                    flow,
                    velocity: v,
                    length: Some(len),
                    friction: Some(f),
                    coefficient: None,
                    loss: losses.then_some(loss),
                    pressure: None,
                    critical: false,
                    overridden: !ov.is_empty(),
                    connected: true,
                    branches,
                },
            );
        } else if let Some(fr) = mep_runs::fitting_recipe(e) {
            let flow = q(x);
            let kin = parent.get(&x).map(|p| p.2);
            let inlet = kin.and_then(|k| ports.iter().find(|p| p.index == k));
            let mut branches = Vec::new();
            for (c, k) in &children {
                let (Some(i), Some(o)) = (inlet, ports.iter().find(|p| p.index == *k)) else { continue };
                let cq = q(*c);
                let kk = ov.coefficient.unwrap_or_else(|| if fr.part == Part::Valve { valve_coefficient(fr.accessory.as_deref()) } else { coefficient(fr.domain, fr.part, i, o) });
                let l = ov.loss.unwrap_or(kk * vp(cq, &size_of(&o.connector)));
                leaving.insert((x, *c), (kk, l));
                branches.push(Branch { to: *c, flow: cq, coefficient: kk, loss: l });
            }
            let v = inlet.map_or(0.0, |i| {
                let a = section(&size_of(&i.connector)).0;
                if a > 0.0 { flow / a } else { 0.0 }
            });
            let size = inlet.map(|i| size_of(&i.connector)).unwrap_or_else(|| fr.legs.first().map(|l| l.size).unwrap_or(Size { shape: Shape::Round, diameter: 0.0, width: 0.0, height: 0.0 }));
            rows.insert(
                x,
                AnalyticalSegment {
                    element: x,
                    kind: "fitting".into(),
                    part: Some(fr.part.label().into()),
                    network_id: 1,
                    size: size.text(),
                    flow,
                    velocity: v,
                    length: None,
                    friction: None,
                    coefficient: None,
                    loss: None,
                    pressure: None,
                    critical: false,
                    overridden: !ov.is_empty(),
                    connected: true,
                    branches,
                },
            );
        } else if x != root && !is_run(x) {
            // A terminal: its connector's loss.
            let Some(&(_, _, k)) = parent.get(&x) else { continue };
            let l = ports.iter().find(|p| p.index == k).map_or(0.0, |p| match p.connector.loss_method {
                LossMethod::Coefficient => p.connector.loss * vp(q(x), &size_of(&p.connector)),
                LossMethod::SpecificLoss => p.connector.loss,
                LossMethod::NotDefined => 0.0,
            });
            own.insert(x, ov.loss.unwrap_or(l));
        }
    }

    // Losses upstream of each element's inlet.
    let mut upstream: HashMap<ElementId, f64> = HashMap::from([(root, 0.0)]);
    for &x in order.iter().skip(1) {
        let (p, _, _) = parent[&x];
        let through = if p == root { 0.0 } else { own.get(&p).copied().unwrap_or(0.0) };
        let leave = leaving.get(&(p, x)).map_or(0.0, |l| l.1);
        upstream.insert(x, upstream.get(&p).copied().unwrap_or(0.0) + through + leave);
    }
    let mut terminals: Vec<TerminalResult> = order
        .iter()
        .filter(|x| **x != root && !is_run(**x))
        .map(|&x| {
            let o = own.get(&x).copied().unwrap_or(0.0);
            TerminalResult { element: x, flow: q(x), own_loss: o, path_loss: upstream[&x] + o, excess: 0.0, critical: false }
        })
        .collect();
    let mut warnings = Vec::new();
    let mut critical_path = Vec::new();
    let critical_loss = if losses {
        let top = terminals.iter().max_by(|a, b| a.path_loss.total_cmp(&b.path_loss)).map(|t| (t.element, t.path_loss));
        if let Some((t, loss)) = top {
            let mut x = t;
            critical_path.push(x);
            while let Some((p, _, _)) = parent.get(&x) {
                critical_path.push(*p);
                x = *p;
            }
            critical_path.reverse();
            for tr in &mut terminals {
                tr.excess = loss - tr.path_loss;
                tr.critical = tr.element == t;
            }
            Some(loss)
        } else {
            warnings.push(format!("{}: no terminal is joined to the equipment, so there is no critical path", sys.name));
            Some(0.0)
        }
    } else {
        None
    };
    let on_path: HashSet<ElementId> = critical_path.iter().copied().collect();
    for (x, r) in &mut rows {
        r.critical = on_path.contains(x);
        if r.kind == "fitting" && losses {
            // The branch on the critical path, else the largest.
            let b = r.branches.iter().find(|b| on_path.contains(&b.to)).or_else(|| r.branches.iter().max_by(|a, b| a.loss.total_cmp(&b.loss)));
            r.coefficient = b.map(|b| b.coefficient);
            r.loss = Some(b.map_or(0.0, |b| b.loss));
        }
        if let Some(c) = critical_loss {
            r.pressure = Some(c - upstream.get(x).copied().unwrap_or(0.0));
        }
    }

    // Sections: chains of run elements with one way on, split at junctions.
    let mut children: HashMap<ElementId, Vec<ElementId>> = HashMap::new();
    for &x in order.iter().skip(1) {
        children.entry(parent[&x].0).or_default().push(x);
    }
    let mut sections = Vec::new();
    let mut starts: VecDeque<ElementId> = children.get(&root).into_iter().flatten().copied().filter(|c| is_run(*c)).collect();
    while let Some(start) = starts.pop_front() {
        let mut chain = vec![start];
        let mut x = start;
        let mut terminal = None;
        loop {
            let kids = children.get(&x).cloned().unwrap_or_default();
            if kids.len() == 1 && is_run(kids[0]) {
                x = kids[0];
                chain.push(x);
                continue;
            }
            if kids.len() == 1 {
                terminal = Some(kids[0]);
            }
            starts.extend(kids.into_iter().filter(|k| is_run(*k)));
            break;
        }
        let leave = |a: ElementId, b: ElementId| leaving.get(&(a, b)).map_or(0.0, |l| l.1);
        let from = parent[&start].0;
        let entry = if from == root { 0.0 } else { leave(from, start) };
        let mut loss = entry;
        for (k, e) in chain.iter().enumerate() {
            if store.get(*e).and_then(mep_runs::segment_recipe).is_some() {
                loss += own.get(e).copied().unwrap_or(0.0);
            }
            if let Some(n) = chain.get(k + 1) {
                loss += leave(*e, *n);
            }
        }
        if let Some(t) = terminal {
            loss += leave(*chain.last().expect("one"), t);
        }
        let mut points: Vec<[f64; 3]> = Vec::new();
        for e in &chain {
            let Some(el) = store.get(*e) else { continue };
            let pts = if let Some(sr) = mep_runs::segment_recipe(el) {
                if parent[e].2 == 1 { vec![sr.end, sr.start] } else { vec![sr.start, sr.end] }
            } else if let Some(fr) = mep_runs::fitting_recipe(el) {
                vec![fr.centre]
            } else {
                Vec::new()
            };
            for p in pts {
                if points.last().is_none_or(|q| length(*q, p) > 1e-6) {
                    points.push(p);
                }
            }
        }
        let first = rows.get(&start);
        let pressure_in = critical_loss.map(|c| c - upstream[&start] + entry);
        sections.push(Section {
            id: sections.len() + 1,
            flow: q(start),
            velocity: first.map_or(0.0, |r| r.velocity),
            loss: losses.then_some(loss),
            pressure_in,
            pressure_out: pressure_in.map(|p| p - loss),
            critical: on_path.contains(&start),
            elements: chain,
            points,
            terminal,
        });
    }

    // Members the walk did not reach: numbered networks of their own, without results.
    let mut rest: Vec<ElementId> = sys.elements.iter().copied().filter(|e| !parent.contains_key(e) && *e != root && is_run(*e)).collect();
    rest.sort();
    if !rest.is_empty() {
        warnings.push(format!("{}: {} segments and fittings are not joined to the equipment; they have no results", sys.name, rest.len()));
    }
    let mut net = 1;
    let mut seen: HashSet<ElementId> = HashSet::new();
    for start in &rest {
        if seen.contains(start) {
            continue;
        }
        net += 1;
        let mut stack = vec![*start];
        while let Some(x) = stack.pop() {
            if !seen.insert(x) {
                continue;
            }
            let Some(e) = store.get(x) else { continue };
            let size = mep_runs::segment_recipe(e).map(|s| s.size.text()).unwrap_or_default();
            let part = mep_runs::fitting_recipe(e).map(|f| f.part.label().to_owned());
            rows.insert(
                x,
                AnalyticalSegment {
                    element: x,
                    kind: if part.is_some() { "fitting" } else { "segment" }.into(),
                    part,
                    network_id: net,
                    size,
                    flow: 0.0,
                    velocity: 0.0,
                    length: None,
                    friction: None,
                    coefficient: None,
                    loss: None,
                    pressure: None,
                    critical: false,
                    overridden: overridden.contains(&x),
                    connected: false,
                    branches: Vec::new(),
                },
            );
            for p in ports_of(x) {
                for (o, _) in partners.get(&(x, p.index)).into_iter().flatten() {
                    if rest.contains(o) {
                        stack.push(*o);
                    }
                }
            }
        }
    }
    // From the equipment outward, then the parts not joined to it.
    let rank: HashMap<ElementId, usize> = order.iter().enumerate().map(|(k, e)| (*e, k)).collect();
    let mut segments: Vec<AnalyticalSegment> = rows.into_values().collect();
    segments.sort_by_key(|a| (a.network_id, rank.get(&a.element).copied().unwrap_or(usize::MAX), a.element));
    Ok(NetworkResult {
        system: sys.id,
        name: sys.name.clone(),
        classification: sys.classification,
        equipment: Some(root),
        calculations: st.calculations,
        equation,
        total_flow: q(root),
        critical_loss,
        critical_path,
        segments,
        terminals,
        sections,
        warnings,
        stale: false,
    })
}

// ---- Commands ----

fn system_of(store: &Store, r: &ElementRef) -> Result<SystemInfo> {
    let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such system"))?;
    mep_systems::systems(store).into_iter().find(|s| s.id == id).ok_or_else(|| bad("system.not_found", format!("element {id} is not a system")))
}

fn equation_of(store: &Store, given: Option<Equation>) -> Equation {
    given.unwrap_or_else(|| mep_systems::settings(store).friction_equation)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalculateParams {
    /// One system; every duct and pipe system when omitted.
    pub system_id: Option<ElementRef>,
    /// The friction equation; the project's setting when omitted.
    pub equation: Option<Equation>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CalculatedSystem {
    pub system: ElementId,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub critical_loss: Option<f64>,
    pub segments: usize,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct CalculateReport {
    pub systems: Vec<CalculatedSystem>,
    pub warnings: Vec<String>,
}

/// Keeps a system's results on it and its elements.
fn keep(txn: &mut Transaction<'_>, r: &NetworkResult, fp: &str) -> Result<()> {
    let v = serde_json::json!({ "fingerprint": fp, "result": r });
    txn.set_param(r.system, NETWORK_KEY, v)?;
    for s in &r.segments {
        let ov = txn.store().get(s.element).map(override_of).unwrap_or_default();
        let mut v = serde_json::json!({ "result": {
            "network_id": s.network_id, "flow": s.flow, "velocity": s.velocity, "friction": s.friction,
            "loss": s.loss, "pressure": s.pressure, "critical": s.critical,
        }});
        if !ov.is_empty() {
            v["override"] = serde_json::to_value(&ov).expect("serialises");
        }
        txn.set_param(s.element, KEY, v)?;
    }
    Ok(())
}

/// Calculates one system and keeps its results.
fn calculate_one(txn: &mut Transaction<'_>, sys: &SystemInfo, equation: Equation) -> Result<NetworkResult> {
    let store = txn.store().clone();
    let r = compute(&store, sys, equation)?;
    let fp = fingerprint(&store, sys, equation);
    keep(txn, &r, &fp)?;
    Ok(r)
}

/// `mep_network.calculate`.
pub(crate) fn calculate(txn: &mut Transaction<'_>, p: CalculateParams) -> Result<CalculateReport> {
    let store = txn.store().clone();
    let equation = equation_of(&store, p.equation);
    let mut report = CalculateReport::default();
    let systems: Vec<SystemInfo> = match &p.system_id {
        Some(r) => vec![system_of(&store, r)?],
        None => mep_systems::systems(&store).into_iter().filter(|s| s.classification.fits(Domain::Duct) || s.classification.fits(Domain::Pipe)).collect(),
    };
    for sys in systems {
        match calculate_one(txn, &sys, equation) {
            Ok(r) => {
                report.warnings.extend(r.warnings.iter().cloned());
                report.systems.push(CalculatedSystem { system: sys.id, name: sys.name.clone(), critical_loss: r.critical_loss, segments: r.segments.len() });
            }
            // One system: its refusal. Every system: a warning, and on to the next.
            Err(e) if p.system_id.is_none() => report.warnings.push(e.message),
            Err(e) => return Err(e),
        }
    }
    for w in &report.warnings {
        txn.warn(Warning { id: "mep_network".into(), message: w.clone(), element_ids: Vec::new() });
    }
    Ok(report)
}

/// The kept results of a system, marked stale when its inputs have changed.
pub fn kept(store: &Store, sys: &SystemInfo) -> Option<NetworkResult> {
    let v = store.get(sys.id)?.param(NETWORK_KEY)?;
    let mut r: NetworkResult = serde_json::from_value(v.get("result")?.clone()).ok()?;
    let fp = v.get("fingerprint").and_then(Value::as_str).unwrap_or_default();
    r.stale = fp != fingerprint(store, sys, r.equation);
    Some(r)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    pub system_id: ElementRef,
}

/// `analytical_segment.list`.
pub(crate) fn list(store: &Store, p: ListParams) -> Result<NetworkResult> {
    let sys = system_of(store, &p.system_id)?;
    kept(store, &sys).ok_or_else(|| bad("mep_network.not_calculated", format!("{} has not been calculated: run mep_network.calculate", sys.name)))
}

/// Why the System Inspector cannot inspect a system, if it cannot (MEP-CALC-003): no equipment,
/// Calculations not All or Performance, open connectors, or fire protection.
pub fn inspect_refusal(store: &Store, sys: &SystemInfo) -> Option<Error> {
    use Classification::*;
    if matches!(sys.classification, FireProtectionWet | FireProtectionDry | FireProtectionPreAction | FireProtectionOther) {
        return Some(bad("system_inspector.fire_protection", "The System Inspector is not offered for fire protection systems"));
    }
    if matches!(sys.classification, Sanitary | Vent | StormDrainage) || !(sys.classification.fits(Domain::Duct) || sys.classification.fits(Domain::Pipe)) {
        return Some(bad("system_inspector.kind", format!("{} is not a duct or pressure pipe system", sys.name)));
    }
    if sys.equipment.is_none() {
        return Some(bad("system_inspector.no_equipment", "System has no equipment: Select Equipment first"));
    }
    let st = mep_systems::system_type(store, &sys.system_type).unwrap_or_else(|| mep_systems::builtin(sys.classification));
    if !matches!(st.calculations, Calculations::All | Calculations::Performance) {
        return Some(bad("system_inspector.calculations", format!("{}: its system type's Calculations must be All or Performance", sys.name)));
    }
    // Open connectors: every port of its segments and fittings, and the member ports of the rest.
    let partners = mep::partners(store);
    let mut open = 0;
    for e in sys.elements.iter().chain(sys.equipment.as_ref()) {
        let Some(el) = store.get(*e) else { continue };
        let run = mep_runs::is_run_element(el);
        for p in mep::ports(store, el) {
            let member = run || sys.ports.contains(&(*e, p.index));
            if member && !p.connector.surface && p.connector.domain != Domain::Electrical && !partners.contains_key(&(*e, p.index)) {
                open += 1;
            }
        }
    }
    if open > 0 {
        return Some(bad("system_inspector.open_connectors", format!("System has open connectors: {open}. Run Show Disconnects")));
    }
    None
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InspectParams {
    pub system_id: ElementRef,
    /// The view it is shown in (the window keeps the session); scripts may omit it.
    #[serde(default)]
    pub view_id: Option<String>,
}

/// `system_inspector.start`: the preconditions, then the system's kept results with its
/// sections (refused while they are stale or missing).
pub(crate) fn inspect(store: &Store, p: InspectParams) -> Result<NetworkResult> {
    let sys = system_of(store, &p.system_id)?;
    if let Some(e) = inspect_refusal(store, &sys) {
        return Err(e);
    }
    match kept(store, &sys) {
        Some(r) if !r.stale => Ok(r),
        _ => Err(bad("system_inspector.stale", format!("{}'s results are stale or missing: run mep_network.calculate first", sys.name))),
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OverrideParams {
    /// A segment or fitting.
    pub element_id: ElementRef,
    /// Pa, in place of the calculated loss.
    pub loss: Option<f64>,
    /// A fitting's coefficient.
    pub coefficient: Option<f64>,
    /// Removes the overrides.
    #[serde(default)]
    pub clear: bool,
}

/// `analytical_segment.override`.
pub(crate) fn set_override(txn: &mut Transaction<'_>, p: OverrideParams) -> Result<()> {
    let store = txn.store().clone();
    let id = store.resolve(&p.element_id).ok_or_else(|| bad("element.not_found", "no such segment"))?;
    let e = store.get(id).expect("resolved");
    let fitting = mep_runs::fitting_recipe(e).is_some();
    if mep_runs::segment_recipe(e).is_none() && !fitting {
        return Err(bad("analytical_segment.not_run", "overrides go on duct and pipe segments and fittings"));
    }
    if p.coefficient.is_some() && !fitting {
        return Err(bad("analytical_segment.coefficient", "a coefficient overrides a fitting's; give a segment a loss"));
    }
    for v in [p.loss, p.coefficient].into_iter().flatten() {
        if !(v.is_finite() && v >= 0.0) {
            return Err(bad("analytical_segment.value", "an override is a number of zero or more"));
        }
    }
    let mut ov = if p.clear { Override::default() } else { override_of(e) };
    if p.loss.is_some() {
        ov.loss = p.loss;
    }
    if p.coefficient.is_some() {
        ov.coefficient = p.coefficient;
    }
    let mut v = e.param(KEY).cloned().unwrap_or_else(|| serde_json::json!({}));
    if ov.is_empty() {
        v.as_object_mut().map(|o| o.remove("override"));
    } else {
        v["override"] = serde_json::to_value(&ov).expect("serialises");
    }
    txn.set_param(id, KEY, v)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EquationParams {
    pub equation: Equation,
}

/// `mep_settings.set_friction_equation`.
pub(crate) fn set_equation(txn: &mut Transaction<'_>, p: EquationParams) -> Result<()> {
    let mut s = mep_systems::settings(txn.store());
    s.friction_equation = p.equation;
    mep_systems::save_settings(txn, &s)
}

/// An element's kept result value (`flow`, `velocity`, `friction`, `loss`, `pressure`,
/// `network_id`, `critical`), for schedules and tags.
pub fn kept_value<'a>(e: &'a crate::element::Element, field: &str) -> Option<&'a Value> {
    e.param(KEY)?.get("result")?.get(field).filter(|v| !v.is_null())
}

// ---- The pressure loss report ----

/// The report sections, in order.
pub const SECTIONS: [&str; 6] = ["project", "system", "total", "segments", "fittings", "critical_path"];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ReportSettings {
    /// A saved preset's id: its settings are used and the rest ignored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset_id: Option<String>,
    /// Sections (`project`, `system`, `total`, `segments`, `fittings`, `critical_path`); all
    /// when empty.
    pub sections: Vec<String>,
    /// Leave out the fitting detail.
    pub without_fittings: bool,
    /// Leave out the critical path.
    pub without_critical_path: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReportPreset {
    pub id: String,
    pub name: String,
    pub settings: ReportSettings,
}

impl ReportSettings {
    fn has(&self, section: &str) -> bool {
        let listed = self.sections.is_empty() || self.sections.iter().any(|s| s == section);
        listed && !(section == "fittings" && self.without_fittings) && !(section == "critical_path" && self.without_critical_path)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ReportFormat {
    Html,
    #[default]
    Csv,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportParams {
    pub system_ids: Vec<ElementRef>,
    #[serde(default)]
    pub settings: ReportSettings,
    pub path: PathBuf,
    #[serde(default)]
    pub format: ReportFormat,
    /// Replace an existing file.
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ReportResult {
    pub path: PathBuf,
    pub systems: usize,
    /// Systems calculated first because their results were stale or missing.
    pub recalculated: Vec<ElementId>,
}

/// Why a system cannot be reported, if it cannot.
pub fn not_reportable(store: &Store, sys: &SystemInfo) -> Option<String> {
    if !(sys.classification.fits(Domain::Duct) || sys.classification.fits(Domain::Pipe)) || matches!(sys.classification, Classification::Sanitary | Classification::Vent) {
        return Some("not a duct or pressure pipe system".into());
    }
    let st = mep_systems::system_type(store, &sys.system_type).unwrap_or_else(|| mep_systems::builtin(sys.classification));
    if st.calculations != Calculations::All {
        return Some(format!("its system type's Calculations is {}, not All", match st.calculations {
            Calculations::FlowOnly => "Flow Only",
            Calculations::Performance => "Performance",
            Calculations::None => "None",
            Calculations::All => "All",
        }));
    }
    if sys.equipment.is_none() {
        return Some("it has no equipment".into());
    }
    None
}

fn csv_field(s: &str) -> String {
    // Formulas escaped, as schedules export them.
    let s = if s.starts_with(['=', '+', '-', '@']) && s.parse::<f64>().is_err() { format!("'{s}") } else { s.to_owned() };
    if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s }
}

fn num(v: Option<f64>) -> String {
    v.map(|v| format!("{v:.6}")).unwrap_or_default()
}

fn label(store: &Store, id: ElementId) -> String {
    store.get(id).and_then(|e| e.name.clone()).unwrap_or_else(|| format!("#{}", id.0))
}

/// The report as CSV: SI base units, one row per segment, fitting, total and critical-path
/// step, with a `section` column.
pub fn report_csv(store: &Store, results: &[NetworkResult], s: &ReportSettings) -> String {
    let mut out = String::from(
        "section,system,element,name,kind,part,size,length (m),flow (m3/s),velocity (m/s),friction (Pa/m),coefficient,loss (Pa),pressure (Pa),critical,overridden\n",
    );
    let row = |out: &mut String, cells: &[String]| {
        out.push_str(&cells.iter().map(|c| csv_field(c)).collect::<Vec<_>>().join(","));
        out.push('\n');
    };
    for r in results {
        if s.has("system") {
            row(&mut out, &["system".into(), r.name.clone(), r.equipment.map(|e| e.0.to_string()).unwrap_or_default(), r.equipment.map(|e| label(store, e)).unwrap_or_default(), r.classification.label().into(), String::new(), String::new(), String::new(), num(Some(r.total_flow)), String::new(), String::new(), String::new(), String::new(), String::new(), String::new(), String::new()]);
        }
        for a in &r.segments {
            let section = if a.kind == "segment" { "segments" } else { "fittings" };
            if !s.has(section) {
                continue;
            }
            row(&mut out, &[
                section.into(), r.name.clone(), a.element.0.to_string(), label(store, a.element), a.kind.clone(), a.part.clone().unwrap_or_default(), a.size.clone(),
                num(a.length), num(Some(a.flow)), num(Some(a.velocity)), num(a.friction), num(a.coefficient), num(a.loss), num(a.pressure),
                if a.critical { "yes" } else { "no" }.into(), if a.overridden { "yes" } else { "no" }.into(),
            ]);
        }
        if s.has("total") {
            let blank = || String::new();
            row(&mut out, &["total".into(), r.name.clone(), blank(), blank(), blank(), blank(), blank(), blank(), num(Some(r.total_flow)), blank(), blank(), blank(), num(r.critical_loss), blank(), blank(), blank()]);
        }
        if s.has("critical_path") {
            for id in &r.critical_path {
                let a = r.segments.iter().find(|a| a.element == *id);
                row(&mut out, &[
                    "critical_path".into(), r.name.clone(), id.0.to_string(), label(store, *id), a.map(|a| a.kind.clone()).unwrap_or_else(|| "terminal".into()), a.and_then(|a| a.part.clone()).unwrap_or_default(), a.map(|a| a.size.clone()).unwrap_or_default(),
                    String::new(), num(a.map(|a| a.flow)), String::new(), String::new(), String::new(),
                    num(a.and_then(|a| a.loss).or_else(|| r.terminals.iter().find(|t| t.element == *id).map(|t| t.own_loss))), String::new(), "yes".into(), String::new(),
                ]);
            }
        }
    }
    out
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The report as an HTML page, in the project's units.
pub fn report_html(store: &Store, results: &[NetworkResult], s: &ReportSettings) -> String {
    let units = crate::units::settings_of(store);
    let f = |v: f64, t: &str| crate::units::format(v, t, &units, None).unwrap_or_else(|_| format!("{v:.3}"));
    let mut h = String::from("<!doctype html><html><head><meta charset=\"utf-8\"><title>Pressure Loss Report</title><style>body{font-family:sans-serif;margin:2em}table{border-collapse:collapse;margin:.5em 0 1.5em}td,th{border:1px solid #999;padding:2px 6px;text-align:right}th{background:#eee}td:first-child,th:first-child{text-align:left}</style></head><body>\n<h1>Pressure Loss Report</h1>\n");
    let _ = writeln!(h, "<p>Units: lengths and sizes in the project's units; flow m³/s; velocity m/s; friction Pa/m; losses and pressures Pa.</p>");
    if s.has("project") {
        let project = store.iter().find(|e| e.class == "IfcProject").and_then(|e| e.name.clone()).unwrap_or_default();
        let _ = writeln!(h, "<h2>Project</h2><p>{}</p>", esc(&project));
    }
    for r in results {
        let _ = writeln!(h, "<h2>{}</h2>", esc(&r.name));
        if s.has("system") {
            let _ = writeln!(
                h,
                "<table><tr><th>Classification</th><td>{}</td></tr><tr><th>Equipment</th><td>{}</td></tr><tr><th>Total flow</th><td>{:.4}</td></tr><tr><th>Friction equation</th><td>{:?}</td></tr></table>",
                r.classification.label(),
                esc(&r.equipment.map(|e| label(store, e)).unwrap_or_default()),
                r.total_flow,
                r.equation
            );
        }
        if s.has("total") {
            let seg: f64 = r.segments.iter().filter(|a| a.kind == "segment" && a.critical).filter_map(|a| a.loss).sum();
            let fit: f64 = r.segments.iter().filter(|a| a.kind == "fitting" && a.critical).filter_map(|a| a.loss).sum();
            let term: f64 = r.terminals.iter().filter(|t| t.critical).map(|t| t.own_loss).sum();
            let _ = writeln!(
                h,
                "<h3>Total pressure loss</h3><table><tr><th>Along the critical path</th><th>Loss (Pa)</th></tr><tr><td>Segments</td><td>{seg:.1}</td></tr><tr><td>Fittings</td><td>{fit:.1}</td></tr><tr><td>Terminal</td><td>{term:.1}</td></tr><tr><th>Total</th><th>{:.1}</th></tr></table>",
                r.critical_loss.unwrap_or(0.0)
            );
        }
        if s.has("segments") {
            h.push_str("<h3>Segment detail</h3><table><tr><th>Segment</th><th>Size</th><th>Length</th><th>Flow</th><th>Velocity</th><th>Friction</th><th>Loss</th><th>Critical</th><th>Overridden</th></tr>\n");
            for a in r.segments.iter().filter(|a| a.kind == "segment") {
                let _ = writeln!(
                    h,
                    "<tr><td>{}</td><td>{}</td><td>{}</td><td>{:.4}</td><td>{:.2}</td><td>{:.2}</td><td>{:.1}</td><td>{}</td><td>{}</td></tr>",
                    esc(&label(store, a.element)),
                    esc(&a.size),
                    a.length.map(|l| f(l, "length")).unwrap_or_default(),
                    a.flow,
                    a.velocity,
                    a.friction.unwrap_or(0.0),
                    a.loss.unwrap_or(0.0),
                    if a.critical { "Yes" } else { "" },
                    if a.overridden { "Yes" } else { "" }
                );
            }
            h.push_str("</table>\n");
        }
        if s.has("fittings") {
            h.push_str("<h3>Fitting and accessory detail</h3><table><tr><th>Fitting</th><th>Part</th><th>Loss method</th><th>Coefficient</th><th>Loss</th><th>Critical</th><th>Overridden</th></tr>\n");
            for a in r.segments.iter().filter(|a| a.kind == "fitting") {
                let _ = writeln!(
                    h,
                    "<tr><td>{}</td><td>{}</td><td>Coefficient</td><td>{:.3}</td><td>{:.1}</td><td>{}</td><td>{}</td></tr>",
                    esc(&label(store, a.element)),
                    esc(a.part.as_deref().unwrap_or("")),
                    a.coefficient.unwrap_or(0.0),
                    a.loss.unwrap_or(0.0),
                    if a.critical { "Yes" } else { "" },
                    if a.overridden { "Yes" } else { "" }
                );
            }
            h.push_str("</table>\n");
        }
        if s.has("critical_path") {
            h.push_str("<h3>Critical path</h3><table><tr><th>Step</th><th>Element</th><th>Loss</th></tr>\n");
            for (i, id) in r.critical_path.iter().enumerate() {
                let loss = r.segments.iter().find(|a| a.element == *id).and_then(|a| a.loss).or_else(|| r.terminals.iter().find(|t| t.element == *id).map(|t| t.own_loss)).unwrap_or(0.0);
                let _ = writeln!(h, "<tr><td>{}</td><td>{}</td><td>{loss:.1}</td></tr>", i + 1, esc(&label(store, *id)));
            }
            let _ = writeln!(h, "<tr><th></th><th>Total</th><th>{:.1}</th></tr></table>", r.critical_loss.unwrap_or(0.0));
        }
    }
    h.push_str("</body></html>\n");
    h
}

/// `pressure_loss_report.export`.
pub(crate) fn export_report(txn: &mut Transaction<'_>, p: ReportParams) -> Result<ReportResult> {
    let store = txn.store().clone();
    if p.path.exists() && !p.overwrite {
        return Err(bad("file.exists", format!("{} exists; pass overwrite to replace it", p.path.display())));
    }
    if p.system_ids.is_empty() {
        return Err(bad("pressure_loss_report.no_systems", "choose the systems to report"));
    }
    let settings = match &p.settings.preset_id {
        Some(id) => mep_systems::settings(&store)
            .report_presets
            .into_iter()
            .find(|x| &x.id == id)
            .map(|x| x.settings)
            .ok_or_else(|| bad("pressure_loss_report.preset", format!("no report preset {id:?}")))?,
        None => p.settings.clone(),
    };
    if let Some(bad_section) = settings.sections.iter().find(|s| !SECTIONS.contains(&s.as_str())) {
        return Err(bad("pressure_loss_report.section", format!("no section {bad_section:?}; the sections are {}", SECTIONS.join(", "))));
    }
    let mut results = Vec::new();
    let mut recalculated = Vec::new();
    for r in &p.system_ids {
        let sys = system_of(&store, r)?;
        if let Some(why) = not_reportable(&store, &sys) {
            return Err(bad("pressure_loss_report.not_reportable", format!("{} cannot be reported: {why}", sys.name)));
        }
        let now = txn.store().clone();
        match kept(&now, &sys) {
            Some(k) if !k.stale => results.push(k),
            k => {
                let eq = k.map_or_else(|| equation_of(&now, None), |k| k.equation);
                results.push(calculate_one(txn, &sys, eq)?);
                recalculated.push(sys.id);
            }
        }
    }
    let store = txn.store().clone();
    let text = match p.format {
        ReportFormat::Csv => report_csv(&store, &results, &settings),
        ReportFormat::Html => report_html(&store, &results, &settings),
    };
    std::fs::write(&p.path, text).map_err(|e| bad("file.write_failed", e.to_string()))?;
    Ok(ReportResult { path: p.path, systems: results.len(), recalculated })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PresetParams {
    pub name: String,
    pub settings: ReportSettings,
}

/// `pressure_loss_report.save_preset`: returns its id.
pub(crate) fn save_preset(txn: &mut Transaction<'_>, p: PresetParams) -> Result<String> {
    let name = p.name.trim().to_owned();
    if name.is_empty() {
        return Err(bad("pressure_loss_report.preset_name", "a preset needs a name"));
    }
    let mut s = mep_systems::settings(txn.store());
    let id = name.to_lowercase().replace(|c: char| !c.is_ascii_alphanumeric(), "_");
    let mut settings = p.settings;
    settings.preset_id = None;
    s.report_presets.retain(|x| x.id != id);
    s.report_presets.push(ReportPreset { id: id.clone(), name, settings });
    mep_systems::save_settings(txn, &s)?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port(direction: [f64; 3], d: f64) -> mep::Port {
        let c: mep::Connector = serde_json::from_value(serde_json::json!({ "domain": "duct", "origin": [0, 0, 0], "direction": direction, "shape": "round", "diameter": d })).unwrap();
        mep::Port { element: ElementId(1), index: 0, connector: c, position: [0.0; 3], direction }
    }

    #[test]
    fn coefficients_by_geometry() {
        let (a, b) = (port([-1.0, 0.0, 0.0], 0.2), port([0.0, 1.0, 0.0], 0.2));
        assert!((coefficient(Domain::Duct, Part::Elbow, &a, &b) - 1.2).abs() < 1e-9);
        let c = port([1.0, 1.0, 0.0].map(|v: f64| v / 2f64.sqrt()), 0.2);
        assert!((coefficient(Domain::Duct, Part::Elbow, &a, &c) - 0.34).abs() < 1e-9);
        let straight = port([1.0, 0.0, 0.0], 0.2);
        assert_eq!(coefficient(Domain::Duct, Part::Tee, &a, &straight), DUCT_JUNCTION.0);
        assert_eq!(coefficient(Domain::Pipe, Part::Tee, &a, &b), PIPE_JUNCTION.1);
        // An expansion from Ø200 to Ø400: half of (4 − 1)².
        let big = port([1.0, 0.0, 0.0], 0.4);
        assert!((coefficient(Domain::Duct, Part::Transition, &a, &big) - 4.5).abs() < 1e-9);
        assert_eq!(coefficient(Domain::Duct, Part::Transition, &big, &a), CONTRACTION);
    }
}
