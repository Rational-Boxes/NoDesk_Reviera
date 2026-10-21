// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Sloped piping (MEP-PIPE-002): the slope of a connected section of pipe set from a control
//! point.
//!
//! - The section is the pipe segments and fittings given, one connected chain (tees in it may
//!   branch out of it). The **control point** is one of its two open ends (the lowest by default);
//!   it stays where it is and the pipe rises by the slope away from it.
//! - Each segment's far end is set to rise `slope` times its length in plan; fittings follow,
//!   their run legs tilted to the slope so the fall carries through them; vertical segments
//!   (steeper than 45°) keep their geometry and move with their neighbours. Taps ride on their
//!   segments.
//! - Elements joined to the section from outside (branches off its tees, the run beyond its far
//!   end): a segment has its joined end moved with the port and its own slope recalculated; any
//!   other element (a fitting, equipment, a terminal) cannot follow, and the change is refused
//!   naming it.
//! - Allowed slopes are a project setting (Mechanical Settings › Slopes); a slope not in the list
//!   is applied with a warning, so imported models stay valid (the Slope Editor offers only the
//!   list).
//!
//! Each segment's slope is written to `Pset_PipeSegmentOccurrence.Gradient`.

use std::collections::{HashMap, HashSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::mep::{self, Domain};
use crate::mep_runs::{self, SegmentRecipe};
use crate::store::Store;
use crate::txn::{Transaction, Warning};
use crate::walls::RECIPE_KEY;

type V3 = [f64; 3];

/// The slope gradient's property.
pub const GRADIENT: &str = "Pset_PipeSegmentOccurrence.Gradient";

/// Reviera's provisional allowed slopes (rise over run): 0.5 %, 1 %, 1/8 in per ft, 2 %, 1/4 in
/// per ft, 4 % (EXPERT_REVIEW E-25).
pub const DEFAULT_SLOPES: [f64; 6] = [0.005, 0.01, 0.0104, 0.02, 0.0208, 0.04];

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn mul(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn len(a: V3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

fn plan(a: V3) -> f64 {
    a[0].hypot(a[1])
}

/// The project's allowed slopes.
pub fn allowed(store: &Store) -> Vec<f64> {
    let s = crate::mep_systems::settings(store).slopes;
    if s.is_empty() { DEFAULT_SLOPES.to_vec() } else { s }
}

/// Writes a segment's recipe and its Gradient.
fn write_segment(txn: &mut Transaction<'_>, id: ElementId, s: &SegmentRecipe) -> Result<()> {
    let v = serde_json::to_value(s).expect("serialises");
    let pipe = s.domain == Domain::Pipe;
    let slope = s.slope;
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
        if pipe && slope != 0.0 {
            e.params.insert(GRADIENT.into(), serde_json::json!({ "type": "IfcPositiveRatioMeasure", "value": slope.abs() }));
        } else {
            e.params.remove(GRADIENT);
        }
    })
}

/// A segment's slope from its ends: rise over plan length (none when vertical).
fn slope_of(s: &SegmentRecipe) -> f64 {
    let d = sub(s.end, s.start);
    let h = plan(d);
    if h < 1e-9 { 0.0 } else { d[2] / h }
}

/// Moves a segment's taps onto its line after its ends moved.
fn retap(s: &mut SegmentRecipe, before: &SegmentRecipe) {
    let (a0, d0) = (before.start, sub(before.end, before.start));
    let l0 = len(d0);
    for t in &mut s.taps {
        let u = if l0 > 0.0 { (sub(t.at, a0)[0] * d0[0] + sub(t.at, a0)[1] * d0[1] + sub(t.at, a0)[2] * d0[2]) / (l0 * l0) } else { 0.0 };
        let on_old = add(a0, mul(d0, u));
        let on_new = add(s.start, mul(sub(s.end, s.start), u));
        t.at = add(t.at, sub(on_new, on_old));
    }
}

/// A point on the section: an element and one of its ports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct EndRef {
    pub element_id: ElementId,
    /// The port: a segment's 0 (start) or 1 (end), a fitting's leg.
    pub port: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SlopeParams {
    /// The section's pipe segments and fittings.
    pub element_ids: Vec<ElementRef>,
    /// Rise over run (0.02 for 2 %); negative turns the fall the other way.
    pub slope: f64,
    /// The end that stays: one of the section's two open ends; the lowest when omitted.
    pub control_point: Option<EndRef>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct SlopeReport {
    pub segments: usize,
    /// The far end's rise (m).
    pub rise: f64,
    /// The control point used.
    pub control_point: Option<EndRef>,
    /// Segments outside the section whose joined end moved, with their new slope.
    pub branches: Vec<ElementId>,
    pub warnings: Vec<String>,
}

fn port_pos(store: &Store, id: ElementId, k: usize) -> Option<V3> {
    mep::port(store, id, k).map(|p| p.position)
}

/// `mep_run.set_slope`.
pub(crate) fn set_slope(txn: &mut Transaction<'_>, p: SlopeParams) -> Result<SlopeReport> {
    let store = txn.store().clone();
    if !p.slope.is_finite() || p.slope.abs() > 1.0 {
        return Err(bad("slope.value", "a slope is rise over run, at most 1 (45°)"));
    }
    let ids: Vec<ElementId> = p.element_ids.iter().map(|r| store.resolve(r).ok_or_else(|| bad("element.not_found", "no such element"))).collect::<Result<_>>()?;
    if ids.is_empty() {
        return Err(bad("slope.empty", "select the section's pipes and fittings"));
    }
    for id in &ids {
        let e = store.get(*id).expect("resolved");
        let pipe = mep_runs::segment_recipe(e).is_some_and(|s| s.domain == Domain::Pipe) || mep_runs::fitting_recipe(e).is_some_and(|f| f.domain == Domain::Pipe);
        if !pipe {
            return Err(bad("slope.not_pipe", format!("element {id} is not a pipe or pipe fitting")));
        }
    }
    let set: HashSet<ElementId> = ids.iter().copied().collect();
    let partners = mep::partners(&store);
    let count = |id: ElementId| store.get(id).map_or(0, mep_runs::port_count);
    // Links inside the section, by element and port.
    let inside = |id: ElementId, k: usize| partners.get(&(id, k)).into_iter().flatten().find(|(o, _)| set.contains(o)).copied();
    let not_one = || bad("slope.section", "Selection is not a single connected section");
    // One chain: connected, each element joined to at most two others in it.
    let mut seen = HashSet::from([ids[0]]);
    let mut stack = vec![ids[0]];
    while let Some(x) = stack.pop() {
        let links: Vec<ElementId> = (0..count(x)).filter_map(|k| inside(x, k)).map(|l| l.0).collect();
        if links.len() > 2 {
            return Err(not_one());
        }
        for o in links {
            if seen.insert(o) {
                stack.push(o);
            }
        }
    }
    if seen.len() != set.len() {
        return Err(not_one());
    }
    // Its two open ends: ports along the run not joined inside (for a one-element section, both
    // of a segment's ends). A tee's branch leg is not an end.
    let run_ports = |id: ElementId| -> Vec<usize> {
        let e = store.get(id).expect("member");
        if mep_runs::segment_recipe(e).is_some() {
            return vec![0, 1];
        }
        let n = count(id);
        let linked: Vec<usize> = (0..n).filter(|k| inside(id, *k).is_some()).collect();
        if linked.len() == 2 {
            return linked;
        }
        // An end fitting: its joined leg and the leg most opposite it.
        let ports = mep::ports(&store, e);
        let Some(&j) = linked.first() else { return (0..n.min(2)).collect() };
        let dj = ports[j].direction;
        let far = (0..n).filter(|k| *k != j).min_by(|a, b| {
            let d = |k: usize| ports[k].direction[0] * dj[0] + ports[k].direction[1] * dj[1] + ports[k].direction[2] * dj[2];
            d(*a).total_cmp(&d(*b))
        });
        let mut v = vec![j];
        v.extend(far);
        v
    };
    let ends: Vec<EndRef> = ids
        .iter()
        .flat_map(|id| run_ports(*id).into_iter().filter(move |k| inside(*id, *k).is_none()).map(move |k| EndRef { element_id: *id, port: k }))
        .collect();
    if ends.len() != 2 {
        return Err(not_one());
    }
    let control = match p.control_point {
        Some(c) => {
            if !ends.contains(&c) {
                return Err(bad("slope.control", "the control point is one of the section's two open ends"));
            }
            c
        }
        None => {
            let z = |e: &EndRef| port_pos(&store, e.element_id, e.port).map_or(0.0, |v| v[2]);
            if z(&ends[0]) <= z(&ends[1]) { ends[0] } else { ends[1] }
        }
    };
    let slope = p.slope;
    let mut report = SlopeReport { control_point: Some(control), ..SlopeReport::default() };
    if !allowed(&store).iter().any(|a| (a - slope.abs()).abs() < 1e-9) && slope != 0.0 {
        report.warnings.push(format!("{:.2} % is not one of the allowed slopes", slope.abs() * 100.0));
    }
    // Old and new port positions, to move what is joined from outside.
    let mut moved: HashMap<(ElementId, usize), (V3, V3)> = HashMap::new();
    let start_at = port_pos(&store, control.element_id, control.port).ok_or_else(not_one)?;
    let (mut at, mut entered, mut point) = (control.element_id, control.port, start_at);
    let mut visited = HashSet::new();
    loop {
        visited.insert(at);
        let e = store.get(at).expect("member");
        let before_ports: Vec<V3> = mep::ports(&store, e).iter().map(|q| q.position).collect();
        let exit;
        let exit_point;
        if let Some(mut s) = mep_runs::segment_recipe(e) {
            let before = s.clone();
            exit = 1 - entered.min(1);
            let (near, far) = if entered == 0 { (s.start, s.end) } else { (s.end, s.start) };
            let d = sub(far, near);
            let steep = d[2].abs() > plan(d);
            let far_new = if steep {
                // Vertical: moved, not re-sloped.
                add(far, sub(point, near))
            } else {
                [far[0], far[1], point[2] + slope * plan(sub(far, point))]
            };
            if entered == 0 {
                s.start = point;
                s.end = far_new;
            } else {
                s.end = point;
                s.start = far_new;
            }
            if !steep {
                s.slope = slope_of(&s);
                report.segments += 1;
            }
            retap(&mut s, &before);
            write_segment(txn, at, &s)?;
            exit_point = far_new;
        } else if let Some(mut f) = mep_runs::fitting_recipe(e) {
            let rp = run_ports(at);
            exit = rp.iter().copied().find(|k| *k != entered).unwrap_or(entered);
            let tilt = |d: V3, rise: f64| -> V3 {
                let h = plan(d);
                if h < 1e-9 {
                    return d;
                }
                let v = [d[0] / h, d[1] / h, rise];
                mul(v, 1.0 / len(v))
            };
            // Back toward the control point the leg falls; on along the run it rises.
            if let Some(l) = f.legs.get_mut(entered) {
                l.direction = tilt(l.direction, -slope);
            }
            if exit != entered
                && let Some(l) = f.legs.get_mut(exit)
            {
                l.direction = tilt(l.direction, slope);
            }
            let leg = f.legs[entered];
            f.centre = sub(point, mul(leg.direction, leg.length));
            let out = f.legs[exit];
            exit_point = add(f.centre, mul(out.direction, out.length));
            let v = serde_json::to_value(&f).expect("serialises");
            txn.modify(at, |x| {
                x.params.insert(RECIPE_KEY.into(), v);
            })?;
        } else {
            return Err(not_one());
        }
        let now = txn.store().clone();
        let after: Vec<V3> = now.get(at).map(|x| mep::ports(&now, x).iter().map(|q| q.position).collect()).unwrap_or_default();
        for (k, (b, a)) in before_ports.iter().zip(&after).enumerate() {
            moved.insert((at, k), (*b, *a));
        }
        match inside(at, exit).filter(|(o, _)| !visited.contains(o)) {
            Some((next, port)) => {
                at = next;
                entered = port;
                point = exit_point;
            }
            None => {
                report.rise = exit_point[2] - start_at[2];
                break;
            }
        }
    }
    // What is joined from outside follows its port, or the change is refused.
    let mut blocked = Vec::new();
    for ((id, k), (b, a)) in &moved {
        if len(sub(*a, *b)) < 1e-9 || (*id == control.element_id && *k == control.port) {
            continue;
        }
        for (o, ko) in partners.get(&(*id, *k)).into_iter().flatten() {
            if set.contains(o) {
                continue;
            }
            let now = txn.store().clone();
            match now.get(*o).and_then(mep_runs::segment_recipe) {
                Some(mut s) => {
                    let before = s.clone();
                    match ko {
                        0 => s.start = *a,
                        1 => s.end = *a,
                        _ => {
                            blocked.push(*o);
                            continue;
                        }
                    }
                    let d = sub(s.end, s.start);
                    if d[2].abs() <= plan(d) {
                        s.slope = slope_of(&s);
                    }
                    retap(&mut s, &before);
                    write_segment(txn, *o, &s)?;
                    report.branches.push(*o);
                }
                // A fitting (a tap or a tee off the section) moves with the port; the segments
                // beyond it follow at their joined ends.
                None if now.get(*o).and_then(mep_runs::fitting_recipe).is_some() => {
                    let delta = sub(*a, *b);
                    mep_runs::transform(txn, *o, &|q| [q[0] + delta[0], q[1] + delta[1]], delta[2])?;
                    report.branches.push(*o);
                    let n = now.get(*o).map_or(0, mep_runs::port_count);
                    for j in (0..n).filter(|j| j != ko) {
                        for (x, kx) in partners.get(&(*o, j)).into_iter().flatten() {
                            if set.contains(x) || report.branches.contains(x) {
                                continue;
                            }
                            let cur = txn.store().clone();
                            match (cur.get(*x).and_then(mep_runs::segment_recipe), kx) {
                                (Some(mut s), 0 | 1) => {
                                    let before = s.clone();
                                    if *kx == 0 {
                                        s.start = add(s.start, delta);
                                    } else {
                                        s.end = add(s.end, delta);
                                    }
                                    let d = sub(s.end, s.start);
                                    if d[2].abs() <= plan(d) {
                                        s.slope = slope_of(&s);
                                    }
                                    retap(&mut s, &before);
                                    write_segment(txn, *x, &s)?;
                                    report.branches.push(*x);
                                }
                                _ => blocked.push(*x),
                            }
                        }
                    }
                }
                None => blocked.push(*o),
            }
        }
    }
    if !blocked.is_empty() {
        blocked.sort();
        blocked.dedup();
        let names: Vec<String> = blocked.iter().map(|b| format!("{}", b.0)).collect();
        return Err(bad(
            "slope.branch",
            format!("A branch cannot be kept connected: element(s) {} joined to the section cannot move with it; leave them out of the section or disconnect them", names.join(", ")),
        ));
    }
    report.branches.sort();
    report.branches.dedup();
    for w in &report.warnings {
        txn.warn(Warning { id: "slope.not_allowed".into(), message: w.clone(), element_ids: Vec::new() });
    }
    Ok(report)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SlopesParams {
    /// Rise over run, each above 0 and at most 1.
    pub slopes: Vec<f64>,
}

/// `mep_settings.set_slopes`.
pub(crate) fn set_slopes(txn: &mut Transaction<'_>, p: SlopesParams) -> Result<()> {
    if p.slopes.iter().any(|s| !(s.is_finite() && *s > 0.0 && *s <= 1.0)) {
        return Err(bad("slope.value", "each slope is rise over run, above 0 and at most 1"));
    }
    let mut s = crate::mep_systems::settings(txn.store());
    let mut v = p.slopes;
    v.sort_by(f64::total_cmp);
    v.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    s.slopes = v;
    crate::mep_systems::save_settings(txn, &s)
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListSlopesParams {}

/// `mep_settings.slopes`.
pub(crate) fn list_slopes(store: &Store, _: ListSlopesParams) -> Result<Vec<f64>> {
    Ok(allowed(store))
}
