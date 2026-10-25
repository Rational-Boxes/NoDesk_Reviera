// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Circuit renumbering (MEP-ELEC-021): closes the gaps in a panel's circuit numbering.
//!
//! - The panel's circuits (branch circuits and feeders, not a feeder on feed-through lugs) move
//!   to the lowest slots from the start number, keeping their order; a multi-pole circuit keeps
//!   its poles (every second slot, as before).
//! - **Locked** circuits stay in their slots and the others flow round them.
//! - A circuit that would pass the panel's last slot refuses the whole renumbering, naming it.
//! - Names are made again by the panel's naming scheme; hand-typed load names are kept.
//! - On a panel of two or three phases, moving circuits changes their phases: the phase loads
//!   before and after are reported, with a warning when the largest phase imbalance grows by
//!   more than 10 percentage points (imbalance: the largest phase's excess over the average, as
//!   a percentage of the average). It does not rebalance.
//! - Nothing to move adds no undo step.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::mep_electrical::{self as el, slots_of};
use crate::mep_panel_schedule as ps;
use crate::store::Store;
use crate::txn::{Transaction, Warning};

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// The growth in phase imbalance that is warned about (percentage points).
pub const IMBALANCE_WARNING: f64 = 10.0;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenumberParams {
    pub panel_id: ElementRef,
    /// The first slot (default 1).
    pub start: Option<u32>,
}

/// One circuit before and after.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Move {
    pub circuit: ElementId,
    pub name_before: String,
    pub name_after: String,
    pub slots_before: Vec<u32>,
    pub slots_after: Vec<u32>,
    pub load_name: String,
    /// Phases (A, B, C) of its slots after.
    pub phases: String,
    pub locked: bool,
    pub changed: bool,
}

/// What renumbering does (or did).
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Renumbering {
    pub moves: Vec<Move>,
    /// The panel's phases (1 to 3); only that many phase loads count.
    pub phases: usize,
    /// VA on phases A, B, C.
    pub phase_before: [f64; 3],
    pub phase_after: [f64; 3],
    /// Per cent over the average.
    pub imbalance_before: f64,
    pub imbalance_after: f64,
    pub warning: Option<String>,
}

fn imbalance(loads: &[f64; 3], n: usize) -> f64 {
    let n = n.clamp(1, 3);
    let used = &loads[..n];
    let avg = used.iter().sum::<f64>() / n as f64;
    if avg <= 1e-9 {
        return 0.0;
    }
    (used.iter().copied().fold(f64::MIN, f64::max) - avg) / avg * 100.0
}

/// Works out the renumbering of a panel: the plan and each circuit's new start slot.
pub fn plan(store: &Store, p: &RenumberParams) -> Result<(Renumbering, Vec<(ElementId, u32)>)> {
    let pid = store.resolve(&p.panel_id).ok_or_else(|| bad("element.not_found", "no such panel"))?;
    let pe = store.get(pid).expect("resolved");
    if !el::is_panel(store, pe) {
        return Err(bad("panel.not_a_panel", format!("element {pid} is not a panel")));
    }
    let pn = el::panel(pe);
    let start = p.start.unwrap_or(1);
    if start == 0 || start > pn.slots {
        return Err(bad("circuit.renumber_start", format!("the start number runs from 1 to {}", pn.slots)));
    }
    let n = el::downstream(store, pe).map_or(1, |ds| ps::phases(&ds));
    let mut on: Vec<el::CircuitInfo> = el::circuits(store).into_iter().filter(|c| c.panel == Some(pid) && !c.slots.is_empty()).collect();
    on.sort_by_key(|c| (c.start_slot, c.id));
    let mut taken: Vec<u32> = on.iter().filter(|c| c.locked).flat_map(|c| c.slots.clone()).collect();
    let mut placed: Vec<(ElementId, u32)> = Vec::new();
    let mut cursor = start;
    for c in on.iter().filter(|c| !c.locked) {
        let fits = |s: u32| slots_of(s, c.poles).iter().all(|k| *k <= pn.slots && !taken.contains(k));
        let mut s = cursor;
        while s <= pn.slots && !fits(s) {
            s += 1;
        }
        if s > pn.slots {
            let what = if c.name.is_empty() { format!("circuit {}", c.id.0) } else { format!("circuit {}", c.name) };
            return Err(bad(
                "circuit.renumber_full",
                format!("{what} ({} pole) does not fit on {} from slot {start}: it has {} slots", c.poles, el::panel_name(pe), pn.slots),
            )
            .with_details(serde_json::json!({ "circuit": c.id })));
        }
        taken.extend(slots_of(s, c.poles));
        placed.push((c.id, s));
        cursor = s + 1;
    }
    let new_start = |c: &el::CircuitInfo| placed.iter().find(|x| x.0 == c.id).map_or(c.start_slot.unwrap_or(1), |x| x.1);
    let mut before = [0.0; 3];
    let mut after = [0.0; 3];
    let letters = ['A', 'B', 'C'];
    let mut moves = Vec::new();
    for c in &on {
        for (k, v) in ps::phase_loads(store, c, n).iter().enumerate() {
            before[k] += v;
        }
        let s = new_start(c);
        let mut moved = c.clone();
        moved.start_slot = Some(s);
        moved.slots = slots_of(s, c.poles);
        for (k, v) in ps::phase_loads(store, &moved, n).iter().enumerate() {
            after[k] += v;
        }
        moves.push(Move {
            circuit: c.id,
            name_before: c.name.clone(),
            name_after: el::circuit_name(store, pid, s, c.poles),
            slots_before: c.slots.clone(),
            slots_after: moved.slots.clone(),
            load_name: c.load_name.clone(),
            phases: moved.slots.iter().map(|s| letters[ps::phase_of_slot(*s, n)]).collect(),
            locked: c.locked,
            changed: moved.slots != c.slots,
        });
    }
    let (ib, ia) = (imbalance(&before, n), imbalance(&after, n));
    let warning = (n > 1 && ia - ib > IMBALANCE_WARNING).then(|| format!("the largest phase imbalance grows from {ib:.0} % to {ia:.0} %; Rebalance Loads may help"));
    let changes = placed.into_iter().filter(|(id, s)| on.iter().any(|c| c.id == *id && c.start_slot != Some(*s))).collect();
    Ok((Renumbering { moves, phases: n, phase_before: before, phase_after: after, imbalance_before: ib, imbalance_after: ia, warning }, changes))
}

/// `circuit.renumber_preview`.
pub(crate) fn preview(store: &Store, p: RenumberParams) -> Result<Renumbering> {
    plan(store, &p).map(|x| x.0)
}

/// `circuit.renumber`.
pub(crate) fn renumber(txn: &mut Transaction<'_>, p: RenumberParams) -> Result<Renumbering> {
    let store = txn.store().clone();
    let (out, changes) = plan(&store, &p)?;
    for (id, s) in &changes {
        let mut r = store.get(*id).and_then(el::recipe).expect("a circuit");
        r.start_slot = Some(*s);
        el::write(txn, *id, &r)?;
    }
    if !changes.is_empty() {
        el::rename_all(txn)?;
    }
    if let Some(w) = &out.warning {
        txn.warn(Warning { id: "circuit.phase_imbalance".into(), message: w.clone(), element_ids: changes.iter().map(|c| c.0).collect() });
    }
    Ok(out)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LockParams {
    pub circuit_ids: Vec<ElementRef>,
    pub locked: bool,
}

/// `circuit.set_locked`.
pub(crate) fn set_locked(txn: &mut Transaction<'_>, p: LockParams) -> Result<()> {
    if p.circuit_ids.is_empty() {
        return Err(bad("circuit.none", "no circuits given"));
    }
    for r in &p.circuit_ids {
        let store = txn.store().clone();
        let (id, mut c) = el::circuit(&store, r)?;
        if c.locked != p.locked {
            c.locked = p.locked;
            el::write(txn, id, &c)?;
        }
    }
    Ok(())
}
