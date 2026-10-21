// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Duct and pipe sizing (MEP-CALC-001, MEP-PIPE-005).
//!
//! - **Flow**: a system's network is walked from its equipment; each segment's design flow is
//!   the sum of the terminals' flows beyond it. Domestic water converts the *sum of the
//!   fixture units* beyond it with a fixture-unit table (the flush-tank or flush-valve curve)
//!   when one is set; otherwise connector flows are summed and the report says so.
//! - **Friction**: Darcy–Weisbach, Δp/L = f/D · ρv²/2, with D the hydraulic diameter
//!   (2wh/(w+h) for rectangles) and f by Colebrook (iterated) or Haaland, 64/Re when laminar.
//!   Air at 20 °C (1.204 kg/m³, 1.81 × 10⁻⁵ Pa·s); water from a table of density and viscosity
//!   by temperature (0–100 °C), interpolated; other fluids are refused.
//! - **Methods**: Friction, Velocity, both (And or Or) and Equal Friction (1 Pa/m for ducts by
//!   default). The smallest size of the segment type's sizing table meeting the targets within
//!   the restrictions (height, width, inner diameter) is chosen; restrictions win, with a
//!   warning. Rectangles keep an aspect ratio up to 4. Branches to terminals take the computed
//!   size, the terminal's connector size, or the larger. Size-locked segments are skipped.
//!
//! Reviera ships no fixture-unit table: the built-in curves of the public-domain reports are not
//! yet transcribed from a verified copy (users import theirs).

use std::collections::{BTreeMap, HashMap, VecDeque};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::mep::{self, Classification, Shape};
use crate::mep_runs::{self, Size};
use crate::store::Store;
use crate::txn::{Transaction, Warning};

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

// ---- Fluids and friction ----

/// Air at 20 °C: density (kg/m³) and dynamic viscosity (Pa·s).
pub const AIR: (f64, f64) = (1.204, 1.81e-5);

/// Water: temperature (°C), density (kg/m³), dynamic viscosity (mPa·s).
const WATER: [(f64, f64, f64); 11] = [
    (0.0, 999.8, 1.792),
    (10.0, 999.7, 1.307),
    (20.0, 998.2, 1.002),
    (30.0, 995.7, 0.798),
    (40.0, 992.2, 0.653),
    (50.0, 988.0, 0.547),
    (60.0, 983.2, 0.467),
    (70.0, 977.8, 0.404),
    (80.0, 971.8, 0.355),
    (90.0, 965.3, 0.315),
    (100.0, 958.4, 0.282),
];

/// Water's density and viscosity (Pa·s) at a temperature.
pub fn water(t: f64) -> Result<(f64, f64)> {
    let (lo, hi) = (WATER[0].0, WATER[WATER.len() - 1].0);
    if !(lo..=hi).contains(&t) {
        return Err(bad("mep_size.fluid_temperature", "Temperature outside fluid table range"));
    }
    let k = WATER.windows(2).position(|w| t <= w[1].0).unwrap_or(WATER.len() - 2);
    let (a, b) = (WATER[k], WATER[k + 1]);
    let u = (t - a.0) / (b.0 - a.0);
    Ok((a.1 + (b.1 - a.1) * u, (a.2 + (b.2 - a.2) * u) / 1000.0))
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Equation {
    #[default]
    Colebrook,
    Haaland,
}

/// The Darcy friction factor at a Reynolds number and relative roughness ε/D.
pub fn friction_factor(re: f64, rel: f64, eq: Equation) -> f64 {
    if re <= 0.0 {
        return 0.0;
    }
    if re < 2300.0 {
        return 64.0 / re;
    }
    match eq {
        Equation::Haaland => {
            let x = -1.8 * ((rel / 3.7).powf(1.11) + 6.9 / re).log10();
            1.0 / (x * x)
        }
        Equation::Colebrook => {
            // Fixed-point on 1/√f, started from Haaland.
            let mut x = 1.0 / friction_factor(re, rel, Equation::Haaland).sqrt();
            for _ in 0..50 {
                let next = -2.0 * (rel / 3.7 + 2.51 * x / re).log10();
                if (next - x).abs() < 1e-12 {
                    x = next;
                    break;
                }
                x = next;
            }
            1.0 / (x * x)
        }
    }
}

/// Area (m²) and hydraulic diameter (m) of a size.
pub(crate) fn section(s: &Size) -> (f64, f64) {
    match s.shape {
        Shape::Round => (std::f64::consts::PI * s.diameter * s.diameter / 4.0, s.diameter),
        _ => (s.width * s.height, 2.0 * s.width * s.height / (s.width + s.height)),
    }
}

/// Velocity (m/s) and friction (Pa/m) of flow `q` (m³/s) in a size.
pub fn flow_in(q: f64, s: &Size, roughness: f64, fluid: (f64, f64), eq: Equation) -> (f64, f64) {
    let (a, d) = section(s);
    if a <= 0.0 || d <= 0.0 {
        return (0.0, 0.0);
    }
    let v = q / a;
    let re = fluid.0 * v * d / fluid.1;
    let f = friction_factor(re, roughness / d, eq);
    (v, f / d * fluid.0 * v * v / 2.0)
}

// ---- Fixture units ----

/// A fixture-unit table: flow (l/s) by fixture units for flush-tank and flush-valve systems.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FixtureTable {
    pub id: String,
    pub name: String,
    /// Where it comes from (code, edition).
    #[serde(default)]
    pub source: String,
    /// Rows `[fixture_units, tank l/s, valve l/s]`, ascending.
    pub rows: Vec<[f64; 3]>,
}

impl FixtureTable {
    /// The flow (l/s) of a fixture-unit total, interpolated (and extrapolated along the last
    /// rows' slope).
    pub fn flow(&self, fu: f64, valves: bool) -> f64 {
        let col = if valves { 2 } else { 1 };
        let r = &self.rows;
        if r.is_empty() || fu <= 0.0 {
            return 0.0;
        }
        if fu <= r[0][0] {
            return r[0][col] * fu / r[0][0].max(1e-9);
        }
        let k = r.windows(2).position(|w| fu <= w[1][0]).unwrap_or(r.len().saturating_sub(2));
        if r.len() < 2 {
            return r[0][col];
        }
        let (a, b) = (r[k], r[k + 1]);
        a[col] + (b[col] - a[col]) * (fu - a[0]) / (b[0] - a[0])
    }
}

/// Parses a fixture-unit CSV (`fixture_units, flow_tank, flow_valve`, ascending; a header line
/// is skipped); flows in `unit` (`l/s` or `gpm`) are stored in l/s.
pub fn parse_fixture_csv(text: &str, unit: &str) -> Result<Vec<[f64; 3]>> {
    let k = match unit {
        "l/s" | "" => 1.0,
        "gpm" => 0.063_090_2,
        u => return Err(bad("fixture_unit_table.unit", format!("flow units are l/s or gpm, not {u:?}"))),
    };
    let mut rows: Vec<[f64; 3]> = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split([',', ';', '\t']).map(str::trim).collect();
        let nums: Option<Vec<f64>> = parts.iter().map(|p| p.parse::<f64>().ok()).collect();
        let Some(v) = nums.filter(|v| v.len() == 3) else {
            if rows.is_empty() && n == 0 {
                continue; // a header
            }
            return Err(bad("fixture_unit_table.row", format!("line {}: write fixture_units, flow_tank, flow_valve", n + 1)));
        };
        if v.iter().any(|x| *x < 0.0) {
            return Err(bad("fixture_unit_table.row", format!("line {}: values cannot be negative", n + 1)));
        }
        if let Some(prev) = rows.last()
            && (v[0] <= prev[0] || v[1] * k < prev[1] || v[2] * k < prev[2])
        {
            return Err(bad("fixture_unit_table.order", format!("line {}: rows must ascend (fixture units rising, flows not falling)", n + 1)));
        }
        rows.push([v[0], v[1] * k, v[2] * k]);
    }
    if rows.len() < 2 {
        return Err(bad("fixture_unit_table.rows", "a table needs at least two rows"));
    }
    Ok(rows)
}

// ---- Flows through a system ----

/// Each element's design flow (m³/s) and fixture units beyond it, from the system's equipment.
pub(crate) struct Flows {
    pub(crate) flow: HashMap<ElementId, f64>,
    pub(crate) units: HashMap<ElementId, f64>,
    /// Terminals (neither runs nor the equipment) joined to each run element.
    pub(crate) terminal_of: HashMap<ElementId, ElementId>,
}

impl Flows {
    /// An element's design flow (m³/s): its fixture units converted by `table` where it has
    /// any, else the terminals' flows beyond it.
    pub(crate) fn design(&self, id: ElementId, table: Option<&FixtureTable>, flush_valves: bool) -> f64 {
        let units = self.units.get(&id).copied().unwrap_or(0.0);
        match table {
            Some(t) if units > 0.0 => t.flow(units, flush_valves) / 1000.0,
            _ => self.flow.get(&id).copied().unwrap_or(0.0),
        }
    }
}

/// The fluid of a system: density (kg/m³) and viscosity (Pa·s).
pub(crate) fn fluid_of(sys: &crate::mep_systems::SystemInfo, st: &crate::mep_systems::SystemType) -> Result<(f64, f64)> {
    if sys.classification.is_air() {
        return Ok(AIR);
    }
    match st.fluid.as_deref() {
        Some(f) if f.eq_ignore_ascii_case("water") => water(st.fluid_temperature.unwrap_or(20.0)),
        Some(f) => Err(bad("mep_size.fluid", format!("{}: the fluid {f:?} has no property table; only water is built in", sys.name))),
        None => water(20.0),
    }
}

/// The fixture-unit table a domestic water system converts with, if any.
pub(crate) fn table_of<'a>(sys: &crate::mep_systems::SystemInfo, st: &crate::mep_systems::SystemType, tables: &'a [FixtureTable]) -> Option<&'a FixtureTable> {
    let domestic = matches!(sys.classification, Classification::DomesticColdWater | Classification::DomesticHotWater);
    domestic.then(|| tables.iter().find(|t| t.id == st.flow_conversion).or(tables.first())).flatten()
}

pub(crate) fn flows(store: &Store, sys: &crate::mep_systems::SystemInfo) -> Result<Flows> {
    let rec = store.get(sys.id).and_then(crate::mep_systems::recipe).expect("a system");
    let root = rec
        .equipment
        .as_deref()
        .and_then(GlobalId::parse)
        .and_then(|g| store.find_by_guid(&g))
        .ok_or_else(|| bad("mep_size.no_equipment", format!("{} has no equipment: Select Equipment first, so flow has a source", sys.name)))?;
    let allowed: Vec<ElementId> = sys.elements.iter().copied().chain([root]).collect();
    let partners = mep::partners(store);
    // Breadth first from the equipment: each element's parent.
    let mut parent: HashMap<ElementId, ElementId> = HashMap::new();
    let mut order = vec![root];
    let mut queue = VecDeque::from([root]);
    let count = |id: ElementId| store.get(id).map_or(0, |e| mep::ports(store, e).len());
    while let Some(x) = queue.pop_front() {
        for k in 0..count(x) {
            for (o, _) in partners.get(&(x, k)).into_iter().flatten() {
                if *o != root && allowed.contains(o) && !parent.contains_key(o) {
                    parent.insert(*o, x);
                    order.push(*o);
                    queue.push_back(*o);
                }
            }
        }
    }
    let mut flow: HashMap<ElementId, f64> = HashMap::new();
    let mut units: HashMap<ElementId, f64> = HashMap::new();
    let mut terminal_of = HashMap::new();
    for (e, k) in &sys.ports {
        if *e == root || store.get(*e).is_some_and(mep_runs::is_run_element) {
            continue;
        }
        if let Some(p) = mep::port(store, *e, *k) {
            *flow.entry(*e).or_default() += p.connector.flow;
            *units.entry(*e).or_default() += p.connector.fixture_units;
        }
    }
    // From the leaves up.
    for x in order.iter().rev() {
        let (f, u) = (flow.get(x).copied().unwrap_or(0.0), units.get(x).copied().unwrap_or(0.0));
        if let Some(p) = parent.get(x) {
            *flow.entry(*p).or_default() += f;
            *units.entry(*p).or_default() += u;
            let terminal = |id: ElementId| id != root && store.get(id).is_some_and(|e| !mep_runs::is_run_element(e));
            if terminal(*x) {
                terminal_of.insert(*p, *x);
            }
        }
    }
    Ok(Flows { flow, units, terminal_of })
}

// ---- Sizing ----

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    /// Friction at or below the target (Pa/m).
    Friction,
    /// Velocity at or below the target (m/s).
    Velocity,
    /// Both targets, combined by `combine`.
    FrictionVelocity,
    /// Friction at or below the target, 1 Pa/m for ducts by default.
    #[default]
    EqualFriction,
    /// Not built yet.
    StaticRegain,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Combine {
    #[default]
    And,
    Or,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BranchRule {
    #[default]
    CalculatedOnly,
    MatchConnector,
    LargerOf,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Targets {
    /// Pa/m.
    pub friction: Option<f64>,
    /// m/s.
    pub velocity: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Restrict {
    /// Rectangular: at most this high (metres).
    pub height: Option<f64>,
    /// Rectangular: at most this wide (metres).
    pub width: Option<f64>,
    /// Round: at most this diameter (metres).
    pub inner_diameter: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SizeParams {
    /// Segments (or elements of the runs) to size; with `system_id`, every segment of it.
    #[serde(default)]
    pub element_ids: Vec<ElementRef>,
    pub system_id: Option<ElementRef>,
    #[serde(default)]
    pub method: Method,
    #[serde(default)]
    pub targets: Targets,
    #[serde(default)]
    pub combine: Combine,
    #[serde(default)]
    pub branch_rule: BranchRule,
    #[serde(default)]
    pub restrict: Restrict,
    #[serde(default)]
    pub equation: Equation,
    /// Domestic water: the flush-valve curve (else flush tank).
    #[serde(default)]
    pub flush_valves: bool,
}

/// One segment's result.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SizedSegment {
    pub segment: ElementId,
    /// m³/s.
    pub flow: f64,
    pub fixture_units: Option<f64>,
    pub size: String,
    /// m/s and Pa/m at the chosen size.
    pub velocity: f64,
    pub friction: f64,
    pub changed: bool,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct SizeReport {
    pub method: String,
    /// How domestic flow was found (the fixture-unit table, or summed flows).
    pub conversion: Option<String>,
    pub sized: Vec<SizedSegment>,
    /// Size-locked segments, left as they were.
    pub skipped: Vec<ElementId>,
    pub warnings: Vec<String>,
}

/// The sizes of a type's sizing table, smallest area first, that a restriction allows.
fn candidates(t: &mep_runs::SegmentType, shape: Shape, r: &Restrict) -> Vec<Size> {
    let sides = t.sizing_sizes(shape);
    let mut out: Vec<Size> = match shape {
        Shape::Round => sides
            .iter()
            .filter(|d| r.inner_diameter.is_none_or(|m| **d <= m + 1e-9))
            .map(|d| Size { shape, diameter: *d, width: 0.0, height: 0.0 })
            .collect(),
        _ => {
            let mut v = Vec::new();
            for w in &sides {
                for h in &sides {
                    let aspect = w.max(*h) / w.min(*h);
                    if aspect <= 4.0 + 1e-9 && r.height.is_none_or(|m| *h <= m + 1e-9) && r.width.is_none_or(|m| *w <= m + 1e-9) {
                        v.push(Size { shape, diameter: 0.0, width: *w, height: *h });
                    }
                }
            }
            v
        }
    };
    let area = |s: &Size| section(s).0;
    out.sort_by(|a, b| area(a).total_cmp(&area(b)).then((a.width - a.height).abs().total_cmp(&(b.width - b.height).abs())));
    out
}

/// `mep.size`.
pub(crate) fn size(txn: &mut Transaction<'_>, p: SizeParams) -> Result<SizeReport> {
    let store = txn.store().clone();
    if p.method == Method::StaticRegain {
        return Err(bad("mep_size.method", "Static Regain sizing is not built yet; use Equal Friction, Friction or Velocity"));
    }
    let ids: Vec<ElementId> = p.element_ids.iter().map(|r| store.resolve(r).ok_or_else(|| bad("element.not_found", "no such element"))).collect::<Result<_>>()?;
    let all = crate::mep_systems::systems(&store);
    let systems: Vec<&crate::mep_systems::SystemInfo> = match &p.system_id {
        Some(r) => {
            let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such system"))?;
            vec![all.iter().find(|s| s.id == id).ok_or_else(|| bad("system.not_found", format!("element {id} is not a system")))?]
        }
        None => all.iter().filter(|s| s.elements.iter().any(|e| ids.contains(e))).collect(),
    };
    if systems.is_empty() {
        return Err(bad("mep_size.no_system", "sizing needs a connected system: select its segments or the system"));
    }
    let mut report = SizeReport { method: format!("{:?}", p.method), ..SizeReport::default() };
    let tables = crate::mep_systems::settings(&store).fixture_unit_tables;
    for sys in systems {
        let st = crate::mep_systems::system_type(&store, &sys.system_type).unwrap_or_else(|| crate::mep_systems::builtin(sys.classification));
        if matches!(sys.classification, Classification::Sanitary | Classification::Vent) {
            return Err(bad("mep_size.sanitary", format!("{}: sanitary and vent pipes are not sized by flow", sys.name)));
        }
        let air = sys.classification.is_air();
        let fluid = fluid_of(sys, &st)?;
        let fl = flows(&store, sys)?;
        let domestic = matches!(sys.classification, Classification::DomesticColdWater | Classification::DomesticHotWater);
        let table = table_of(sys, &st, &tables);
        if domestic {
            report.conversion = Some(match table {
                Some(t) => format!("{} ({}), {} curve", t.name, t.source, if p.flush_valves { "flush valve" } else { "flush tank" }),
                None => "summed connector flows (no fixture-unit table is set)".into(),
            });
        }
        let (default_f, default_v) = if air { (1.0, 8.0) } else { (300.0, 1.5) };
        let tf = p.targets.friction.unwrap_or(default_f);
        let tv = p.targets.velocity.unwrap_or(default_v);
        let segs: Vec<ElementId> = sys
            .elements
            .iter()
            .copied()
            .filter(|e| ids.is_empty() || ids.contains(e) || p.system_id.is_some())
            .filter(|e| store.get(*e).and_then(mep_runs::segment_recipe).is_some_and(|s| s.corner.is_none()))
            .collect();
        for sid in segs {
            let s = store.get(sid).and_then(mep_runs::segment_recipe).expect("a segment");
            if s.size_locked {
                report.skipped.push(sid);
                continue;
            }
            let Some(t) = GlobalId::parse(&s.segment_type).and_then(|g| store.find_by_guid(&g)).and_then(|t| store.get(t)).and_then(mep_runs::segment_type) else {
                continue;
            };
            let units = fl.units.get(&sid).copied().unwrap_or(0.0);
            let q = fl.design(sid, table, p.flush_valves);
            if q <= 0.0 {
                report.warnings.push(format!("Segment {} carries no flow (its terminals have none); it is left as it is", sid.0));
                continue;
            }
            let ok = |z: &Size| {
                let (v, f) = flow_in(q, z, t.roughness, fluid, p.equation);
                let (fo, vo) = (f <= tf + 1e-9, v <= tv + 1e-9);
                match p.method {
                    Method::Friction | Method::EqualFriction => fo,
                    Method::Velocity => vo,
                    Method::FrictionVelocity => match p.combine {
                        Combine::And => fo && vo,
                        Combine::Or => fo || vo,
                    },
                    Method::StaticRegain => unreachable!(),
                }
            };
            let cands = candidates(&t, s.size.shape, &p.restrict);
            if cands.is_empty() {
                report.warnings.push(format!("Segment {}: no size of its type's table fits the restrictions", sid.0));
                continue;
            }
            let mut chosen = match cands.iter().find(|z| ok(z)) {
                Some(z) => *z,
                None => {
                    report.warnings.push(format!("Segment {}: no size within the restrictions meets the targets; the largest allowed is used", sid.0));
                    *cands.last().expect("some")
                }
            };
            // A branch to a terminal.
            if p.branch_rule != BranchRule::CalculatedOnly
                && let Some(term) = fl.terminal_of.get(&sid)
            {
                let conn = mep::ports(&store, store.get(*term).expect("terminal"))
                    .into_iter()
                    .find(|q| q.connector.domain == s.domain)
                    .map(|q| Size { shape: q.connector.shape, diameter: q.connector.diameter, width: q.connector.width, height: q.connector.height });
                if let Some(c) = conn.filter(|c| c.shape == chosen.shape)
                    && (p.branch_rule == BranchRule::MatchConnector || section(&c).0 > section(&chosen).0)
                {
                    chosen = c;
                }
            }
            let (v, f) = flow_in(q, &chosen, t.roughness, fluid, p.equation);
            let changed = chosen != s.size;
            if changed {
                apply(txn, sid, chosen)?;
            }
            report.sized.push(SizedSegment {
                segment: sid,
                flow: q,
                fixture_units: (units > 0.0).then_some(units),
                size: chosen.text(),
                velocity: v,
                friction: f,
                changed,
            });
        }
    }
    for w in &report.warnings {
        txn.warn(Warning { id: "mep.size".into(), message: w.clone(), element_ids: Vec::new() });
    }
    Ok(report)
}

/// Gives a segment a size, and the fitting legs joined to it.
fn apply(txn: &mut Transaction<'_>, id: ElementId, z: Size) -> Result<()> {
    let mut args = serde_json::json!({ "element_ids": [id.0] });
    match z.shape {
        Shape::Round => args["diameter"] = z.diameter.into(),
        _ => {
            args["width"] = z.width.into();
            args["height"] = z.height.into();
        }
    }
    let p: mep_runs::SetSizeParams = serde_json::from_value(args).expect("valid");
    mep_runs::set_size(txn, p)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LockParams {
    pub element_ids: Vec<ElementRef>,
    pub locked: bool,
}

/// `mep_run.lock_size`: size-locked segments keep their size when sized.
pub(crate) fn lock_size(txn: &mut Transaction<'_>, p: LockParams) -> Result<usize> {
    let store = txn.store().clone();
    let mut n = 0;
    for r in &p.element_ids {
        let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such segment"))?;
        let Some(mut s) = store.get(id).and_then(mep_runs::segment_recipe) else { continue };
        s.size_locked = p.locked;
        let v = serde_json::to_value(&s).expect("serialises");
        txn.modify(id, |e| {
            e.params.insert(crate::walls::RECIPE_KEY.into(), v);
        })?;
        n += 1;
    }
    Ok(n)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FixtureTableParams {
    pub name: String,
    /// The code and edition it comes from.
    #[serde(default)]
    pub source: String,
    /// CSV: `fixture_units, flow_tank, flow_valve`, ascending.
    pub csv: String,
    /// `l/s` (default) or `gpm`.
    #[serde(default)]
    pub unit: String,
    /// Domestic water system types to use it (their ids); none: it becomes the default (the
    /// first table is used where a type names none).
    #[serde(default)]
    pub system_types: Vec<String>,
}

/// `fixture_unit_method.set`: imports a fixture-unit table; returns its id.
pub(crate) fn set_fixture_table(txn: &mut Transaction<'_>, p: FixtureTableParams) -> Result<String> {
    let rows = parse_fixture_csv(&p.csv, p.unit.trim())?;
    let mut s = crate::mep_systems::settings(txn.store());
    let name = p.name.trim().to_owned();
    if name.is_empty() {
        return Err(bad("fixture_unit_table.name", "a table needs a name"));
    }
    let id = name.to_lowercase().replace(|c: char| !c.is_ascii_alphanumeric(), "_");
    s.fixture_unit_tables.retain(|t| t.id != id);
    let t = FixtureTable { id: id.clone(), name, source: p.source, rows };
    if p.system_types.is_empty() {
        s.fixture_unit_tables.insert(0, t);
    } else {
        s.fixture_unit_tables.push(t);
    }
    crate::mep_systems::save_settings(txn, &s)?;
    for st in p.system_types {
        crate::mep_systems::set_flow_conversion(txn, &st, &id)?;
    }
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EvaluateFixtureParams {
    /// The table's id; the default when omitted.
    pub table_id: Option<String>,
    pub fixture_units: f64,
    #[serde(default)]
    pub flush_valves: bool,
}

/// `fixture_unit_method.evaluate`: the flow (l/s) of a fixture-unit total.
pub(crate) fn evaluate_fixture(store: &Store, p: EvaluateFixtureParams) -> Result<f64> {
    let tables = crate::mep_systems::settings(store).fixture_unit_tables;
    let t = match &p.table_id {
        Some(id) => tables.iter().find(|t| &t.id == id),
        None => tables.first(),
    }
    .ok_or_else(|| bad("fixture_unit_table.none", "no fixture-unit table: import one with fixture_unit_method.set"))?;
    Ok(t.flow(p.fixture_units, p.flush_valves))
}

/// Segments' flows without changing them (for the System Inspector and reports).
pub fn segment_flows(store: &Store, system: ElementId) -> Result<BTreeMap<ElementId, f64>> {
    let all = crate::mep_systems::systems(store);
    let sys = all.iter().find(|s| s.id == system).ok_or_else(|| bad("system.not_found", "no such system"))?;
    let fl = flows(store, sys)?;
    Ok(sys.elements.iter().filter(|e| store.get(**e).and_then(mep_runs::segment_recipe).is_some()).map(|e| (*e, fl.flow.get(e).copied().unwrap_or(0.0))).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colebrook_and_haaland_agree_in_steel() {
        // Water at 20 °C in a 50 mm steel pipe (ε 0.045 mm) at 1.5 m/s.
        let (rho, mu) = water(20.0).unwrap();
        let s = Size { shape: Shape::Round, diameter: 0.05, width: 0.0, height: 0.0 };
        let q = 1.5 * std::f64::consts::PI * 0.05 * 0.05 / 4.0;
        let (_, fc) = flow_in(q, &s, 0.045e-3, (rho, mu), Equation::Colebrook);
        let (_, fh) = flow_in(q, &s, 0.045e-3, (rho, mu), Equation::Haaland);
        assert!(((fc - fh) / fc).abs() < 0.02, "{fc} {fh}");
        // About 500 Pa/m (f ≈ 0.022).
        assert!((400.0..600.0).contains(&fc), "{fc}");
    }

    #[test]
    fn water_properties_and_their_range() {
        let (rho, mu) = water(15.0).unwrap();
        assert!((rho - 998.95).abs() < 0.01 && (mu - 1.1545e-3).abs() < 1e-7);
        assert_eq!(water(120.0).unwrap_err().message, "Temperature outside fluid table range");
    }

    #[test]
    fn fixture_tables_parse_and_interpolate() {
        let t = FixtureTable { id: "t".into(), name: "t".into(), source: String::new(), rows: parse_fixture_csv("fixture_units,tank,valve\n10,1.0,2.0\n50,2.0,3.0\n", "l/s").unwrap() };
        assert!((t.flow(30.0, false) - 1.5).abs() < 1e-12);
        assert!((t.flow(30.0, true) - 2.5).abs() < 1e-12);
        let e = parse_fixture_csv("10,1,2\n8,1.2,2.2\n", "l/s").unwrap_err();
        assert!(e.message.starts_with("line 2:"), "{}", e.message);
        assert!((parse_fixture_csv("1,1,1\n2,2,2", "gpm").unwrap()[1][1] - 0.126_180_4).abs() < 1e-9);
    }

    #[test]
    fn restricted_height_widens_rectangles() {
        let t: mep_runs::SegmentType = serde_json::from_value(serde_json::json!({ "kind": "recipe.mep_segment_type", "domain": "duct", "shape": "rectangular", "roughness": 9.0e-5 })).unwrap();
        let ok = |z: &Size| flow_in(1.0, z, 9.0e-5, AIR, Equation::Colebrook).1 <= 1.0;
        let free = candidates(&t, Shape::Rectangular, &Restrict::default()).into_iter().find(|z| ok(z)).unwrap();
        let low = candidates(&t, Shape::Rectangular, &Restrict { height: Some(0.3), ..Restrict::default() }).into_iter().find(|z| ok(z)).unwrap();
        assert!(low.height <= 0.3 + 1e-9 && low.width > free.width, "{free:?} {low:?}");
        assert!(free.height > 0.3, "{free:?}");
        // Every candidate keeps an aspect ratio up to 4.
        assert!(candidates(&t, Shape::Rectangular, &Restrict::default()).iter().all(|z| z.width.max(z.height) / z.width.min(z.height) <= 4.0 + 1e-9));
    }
}
