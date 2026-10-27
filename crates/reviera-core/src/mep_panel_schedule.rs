// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Panel schedules (MEP-ELEC-013): a live table of one panel's circuits.
//!
//! A panel schedule is a schedule (`rxd:schedules/<guid>`, [`crate::schedules`]) of kind
//! `schedule.panel` naming its panel by GlobalId. Its table, laid out one column (slot order):
//! - the header: panel, distribution system, supply, slots;
//! - one row per slot: the circuit starting there (number, load name, trip, poles, the load on
//!   each phase, total, classification), the further poles of a multi-pole circuit, or an empty
//!   slot; feeders on the feed-through lugs after the slots;
//! - the totals per phase, in VA and amperes;
//! - the loads summary by load classification: connected load, demand factor and estimated
//!   demand, sub-panels' loads rolling up by their own classifications (MEP-ELEC-006, 010).
//!
//! Load names and trip ratings are edited in their cells (`circuit.set`). Slots are phases A, B,
//! C in turn down each column (slots 1 and 2 are A, 3 and 4 B, 5 and 6 C).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Deserialize;

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::mep;
use crate::mep_electrical::{self as el, CircuitInfo, Phase};
use crate::mep_loads::{self, Load};
use crate::schedules::{self, Cell, Row, ScheduleDef, Table};
use crate::store::Store;
use crate::txn::Transaction;

pub const KIND_PANEL: &str = "schedule.panel";
/// Its column keys and headings.
pub const COLUMNS: [(&str, &str); 10] = [
    ("slot", "Slot"),
    ("circuit", "Circuit"),
    ("circuit.load_name", "Load Name"),
    ("classification", "Classification"),
    ("circuit.rating", "Trip (A)"),
    ("poles", "Poles"),
    ("load", "Load (VA)"),
    ("phase_a", "A (VA)"),
    ("phase_b", "B (VA)"),
    ("phase_c", "C (VA)"),
];

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

fn text(s: impl Into<String>) -> Cell {
    Cell { text: s.into(), value: None, editable: false }
}

fn va(v: f64) -> Cell {
    Cell { text: format!("{v:.0}"), value: Some(v), editable: false }
}

/// A phase load: blank when there is none.
fn phase_va(v: f64) -> Cell {
    if v.abs() < 0.5 { Cell { text: String::new(), value: Some(0.0), editable: false } } else { va(v) }
}

/// The first column of the phase loads.
const PHASE_COL: usize = 7;

fn row(elements: Vec<ElementId>, cells: Vec<Cell>) -> Row {
    let mut cells = cells;
    cells.resize_with(COLUMNS.len(), || text(""));
    Row { elements, cells }
}

/// The phases a panel's system has: three, two (single-phase three-wire) or one.
pub fn phases(ds: &el::DistributionSystem) -> usize {
    match ds.phase {
        Phase::Three => 3,
        Phase::Single if ds.wires >= 3 => 2,
        Phase::Single => 1,
    }
}

/// The phase (0 = A) of a slot on a panel of `n` phases.
pub fn phase_of_slot(slot: u32, n: usize) -> usize {
    (slot.saturating_add(1) / 2).saturating_sub(1) as usize % n.max(1)
}

/// A circuit's load on each phase (VA): unbalanced connectors with one load per pole put each on
/// its pole's phase, others are shared evenly between the circuit's poles (all the panel's
/// phases for a feeder on lugs).
pub fn phase_loads(store: &Store, c: &CircuitInfo, n: usize) -> [f64; 3] {
    let mut out = [0.0; 3];
    let slots: Vec<u32> = if c.slots.is_empty() { (0..n as u32).map(|k| 2 * k + 1).collect() } else { c.slots.clone() };
    let spread = |out: &mut [f64; 3], v: f64| {
        for s in &slots {
            out[phase_of_slot(*s, n)] += v / slots.len() as f64;
        }
    };
    if c.feeder {
        spread(&mut out, c.load);
        return out;
    }
    for (id, k) in &c.ports {
        let Some(e) = mep::port(store, *id, *k).and_then(|p| p.connector.electrical) else { continue };
        if e.load.len() == slots.len() && slots.len() > 1 {
            for (s, v) in slots.iter().zip(&e.load) {
                out[phase_of_slot(*s, n)] += v;
            }
        } else {
            spread(&mut out, e.load.iter().sum());
        }
    }
    out
}

/// Every load a panel carries, by classification id, through its sub-panels.
pub fn classified_loads(store: &Store, panel: ElementId, all: &[CircuitInfo]) -> BTreeMap<String, Vec<Load>> {
    let s = el::settings(store);
    let mut out: BTreeMap<String, Vec<Load>> = BTreeMap::new();
    let mut seen = vec![panel];
    let mut stack = vec![panel];
    while let Some(p) = stack.pop() {
        for c in all.iter().filter(|c| c.panel == Some(p)) {
            if c.feeder {
                if let Some(child) = c.elements.first().copied().filter(|ch| !seen.contains(ch)) {
                    seen.push(child);
                    stack.push(child);
                }
                continue;
            }
            for (id, k) in &c.ports {
                let Some(e) = mep::port(store, *id, *k).and_then(|p| p.connector.electrical) else { continue };
                let class = mep_loads::classify(&s, &e.load_classification).id.clone();
                out.entry(class).or_default().push(Load { va: e.load.iter().sum(), motor: e.motor });
            }
        }
    }
    out
}

/// One line of the loads summary.
#[derive(Debug, Clone, PartialEq, serde::Serialize, JsonSchema)]
pub struct SummaryLine {
    pub classification: String,
    pub connected: f64,
    pub demand_factor: String,
    pub demand: f64,
}

/// The loads summary of a panel, in the order of the classifications.
pub fn summary(store: &Store, panel: ElementId, all: &[CircuitInfo]) -> Vec<SummaryLine> {
    let s = el::settings(store);
    let loads = classified_loads(store, panel, all);
    s.load_classifications
        .iter()
        .filter_map(|c| {
            let l = loads.get(&c.id)?;
            let df = s.demand_factors.iter().find(|d| d.id == c.demand_factor);
            Some(SummaryLine {
                classification: c.name.clone(),
                connected: l.iter().map(|x| x.va).sum(),
                demand_factor: df.map_or_else(|| "100 %".into(), |d| d.name.clone()),
                demand: df.map_or_else(|| l.iter().map(|x| x.va).sum(), |d| mep_loads::evaluate(d, l)),
            })
        })
        .collect()
}

fn panel_of(store: &Store, def: &ScheduleDef) -> Result<ElementId> {
    def.panel
        .as_deref()
        .and_then(GlobalId::parse)
        .and_then(|g| store.find_by_guid(&g))
        .ok_or_else(|| bad("panel_schedule.no_panel", "the schedule's panel is no longer in the model"))
}

/// The table of a panel schedule.
pub fn table(store: &Store, schedule: &Element, def: &ScheduleDef) -> Result<Table> {
    let pid = panel_of(store, def)?;
    let pe = store.get(pid).expect("found");
    let pn = el::panel(pe);
    let s = el::settings(store);
    let ds = s.distribution_systems.iter().find(|d| d.id == pn.distribution_system);
    let n = ds.map_or(1, phases);
    let all = el::circuits(store);
    let on: Vec<&CircuitInfo> = all.iter().filter(|c| c.panel == Some(pid)).collect();
    let name = el::panel_name(pe);
    let mut rows = Vec::new();
    let supply = match el::supply_of(store, pid) {
        Some(f) => {
            let from = f.panel.and_then(|p| store.get(p)).map(el::panel_name).unwrap_or_default();
            if f.lugs { format!("{from}, feed-through lugs") } else { format!("{from}, circuit {}", f.name) }
        }
        None => "No supply".into(),
    };
    let used: u32 = on.iter().map(|c| c.slots.len() as u32).sum();
    for (k, v) in [
        ("Panel", name.clone()),
        ("Distribution system", ds.map_or_else(|| pn.distribution_system.clone(), |d| d.name.clone())),
        ("Supply", supply),
        ("Slots", format!("{} ({used} used)", pn.slots)),
    ] {
        rows.push(row(Vec::new(), vec![text(k), text(v)]));
    }
    rows.push(row(Vec::new(), Vec::new()));
    let class_of = |c: &CircuitInfo| -> String {
        let names: Vec<String> = c
            .ports
            .iter()
            .filter_map(|(id, k)| mep::port(store, *id, *k)?.connector.electrical.map(|e| mep_loads::classify(&s, &e.load_classification).name.clone()))
            .collect();
        match names.first() {
            _ if c.feeder => "Feeder".into(),
            Some(f) if names.iter().all(|x| x == f) => f.clone(),
            Some(_) => "Mixed".into(),
            None => String::new(),
        }
    };
    let mut totals = [0.0; 3];
    let circuit_row = |c: &CircuitInfo, slot: String, totals: &mut [f64; 3]| -> Row {
        let ph = phase_loads(store, c, n);
        for (t, v) in totals.iter_mut().zip(ph) {
            *t += v;
        }
        let phase = |k: usize| if k < n { phase_va(ph[k]) } else { text("") };
        row(
            vec![c.id],
            vec![
                text(slot),
                text(c.name.clone()),
                Cell { text: c.load_name.clone(), value: None, editable: true },
                text(class_of(c)),
                Cell { text: c.rating.map(|r| format!("{r}")).unwrap_or_default(), value: c.rating, editable: true },
                text(c.poles.to_string()),
                va(c.load),
                phase(0),
                phase(1),
                phase(2),
            ],
        )
    };
    for slot in 1..=pn.slots {
        if let Some(c) = on.iter().find(|c| c.slots.first() == Some(&slot)) {
            rows.push(circuit_row(c, slot.to_string(), &mut totals));
        } else if let Some(c) = on.iter().find(|c| c.slots.contains(&slot)) {
            // A further pole of a multi-pole circuit.
            rows.push(row(vec![c.id], vec![text(slot.to_string()), text(format!("↳ {}", c.name))]));
        } else {
            rows.push(row(Vec::new(), vec![text(slot.to_string())]));
        }
    }
    for c in on.iter().filter(|c| c.lugs) {
        rows.push(circuit_row(c, "Lugs".into(), &mut totals));
    }
    let phase_total = |k: usize| if k < n { va(totals[k]) } else { text("") };
    rows.push(row(Vec::new(), vec![text("Total"), text(""), text(""), text(""), text(""), text(""), va(totals.iter().sum()), phase_total(0), phase_total(1), phase_total(2)]));
    // Phase currents: line-to-ground volts (or line-to-line over √3 without a neutral).
    if let Some(v) = ds.and_then(|d| d.v_lg.or(d.v_ll.map(|v| v / 3f64.sqrt()))).filter(|v| *v > 0.0) {
        let amps = |k: usize| if k < n { Cell { text: format!("{:.1}", totals[k] / v), value: Some(totals[k] / v), editable: false } } else { text("") };
        rows.push(row(Vec::new(), vec![text("Current (A)"), text(""), text(""), text(""), text(""), text(""), text(""), amps(0), amps(1), amps(2)]));
    }
    rows.push(row(Vec::new(), Vec::new()));
    let lines = summary(store, pid, &all);
    // The summary: classification, demand factor, connected and demand (the Load and A columns).
    rows.push(row(Vec::new(), vec![text("Loads summary"), text(""), text("Classification"), text("Demand factor"), text(""), text(""), text("Connected (VA)"), text("Demand (VA)")]));
    for l in &lines {
        rows.push(row(Vec::new(), vec![text(""), text(""), text(l.classification.clone()), text(l.demand_factor.clone()), text(""), text(""), va(l.connected), va(l.demand)]));
    }
    let (conn, dem): (f64, f64) = lines.iter().fold((0.0, 0.0), |(a, b), l| (a + l.connected, b + l.demand));
    rows.push(row(Vec::new(), vec![text("Demand total"), text(""), text(""), text(""), text(""), text(""), va(conn), va(dem)]));
    Ok(Table {
        title: schedule.name.clone().unwrap_or_else(|| name.clone()),
        fields: COLUMNS.iter().map(|c| c.0.to_owned()).collect(),
        headings: headings(n),
        rows,
        totals: None,
    })
}

/// Headings for every column, those of phases the panel does not have left blank.
fn headings(n: usize) -> Vec<String> {
    COLUMNS
        .iter()
        .enumerate()
        .map(|(k, c)| if k >= PHASE_COL && k - PHASE_COL >= n { String::new() } else { c.1.to_owned() })
        .collect()
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub panel_ids: Vec<ElementRef>,
    /// A template (not yet used: the one-column layout).
    pub template_id: Option<String>,
}

/// `panel_schedule.create`: one schedule per panel; returns them.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<Vec<ElementId>> {
    if p.panel_ids.is_empty() {
        return Err(bad("panel_schedule.no_panels", "give the panels to schedule"));
    }
    let mut out = Vec::new();
    for r in &p.panel_ids {
        let store = txn.store().clone();
        let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such panel"))?;
        let e = store.get(id).expect("resolved");
        if !el::is_panel(&store, e) {
            return Err(bad("panel.not_a_panel", format!("element {id} is not a panel")));
        }
        let base = format!("{} Panel Schedule", el::panel_name(e));
        let taken: Vec<String> = store.iter().filter(|x| schedules::is_schedule(x)).filter_map(|x| x.name.clone()).collect();
        let name = std::iter::once(base.clone()).chain((2..).map(|n| format!("{base} {n}"))).find(|n| !taken.contains(n)).expect("a free name");
        let def = ScheduleDef {
            kind: KIND_PANEL.into(),
            category: "cat.electrical_equipment".into(),
            fields: Vec::new(),
            sort: Vec::new(),
            itemize: true,
            grand_totals: false,
            panel: Some(e.guid().to_string()),
            filters: Vec::new(),
            filter_by_sheet: false,
        };
        let guid = GlobalId::new();
        out.push(txn.create(
            NewElement::new(format!("{}{}", schedules::CLASS_PREFIX, guid.as_str()))
                .guid(guid)
                .name(name)
                .param(schedules::SCHEDULE_KEY, serde_json::to_value(&def).expect("serialises")),
        )?);
    }
    Ok(out)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChangePanelParams {
    pub view_id: ElementRef,
    pub panel_id: ElementRef,
}

/// `panel_schedule.change_panel`: the schedule shows another panel.
pub(crate) fn change_panel(txn: &mut Transaction<'_>, p: ChangePanelParams) -> Result<()> {
    let store = txn.store().clone();
    let sid = store.resolve(&p.view_id).ok_or_else(|| bad("schedule.not_found", "no such schedule"))?;
    let mut def = store
        .get(sid)
        .and_then(schedules::schedule_def)
        .filter(|d| d.kind == KIND_PANEL)
        .ok_or_else(|| bad("panel_schedule.not_found", format!("element {sid} is not a panel schedule")))?;
    let pid = store.resolve(&p.panel_id).ok_or_else(|| bad("element.not_found", "no such panel"))?;
    let pe = store.get(pid).expect("resolved");
    if !el::is_panel(&store, pe) {
        return Err(bad("panel.not_a_panel", format!("element {pid} is not a panel")));
    }
    def.panel = Some(pe.guid().to_string());
    txn.set_param(sid, schedules::SCHEDULE_KEY, serde_json::to_value(&def).expect("serialises"))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SummaryParams {
    pub panel_id: ElementRef,
}

/// `panel.loads_summary`: a panel's loads by classification, with demand.
pub(crate) fn summary_command(store: &Store, p: SummaryParams) -> Result<Vec<SummaryLine>> {
    let pid = store.resolve(&p.panel_id).ok_or_else(|| bad("element.not_found", "no such panel"))?;
    Ok(summary(store, pid, &el::circuits(store)))
}
