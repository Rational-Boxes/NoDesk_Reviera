// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Editing sketch curves in place (ARC-SKCH-001), for the window's sketch Modify and footprint
//! grips, and for scripts and agents (`sketch.move_curve`, `sketch.move_corner`):
//! - moving one curve across itself, its neighbours stretched or shortened to meet it;
//! - moving a corner, with every curve ending there.
//!
//! The functions work on lines and report which lines left their direction (those leave the
//! wall they were locked to); the commands apply them to a roof's, floor's or ceiling's current
//! footprint and save it as one undoable edit.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::walls::Line;

/// Points closer than this (metres) are the same corner.
const SAME: f64 = 1e-6;

fn close(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) < SAME
}

fn unit(l: Line) -> [f64; 2] {
    let d = [l.end[0] - l.start[0], l.end[1] - l.start[1]];
    let len = d[0].hypot(d[1]).max(1e-12);
    [d[0] / len, d[1] / len]
}

/// Where the infinite lines through `a` and `b` cross; `None` when they are parallel.
fn crossing(a: Line, b: Line) -> Option<[f64; 2]> {
    let (u, v) = (unit(a), unit(b));
    let den = u[0] * v[1] - u[1] * v[0];
    if den.abs() < 1e-9 {
        return None;
    }
    let w = [b.start[0] - a.start[0], b.start[1] - a.start[1]];
    let t = (w[0] * v[1] - w[1] * v[0]) / den;
    Some([a.start[0] + u[0] * t, a.start[1] + u[1] * t])
}

/// Moves line `i` by the part of `v` across it. Each line meeting it at a corner keeps its
/// direction and is stretched or shortened to meet it again. Returns the lines that left their
/// own line (only `i`, unless it did not move).
pub fn move_curve(lines: &mut [Line], i: usize, v: [f64; 2]) -> Vec<usize> {
    let old = lines[i];
    let u = unit(old);
    let n = [-u[1], u[0]];
    let d = v[0] * n[0] + v[1] * n[1];
    if d.abs() < 1e-12 {
        return Vec::new();
    }
    let shift = |p: [f64; 2]| [p[0] + n[0] * d, p[1] + n[1] * d];
    let line = Line {
        start: shift(old.start),
        end: shift(old.end),
    };
    let mut new = line;
    for (j, c) in lines.iter_mut().enumerate() {
        if j == i {
            continue;
        }
        for (k, corner) in [old.start, old.end].into_iter().enumerate() {
            let moved = if k == 0 { line.start } else { line.end };
            let at = crossing(*c, line).unwrap_or(moved);
            let mut met = false;
            if close(c.start, corner) {
                c.start = at;
                met = true;
            }
            if close(c.end, corner) {
                c.end = at;
                met = true;
            }
            // The moved line ends where its neighbour now meets it.
            if met {
                if k == 0 {
                    new.start = at;
                } else {
                    new.end = at;
                }
            }
        }
    }
    lines[i] = new;
    vec![i]
}

/// Moves the corner at `from` to `to`: every line ending there follows. Returns those lines.
pub fn move_corner(lines: &mut [Line], from: [f64; 2], to: [f64; 2]) -> Vec<usize> {
    let mut moved = Vec::new();
    for (i, c) in lines.iter_mut().enumerate() {
        let mut hit = false;
        if close(c.start, from) {
            c.start = to;
            hit = true;
        }
        if close(c.end, from) {
            c.end = to;
            hit = true;
        }
        if hit {
            moved.push(i);
        }
    }
    moved
}

/// Changes line `i`'s length, keeping its start; the corner at its end moves along it.
pub fn set_length(lines: &mut [Line], i: usize, length: f64) -> Vec<usize> {
    let l = lines[i];
    let u = unit(l);
    let to = [l.start[0] + u[0] * length, l.start[1] + u[1] * length];
    move_corner(lines, l.end, to)
}

/// A roof's, floor's or ceiling's footprint now: each curve's key, current line (the corners
/// where its loop's current lines meet) and lock.
pub fn current_curves(
    store: &Store,
    e: &Element,
) -> Option<Vec<(u32, Line, Option<crate::sketch::Lock>)>> {
    let sketch = crate::sketch::sketch_of(e)?;
    let mut out = Vec::new();
    for (corners, keys) in crate::sketch::rings(store, &sketch) {
        for (i, k) in keys.iter().enumerate() {
            let c = sketch.curves.iter().find(|c| c.key == *k)?;
            out.push((
                *k,
                Line {
                    start: corners[i],
                    end: corners[(i + 1) % corners.len()],
                },
                c.lock.clone(),
            ));
        }
    }
    Some(out)
}

/// What to move.
enum Edit {
    Curve { key: u32, vector: [f64; 2] },
    Corner { from: [f64; 2], to: [f64; 2] },
}

/// Applies an edit to an element's footprint and saves it through the element's own edit
/// command (one undoable step).
fn apply(txn: &mut crate::txn::Transaction<'_>, r: &ElementRef, edit: Edit) -> Result<()> {
    let store = txn.store().clone();
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such element"))?;
    let e = store.get(id).expect("resolved");
    let roof = crate::roofs::roof_recipe(e);
    let slab = crate::floors::slab_recipe(e);
    if roof.is_none() && slab.is_none() {
        return Err(Error::new(
            "sketch.not_sketched",
            format!("element {id} is not a roof, floor or ceiling with a footprint"),
        ));
    }
    let curves = current_curves(&store, e)
        .ok_or_else(|| Error::new("sketch.not_sketched", "the element has no footprint"))?;
    let mut lines: Vec<Line> = curves.iter().map(|c| c.1).collect();
    let moved = match edit {
        Edit::Curve { key, vector } => {
            let i = curves.iter().position(|c| c.0 == key).ok_or_else(|| {
                Error::new(
                    "sketch.no_such_curve",
                    format!("the footprint has no curve {key}"),
                )
            })?;
            move_curve(&mut lines, i, vector)
        }
        Edit::Corner { from, to } => {
            let moved = move_corner(&mut lines, from, to);
            if moved.is_empty() {
                return Err(Error::new(
                    "sketch.no_such_corner",
                    format!("the footprint has no corner at ({}, {})", from[0], from[1]),
                ));
            }
            moved
        }
    };
    let lock = |k: usize| -> Option<crate::sketch::LockArg> {
        if moved.contains(&k) {
            return None;
        }
        let l = curves[k].2.as_ref()?;
        Some(crate::sketch::LockArg {
            element: ElementRef::Guid(GlobalId::parse(&l.element)?),
            sub: l.sub.clone(),
        })
    };
    match roof {
        Some(recipe) => {
            let args = (0..lines.len())
                .map(|k| {
                    let edge = recipe.edge(curves[k].0);
                    crate::roofs::RoofCurveArg {
                        line: lines[k],
                        lock: lock(k),
                        defines_slope: Some(edge.defines_slope),
                        slope: edge.slope.map(serde_json::Value::from),
                        overhang: Some(edge.overhang.into()),
                        eave: edge.eave,
                    }
                })
                .collect();
            crate::roofs::edit_footprint(
                txn,
                crate::roofs::EditFootprintParams {
                    roof_id: ElementRef::Id(id),
                    curves: args,
                },
            )
        }
        None => {
            let args = (0..lines.len())
                .map(|k| crate::sketch::CurveArg {
                    line: lines[k],
                    lock: lock(k),
                })
                .collect();
            crate::floors::edit_boundary(
                txn,
                crate::floors::EditBoundaryParams {
                    floor_id: ElementRef::Id(id),
                    loops: vec![args],
                },
            )
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveCurveParams {
    /// A roof, floor or ceiling.
    pub element_id: ElementRef,
    /// The footprint curve's key (1-based, in the order the curves were given).
    pub key: u32,
    /// How far to move it (metres, plan); only the part across the curve counts.
    pub vector: [f64; 2],
}

pub(crate) fn move_curve_command(
    txn: &mut crate::txn::Transaction<'_>,
    p: MoveCurveParams,
) -> Result<()> {
    apply(
        txn,
        &p.element_id,
        Edit::Curve {
            key: p.key,
            vector: p.vector,
        },
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveCornerParams {
    /// A roof, floor or ceiling.
    pub element_id: ElementRef,
    /// The corner now (plan, metres; within a micrometre).
    pub from: [f64; 2],
    pub to: [f64; 2],
}

pub(crate) fn move_corner_command(
    txn: &mut crate::txn::Transaction<'_>,
    p: MoveCornerParams,
) -> Result<()> {
    apply(
        txn,
        &p.element_id,
        Edit::Corner {
            from: p.from,
            to: p.to,
        },
    )
}

/// The ids of curves are their keys; this lists an element's curves for callers choosing one.
pub fn curve_keys(store: &Store, id: ElementId) -> Vec<(u32, Line)> {
    store
        .get(id)
        .and_then(|e| current_curves(store, e))
        .map(|c| c.into_iter().map(|(k, l, _)| (k, l)).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> Vec<Line> {
        let c = [[0.0, 0.0], [10.0, 0.0], [10.0, 6.0], [0.0, 6.0]];
        (0..4)
            .map(|k| Line {
                start: c[k],
                end: c[(k + 1) % 4],
            })
            .collect()
    }

    #[test]
    fn moving_a_side_stretches_its_neighbours() {
        let mut r = rect();
        assert_eq!(move_curve(&mut r, 1, [2.0, 5.0]), vec![1]);
        assert_eq!((r[1].start, r[1].end), ([12.0, 0.0], [12.0, 6.0]));
        assert_eq!(r[0].end, [12.0, 0.0]);
        assert_eq!(r[2].start, [12.0, 6.0]);
        // A slanted neighbour keeps its direction, and the moved side ends where it meets it.
        let mut r = rect();
        r[1] = Line {
            start: [10.0, 0.0],
            end: [8.0, 6.0],
        };
        r[2].start = [8.0, 6.0];
        move_curve(&mut r, 2, [0.0, 3.0]);
        let e = r[1].end;
        assert!(
            (e[1] - 9.0).abs() < 1e-9 && (e[0] - 7.0).abs() < 1e-9,
            "{e:?}"
        );
        assert_eq!(r[2].start, e);
    }

    #[test]
    fn corners_and_lengths_move_shared_ends() {
        let mut r = rect();
        assert_eq!(move_corner(&mut r, [10.0, 6.0], [11.0, 7.0]), vec![1, 2]);
        assert_eq!(r[1].end, [11.0, 7.0]);
        assert_eq!(r[2].start, [11.0, 7.0]);
        let mut r = rect();
        set_length(&mut r, 0, 12.0);
        assert_eq!(r[0].end, [12.0, 0.0]);
        assert_eq!(r[1].start, [12.0, 0.0]);
    }
}
