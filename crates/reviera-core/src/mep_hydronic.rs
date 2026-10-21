// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Hydronic network analysis and hydraulic separation (MEP-PIPE-008).
//!
//! With the analysis on (Mechanical Settings › Pipe › Calculation), closed-loop hydronic networks
//! report each pump's flow and head.
//!
//! - **The network** is everything joined through pipe connectors from the pumps: pipes and
//!   fittings, and equipment through their Hydronic Supply and Return connectors. Pumps, boilers
//!   and chillers are the components of those categories; other equipment with hydronic
//!   connectors (fan coils, coils, AHUs) are terminals.
//! - **Hydraulic separation** marks a pipe, fitting or tank as a decoupler (RXD element data
//!   `rxd:analysis`). The decoupler and the tees joined directly to it (a common pipe between
//!   close tees) form one hydraulic node with no loss that splits the network into **loops**: the
//!   parts left joined when the nodes are taken out. A loop with a boiler or chiller is primary;
//!   those reached across one node secondary, across two tertiary.
//! - **A valid loop** has one pump, a source (in its network of loops), pipe segments, and
//!   terminals or a separation for flow to circulate through.
//! - **Flow**: a terminal's design flow is its supply connector's; a loop's flow is its
//!   terminals' and, across each separation it serves, the flows of the loops beyond (primary
//!   pumps add up the loops they serve).
//! - **Head**: for each terminal (or separation it serves), the circuit from the pump's
//!   discharge through it and back to the pump's suction, without passing another terminal or
//!   pump; its loss is the pipes' friction and the fittings' losses at the flows through them
//!   (as MEP-CALC-002 computes them, with overrides) and the equipment's connector losses. The
//!   pump's head is the largest. Across a separation the loop beyond is not seen: a secondary
//!   pump's head excludes the chiller's pressure drop.
//!
//! Results are computed when asked (`hydronic.analyze`); nothing is kept.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::mep::{self, Classification, Domain, FlowDirection, LossMethod, Port};
use crate::mep_network::{coefficient, override_of, size_of, valve_coefficient};
use crate::mep_runs::{self, Part};
use crate::mep_sizing::{self, flow_in, section};
use crate::mep_systems;
use crate::mesh_objects::{self, Category};
use crate::store::Store;
use crate::txn::Transaction;

/// RXD element data on a separating pipe, fitting or tank: `{"hydraulic_separation": true}`.
pub const ANALYSIS_KEY: &str = "rxd:analysis";

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

// ---- Settings and separations ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EnabledParams {
    pub enabled: bool,
}

/// `hydronic.set_enabled`.
pub(crate) fn set_enabled(txn: &mut Transaction<'_>, p: EnabledParams) -> Result<()> {
    let mut s = mep_systems::settings(txn.store());
    s.hydronic_analysis = p.enabled;
    mep_systems::save_settings(txn, &s)
}

/// Whether an element is marked as a hydraulic separation.
pub fn is_separation(e: &Element) -> bool {
    e.param(ANALYSIS_KEY).and_then(|v| v.get("hydraulic_separation")).and_then(|v| v.as_bool()) == Some(true)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SeparationParams {
    /// A pipe, pipe fitting or tank (equipment with pipe connectors).
    pub element_id: ElementRef,
}

fn separable(store: &Store, r: &ElementRef) -> Result<ElementId> {
    let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such element"))?;
    let e = store.get(id).expect("resolved");
    let pipe_ports = mep::ports(store, e).iter().filter(|p| p.connector.domain == Domain::Pipe).count();
    let run = mep_runs::segment_recipe(e).map(|s| s.domain).or_else(|| mep_runs::fitting_recipe(e).map(|f| f.domain));
    let ok = match run {
        Some(d) => d == Domain::Pipe,
        None => mesh_objects::mesh_object_recipe(e).is_some() && pipe_ports >= 2,
    };
    if !ok {
        return Err(bad("hydraulic_separation.not_pipe", format!("element {id} is not a pipe, pipe fitting or tank")));
    }
    Ok(id)
}

/// `hydraulic_separation.add`.
pub(crate) fn add(txn: &mut Transaction<'_>, p: SeparationParams) -> Result<ElementId> {
    let id = separable(txn.store(), &p.element_id)?;
    txn.set_param(id, ANALYSIS_KEY, json!({ "hydraulic_separation": true }))?;
    Ok(id)
}

/// `hydraulic_separation.remove`.
pub(crate) fn remove(txn: &mut Transaction<'_>, p: SeparationParams) -> Result<()> {
    let store = txn.store().clone();
    let id = store.resolve(&p.element_id).ok_or_else(|| bad("element.not_found", "no such element"))?;
    if !store.get(id).is_some_and(is_separation) {
        return Err(bad("hydraulic_separation.none", format!("element {id} is not a hydraulic separation")));
    }
    txn.modify(id, |e| {
        e.params.remove(ANALYSIS_KEY);
    })
}

// ---- The analysis ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LoadResult {
    /// A terminal, or a separation element the loop serves.
    pub element: ElementId,
    /// m³/s.
    pub flow: f64,
    /// Its circuit's loss, pump discharge to suction (Pa).
    pub loss: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LoopResult {
    /// 1, 2, … in the order of their pumps.
    pub id: usize,
    /// `primary`, `secondary` or `tertiary` (and deeper).
    pub role: String,
    /// 0 for primary.
    pub level: usize,
    pub pump: ElementId,
    /// The boiler or chiller in the loop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ElementId>,
    /// m³/s.
    pub flow: f64,
    /// Pa.
    pub head: f64,
    /// The terminal (or separation) whose circuit sets the head.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub critical: Option<ElementId>,
    pub loads: Vec<LoadResult>,
    /// The separation elements at its edges.
    pub separations: Vec<ElementId>,
    /// How many elements it has (pipes, fittings, equipment).
    pub elements: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HydronicReport {
    pub loops: Vec<LoopResult>,
    /// Loops that cannot be analysed, and why.
    pub problems: Vec<String>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnalyzeParams {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Run,
    Pump,
    Source,
    Terminal,
}

fn role(store: &Store, e: &Element) -> Option<Role> {
    if mep_runs::is_run_element(e) {
        return Some(Role::Run);
    }
    let c = mesh_objects::type_of(store, e)?.1.category;
    Some(match c {
        Category::Pump => Role::Pump,
        Category::Boiler | Category::Chiller => Role::Source,
        _ => Role::Terminal,
    })
}

fn label(store: &Store, id: ElementId) -> String {
    let e = store.get(id);
    let name = e.and_then(|e| mesh_objects::type_of(store, e)).and_then(|(t, _)| store.get(t)).and_then(|t| t.name.clone());
    format!("{} {}", name.or_else(|| e.and_then(|e| e.name.clone())).unwrap_or_else(|| e.map(|e| e.class.clone()).unwrap_or_default()), id.0)
}

/// An element's port.
type PortRef = (ElementId, usize);

/// One step of a circuit: entering `x` at port `k` and leaving `y` (the same element, or another
/// of a separation node) at port `j`.
#[derive(Debug, Clone, Copy)]
struct Step {
    x: ElementId,
    k: usize,
    y: ElementId,
    j: usize,
}

struct Net<'a> {
    store: &'a Store,
    partners: HashMap<(ElementId, usize), Vec<(ElementId, usize)>>,
    ports: HashMap<ElementId, Vec<Port>>,
    group_of: HashMap<ElementId, usize>,
    groups: Vec<BTreeSet<ElementId>>,
}

impl Net<'_> {
    /// The hydronic ports of an element: all pipe ports of pipes and fittings; equipment's
    /// Hydronic Supply and Return.
    fn hports(&self, x: ElementId) -> Vec<&Port> {
        let run = self.store.get(x).is_some_and(mep_runs::is_run_element);
        self.ports
            .get(&x)
            .map(|v| {
                v.iter()
                    .filter(|p| p.connector.domain == Domain::Pipe)
                    .filter(|p| run || matches!(p.connector.classification, Some(Classification::HydronicSupply | Classification::HydronicReturn)))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn port(&self, x: ElementId, k: usize) -> Option<&Port> {
        self.ports.get(&x)?.iter().find(|p| p.index == k)
    }

    fn joined(&self, x: ElementId, k: usize) -> Vec<(ElementId, usize)> {
        self.partners.get(&(x, k)).cloned().unwrap_or_default()
    }

    /// The ports a circuit can leave by after entering `x` at `k`: another port of `x`; of a
    /// separation node, any port of its members leading out of it.
    fn exits(&self, x: ElementId, k: usize) -> Vec<(ElementId, usize)> {
        match self.group_of.get(&x) {
            Some(g) => self.groups[*g]
                .iter()
                .flat_map(|y| self.hports(*y).into_iter().map(move |p| (*y, p.index)))
                .filter(|(y, j)| (*y, *j) != (x, k))
                .filter(|(y, j)| self.joined(*y, *j).iter().any(|(o, _)| self.group_of.get(o) != Some(g)))
                .collect(),
            None => self.hports(x).into_iter().filter(|p| p.index != k).map(|p| (x, p.index)).collect(),
        }
    }

    /// The shortest circuit from leaving by `starts` to arriving where `target` says, through
    /// elements `pass` allows.
    fn search(&self, starts: &[(ElementId, usize)], target: &dyn Fn(ElementId, usize) -> bool, pass: &dyn Fn(ElementId) -> bool) -> Option<(Vec<Step>, (ElementId, usize))> {
        let mut pred: HashMap<PortRef, Option<(PortRef, PortRef)>> = HashMap::new();
        let mut queue = VecDeque::new();
        let key = |o: ElementId, ko: usize| match self.group_of.get(&o) {
            // A node is entered once, wherever.
            Some(g) => (ElementId(u64::MAX - *g as u64), 0),
            None => (o, ko),
        };
        let mut entered: HashMap<(ElementId, usize), (ElementId, usize)> = HashMap::new();
        for (y, j) in starts {
            for (o, ko) in self.joined(*y, *j) {
                let s = key(o, ko);
                if pred.contains_key(&s) {
                    continue;
                }
                pred.insert(s, None);
                entered.insert(s, (o, ko));
                queue.push_back(s);
            }
        }
        while let Some(s) = queue.pop_front() {
            let (x, k) = entered[&s];
            if target(x, k) {
                let mut steps = Vec::new();
                let mut cur = s;
                while let Some(Some((prev, exit))) = pred.get(&cur) {
                    let (px, pk) = entered[prev];
                    steps.push(Step { x: px, k: pk, y: exit.0, j: exit.1 });
                    cur = *prev;
                }
                steps.reverse();
                return Some((steps, (x, k)));
            }
            if !pass(x) {
                continue;
            }
            for (y, j) in self.exits(x, k) {
                for (o, ko) in self.joined(y, j) {
                    if self.group_of.contains_key(&o) && self.group_of.get(&o) == self.group_of.get(&x) {
                        continue;
                    }
                    let n = key(o, ko);
                    if pred.contains_key(&n) {
                        continue;
                    }
                    pred.insert(n, Some((s, (y, j))));
                    entered.insert(n, (o, ko));
                    queue.push_back(n);
                }
            }
        }
        None
    }
}

struct Comp {
    elements: BTreeSet<ElementId>,
    pumps: Vec<ElementId>,
    sources: Vec<ElementId>,
    terminals: Vec<ElementId>,
    groups: BTreeSet<usize>,
    segments: usize,
}

/// `hydronic.analyze`: every loop's pump flow and head.
pub fn analyze(store: &Store, _: AnalyzeParams) -> Result<HydronicReport> {
    let settings = mep_systems::settings(store);
    if !settings.hydronic_analysis {
        return Err(bad("hydronic.off", "Hydronic network analysis is off: turn it on in Mechanical Settings › Pipe › Calculation (hydronic.set_enabled)"));
    }
    let equation = settings.friction_equation;
    let mut net = Net { store, partners: mep::partners(store), ports: HashMap::new(), group_of: HashMap::new(), groups: Vec::new() };
    for e in store.iter() {
        if role(store, e).is_some() {
            let p = mep::ports(store, e);
            if p.iter().any(|p| p.connector.domain == Domain::Pipe) {
                net.ports.insert(e.id(), p);
            }
        }
    }
    // Separation nodes: each separation and the tees and crosses joined directly to it.
    let mut groups: Vec<BTreeSet<ElementId>> = Vec::new();
    for e in store.iter().filter(|e| is_separation(e) && net.ports.contains_key(&e.id())) {
        let mut g = BTreeSet::from([e.id()]);
        if mep_runs::is_run_element(e) {
            for p in net.hports(e.id()) {
                for (o, _) in net.joined(e.id(), p.index) {
                    if store.get(o).is_some_and(|x| mep_runs::fitting_recipe(x).is_some()) && net.hports(o).len() >= 3 {
                        g.insert(o);
                    }
                }
            }
        }
        // Merged with any node it touches.
        let (touching, rest): (Vec<_>, Vec<_>) = groups.into_iter().partition(|h| !h.is_disjoint(&g));
        for h in touching {
            g.extend(h);
        }
        groups = rest;
        groups.push(g);
    }
    for (k, g) in groups.iter().enumerate() {
        for x in g {
            net.group_of.insert(*x, k);
        }
    }
    net.groups = groups;

    // Loops: what stays joined without the nodes, from the pumps and the nodes' sides.
    let mut seeds: Vec<ElementId> = net.ports.keys().copied().filter(|x| store.get(*x).and_then(|e| role(store, e)) == Some(Role::Pump)).collect();
    seeds.sort();
    for g in &net.groups {
        for y in g {
            for p in net.hports(*y) {
                seeds.extend(net.joined(*y, p.index).into_iter().map(|(o, _)| o).filter(|o| !net.group_of.contains_key(o)));
            }
        }
    }
    let mut seen: BTreeSet<ElementId> = BTreeSet::new();
    let mut comps: Vec<Comp> = Vec::new();
    for seed in seeds {
        if seen.contains(&seed) || net.group_of.contains_key(&seed) {
            continue;
        }
        let mut c = Comp { elements: BTreeSet::new(), pumps: Vec::new(), sources: Vec::new(), terminals: Vec::new(), groups: BTreeSet::new(), segments: 0 };
        let mut stack = vec![seed];
        while let Some(x) = stack.pop() {
            if !seen.insert(x) {
                continue;
            }
            c.elements.insert(x);
            let e = store.get(x).expect("in the store");
            match role(store, e) {
                Some(Role::Pump) => c.pumps.push(x),
                Some(Role::Source) => c.sources.push(x),
                Some(Role::Terminal) => c.terminals.push(x),
                _ => {}
            }
            if mep_runs::segment_recipe(e).is_some() {
                c.segments += 1;
            }
            for p in net.hports(x) {
                for (o, _) in net.joined(x, p.index) {
                    match net.group_of.get(&o) {
                        Some(g) => {
                            c.groups.insert(*g);
                        }
                        None if net.ports.contains_key(&o) => stack.push(o),
                        None => {}
                    }
                }
            }
        }
        // Pass-through equipment with no hydronic ports leading on is not part of a loop.
        comps.push(c);
    }

    // Levels: primary where a source is, one deeper across each node.
    let mut level: Vec<Option<usize>> = comps.iter().map(|c| (!c.sources.is_empty()).then_some(0)).collect();
    let mut queue: VecDeque<usize> = (0..comps.len()).filter(|k| level[*k] == Some(0)).collect();
    while let Some(a) = queue.pop_front() {
        for b in 0..comps.len() {
            if level[b].is_none() && !comps[a].groups.is_disjoint(&comps[b].groups) {
                level[b] = Some(level[a].unwrap_or(0) + 1);
                queue.push_back(b);
            }
        }
    }

    // The fluid: water at the hydronic supply system's temperature (the type of a system holding
    // a pump, else the built-in type).
    let st = mep_systems::systems(store)
        .into_iter()
        .find(|s| s.classification == Classification::HydronicSupply && (s.equipment.is_some_and(|e| comps.iter().any(|c| c.pumps.contains(&e))) || s.elements.iter().any(|e| comps.iter().any(|c| c.pumps.contains(e)))))
        .and_then(|s| mep_systems::system_type(store, &s.system_type))
        .unwrap_or_else(|| mep_systems::builtin(Classification::HydronicSupply));
    let fluid = mep_sizing::water(st.fluid_temperature.unwrap_or(20.0))?;

    let mut problems = Vec::new();
    let mut results: BTreeMap<usize, LoopResult> = BTreeMap::new();
    let mut totals: HashMap<usize, f64> = HashMap::new();
    // Deepest first, so a loop knows the flows of those beyond its separations.
    let mut order: Vec<usize> = (0..comps.len()).collect();
    order.sort_by_key(|k| std::cmp::Reverse(level[*k].unwrap_or(0)));
    for k in order {
        let c = &comps[k];
        let names = |v: &[ElementId]| v.iter().map(|x| label(store, *x)).collect::<Vec<_>>().join(", ");
        let what = match c.pumps.first() {
            Some(p) => format!("The loop of {}", label(store, *p)),
            None => format!("A loop of {} elements", c.elements.len()),
        };
        if c.pumps.len() != 1 {
            if c.pumps.is_empty() {
                problems.push(format!("{what} has no pump"));
            } else {
                problems.push(format!("{what} has {} pumps ({}): one pump per loop; add a hydraulic separation between them", c.pumps.len(), names(&c.pumps)));
            }
            continue;
        }
        let Some(lv) = level[k] else {
            problems.push(format!("{what} has no source (boiler or chiller) in its network"));
            continue;
        };
        if c.segments == 0 {
            problems.push(format!("{what} has no pipe segments"));
            continue;
        }
        let pump = c.pumps[0];
        let pp = net.hports(pump);
        let (Some(dis), Some(suc)) = (pp.iter().find(|p| p.connector.flow_direction == FlowDirection::Out).map(|p| p.index), pp.iter().find(|p| p.connector.flow_direction == FlowDirection::In).map(|p| p.index)) else {
            problems.push(format!("{what}: the pump needs a discharge (out) and a suction (in) hydronic connector"));
            continue;
        };
        // Its loads: its terminals, and the nodes it serves (toward deeper loops).
        let mut loads: Vec<(ElementId, Option<usize>, f64)> = Vec::new();
        for t in &c.terminals {
            let q = net.hports(*t).iter().filter(|p| p.connector.flow_direction == FlowDirection::In).map(|p| p.connector.flow).fold(0.0, f64::max);
            loads.push((*t, None, q));
        }
        for g in &c.groups {
            let beyond: Vec<usize> = (0..comps.len()).filter(|b| *b != k && comps[*b].groups.contains(g) && level[*b].is_some_and(|l| l > lv)).collect();
            if beyond.is_empty() {
                continue;
            }
            let q: f64 = beyond.iter().map(|b| totals.get(b).copied().unwrap_or(0.0)).sum();
            let rep = net.groups[*g].iter().copied().find(|x| store.get(*x).is_some_and(is_separation)).expect("a separation");
            loads.push((rep, Some(*g), q));
        }
        if loads.is_empty() {
            problems.push(format!("{what} has no terminal or separation for flow to circulate through"));
            continue;
        }
        let in_loop = |x: ElementId| c.elements.contains(&x) || net.group_of.get(&x).is_some_and(|g| c.groups.contains(g));
        // Each load's circuit.
        let mut circuits: Vec<(usize, Vec<Step>)> = Vec::new();
        let mut failed = false;
        for (n, (l, lg, _)) in loads.iter().enumerate() {
            let is_load = |x: ElementId| match lg {
                Some(g) => net.group_of.get(&x) == Some(g),
                None => x == *l,
            };
            let others = |x: ElementId| loads.iter().any(|(m, mg, _)| match mg {
                Some(g) => net.group_of.get(&x) == Some(g),
                None => x == *m,
            });
            let pass = |x: ElementId| in_loop(x) && x != pump && !others(x);
            let to_load = |x: ElementId, k: usize| is_load(x) && (lg.is_some() || net.port(x, k).is_some_and(|p| p.connector.flow_direction != FlowDirection::Out));
            let Some((mut steps, (lx, lk))) = net.search(&[(pump, dis)], &to_load, &pass) else {
                problems.push(format!("{what}: {} is not on a circuit from the pump's discharge", label(store, *l)));
                failed = true;
                break;
            };
            let outs: Vec<(ElementId, usize)> = match lg {
                Some(_) => net.exits(lx, lk),
                None => net.hports(lx).iter().filter(|p| p.index != lk && p.connector.flow_direction != FlowDirection::In).map(|p| (lx, p.index)).collect(),
            };
            let back = |x: ElementId, k: usize| x == pump && k == suc;
            let Some((rest, _)) = net.search(&outs, &back, &pass) else {
                problems.push(format!("{what}: {} is not on a circuit back to the pump's suction", label(store, *l)));
                failed = true;
                break;
            };
            // The load itself, entered at `lk`, left where the way back starts.
            let leave = rest.first().map(|s| (s.x, s.k)).and_then(|(fx, fk)| outs.iter().copied().find(|(y, j)| net.joined(*y, *j).contains(&(fx, fk))));
            let leave = leave.or_else(|| outs.iter().copied().find(|(y, j)| net.joined(*y, *j).contains(&(pump, suc))));
            let (ly, lj) = leave.unwrap_or((lx, lk));
            steps.push(Step { x: lx, k: lk, y: ly, j: lj });
            steps.extend(rest);
            circuits.push((n, steps));
        }
        if failed {
            continue;
        }
        // Flows: each way out of each element carries the loads whose circuits leave by it.
        let mut flow: HashMap<(ElementId, usize), f64> = HashMap::new();
        for (n, steps) in &circuits {
            for s in steps {
                *flow.entry((s.y, s.j)).or_default() += loads[*n].2;
            }
        }
        let total: f64 = loads.iter().map(|l| l.2).sum();
        totals.insert(k, total);
        let vp = |q: f64, p: &Port| {
            let a = section(&size_of(&p.connector)).0;
            if a <= 0.0 { 0.0 } else { fluid.0 * (q / a).powi(2) / 2.0 }
        };
        let equipment_loss = |p: Option<&Port>, q: f64| {
            p.map_or(0.0, |p| match p.connector.loss_method {
                LossMethod::Coefficient => p.connector.loss * vp(q, p),
                LossMethod::SpecificLoss => p.connector.loss,
                LossMethod::NotDefined => 0.0,
            })
        };
        let mut load_results = Vec::new();
        for (n, steps) in &circuits {
            let mut loss = 0.0;
            for s in steps {
                if net.group_of.contains_key(&s.x) {
                    continue;
                }
                let q = flow.get(&(s.y, s.j)).copied().unwrap_or(0.0);
                let e = store.get(s.x).expect("in the store");
                let ov = override_of(e);
                if let Some(sr) = mep_runs::segment_recipe(e) {
                    let roughness = GlobalId::parse(&sr.segment_type).and_then(|g| store.find_by_guid(&g)).and_then(|t| store.get(t)).and_then(mep_runs::segment_type).map_or(0.0, |t| t.roughness);
                    let len = match sr.corner {
                        Some(c) => dist(sr.start, c) + dist(c, sr.end),
                        None => dist(sr.start, sr.end),
                    };
                    loss += ov.loss.unwrap_or(flow_in(q, &sr.size, roughness, fluid, equation).1 * len);
                } else if let Some(fr) = mep_runs::fitting_recipe(e) {
                    let (Some(i), Some(o)) = (net.port(s.x, s.k), net.port(s.x, s.j)) else { continue };
                    let kk = ov.coefficient.unwrap_or_else(|| if fr.part == Part::Valve { valve_coefficient(fr.accessory.as_deref()) } else { coefficient(fr.domain, fr.part, i, o) });
                    loss += ov.loss.unwrap_or(kk * vp(q, o));
                } else {
                    // Equipment: the losses on the connectors it is entered and left by.
                    let q = if s.x == loads[*n].0 { loads[*n].2 } else { q };
                    loss += equipment_loss(net.port(s.x, s.k), q) + if s.j != s.k { equipment_loss(net.port(s.x, s.j), q) } else { 0.0 };
                }
            }
            load_results.push(LoadResult { element: loads[*n].0, flow: loads[*n].2, loss });
        }
        let crit = load_results.iter().max_by(|a, b| a.loss.total_cmp(&b.loss)).cloned();
        let mut separations: Vec<ElementId> = c.groups.iter().flat_map(|g| net.groups[*g].iter().copied()).filter(|x| store.get(*x).is_some_and(is_separation)).collect();
        separations.sort();
        results.insert(
            pump.0 as usize,
            LoopResult {
                id: 0,
                role: match lv {
                    0 => "primary",
                    1 => "secondary",
                    _ => "tertiary",
                }
                .into(),
                level: lv,
                pump,
                source: c.sources.first().copied(),
                flow: total,
                head: crit.as_ref().map_or(0.0, |c| c.loss),
                critical: crit.map(|c| c.element),
                loads: load_results,
                separations,
                elements: c.elements.len(),
            },
        );
    }
    let mut loops: Vec<LoopResult> = results.into_values().collect();
    for (k, l) in loops.iter_mut().enumerate() {
        l.id = k + 1;
    }
    Ok(HydronicReport { loops, problems })
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_by_default() {
        let store = Store::default();
        assert_eq!(analyze(&store, AnalyzeParams::default()).unwrap_err().code, "hydronic.off");
    }
}
