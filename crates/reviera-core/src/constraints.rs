// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Constraints (spec Ch 3 §3.14.1–2, ARC-CNST-001/002): locked dimensions, EQ equality and
//! locked alignments between parallel references (walls' faces and location lines, grids,
//! reference planes), kept by a solver that runs inside every editing transaction.
//!
//! A constraint is a model element, an RXD record of class `rxd:model/constraints/<guid>` with its
//! data in `rxd:constraint`: its kind, its references (as dimensions hold them, [`DimRef`]), the
//! direction it measures across, the locked distance, and the dimension that made it.
//!
//! **Solving.** When a command changes elements, the constraints of each direction are gathered;
//! the elements the command touched stay where it put them, and every other referenced element
//! may move across the constraint's direction. The least change that satisfies the equations
//!
//! - distance: `(p_b + x_b) − (p_a + x_a) = d`,
//! - alignment: the same with `d = 0`,
//! - equality: consecutive gaps equal,
//!
//! is found (the minimum-norm solution of the linear system) and applied as moves. If no change
//! satisfies them the command fails with "Constraints are not satisfied", naming them. Elements
//! that cannot move this way (doors, imported elements) stay fixed.
//!
//! Not built yet: angular and radial constraints, constraints on hosted inserts' positions along
//! their walls, sketch locks, soft (unlocked) alignments.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::annotations::{self, DimRef, DimRefArg};
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;

pub const CLASS_PREFIX: &str = "rxd:model/constraints/";
pub const DATA_KEY: &str = "rxd:constraint";
/// How close an equation must hold (metres).
const TOL: f64 = 1e-6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Kind {
    Distance,
    Alignment,
    Equality,
}

/// A constraint's stored data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConstraintData {
    pub kind: Kind,
    /// Two references (distance, alignment) or more (equality), in order across.
    pub refs: Vec<DimRef>,
    /// The unit direction it measures across (plan).
    pub across: [f64; 2],
    /// A distance's value: the second reference's position minus the first's, across.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// The dimension (GlobalId) that made it, and its segment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimension: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segment: Option<usize>,
}

pub fn is_constraint(e: &Element) -> bool {
    e.class.starts_with(CLASS_PREFIX)
}

pub fn data(e: &Element) -> Option<ConstraintData> {
    if !is_constraint(e) {
        return None;
    }
    serde_json::from_value(e.param(DATA_KEY)?.clone()).ok()
}

/// Every constraint of a model.
pub fn all(store: &Store) -> Vec<(ElementId, ConstraintData)> {
    store
        .iter()
        .filter_map(|e| Some((e.id(), data(e)?)))
        .collect()
}

fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

/// A reference's position across `across` now (`None` when its element is gone).
fn position(store: &Store, r: &DimRef, across: [f64; 2]) -> Option<f64> {
    let (p, _) = annotations::resolve_ref(store, r)?;
    Some(dot(p, across))
}

/// Whether the elements of a constraint all still exist.
fn alive(store: &Store, c: &ConstraintData) -> bool {
    c.refs.iter().all(|r| {
        r.element.as_deref().is_none_or(|g| {
            GlobalId::parse(g)
                .and_then(|g| store.find_by_guid(&g))
                .is_some()
        })
    })
}

/// The constraint's equations as rows of `(coefficients by element GlobalId, right-hand side)`
/// in the change `x` of each element's position across: `Σ c·x = rhs`.
fn equations(store: &Store, c: &ConstraintData) -> Vec<(BTreeMap<String, f64>, f64)> {
    let pos: Vec<Option<f64>> = c
        .refs
        .iter()
        .map(|r| position(store, r, c.across))
        .collect();
    if pos.iter().any(Option::is_none) {
        return Vec::new();
    }
    let pos: Vec<f64> = pos.into_iter().flatten().collect();
    let var = |i: usize| c.refs[i].element.clone();
    let mut rows = Vec::new();
    let gap = |rows: &mut Vec<(BTreeMap<String, f64>, f64)>, a: usize, b: usize, target: f64| {
        // (p_b + x_b) − (p_a + x_a) = target.
        let mut co = BTreeMap::new();
        if let Some(v) = var(b) {
            *co.entry(v).or_insert(0.0) += 1.0;
        }
        if let Some(v) = var(a) {
            *co.entry(v).or_insert(0.0) -= 1.0;
        }
        rows.push((co, target - (pos[b] - pos[a])));
    };
    match c.kind {
        Kind::Distance => gap(&mut rows, 0, 1, c.value.unwrap_or(0.0)),
        Kind::Alignment => gap(&mut rows, 0, 1, 0.0),
        Kind::Equality => {
            // (gap_i) − (gap_0) = 0 for i ≥ 1.
            for i in 1..c.refs.len().saturating_sub(1) {
                let mut co = BTreeMap::new();
                for (k, s) in [(i + 1, 1.0), (i, -1.0), (1, -1.0), (0, 1.0)] {
                    if let Some(v) = var(k) {
                        *co.entry(v).or_insert(0.0) += s;
                    }
                }
                let rhs = -((pos[i + 1] - pos[i]) - (pos[1] - pos[0]));
                rows.push((co, rhs));
            }
        }
    }
    rows
}

/// Whether an element can be moved across by the solver.
fn movable(store: &Store, guid: &str) -> bool {
    GlobalId::parse(guid)
        .and_then(|g| store.find_by_guid(&g))
        .and_then(|id| store.get(id))
        .is_some_and(|e| {
            // Pinned elements stay put.
            crate::modify::check_unpinned(store, e.id()).is_ok()
                && (crate::walls::wall_recipe(e).is_some()
                    || crate::grids::is_grid(e)
                    || crate::ref_planes::recipe(e).is_some())
        })
}

/// The minimum-norm `x` with `A x = b` (A is `m × n`), or `None` when the equations conflict.
fn least_norm(a: &[Vec<f64>], b: &[f64], n: usize) -> Option<Vec<f64>> {
    let m = a.len();
    if m == 0 {
        return Some(vec![0.0; n]);
    }
    // (A Aᵀ + εI) y = b, x = Aᵀ y; then check the residual.
    let mut g = vec![vec![0.0; m + 1]; m];
    for i in 0..m {
        for j in 0..m {
            g[i][j] = (0..n).map(|k| a[i][k] * a[j][k]).sum::<f64>();
        }
        g[i][i] += 1e-12;
        g[i][m] = b[i];
    }
    // Gaussian elimination with partial pivoting.
    for col in 0..m {
        let piv = (col..m).max_by(|&p, &q| g[p][col].abs().total_cmp(&g[q][col].abs()))?;
        if g[piv][col].abs() < 1e-15 {
            continue;
        }
        g.swap(col, piv);
        for row in 0..m {
            if row != col {
                let f = g[row][col] / g[col][col];
                if f != 0.0 {
                    let pivot = g[col].clone();
                    for (v, p) in g[row].iter_mut().zip(&pivot).skip(col) {
                        *v -= f * p;
                    }
                }
            }
        }
    }
    let y: Vec<f64> = (0..m)
        .map(|i| {
            if g[i][i].abs() < 1e-15 {
                0.0
            } else {
                g[i][m] / g[i][i]
            }
        })
        .collect();
    let x: Vec<f64> = (0..n)
        .map(|k| (0..m).map(|i| a[i][k] * y[i]).sum())
        .collect();
    let ok = (0..m).all(|i| ((0..n).map(|k| a[i][k] * x[k]).sum::<f64>() - b[i]).abs() < 1e-5);
    ok.then_some(x)
}

/// Directions in parallel classes: a representative per class (sign ignored).
fn class_of(classes: &mut Vec<[f64; 2]>, d: [f64; 2]) -> usize {
    for (i, c) in classes.iter().enumerate() {
        if (c[0] * d[1] - c[1] * d[0]).abs() < 1e-3 {
            return i;
        }
    }
    classes.push(d);
    classes.len() - 1
}

/// Keeps the constraints after an edit: called inside every editing transaction before it
/// commits. `fixed` are the elements the edit moved (GlobalIds); the rest of the constrained
/// elements may move across.
pub(crate) fn enforce(txn: &mut Transaction<'_>) -> Result<()> {
    let store = txn.store().clone();
    let cons: Vec<(ElementId, ConstraintData)> = all(&store)
        .into_iter()
        .filter(|(_, c)| alive(&store, c))
        .collect();
    if cons.is_empty() {
        return Ok(());
    }
    let changes = txn.pending_changes();
    let mut fixed: BTreeSet<String> = changes
        .modified
        .iter()
        .chain(&changes.added)
        .filter_map(|id| store.get(*id))
        .filter(|e| !is_constraint(e))
        .map(|e| e.guid().to_string())
        .collect();
    // A constraint added or changed in this edit holds its references' outer ends still (EQ
    // moves the inner ones).
    for (id, c) in &cons {
        if changes.added.contains(id) || changes.modified.contains(id) {
            for r in [c.refs.first(), c.refs.last()].into_iter().flatten() {
                if let Some(g) = &r.element {
                    fixed.insert(g.clone());
                }
            }
        }
    }
    solve(txn, &cons, &fixed)
}

fn solve(
    txn: &mut Transaction<'_>,
    cons: &[(ElementId, ConstraintData)],
    fixed: &BTreeSet<String>,
) -> Result<()> {
    let store = txn.store().clone();
    let mut classes: Vec<[f64; 2]> = Vec::new();
    let mut by_class: BTreeMap<usize, Vec<&(ElementId, ConstraintData)>> = BTreeMap::new();
    for c in cons {
        let k = class_of(&mut classes, c.1.across);
        by_class.entry(k).or_default().push(c);
    }
    for (k, group) in by_class {
        let across = classes[k];
        let mut rows: Vec<(BTreeMap<String, f64>, f64, ElementId)> = Vec::new();
        for (id, c) in &group {
            // Equations in this class's direction (a constraint stored the other way round
            // negates its coefficients and right-hand side together, which is the same row).
            let s = dot(c.across, across).signum();
            for (co, rhs) in equations(&store, c) {
                rows.push((
                    co.into_iter().map(|(g, v)| (g, v * s)).collect(),
                    rhs * s,
                    *id,
                ));
            }
        }
        // Free variables: movable elements the edit did not touch.
        let vars: Vec<String> = rows
            .iter()
            .flat_map(|r| r.0.keys().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|g| !fixed.contains(g) && movable(&store, g))
            .collect();
        let index: BTreeMap<&str, usize> = vars
            .iter()
            .enumerate()
            .map(|(i, g)| (g.as_str(), i))
            .collect();
        // Least *weighted* change: an EQ moves its inner references, so its outer ones are
        // expensive to move.
        let outer: BTreeSet<&str> = group
            .iter()
            .filter(|(_, c)| c.kind == Kind::Equality)
            .flat_map(|(_, c)| [c.refs.first(), c.refs.last()])
            .flatten()
            .filter_map(|r| r.element.as_deref())
            .collect();
        // First with them held, then (if that has no solution) free.
        let held: Vec<f64> = vars
            .iter()
            .map(|g| if outer.contains(g.as_str()) { 0.0 } else { 1.0 })
            .collect();
        let free = vec![1.0; vars.len()];
        let matrix = |scale: &[f64]| -> Vec<Vec<f64>> {
            rows.iter()
                .map(|(co, _, _)| {
                    let mut row = vec![0.0; vars.len()];
                    for (g, v) in co {
                        if let Some(&i) = index.get(g.as_str()) {
                            row[i] += v * scale[i];
                        }
                    }
                    row
                })
                .collect()
        };
        let b: Vec<f64> = rows.iter().map(|r| r.1).collect();
        let solved = least_norm(&matrix(&held), &b, vars.len())
            .map(|x| (x, held.clone()))
            .or_else(|| least_norm(&matrix(&free), &b, vars.len()).map(|x| (x, free.clone())));
        let Some((x, scale)) = solved else {
            let ids: BTreeSet<u64> = rows.iter().map(|r| r.2.0).collect();
            return Err(Error::new(
                "constraint.unsatisfied",
                format!(
                    "Constraints are not satisfied: {} (remove them with `constraint.delete`)",
                    ids.iter()
                        .map(|i| format!("#{i}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )
            .with_details(serde_json::json!({ "constraint_ids": ids })));
        };
        for ((g, z), w) in vars.iter().zip(x).zip(&scale) {
            let dx = z * w;
            if dx.abs() < 1e-9 {
                continue;
            }
            let id = GlobalId::parse(g)
                .and_then(|g| store.find_by_guid(&g))
                .expect("movable");
            crate::modify::transform(
                txn,
                &[ElementRef::Id(id)],
                crate::modify::Xform::Translate([across[0] * dx, across[1] * dx, 0.0]),
                false,
            )?;
        }
    }
    // Every equation must hold now.
    let store = txn.store().clone();
    for (id, c) in cons {
        if equations(&store, c)
            .iter()
            .any(|(_, rhs)| rhs.abs() > TOL * 10.0)
        {
            return Err(Error::new(
                "constraint.unsatisfied",
                format!(
                    "Constraints are not satisfied: #{} (remove it with `constraint.delete`)",
                    id.0
                ),
            )
            .with_details(serde_json::json!({ "constraint_ids": [id.0] })));
        }
    }
    Ok(())
}

pub(crate) fn create(txn: &mut Transaction<'_>, c: &ConstraintData) -> Result<ElementId> {
    let guid = GlobalId::new();
    txn.create(
        NewElement::new(format!("{CLASS_PREFIX}{}", guid.as_str()))
            .guid(guid)
            .param(DATA_KEY, serde_json::to_value(c).expect("serialises")),
    )
}

fn dimension(
    store: &Store,
    r: &ElementRef,
) -> Result<(ElementId, String, annotations::DimensionData)> {
    store
        .resolve(r)
        .and_then(|id| {
            let e = store.get(id)?;
            Some((id, e.guid().to_string(), annotations::dimension_data(e)?))
        })
        .ok_or_else(|| Error::new("dim.not_found", format!("no dimension {r:?}")))
}

/// The constraints a dimension made (and its segment, if any).
pub fn of_dimension(store: &Store, guid: &str) -> Vec<(ElementId, ConstraintData)> {
    all(store)
        .into_iter()
        .filter(|(_, c)| c.dimension.as_deref() == Some(guid))
        .collect()
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LockParams {
    pub dimension_id: ElementRef,
    /// The segment (0 first); omitted, every segment.
    pub segment: Option<usize>,
}

/// `constraint.lock_dimension`: locks a dimension's segments at their values.
pub(crate) fn lock_dimension(txn: &mut Transaction<'_>, p: LockParams) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let (_, guid, d) = dimension(&store, &p.dimension_id)?;
    let m = annotations::measure(&store, &d)
        .ok_or_else(|| Error::new("dim.unmeasurable", "the dimension cannot be measured"))?;
    let segments: Vec<usize> = match p.segment {
        Some(s) if s < m.values.len() => vec![s],
        Some(s) => {
            return Err(Error::new(
                "dim.segment",
                format!(
                    "the dimension has {} segments; {s} is not one",
                    m.values.len()
                ),
            ));
        }
        None => (0..m.values.len()).collect(),
    };
    let existing = of_dimension(&store, &guid);
    let mut out = Vec::new();
    for s in segments {
        if existing
            .iter()
            .any(|(_, c)| c.kind == Kind::Distance && c.segment == Some(s))
        {
            continue;
        }
        let c = ConstraintData {
            kind: Kind::Distance,
            refs: vec![d.refs[s].clone(), d.refs[s + 1].clone()],
            across: m.across,
            value: Some(m.positions[s + 1] - m.positions[s]),
            dimension: Some(guid.clone()),
            segment: Some(s),
        };
        out.push(create(txn, &c)?);
    }
    Ok(out)
}

/// `constraint.unlock_dimension`: removes a dimension's locks (one segment's, or all).
pub(crate) fn unlock_dimension(txn: &mut Transaction<'_>, p: LockParams) -> Result<()> {
    let store = txn.store().clone();
    let (_, guid, _) = dimension(&store, &p.dimension_id)?;
    for (id, c) in of_dimension(&store, &guid) {
        if c.kind == Kind::Distance && (p.segment.is_none() || c.segment == p.segment) {
            txn.delete(id)?;
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EqualityParams {
    pub dimension_id: ElementRef,
    pub on: bool,
}

/// `constraint.set_equality`: makes a dimension's segments equal (EQ), moving its inner
/// references, or removes the equality.
pub(crate) fn set_equality(
    txn: &mut Transaction<'_>,
    p: EqualityParams,
) -> Result<Option<ElementId>> {
    let store = txn.store().clone();
    let (_, guid, d) = dimension(&store, &p.dimension_id)?;
    let existing: Vec<ElementId> = of_dimension(&store, &guid)
        .into_iter()
        .filter(|(_, c)| c.kind == Kind::Equality)
        .map(|(id, _)| id)
        .collect();
    if !p.on {
        for id in existing {
            txn.delete(id)?;
        }
        return Ok(None);
    }
    if let Some(id) = existing.first() {
        return Ok(Some(*id));
    }
    if d.refs.len() < 3 {
        return Err(Error::new(
            "dim.eq_needs_three",
            "EQ needs a dimension with at least two segments",
        ));
    }
    let m = annotations::measure(&store, &d)
        .ok_or_else(|| Error::new("dim.unmeasurable", "the dimension cannot be measured"))?;
    let c = ConstraintData {
        kind: Kind::Equality,
        refs: d.refs.clone(),
        across: m.across,
        value: None,
        dimension: Some(guid),
        segment: None,
    };
    create(txn, &c).map(Some)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AlignmentParams {
    pub ref_a: DimRefArg,
    pub ref_b: DimRefArg,
}

/// `constraint.lock_alignment`: locks two aligned parallel references together.
pub(crate) fn lock_alignment(txn: &mut Transaction<'_>, p: AlignmentParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let a = annotations::ref_of(&store, &p.ref_a)?;
    let b = annotations::ref_of(&store, &p.ref_b)?;
    if (a.direction[0] * b.direction[1] - a.direction[1] * b.direction[0]).abs() > 1e-3 {
        return Err(Error::new(
            "constraint.not_parallel",
            "aligned references must be parallel",
        ));
    }
    let across = [-a.direction[1], a.direction[0]];
    let (pa, pb) = (
        position(&store, &a, across).unwrap_or(0.0),
        position(&store, &b, across).unwrap_or(0.0),
    );
    if (pb - pa).abs() > 1e-4 {
        return Err(Error::new(
            "constraint.not_aligned",
            "the references are not aligned: align them first (Modify › Align)",
        ));
    }
    create(
        txn,
        &ConstraintData {
            kind: Kind::Alignment,
            refs: vec![a, b],
            across,
            value: None,
            dimension: None,
            segment: None,
        },
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteParams {
    pub constraint_ids: Vec<ElementRef>,
}

/// `constraint.delete`.
pub(crate) fn delete(txn: &mut Transaction<'_>, p: DeleteParams) -> Result<()> {
    let store = txn.store().clone();
    for r in &p.constraint_ids {
        let id = store
            .resolve(r)
            .filter(|id| store.get(*id).is_some_and(is_constraint))
            .ok_or_else(|| Error::new("constraint.not_found", format!("no constraint {r:?}")))?;
        txn.delete(id)?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    /// Only constraints on this element.
    pub element_id: Option<ElementRef>,
}

/// A constraint as listed.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ConstraintInfo {
    pub id: ElementId,
    #[serde(flatten)]
    pub data: ConstraintData,
    /// Whether it holds now.
    pub satisfied: bool,
    /// Whether all its references still exist.
    pub alive: bool,
}

/// `constraint.list`.
pub(crate) fn list(store: &Store, p: ListParams) -> Result<Vec<ConstraintInfo>> {
    let only = p
        .element_id
        .as_ref()
        .map(|r| {
            store
                .resolve(r)
                .and_then(|id| store.get(id))
                .map(|e| e.guid().to_string())
                .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))
        })
        .transpose()?;
    Ok(all(store)
        .into_iter()
        .filter(|(_, c)| {
            only.as_ref()
                .is_none_or(|g| c.refs.iter().any(|r| r.element.as_deref() == Some(g)))
        })
        .map(|(id, c)| ConstraintInfo {
            id,
            satisfied: equations(store, &c).iter().all(|(_, r)| r.abs() < 1e-5),
            alive: alive(store, &c),
            data: c,
        })
        .collect())
}

/// Parameter on a view: show every constraint (Reveal Constraints).
pub const REVEAL_KEY: &str = "rxd:reveal_constraints";

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RevealParams {
    pub view_id: ElementRef,
    pub on: bool,
}

/// `view.reveal_constraints`.
pub(crate) fn reveal(txn: &mut Transaction<'_>, p: RevealParams) -> Result<()> {
    let store = txn.store().clone();
    let id = store
        .resolve(&p.view_id)
        .filter(|id| store.get(*id).is_some_and(crate::views::is_view))
        .ok_or_else(|| Error::new("view.not_found", format!("no view {:?}", p.view_id)))?;
    txn.modify(id, |e| {
        if p.on {
            e.params.insert(REVEAL_KEY.into(), true.into());
        } else {
            e.params.remove(REVEAL_KEY);
        }
    })
}

/// Whether a view reveals its constraints.
pub fn revealed(e: &Element) -> bool {
    e.param(REVEAL_KEY).and_then(serde_json::Value::as_bool) == Some(true)
}

/// The two ends of each constraint's references now, to draw: `(id, kind, points)`.
pub fn drawn(store: &Store) -> Vec<(ElementId, Kind, Vec<[f64; 2]>)> {
    all(store)
        .into_iter()
        .filter(|(_, c)| alive(store, c))
        .map(|(id, c)| {
            let pts = c
                .refs
                .iter()
                .filter_map(|r| {
                    let (p, dir) = annotations::resolve_ref(store, r)?;
                    let t = dot([r.point[0] - p[0], r.point[1] - p[1]], dir);
                    Some([p[0] + dir[0] * t, p[1] + dir[1] * t])
                })
                .collect();
            (id, c.kind, pts)
        })
        .collect()
}

/// Whether a constraint refers to the element with GlobalId `guid`.
pub fn uses(c: &ConstraintData, guid: &str) -> bool {
    c.refs.iter().any(|r| r.element.as_deref() == Some(guid))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn least_norm_solves_and_rejects_conflicts() {
        // x0 − x1 = 1: the least change splits it.
        let x = least_norm(&[vec![1.0, -1.0]], &[1.0], 2).unwrap();
        assert!((x[0] - 0.5).abs() < 1e-9 && (x[1] + 0.5).abs() < 1e-9);
        // x0 = 1 and x0 = 2 conflict.
        assert!(least_norm(&[vec![1.0], vec![1.0]], &[1.0, 2.0], 1).is_none());
        // Redundant but consistent.
        assert!(least_norm(&[vec![1.0], vec![2.0]], &[1.0, 2.0], 1).is_some());
    }
}
