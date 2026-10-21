// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Automatic penetrations (MEP-PEN-001): openings where ducts, pipes, cable tray and conduit pass
//! through walls, floors and ceilings, and flags where they pass through beams.
//!
//! Penetrations are **derived**: found afresh from the runs and their hosts whenever bodies are
//! built or the model is saved, so an opening follows its run as it moves, resizes with it and
//! goes when the run no longer passes through (or is deleted). Each straight segment's axis is
//! met with each host:
//! - a wall's centre plane (straight walls, within its length and height): the opening is the
//!   run's section plus the clearance, projected along the run onto the wall (round, or a
//!   rectangle; slanted runs give a longer, convex outline), cut through the wall's thickness;
//! - a floor's or ceiling's mid-plane, inside its outline: the section projected onto plan,
//!   cut plumb through the slab;
//! - a beam's body (by its bounding box): flagged only, as openings in framing need an engineer.
//!
//! Rules per host category and system classification choose *create*, *flag* (report without
//! opening) or *never*, and the clearance (25 mm by default). In IFC each opening is an
//! `IfcOpeningElement` (OPENING) voiding its host, written when the model is saved.
//!
//! Not built yet: sleeves, fire-seal annotations, roofs and curved walls, provisions for voids in
//! linked models.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::mep::{Classification, Domain, Shape};
use crate::mep_runs::{self, SegmentRecipe};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::openings::Cut;

type V3 = [f64; 3];

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HostKind {
    Wall,
    Floor,
    Ceiling,
    Framing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// An opening is made.
    Create,
    /// The crossing is reported, not opened.
    Flag,
    /// Nothing.
    Never,
}

/// A rule: for a host category and (optionally) a system classification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Rule {
    pub host: HostKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classification: Option<Classification>,
    pub mode: Mode,
    /// Metres around the run's section; 25 mm when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clearance: Option<f64>,
}

/// The default clearance (metres).
pub const CLEARANCE: f64 = 0.025;

/// The mode and clearance for a host and classification: the user's rule for both, then for the
/// host, then the default (create in walls, floors and ceilings; flag in framing).
pub fn rule_for(rules: &[Rule], host: HostKind, c: Option<Classification>) -> (Mode, f64) {
    let r = rules
        .iter()
        .find(|r| r.host == host && r.classification.is_some() && r.classification == c)
        .or_else(|| rules.iter().find(|r| r.host == host && r.classification.is_none()));
    let mode = r.map(|r| r.mode).unwrap_or(if host == HostKind::Framing { Mode::Flag } else { Mode::Create });
    (mode, r.and_then(|r| r.clearance).unwrap_or(CLEARANCE))
}

/// A run crossing a host.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Crossing {
    pub run: ElementId,
    pub host: ElementId,
    pub host_kind: HostKind,
    pub mode: Mode,
    /// Where the run's axis meets the host's middle.
    pub point: V3,
    pub clearance: f64,
    /// The run's size (`Ø100`, `600×300`).
    pub size: String,
    /// The opening's outline in the host's terms: along the wall and up (walls), or in plan
    /// (floors and ceilings); empty when not opened.
    pub outline: Vec<[f64; 2]>,
    /// The run's direction (unit) and how far the opening reaches each way from `point` to
    /// clear the host.
    #[serde(skip)]
    pub axis: V3,
    #[serde(skip)]
    pub reach: f64,
    /// The run's section with the clearance, at `point`, square to the axis.
    #[serde(skip)]
    pub section: Vec<V3>,
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
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn unit(a: V3) -> Option<V3> {
    let l = dot(a, a).sqrt();
    (l > 1e-9).then(|| mul(a, 1.0 / l))
}

/// The run's section grown by the clearance, around `c`, square to the axis `u`.
fn section(s: &SegmentRecipe, c: V3, u: V3, clearance: f64) -> Vec<V3> {
    let w = match s.across.and_then(|a| unit(sub(a, mul(u, dot(a, u))))) {
        Some(w) => w,
        None => unit(cross([0.0, 0.0, 1.0], u)).unwrap_or([1.0, 0.0, 0.0]),
    };
    let h = cross(u, w);
    match s.size.shape {
        Shape::Round => {
            let r = s.size.diameter / 2.0 + clearance;
            (0..24).map(|k| {
                let t = std::f64::consts::TAU * k as f64 / 24.0;
                add(c, add(mul(w, r * t.cos()), mul(h, r * t.sin())))
            }).collect()
        }
        _ => {
            let (a, b) = (s.size.width / 2.0 + clearance, s.size.height / 2.0 + clearance);
            [(-a, -b), (a, -b), (a, b), (-a, b)].iter().map(|(x, y)| add(c, add(mul(w, *x), mul(h, *y)))).collect()
        }
    }
}

/// A polygon counter-clockwise.
fn ccw(mut p: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    let area: f64 = p.iter().zip(p.iter().cycle().skip(1)).map(|(a, b)| a[0] * b[1] - b[0] * a[1]).sum();
    if area < 0.0 {
        p.reverse();
    }
    p
}

fn inside(p: [f64; 2], ring: &[[f64; 2]]) -> bool {
    let mut c = false;
    let n = ring.len();
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + n - 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            c = !c;
        }
    }
    c
}

/// Every crossing of the model's runs (or those given) with walls, floors, ceilings and beams.
pub fn crossings(store: &Store, runs: Option<&[ElementId]>) -> Vec<Crossing> {
    let segs: Vec<(ElementId, SegmentRecipe)> = store
        .iter()
        .filter_map(|e| Some((e.id(), mep_runs::segment_recipe(e)?)))
        .filter(|(id, s)| s.corner.is_none() && runs.is_none_or(|r| r.contains(id)) && s.domain != Domain::Electrical)
        .collect();
    if segs.is_empty() {
        return Vec::new();
    }
    let rules = crate::mep_systems::settings(store).penetration_rules;
    let mut out = Vec::new();
    let walls = crate::walls::solve(store);
    let floors: Vec<(ElementId, HostKind, crate::floors::FloorGeometry)> = store
        .iter()
        .filter_map(|e| {
            let (kind, _) = crate::floors::slab_recipe(e)?;
            let g = crate::floors::regenerate(store, e).ok()?;
            Some((e.id(), if kind == crate::floors::SlabKind::Ceiling { HostKind::Ceiling } else { HostKind::Floor }, g))
        })
        .collect();
    let beams = crate::beams::all(store);
    for (id, s) in &segs {
        let Some(u) = unit(sub(s.end, s.start)) else { continue };
        let len = dot(sub(s.end, s.start), u);
        let class = s.classification;
        let along = |t: f64| add(s.start, mul(u, t));
        // Walls.
        for (wid, body) in &walls.bodies {
            if store.get(*wid).and_then(crate::walls::wall_recipe).is_some_and(|r| crate::walls::curved::path_of(&r).is_some()) {
                continue;
            }
            let g = &body.geometry;
            let (wu, wn) = (g.direction(), g.normal());
            let (n3, w3) = ([wn[0], wn[1], 0.0], [wu[0], wu[1], 0.0]);
            let denom = dot(u, n3);
            if denom.abs() < 0.05 {
                continue; // Running along the wall.
            }
            let s0 = [g.start[0], g.start[1], 0.0];
            let t = dot(sub(s0, s.start), n3) / denom;
            if !(0.0..=len).contains(&t) {
                continue;
            }
            let q = along(t);
            let x = dot(sub(q, s0), w3);
            if x < 0.0 || x > g.length() || q[2] < g.base_z || q[2] > g.base_z + g.height {
                continue;
            }
            let (mode, c) = rule_for(&rules, HostKind::Wall, class);
            if mode == Mode::Never {
                continue;
            }
            let sec = section(s, q, u, c);
            // Each corner along the run onto the wall's plane, in (along, up).
            let outline = ccw(sec
                .iter()
                .map(|v| {
                    let k = dot(sub(*v, s0), n3) / denom;
                    let p = sub(*v, mul(u, k));
                    [dot(sub(p, s0), w3), p[2]]
                })
                .collect());
            out.push(Crossing {
                run: *id,
                host: *wid,
                host_kind: HostKind::Wall,
                mode,
                point: q,
                clearance: c,
                size: s.size.text(),
                outline: if mode == Mode::Create { outline } else { Vec::new() },
                axis: u,
                reach: (g.width / 2.0 + 0.01) / denom.abs(),
                section: sec,
            });
        }
        // Floors and ceilings.
        for (fid, kind, g) in &floors {
            if u[2].abs() < 0.05 {
                continue;
            }
            let mid = g.top - g.thickness / 2.0;
            let t = (mid - s.start[2]) / u[2];
            if !(0.0..=len).contains(&t) {
                continue;
            }
            let q = along(t);
            let in_slab = g.profiles.iter().any(|p| inside([q[0], q[1]], &p.outer) && !p.holes.iter().any(|h| inside([q[0], q[1]], h)));
            if !in_slab {
                continue;
            }
            let (mode, c) = rule_for(&rules, *kind, class);
            if mode == Mode::Never {
                continue;
            }
            let sec = section(s, q, u, c);
            let outline = ccw(sec
                .iter()
                .map(|v| {
                    let k = (v[2] - mid) / u[2];
                    let p = sub(*v, mul(u, k));
                    [p[0], p[1]]
                })
                .collect());
            out.push(Crossing {
                run: *id,
                host: *fid,
                host_kind: *kind,
                mode,
                point: q,
                clearance: c,
                size: s.size.text(),
                outline: if mode == Mode::Create { outline } else { Vec::new() },
                axis: u,
                reach: (g.thickness / 2.0 + 0.01) / u[2].abs(),
                section: sec,
            });
        }
        // Beams: by bounding box.
        let solids = mep_runs::segment_solid(s);
        let (lo, hi) = bbox(solids.iter().flat_map(|x| x.points.iter().copied()));
        for (bid, g) in &beams {
            let (m, _) = g.mesh();
            let (blo, bhi) = bbox(m.iter().copied());
            if (0..3).any(|k| hi[k] < blo[k] || lo[k] > bhi[k]) {
                continue;
            }
            let (mode, c) = rule_for(&rules, HostKind::Framing, class);
            if mode == Mode::Never {
                continue;
            }
            let mid = [(lo[0].max(blo[0]) + hi[0].min(bhi[0])) / 2.0, (lo[1].max(blo[1]) + hi[1].min(bhi[1])) / 2.0, (lo[2].max(blo[2]) + hi[2].min(bhi[2])) / 2.0];
            out.push(Crossing {
                run: *id,
                host: *bid,
                host_kind: HostKind::Framing,
                // Beams are never opened automatically; a create rule still only flags.
                mode: Mode::Flag,
                point: mid,
                clearance: c,
                size: s.size.text(),
                outline: Vec::new(),
                axis: u,
                reach: 0.0,
                section: Vec::new(),
            });
        }
    }
    out
}

fn bbox(points: impl Iterator<Item = V3>) -> (V3, V3) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in points {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    (lo, hi)
}

/// The wall cuts the openings make, by wall.
pub fn wall_cuts(store: &Store) -> Vec<(ElementId, Cut)> {
    crossings(store, None)
        .into_iter()
        .filter(|c| c.host_kind == HostKind::Wall && c.mode == Mode::Create && c.outline.len() >= 3)
        .map(|c| {
            let (lo, hi) = bbox(c.outline.iter().map(|p| [p[0], p[1], 0.0]));
            (c.host, Cut { x: [lo[0], hi[0]], z: [lo[1], hi[1]], outline: c.outline })
        })
        .collect()
}

/// The plan loops the openings cut from a floor or ceiling.
pub fn slab_loops(store: &Store, host: ElementId) -> Vec<Vec<[f64; 2]>> {
    if !store.iter().any(|e| mep_runs::segment_recipe(e).is_some()) {
        return Vec::new();
    }
    crossings(store, None)
        .into_iter()
        .filter(|c| c.host == host && matches!(c.host_kind, HostKind::Floor | HostKind::Ceiling) && c.mode == Mode::Create && c.outline.len() >= 3)
        .map(|c| c.outline)
        .collect()
}

/// An opening's solid: the run's section (with clearance) swept along the run through the host.
pub fn opening_solid(c: &Crossing) -> Option<mep_runs::Solid> {
    if c.section.len() < 3 || c.mode != Mode::Create {
        return None;
    }
    let n = c.section.len();
    let mut points: Vec<V3> = c.section.iter().map(|p| sub(*p, mul(c.axis, c.reach))).collect();
    points.extend(c.section.iter().map(|p| add(*p, mul(c.axis, c.reach))));
    let mut faces = vec![(0..n).rev().collect::<Vec<_>>(), (n..2 * n).collect()];
    for k in 0..n {
        let j = (k + 1) % n;
        faces.push(vec![k, j, n + j, n + k]);
    }
    Some(mep_runs::Solid { points, faces })
}

// ---- Commands ----

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PenetrationsParams {
    /// Segments (every run when empty).
    #[serde(default)]
    pub run_ids: Vec<ElementRef>,
}

/// `mep.penetrations`.
pub(crate) fn penetrations(store: &Store, p: PenetrationsParams) -> Result<Vec<Crossing>> {
    let ids: Vec<ElementId> = p.run_ids.iter().map(|r| store.resolve(r).ok_or_else(|| bad("element.not_found", "no such run"))).collect::<Result<_>>()?;
    Ok(crossings(store, (!ids.is_empty()).then_some(ids.as_slice())))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuleParams {
    pub host_category: HostKind,
    /// A system classification (supply_air, domestic_cold_water, …); every one when omitted.
    pub system: Option<Classification>,
    pub mode: Mode,
    /// Metres around the run's section (0 to 0.5); 25 mm when omitted.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub clearance: Option<f64>,
}

/// `mep.set_penetration_rule`.
pub(crate) fn set_rule(txn: &mut Transaction<'_>, p: RuleParams) -> Result<()> {
    if p.clearance.is_some_and(|c| !(0.0..=0.5).contains(&c)) {
        return Err(bad("mep.penetration_clearance", "a clearance runs from 0 to 500 mm"));
    }
    if p.host_category == HostKind::Framing && p.mode == Mode::Create {
        return Err(bad("mep.penetration_framing", "openings in structural framing need an engineer's approval: framing is flagged, not opened"));
    }
    let mut s = crate::mep_systems::settings(txn.store());
    s.penetration_rules.retain(|r| !(r.host == p.host_category && r.classification == p.system));
    s.penetration_rules.push(Rule { host: p.host_category, classification: p.system, mode: p.mode, clearance: p.clearance });
    crate::mep_systems::save_settings(txn, &s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_by_host_and_system() {
        let rules = vec![
            Rule { host: HostKind::Wall, classification: None, mode: Mode::Flag, clearance: Some(0.05) },
            Rule { host: HostKind::Wall, classification: Some(Classification::SupplyAir), mode: Mode::Create, clearance: None },
        ];
        assert_eq!(rule_for(&rules, HostKind::Wall, Some(Classification::SupplyAir)), (Mode::Create, CLEARANCE));
        assert_eq!(rule_for(&rules, HostKind::Wall, Some(Classification::Sanitary)), (Mode::Flag, 0.05));
        assert_eq!(rule_for(&[], HostKind::Floor, None), (Mode::Create, CLEARANCE));
        assert_eq!(rule_for(&[], HostKind::Framing, None).0, Mode::Flag);
    }
}
