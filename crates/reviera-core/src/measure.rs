// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Measure (ARC-MOD-023): read-outs that change nothing.
//!
//! - `measure.between`: the distance between two points, its components and, in plan, its angle
//!   from the x axis;
//! - `measure.chain`: the segments of a chain of points and their total;
//! - `measure.along`: the length of an element along its curve: a wall's or curtain wall's
//!   location line (along a curved wall's curve), a ramp's or railing's path.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::id::ElementRef;
use crate::store::Store;

/// A point in metres: plan (x, y) or with a height (x, y, z).
fn xyz(p: &[f64]) -> Result<[f64; 3]> {
    match *p {
        [x, y] => Ok([x, y, 0.0]),
        [x, y, z] => Ok([x, y, z]),
        _ => Err(Error::new("measure.bad_point", "a point is [x, y] or [x, y, z]")),
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BetweenParams {
    pub a: Vec<f64>,
    pub b: Vec<f64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Between {
    pub distance: f64,
    pub dx: f64,
    pub dy: f64,
    pub dz: f64,
    /// The plan distance, and its angle from the x axis (degrees, counter-clockwise).
    pub plan_distance: f64,
    pub angle: f64,
}

pub(crate) fn between(p: BetweenParams) -> Result<Between> {
    let (a, b) = (xyz(&p.a)?, xyz(&p.b)?);
    let (dx, dy, dz) = (b[0] - a[0], b[1] - a[1], b[2] - a[2]);
    Ok(Between {
        distance: (dx * dx + dy * dy + dz * dz).sqrt(),
        dx,
        dy,
        dz,
        plan_distance: dx.hypot(dy),
        angle: dy.atan2(dx).to_degrees(),
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChainParams {
    pub points: Vec<Vec<f64>>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Chain {
    pub segments: Vec<f64>,
    pub total: f64,
}

pub(crate) fn chain(p: ChainParams) -> Result<Chain> {
    let pts = p.points.iter().map(|q| xyz(q)).collect::<Result<Vec<_>>>()?;
    if pts.len() < 2 {
        return Err(Error::new("measure.bad_chain", "a chain needs two points or more"));
    }
    let segments: Vec<f64> = pts
        .windows(2)
        .map(|w| {
            let d = [w[1][0] - w[0][0], w[1][1] - w[0][1], w[1][2] - w[0][2]];
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
        })
        .collect();
    Ok(Chain {
        total: segments.iter().sum(),
        segments,
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AlongParams {
    pub element_id: ElementRef,
}

fn path_length(path: &[[f64; 2]]) -> f64 {
    path.windows(2)
        .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
        .sum()
}

/// The length of an element along its curve (`measure.along`).
pub(crate) fn along(store: &Store, p: AlongParams) -> Result<f64> {
    let id = store
        .resolve(&p.element_id)
        .ok_or_else(|| Error::new("element.not_found", "no such element"))?;
    let e = store.get(id).expect("resolved");
    if let Some(r) = crate::walls::wall_recipe(e) {
        return Ok(crate::walls::curved::path_of(&r).map_or(r.curve.length(), |c| c.length()));
    }
    if let Some(r) = crate::curtain::curtain_recipe(e) {
        return Ok(r.curve.length());
    }
    if let Some(r) = crate::ramps::ramp_recipe(e) {
        return Ok(path_length(&r.path));
    }
    if let Some(g) = crate::grids::grid_data(e) {
        return Ok(g.curve.length());
    }
    Err(Error::new(
        "measure.no_length",
        format!(
            "element {id} ({}) has no length to measure along: walls, curtain walls, ramps and grids do",
            e.class
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances_and_chains() {
        let b = between(BetweenParams {
            a: vec![0.0, 0.0],
            b: vec![3.0, 4.0, 12.0],
        })
        .unwrap();
        assert!((b.distance - 13.0).abs() < 1e-12 && (b.plan_distance - 5.0).abs() < 1e-12);
        assert!((b.angle - 53.130_102_354_155_98).abs() < 1e-9);
        let c = chain(ChainParams {
            points: vec![vec![0.0, 0.0], vec![3.0, 0.0], vec![3.0, 4.0]],
        })
        .unwrap();
        assert_eq!(c.segments, vec![3.0, 4.0]);
        assert_eq!(c.total, 7.0);
    }
}
