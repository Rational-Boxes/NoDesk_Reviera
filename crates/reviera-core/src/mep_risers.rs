// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Rise and drop symbols in plans (MEP-DUCT-011).
//!
//! A vertical duct, pipe, cable tray or conduit segment gets a symbol in a plan by the geometry,
//! not the flow direction:
//! - **rise** when it starts in the view (at or above its bottom) and continues above its top;
//! - **drop** when it comes from below the view's bottom and ends under the cut plane;
//! - **tee up** and **tee down** when that end joins a tee, tap or cross;
//! - nothing when it passes right through, or the cut plane cuts it (it is drawn cut).
//!
//! Two-line symbols outline the section (a circle, or the rectangle turned with the run) with one
//! diagonal for a rise and both for a drop; single-line ones are a circle of the rise/drop
//! annotation size (3 mm on paper), empty for a rise, crossed for a drop, with a bar through it
//! for a tee up and a bar and cross for a tee down. Single-line at Coarse detail (and Medium for
//! pipe), two-line otherwise. These are Reviera's own symbols (the system type's `rise_drop`
//! symbol set; the default set is the only one so far). They are view graphics: never selected,
//! never exported.

use serde::Serialize;

use crate::id::ElementId;
use crate::mep::{self, Domain, Shape};
use crate::mep_runs::{self, Part};
use crate::store::Store;
use crate::views::DetailLevel;

/// The rise/drop annotation size on paper (metres).
pub const SYMBOL_SIZE: f64 = 0.003;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Rise,
    Drop,
    TeeUp,
    TeeDown,
}

/// One symbol: its lines in plan (metres).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Symbol {
    pub element: ElementId,
    pub kind: Kind,
    pub single_line: bool,
    pub at: [f64; 2],
    /// The cut plane cuts it: its section is filled, so the symbol is drawn in the paper colour.
    pub cut: bool,
    pub lines: Vec<[[f64; 2]; 2]>,
}

fn circle(c: [f64; 2], r: f64) -> Vec<[[f64; 2]; 2]> {
    let n = 24;
    let at = |k: usize| {
        let t = std::f64::consts::TAU * k as f64 / n as f64;
        [c[0] + r * t.cos(), c[1] + r * t.sin()]
    };
    (0..n).map(|k| [at(k), at(k + 1)]).collect()
}

/// The symbols of a plan whose view range runs from `bottom` through `cut` to `top` (metres),
/// at a detail level and scale (100 for 1:100).
pub fn symbols(store: &Store, bottom: f64, cut: f64, top: f64, detail: DetailLevel, scale: f64) -> Vec<Symbol> {
    let partners = mep::partners(store);
    let tee_at = |id: ElementId, port: usize| {
        partners.get(&(id, port)).into_iter().flatten().any(|(o, _)| {
            store.get(*o).and_then(mep_runs::fitting_recipe).is_some_and(|f| matches!(f.part, Part::Tee | Part::Tap | Part::Cross))
        })
    };
    let mut out = Vec::new();
    for e in store.iter() {
        let Some(s) = mep_runs::segment_recipe(e) else { continue };
        if s.corner.is_some() {
            continue;
        }
        let d = [s.end[0] - s.start[0], s.end[1] - s.start[1], s.end[2] - s.start[2]];
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if l < 1e-6 || d[2].abs() < 0.999 * l {
            continue;
        }
        // The lower and upper ends with their ports.
        let (lo, lo_port, hi, hi_port) = if s.start[2] <= s.end[2] { (s.start[2], 0, s.end[2], 1) } else { (s.end[2], 1, s.start[2], 0) };
        let eps = 1e-6;
        let kind = if lo >= bottom - eps && lo < top && hi > top + eps {
            if tee_at(e.id(), lo_port) { Kind::TeeUp } else { Kind::Rise }
        } else if lo < bottom - eps && hi > bottom && hi <= cut + eps {
            if tee_at(e.id(), hi_port) { Kind::TeeDown } else { Kind::Drop }
        } else {
            continue;
        };
        let single = match detail {
            DetailLevel::Coarse => true,
            DetailLevel::Medium => s.domain == Domain::Pipe,
            DetailLevel::Fine => false,
        } || matches!(kind, Kind::TeeUp | Kind::TeeDown);
        let c = [s.start[0], s.start[1]];
        let mut lines = Vec::new();
        if single {
            let r = SYMBOL_SIZE / 2.0 * scale;
            lines.extend(circle(c, r));
            let k = r * std::f64::consts::FRAC_1_SQRT_2;
            let cross = [[[c[0] - k, c[1] - k], [c[0] + k, c[1] + k]], [[c[0] - k, c[1] + k], [c[0] + k, c[1] - k]]];
            // The tee's bar runs along the run it branches from: across the section.
            let a = s.across.map_or([1.0, 0.0], |a| {
                let h = a[0].hypot(a[1]).max(1e-12);
                [a[0] / h, a[1] / h]
            });
            let bar = [[c[0] - a[0] * 2.0 * r, c[1] - a[1] * 2.0 * r], [c[0] + a[0] * 2.0 * r, c[1] + a[1] * 2.0 * r]];
            match kind {
                Kind::Rise => {}
                Kind::Drop => lines.extend(cross),
                Kind::TeeUp => lines.push(bar),
                Kind::TeeDown => {
                    lines.push(bar);
                    lines.extend(cross);
                }
            }
        } else {
            match s.size.shape {
                Shape::Rectangular => {
                    let a = s.across.map_or([1.0, 0.0], |a| {
                        let h = a[0].hypot(a[1]).max(1e-12);
                        [a[0] / h, a[1] / h]
                    });
                    let b = [-a[1], a[0]];
                    let (w, h) = (s.size.width / 2.0, s.size.height / 2.0);
                    let corner = |i: f64, j: f64| [c[0] + a[0] * w * i + b[0] * h * j, c[1] + a[1] * w * i + b[1] * h * j];
                    let q = [corner(-1.0, -1.0), corner(1.0, -1.0), corner(1.0, 1.0), corner(-1.0, 1.0)];
                    for k in 0..4 {
                        lines.push([q[k], q[(k + 1) % 4]]);
                    }
                    lines.push([q[0], q[2]]);
                    if kind == Kind::Drop {
                        lines.push([q[1], q[3]]);
                    }
                }
                _ => {
                    let r = if s.size.shape == Shape::Round { s.size.diameter } else { s.size.width.max(s.size.height) } / 2.0;
                    lines.extend(circle(c, r));
                    let k = r * std::f64::consts::FRAC_1_SQRT_2;
                    lines.push([[c[0] - k, c[1] - k], [c[0] + k, c[1] + k]]);
                    if kind == Kind::Drop {
                        lines.push([[c[0] - k, c[1] + k], [c[0] + k, c[1] - k]]);
                    }
                }
            }
        }
        out.push(Symbol { element: e.id(), kind, single_line: single, at: c, cut: lo < cut && hi > cut, lines });
    }
    out
}
