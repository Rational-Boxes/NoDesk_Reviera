// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Curved roofs: arched (barrel and tapering) vaults (ARC-ROOF-008) and domes (ARC-ROOF-009).
//!
//! Both are generated as a fine grid over the curved underside:
//! - each grid cell is split into flat triangles, which are the roof's faces;
//! - so wall attachment, underside queries and plan views treat them like any roof.
//!
//! The body is one closed shell: the grid offset along its smoothed normals by the type's
//! thickness, closed at its edges. This approximates a constant-thickness shell.
//!
//! - **Arched roof.** A four-sided footprint (the roof's sketch) springs from two opposite edges.
//!   Each section between them is a circular segment, semicircle, half ellipse or catenary.
//!   Sections are taken every 500 mm along the vault, at equal parameters, so a trapezoid
//!   gives a smooth tapering vault. The rise is constant, or scales with the span (constant
//!   slope at the springing). Overhangs continue the arch past the spring edges (along its
//!   tangent) and past the ends.
//! - **Dome.** Over a circle, an ellipse or a regular polygon (a cloister vault: every ring a
//!   scaled polygon, so the hips are straight in plan). The profile is a hemisphere, a
//!   segmental, pointed or onion dome, and an oculus may be left open at the crown. A dome has
//!   no footprint sketch; its plan is its own.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{RoofFace, RoofGeometry};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};

/// Grid spacing along a vault.
const STEP: f64 = 0.5;
/// Steps across an arch's profile.
const ACROSS: usize = 24;
/// The steepest an overhang continues an arch's tangent.
const MAX_OVERHANG_SLOPE: f64 = 5.671_281_819_617_709; // tan 80°

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArchProfile {
    #[default]
    Segment,
    Semicircle,
    Elliptical,
    Catenary,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Taper {
    /// Every section rises the same height.
    #[default]
    ConstantRise,
    /// The rise scales with the span (the same angle at the springing).
    ConstantSlope,
}

/// An arched roof's settings (in the roof's recipe).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Arch {
    /// The two opposite footprint curves the arch springs from.
    pub spring: [u32; 2],
    /// The rise at the first end (the end at the spring edges' starts); a semicircle's is half
    /// its span.
    pub rise: f64,
    #[serde(default)]
    pub profile: ArchProfile,
    #[serde(default)]
    pub taper: Taper,
    /// Past the spring edges (horizontal), continuing the arch's tangent.
    #[serde(default)]
    pub overhang: f64,
    /// Past the ends.
    #[serde(default)]
    pub end_overhang: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DomeProfile {
    #[default]
    Hemisphere,
    Segmental,
    Pointed,
    Onion,
}

/// A dome's plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DomePlan {
    Circle {
        radius: f64,
    },
    Ellipse {
        radii: [f64; 2],
        /// Of the first radius from the x axis (radians).
        #[serde(default)]
        rotation: f64,
    },
    /// A regular polygon by its circumradius; a cloister vault.
    Polygon {
        radius: f64,
        sides: u32,
        #[serde(default)]
        rotation: f64,
    },
}

/// A dome's settings (in the roof's recipe).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Dome {
    pub center: [f64; 2],
    pub plan: DomePlan,
    /// The crown above the springing (a hemisphere's is its radius; a pointed dome's follows
    /// from its ratio).
    pub rise: f64,
    #[serde(default)]
    pub profile: DomeProfile,
    /// Pointed: the arcs' radius as a fraction of the span (default 1); onion: the bulge
    /// (default 1.25).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ratio: Option<f64>,
    /// The diameter left open at the crown.
    #[serde(default)]
    pub oculus: f64,
    /// Fewer, flat facets (a polygonal dome).
    #[serde(default)]
    pub faceted: bool,
}

impl Dome {
    /// The plan's characteristic radius: a circle's, an ellipse's mean, a polygon's apothem.
    pub fn radius(&self) -> f64 {
        match &self.plan {
            DomePlan::Circle { radius } => *radius,
            DomePlan::Ellipse { radii, .. } => (radii[0] + radii[1]) / 2.0,
            DomePlan::Polygon { radius, sides, .. } => {
                radius * (std::f64::consts::PI / f64::from((*sides).max(3))).cos()
            }
        }
    }
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit(a: [f64; 3]) -> [f64; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if l < 1e-15 {
        return [0.0, 0.0, 1.0];
    }
    [a[0] / l, a[1] / l, a[2] / l]
}

/// A surface sampled on a grid of rows; `wrap` joins each row's last point to its first (a
/// dome's rings).
pub(super) struct Grid {
    pub(super) rows: Vec<Vec<[f64; 3]>>,
    pub(super) wrap: bool,
}

impl Grid {
    fn cols(&self) -> usize {
        self.rows[0].len()
    }

    /// The cells' triangles as grid indices `(row, col)`, each counter-clockwise from above.
    fn triangles(&self) -> Vec<[(usize, usize); 3]> {
        let (nr, nc) = (self.rows.len(), self.cols());
        let spans = if self.wrap { nc } else { nc - 1 };
        let mut tris = Vec::new();
        for r in 0..nr - 1 {
            for c in 0..spans {
                let c1 = (c + 1) % nc;
                tris.push([(r, c), (r, c1), (r + 1, c1)]);
                tris.push([(r, c), (r + 1, c1), (r + 1, c)]);
            }
        }
        // Up, by the total area seen from above.
        let at = |(r, c): (usize, usize)| self.rows[r][c];
        let up: f64 = tris
            .iter()
            .map(|t| cross(sub(at(t[1]), at(t[0])), sub(at(t[2]), at(t[0])))[2])
            .sum();
        if up < 0.0 {
            for t in &mut tris {
                t.swap(1, 2);
            }
        }
        tris
    }
}

/// The roof from a grid: one face per (non-degenerate) triangle, and a closed shell of
/// `thickness` along the smoothed normals as the body.
pub(super) fn roof_of(
    g: &Grid,
    plate: f64,
    thickness: f64,
    footprint: Vec<[f64; 2]>,
    predefined: &'static str,
) -> RoofGeometry {
    let (nr, nc) = (g.rows.len(), g.cols());
    let tris = g.triangles();
    let at = |(r, c): (usize, usize)| g.rows[r][c];
    // Vertex normals: the sum of the adjacent triangles' (area-weighted) normals.
    let mut normals = vec![vec![[0.0; 3]; nc]; nr];
    let mut faces = Vec::new();
    for t in &tris {
        let n = cross(sub(at(t[1]), at(t[0])), sub(at(t[2]), at(t[0])));
        for v in t {
            let m = &mut normals[v.0][v.1];
            m[0] += n[0];
            m[1] += n[1];
            m[2] += n[2];
        }
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if len < 1e-12 || n[2] <= 1e-9 * len {
            continue;
        }
        faces.push(RoofFace {
            key: None,
            bottom: t.iter().map(|v| at(*v)).collect(),
            normal: unit(n),
            slab: (Vec::new(), Vec::new()),
            holes: Vec::new(),
            faceted: true,
        });
    }
    // Coincident grid points (an apex) share one normal.
    let key = |p: [f64; 3]| {
        [
            (p[0] * 1e9).round() as i64,
            (p[1] * 1e9).round() as i64,
            (p[2] * 1e9).round() as i64,
        ]
    };
    let mut sums: std::collections::HashMap<[i64; 3], [f64; 3]> = Default::default();
    for (row, ns) in g.rows.iter().zip(&normals) {
        for (p, n) in row.iter().zip(ns) {
            let s = sums.entry(key(*p)).or_insert([0.0; 3]);
            *s = [s[0] + n[0], s[1] + n[1], s[2] + n[2]];
        }
    }
    let index = |r: usize, c: usize| (r * nc + c) as u32;
    let mut positions: Vec<[f64; 3]> = g.rows.iter().flatten().copied().collect();
    let top_at = (nr * nc) as u32;
    for p in g.rows.iter().flatten() {
        let n = unit(sums[&key(*p)]);
        positions.push([
            p[0] + n[0] * thickness,
            p[1] + n[1] * thickness,
            p[2] + n[2] * thickness,
        ]);
    }
    let mut indices = Vec::new();
    for t in &tris {
        let [a, b, c] = t.map(|(r, c)| index(r, c));
        // The underside faces down, the top up.
        indices.extend([a, c, b]);
        indices.extend([top_at + a, top_at + b, top_at + c]);
    }
    // The edges: each boundary edge of the grid closed by a strip, wound against the
    // triangles' own use of it (so the shell is consistently oriented).
    let directed: std::collections::HashSet<(u32, u32)> = tris
        .iter()
        .flat_map(|t| {
            let [a, b, c] = t.map(|(r, c)| index(r, c));
            [(a, b), (b, c), (c, a)]
        })
        .collect();
    let mut boundary: Vec<(u32, u32)> = Vec::new();
    let spans = if g.wrap { nc } else { nc - 1 };
    for c in 0..spans {
        let c1 = (c + 1) % nc;
        boundary.push((index(0, c), index(0, c1)));
        boundary.push((index(nr - 1, c), index(nr - 1, c1)));
    }
    if !g.wrap {
        for r in 0..nr - 1 {
            boundary.push((index(r, 0), index(r + 1, 0)));
            boundary.push((index(r, nc - 1), index(r + 1, nc - 1)));
        }
    }
    for (p, q) in boundary {
        let (p, q) = if directed.contains(&(p, q)) {
            (p, q)
        } else {
            (q, p)
        };
        indices.extend([p, q, top_at + q, p, top_at + q, top_at + p]);
    }
    if let Some(f) = faces.first_mut() {
        f.slab = (positions, indices);
    }
    let keys = (1..=footprint.len() as u32).collect();
    RoofGeometry {
        faces,
        plate,
        thickness,
        footprint,
        keys,
        predefined,
        trims: Vec::new(),
        uncut: 0,
        surface: None,
    }
}

/// An arch profile's height at `u` (0 and 1 the springings) over span `s` with rise `h`.
fn arch_z(profile: ArchProfile, u: f64, s: f64, h: f64) -> f64 {
    let x = (u - 0.5) * s;
    match profile {
        ArchProfile::Segment | ArchProfile::Semicircle => {
            let h = if profile == ArchProfile::Semicircle {
                s / 2.0
            } else {
                h
            };
            let r = (s * s / 4.0 + h * h) / (2.0 * h);
            (r * r - x * x).max(0.0).sqrt() - (r - h)
        }
        ArchProfile::Elliptical => h * (1.0 - (2.0 * u - 1.0).powi(2)).max(0.0).sqrt(),
        ArchProfile::Catenary => {
            let a: f64 = 2.0;
            h * (a.cosh() - (a * (2.0 * u - 1.0)).cosh()) / (a.cosh() - 1.0)
        }
    }
}

/// An arched roof over the counter-clockwise footprint `ring` (corner `i` starts curve
/// `keys[i]`), springing at `plate`.
pub fn arch(
    ring: &[[f64; 2]],
    keys: &[u32],
    a: &Arch,
    plate: f64,
    thickness: f64,
) -> Result<RoofGeometry> {
    if ring.len() != 4 {
        return Err(Error::new(
            "roof.arch_needs_four_sides",
            "an arched roof's footprint has four sides",
        ));
    }
    let i = keys.iter().position(|k| *k == a.spring[0]).ok_or_else(|| {
        Error::new(
            "roof.no_such_curve",
            "the arch's spring edge is not in the footprint",
        )
    })?;
    if keys[(i + 2) % 4] != a.spring[1] {
        return Err(Error::new(
            "roof.spring_edges_not_opposite",
            "an arch springs from two opposite edges of its footprint",
        ));
    }
    if (a.rise.is_nan() || a.rise <= 0.0) && a.profile != ArchProfile::Semicircle {
        return Err(Error::new(
            "roof.bad_rise",
            "the arch's rise must be positive",
        ));
    }
    let c = |k: usize| ring[(i + k) % 4];
    // Spring edge A runs c0 → c1; B the same way, c3 → c2.
    let (a0, a1, b0, b1) = (c(0), c(1), c(3), c(2));
    let lerp =
        |p: [f64; 2], q: [f64; 2], t: f64| [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t];
    let len_a = (a1[0] - a0[0]).hypot(a1[1] - a0[1]);
    let len_b = (b1[0] - b0[0]).hypot(b1[1] - b0[1]);
    let len = (len_a + len_b) / 2.0;
    let span = |t: f64| {
        let (l, r) = (lerp(a0, a1, t), lerp(b0, b1, t));
        (r[0] - l[0]).hypot(r[1] - l[1])
    };
    let s0 = span(0.0);
    let rise = |t: f64| match a.taper {
        Taper::ConstantRise => a.rise,
        Taper::ConstantSlope => a.rise * span(t) / s0,
    };
    let (t0, t1) = (-a.end_overhang / len, 1.0 + a.end_overhang / len);
    let steps = (((t1 - t0) * len) / STEP).ceil().max(2.0) as usize;
    // Cosine spacing across: finer where the arch is steep.
    let us: Vec<f64> = (0..=ACROSS)
        .map(|k| 0.5 - 0.5 * (std::f64::consts::PI * k as f64 / ACROSS as f64).cos())
        .collect();
    let mut rows = Vec::new();
    for k in 0..=steps {
        let t = t0 + (t1 - t0) * k as f64 / steps as f64;
        let (l, r) = (lerp(a0, a1, t), lerp(b0, b1, t));
        let s = (r[0] - l[0]).hypot(r[1] - l[1]);
        let h = rise(t);
        let mut row: Vec<[f64; 3]> = us
            .iter()
            .map(|u| {
                let p = lerp(l, r, *u);
                [p[0], p[1], plate + arch_z(a.profile, *u, s, h)]
            })
            .collect();
        if a.overhang > 0.0 {
            // Continue each springing's tangent outward.
            let d = 1e-4;
            let slope = |u0: f64, u1: f64| {
                ((arch_z(a.profile, u1, s, h) - arch_z(a.profile, u0, s, h))
                    / ((u1 - u0).abs() * s))
                    .abs()
                    .min(MAX_OVERHANG_SLOPE)
            };
            let out = [(l[0] - r[0]) / s, (l[1] - r[1]) / s];
            let drop = a.overhang * slope(0.0, d);
            row.insert(
                0,
                [
                    l[0] + out[0] * a.overhang,
                    l[1] + out[1] * a.overhang,
                    plate - drop,
                ],
            );
            let drop = a.overhang * slope(1.0 - d, 1.0);
            row.push([
                r[0] - out[0] * a.overhang,
                r[1] - out[1] * a.overhang,
                plate - drop,
            ]);
        }
        rows.push(row);
    }
    let mut geo = roof_of(
        &Grid { rows, wrap: false },
        plate,
        thickness,
        ring.to_vec(),
        "BARREL_ROOF",
    );
    geo.surface = Some(Surface::Vault(ArchFrame { a0, a1, b0, b1, plate, profile: a.profile, rise: a.rise, taper: a.taper }));
    Ok(geo)
}

/// A dome profile point at `s` (0 the springing, 1 the crown): the plan radius as a fraction of
/// the plan's, and the height above the springing.
fn dome_point(d: &Dome, s: f64) -> (f64, f64) {
    let r = d.radius();
    let half_pi = std::f64::consts::FRAC_PI_2;
    match d.profile {
        DomeProfile::Hemisphere => {
            let a = s * half_pi;
            (a.cos(), r * a.sin())
        }
        DomeProfile::Segmental => {
            // A sphere through the springing circle and the crown at the rise.
            let h = d.rise.min(r).max(1e-6);
            let big = (r * r + h * h) / (2.0 * h);
            let a0 = ((big - h) / big).asin();
            let a = a0 + (half_pi - a0) * s;
            (big * a.cos() / r, big * a.sin() - (big - h))
        }
        DomeProfile::Pointed => {
            // Two arcs of radius k · span, centred on the springing line, meeting at the crown.
            let k = d.ratio.unwrap_or(1.0).max(0.51);
            let big = k * 2.0 * r;
            let centre = r - big; // signed, from the axis toward this side
            let a_top = ((0.0 - centre) / big).acos();
            let a = a_top * s;
            let x = centre + big * a.cos();
            (x / r, big * a.sin())
        }
        DomeProfile::Onion => {
            let b = d.ratio.unwrap_or(1.25).max(1.0);
            let x = (1.0 - s).powf(0.6) * (1.0 + (b - 1.0) * (std::f64::consts::PI * s).sin());
            (x, d.rise * s)
        }
    }
}

/// A dome.
pub fn dome(d: &Dome, plate: f64, thickness: f64) -> Result<RoofGeometry> {
    let r = d.radius();
    if r.is_nan() || r <= 0.1 {
        return Err(Error::new("roof.bad_dome", "the dome's plan is too small"));
    }
    if d.profile == DomeProfile::Segmental && (d.rise.is_nan() || d.rise <= 0.0) {
        return Err(Error::new(
            "roof.bad_rise",
            "the dome's rise must be positive",
        ));
    }
    if d.profile == DomeProfile::Onion && (d.rise.is_nan() || d.rise <= 0.0) {
        return Err(Error::new(
            "roof.bad_rise",
            "the dome's rise must be positive",
        ));
    }
    if let DomePlan::Polygon { sides, .. } = d.plan
        && sides < 3
    {
        return Err(Error::new(
            "roof.bad_dome",
            "a polygonal dome needs three sides or more",
        ));
    }
    // Directions round the plan, and the plan's springing point in each (a polygon's corners
    // included, so the hips are straight).
    let tau = std::f64::consts::TAU;
    let dirs: Vec<[f64; 2]> = match &d.plan {
        DomePlan::Circle { radius } => {
            let n = if d.faceted { 16 } else { 48 };
            (0..n)
                .map(|j| {
                    let t = tau * j as f64 / n as f64;
                    [radius * t.cos(), radius * t.sin()]
                })
                .collect()
        }
        DomePlan::Ellipse { radii, rotation } => {
            let n = if d.faceted { 16 } else { 48 };
            let (c, s) = (rotation.cos(), rotation.sin());
            (0..n)
                .map(|j| {
                    let t = tau * j as f64 / n as f64;
                    let (x, y) = (radii[0] * t.cos(), radii[1] * t.sin());
                    [x * c - y * s, x * s + y * c]
                })
                .collect()
        }
        DomePlan::Polygon {
            radius,
            sides,
            rotation,
        } => {
            let per = if d.faceted { 1 } else { 6 };
            let n = *sides as usize;
            let corner = |j: usize| {
                let t = rotation + tau * j as f64 / n as f64;
                [radius * t.cos(), radius * t.sin()]
            };
            (0..n)
                .flat_map(|j| {
                    let (p, q) = (corner(j), corner(j + 1));
                    (0..per).map(move |k| {
                        let f = k as f64 / per as f64;
                        [p[0] + (q[0] - p[0]) * f, p[1] + (q[1] - p[1]) * f]
                    })
                })
                .collect()
        }
    };
    let rings = if d.faceted { 5 } else { 16 };
    // Up the profile, stopping at the oculus.
    let open = (d.oculus / 2.0 / r).clamp(0.0, 0.95);
    let mut samples: Vec<(f64, f64)> = (0..=rings)
        .map(|k| dome_point(d, k as f64 / rings as f64))
        .collect();
    if open > 0.0
        && let Some(k) = samples.iter().position(|(x, _)| *x <= open)
    {
        let (x0, z0) = samples[k - 1];
        let (x1, z1) = samples[k];
        let f = if (x0 - x1).abs() < 1e-12 {
            1.0
        } else {
            (x0 - open) / (x0 - x1)
        };
        samples.truncate(k);
        samples.push((open, z0 + (z1 - z0) * f));
    }

    let rows: Vec<Vec<[f64; 3]>> = samples
        .iter()
        .map(|(x, z)| {
            dirs.iter()
                .map(|p| [d.center[0] + p[0] * x, d.center[1] + p[1] * x, plate + z])
                .collect()
        })
        .collect();
    let footprint = dirs
        .iter()
        .map(|p| [d.center[0] + p[0], d.center[1] + p[1]])
        .collect();
    let mut geo = roof_of(
        &Grid { rows, wrap: true },
        plate,
        thickness,
        footprint,
        "DOME_ROOF",
    );
    geo.surface = Some(Surface::Dome { dome: d.clone(), plate });
    Ok(geo)
}

// ---- Glazing on vaults and domes (ARC-GLZ-006) ----

/// A vault's springing edges and section, to evaluate its surface anywhere.
#[derive(Debug, Clone, PartialEq)]
pub struct ArchFrame {
    /// Spring edge A runs `a0` → `a1`, edge B `b0` → `b1`, the same way.
    pub a0: [f64; 2],
    pub a1: [f64; 2],
    pub b0: [f64; 2],
    pub b1: [f64; 2],
    pub plate: f64,
    pub profile: ArchProfile,
    pub rise: f64,
    pub taper: Taper,
}

impl ArchFrame {
    fn ends(&self, t: f64) -> ([f64; 2], [f64; 2]) {
        let lerp = |p: [f64; 2], q: [f64; 2]| [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t];
        (lerp(self.a0, self.a1), lerp(self.b0, self.b1))
    }

    fn span(&self, t: f64) -> f64 {
        let (l, r) = self.ends(t);
        (r[0] - l[0]).hypot(r[1] - l[1])
    }

    /// The point `t` along the vault (0 to 1 between its ends) and `u` across (0 to 1 between
    /// the springings).
    pub fn point(&self, t: f64, u: f64) -> [f64; 3] {
        let (l, r) = self.ends(t);
        let s = self.span(t);
        let h = match self.taper {
            Taper::ConstantRise => self.rise,
            Taper::ConstantSlope => self.rise * s / self.span(0.0).max(1e-9),
        };
        [l[0] + (r[0] - l[0]) * u, l[1] + (r[1] - l[1]) * u, self.plate + arch_z(self.profile, u, s, h)]
    }

    /// A point inside, under the surface at `t`.
    fn inside(&self, t: f64) -> [f64; 3] {
        let (l, r) = self.ends(t);
        [(l[0] + r[0]) / 2.0, (l[1] + r[1]) / 2.0, self.plate]
    }

    fn length(&self) -> f64 {
        ((self.a1[0] - self.a0[0]).hypot(self.a1[1] - self.a0[1]) + (self.b1[0] - self.b0[0]).hypot(self.b1[1] - self.b0[1])) / 2.0
    }
}

/// A curved roof's exact surface.
#[derive(Debug, Clone, PartialEq)]
pub enum Surface {
    Vault(ArchFrame),
    Dome { dome: Dome, plate: f64 },
}

/// When a glazed vault's or dome's four-cornered panels are split into two triangles.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Triangulate {
    /// Only those whose corners are not in one plane (a tapering vault's, say).
    #[default]
    Auto,
    Always,
    /// Never: a warped panel stays one (bent) pane.
    Never,
}

/// How a glazed vault or dome is divided (in the roof's recipe); each count follows the
/// glazing type's spacing when absent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CurvedGrid {
    /// A vault: the bays along it (metres apart at most); a dome: its meridians (a count).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub u: Option<f64>,
    /// A vault: the divisions round the arch; a dome: its rings (counts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v: Option<f64>,
    #[serde(default)]
    pub triangulate: Triangulate,
}

/// Panels within this of their plane are planar.
const PLANAR: f64 = 0.001;

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

/// Parameters 0..=1 at equal lengths along a curve sampled by `f`.
fn equal_lengths(n: usize, f: impl Fn(f64) -> [f64; 3]) -> (Vec<f64>, f64) {
    const FINE: usize = 400;
    let pts: Vec<[f64; 3]> = (0..=FINE).map(|k| f(k as f64 / FINE as f64)).collect();
    let mut acc = vec![0.0];
    for w in pts.windows(2) {
        let d = sub(w[1], w[0]);
        acc.push(acc.last().copied().unwrap_or(0.0) + dot(d, d).sqrt());
    }
    let total = *acc.last().unwrap_or(&0.0);
    let params = (0..=n)
        .map(|k| {
            let want = total * k as f64 / n as f64;
            let i = acc.iter().position(|a| *a >= want - 1e-12).unwrap_or(FINE).max(1);
            let (a0, a1) = (acc[i - 1], acc[i]);
            let f = if a1 > a0 { (want - a0) / (a1 - a0) } else { 0.0 };
            ((i - 1) as f64 + f) / FINE as f64
        })
        .collect();
    (params, total)
}

/// A planar polygon's corners moved in by `d` from every edge (`n` its normal, the corners
/// counter-clockwise about it).
fn inset(poly: &[[f64; 3]], n: [f64; 3], d: f64) -> Vec<[f64; 3]> {
    let k = poly.len();
    let inward = |i: usize| unit(cross(n, sub(poly[(i + 1) % k], poly[i])));
    (0..k)
        .map(|i| {
            let (m0, m1) = (inward((i + k - 1) % k), inward(i));
            let s = d / (1.0 + dot(m0, m1)).max(0.2);
            add(poly[i], scale(add(m0, m1), s))
        })
        .collect()
}

/// A point inside a curved roof under row `r` of its grid (to tell outward from inward).
type Inside = Box<dyn Fn(usize) -> [f64; 3]>;

/// A glazed vault's or dome's panels and bars, on the glazing type's bars and thickness.
pub fn glazing(surface: &Surface, grid: &CurvedGrid, ty: &crate::curtain::CurtainType) -> super::glazing::Glazing {
    let spacing = |r: crate::curtain::Rule, default: f64| if r.spacing > 0.0 { r.spacing } else { default };
    let (rows, wrap, inside): (Vec<Vec<[f64; 3]>>, bool, Inside) = match surface {
        Surface::Vault(f) => {
            let bays = grid.u.map_or_else(|| (f.length() / spacing(ty.u, 1.125)).ceil(), |m| (f.length() / m.max(0.2)).ceil()).max(1.0) as usize;
            let (_, arc) = equal_lengths(ACROSS * 4, |u| f.point(0.0, u));
            let divisions = grid.v.map_or_else(|| (arc / spacing(ty.v, 2.0)).ceil(), |n| n.round()).max(2.0) as usize;
            let (us, _) = equal_lengths(divisions, |u| f.point(0.0, u));
            let ts: Vec<f64> = (0..=bays).map(|k| k as f64 / bays as f64).collect();
            let rows = ts.iter().map(|t| us.iter().map(|u| f.point(*t, *u)).collect()).collect();
            let f2 = f.clone();
            let ts2 = ts.clone();
            (rows, false, Box::new(move |r: usize| f2.inside(ts2[r.min(ts2.len() - 1)])))
        }
        Surface::Dome { dome: d, plate } => {
            let r = d.radius();
            let meridians = grid.u.map_or_else(|| (std::f64::consts::TAU * r / spacing(ty.u, 1.125)).ceil(), f64::round).max(3.0) as usize;
            let (_, arc) = equal_lengths(64, |s| {
                let (x, z) = dome_point(d, s);
                [x * r, 0.0, z]
            });
            let rings = grid.v.map_or_else(|| (arc / spacing(ty.v, 2.0)).ceil(), f64::round).max(1.0) as usize;
            let dirs: Vec<[f64; 2]> = match &d.plan {
                DomePlan::Polygon { radius, sides, rotation } => {
                    let n = *sides as usize;
                    let per = (meridians / n).max(1);
                    let corner = |j: usize| {
                        let t = rotation + std::f64::consts::TAU * j as f64 / n as f64;
                        [radius * t.cos(), radius * t.sin()]
                    };
                    (0..n)
                        .flat_map(|j| {
                            let (p, q) = (corner(j), corner(j + 1));
                            (0..per).map(move |k| {
                                let f = k as f64 / per as f64;
                                [p[0] + (q[0] - p[0]) * f, p[1] + (q[1] - p[1]) * f]
                            })
                        })
                        .collect()
                }
                DomePlan::Ellipse { radii, rotation } => {
                    let (c, s) = (rotation.cos(), rotation.sin());
                    (0..meridians)
                        .map(|j| {
                            let t = std::f64::consts::TAU * j as f64 / meridians as f64;
                            let (x, y) = (radii[0] * t.cos(), radii[1] * t.sin());
                            [x * c - y * s, x * s + y * c]
                        })
                        .collect()
                }
                DomePlan::Circle { radius } => (0..meridians)
                    .map(|j| {
                        let t = std::f64::consts::TAU * j as f64 / meridians as f64;
                        [radius * t.cos(), radius * t.sin()]
                    })
                    .collect(),
            };
            // Rings at equal lengths up the profile, stopping at the oculus.
            let (ss, _) = equal_lengths(rings, |s| {
                let (x, z) = dome_point(d, s);
                [x * r, 0.0, z]
            });
            let open = (d.oculus / 2.0 / r).clamp(0.0, 0.95);
            let mut samples: Vec<(f64, f64)> = ss.iter().map(|s| dome_point(d, *s)).collect();
            if open > 0.0
                && let Some(k) = samples.iter().position(|(x, _)| *x <= open)
                && k > 0
            {
                samples.truncate(k);
                samples.push((open, samples[k - 1].1));
            }
            let rows = samples
                .iter()
                .map(|(x, z)| dirs.iter().map(|p| [d.center[0] + p[0] * x, d.center[1] + p[1] * x, plate + z]).collect())
                .collect();
            let centre = [d.center[0], d.center[1], *plate];
            (rows, true, Box::new(move |_| centre))
        }
    };
    let (nr, nc) = (rows.len(), rows[0].len());
    let spans = if wrap { nc } else { nc - 1 };
    let t = ty.panel_thickness;
    let mut parts = Vec::new();
    // Each edge's cell normals, for the bar along it.
    let mut edge_normals: std::collections::BTreeMap<[(usize, usize); 2], [f64; 3]> = Default::default();
    let key = |a: (usize, usize), b: (usize, usize)| if a <= b { [a, b] } else { [b, a] };
    let same = |p: [f64; 3], q: [f64; 3]| dot(sub(p, q), sub(p, q)) < 1e-12;
    let edges_of = |poly: &[(usize, usize)], n: [f64; 3], edge_normals: &mut std::collections::BTreeMap<[(usize, usize); 2], [f64; 3]>| {
        for k in 0..poly.len() {
            let e = edge_normals.entry(key(poly[k], poly[(k + 1) % poly.len()])).or_insert([0.0; 3]);
            *e = add(*e, n);
        }
    };
    let width = ty.interior.or(ty.border).map_or(0.0, |m| m.width);
    for r in 0..nr - 1 {
        for c in 0..spans {
            let c1 = (c + 1) % nc;
            let mut cell = vec![(r, c), (r, c1), (r + 1, c1), (r + 1, c)];
            // A crown (or a degenerate edge) makes it a triangle.
            cell.dedup_by(|a, b| same(rows[a.0][a.1], rows[b.0][b.1]));
            if cell.len() > 3 && same(rows[cell[0].0][cell[0].1], rows[cell[cell.len() - 1].0][cell[cell.len() - 1].1]) {
                cell.pop();
            }
            if cell.len() < 3 {
                continue;
            }
            let at = |v: (usize, usize)| rows[v.0][v.1];
            let mut n = unit(cross(sub(at(cell[1]), at(cell[0])), sub(at(cell[cell.len() - 1]), at(cell[0]))));
            // Outward: away from the inside.
            let centroid = scale(cell.iter().fold([0.0; 3], |s, v| add(s, at(*v))), 1.0 / cell.len() as f64);
            if dot(n, sub(centroid, inside(r))) < 0.0 {
                cell.reverse();
                n = scale(n, -1.0);
            }
            let warp = if cell.len() == 4 { dot(sub(at(cell[3]), at(cell[0])), n).abs() } else { 0.0 };
            let split = cell.len() == 4
                && match grid.triangulate {
                    Triangulate::Always => true,
                    Triangulate::Never => false,
                    Triangulate::Auto => warp > PLANAR,
                };
            let polys: Vec<Vec<(usize, usize)>> = if split { vec![vec![cell[0], cell[1], cell[2]], vec![cell[0], cell[2], cell[3]]] } else { vec![cell.clone()] };
            for poly in polys {
                let pts: Vec<[f64; 3]> = poly.iter().map(|v| at(*v)).collect();
                let pn = unit(cross(sub(pts[1], pts[0]), sub(pts[pts.len() - 1], pts[0])));
                edges_of(&poly, pn, &mut edge_normals);
                let inner = inset(&pts, pn, width / 2.0);
                let k = inner.len() as u32;
                let mut mesh_pts = inner.clone();
                mesh_pts.extend(inner.iter().map(|p| add(*p, scale(pn, t))));
                let mut idx = Vec::new();
                for i in 1..k - 1 {
                    idx.extend([0, i + 1, i]);
                    idx.extend([k, k + i, k + i + 1]);
                }
                for i in 0..k {
                    let j = (i + 1) % k;
                    idx.extend([i, j, k + j, i, k + j, k + i]);
                }
                let size = [(dot(sub(pts[1], pts[0]), sub(pts[1], pts[0]))).sqrt(), (dot(sub(pts[pts.len() - 1], pts[0]), sub(pts[pts.len() - 1], pts[0]))).sqrt()];
                parts.push(super::glazing::GlazingPart { mullion: false, panel: ty.panel, mesh: (mesh_pts, idx), axis: None, size: Some(size) });
            }
        }
    }
    // A bar along every panel edge, turned to the panels' mean normal there; the border's on the
    // grid's outer edges.
    for ([a, b], n) in edge_normals {
        let (pa, pb) = (rows[a.0][a.1], rows[b.0][b.1]);
        if same(pa, pb) {
            continue;
        }
        let up = unit(n);
        let outer = (a.0 == b.0 && (a.0 == 0 || a.0 == nr - 1)) || (!wrap && a.1 == b.1 && (a.1 == 0 || a.1 == nc - 1));
        let Some(m) = (if outer { ty.border.or(ty.interior) } else { ty.interior }) else { continue };
        let lift = |p: [f64; 3]| add(p, scale(up, t / 2.0));
        let (la, lb) = (lift(pa), lift(pb));
        parts.push(super::glazing::GlazingPart { mullion: true, panel: ty.panel, mesh: super::glazing::bar(la, lb, up, m.width, m.depth), axis: Some([la, lb]), size: None });
    }
    super::glazing::Glazing { planes: Vec::new(), parts }
}

/// What `roof.set_glazing` returns: the panels and how flat they are.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct GlazingInfo {
    pub panels: usize,
    pub triangles: usize,
    pub bars: usize,
    /// The largest distance of a four-cornered panel's corner from its plane, metres (0 when
    /// every panel is planar).
    pub max_warp: f64,
}

fn info_of(gl: &super::glazing::Glazing) -> GlazingInfo {
    let panels: Vec<_> = gl.parts.iter().filter(|p| !p.mullion).collect();
    let corners = |p: &&super::glazing::GlazingPart| p.mesh.0.len() / 2;
    let max_warp = panels
        .iter()
        .filter(|p| corners(p) == 4)
        .map(|p| {
            let q = &p.mesh.0;
            let n = unit(cross(sub(q[1], q[0]), sub(q[3], q[0])));
            dot(sub(q[2], q[0]), n).abs()
        })
        .fold(0.0, f64::max);
    GlazingInfo { panels: panels.len(), triangles: panels.iter().filter(|p| corners(p) == 3).count(), bars: gl.parts.len() - panels.len(), max_warp }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetGlazingParams {
    pub roof_id: ElementRef,
    /// A vault: bay spacing along it (a length); a dome: the number of meridians. Null for the
    /// glazing type's spacing.
    pub grid_u: Option<Value>,
    /// A vault: divisions round the arch; a dome: rings (counts).
    pub grid_v: Option<f64>,
    pub triangulate: Option<Triangulate>,
}

/// `roof.set_glazing`: how a glazed vault or dome is divided into panels (ARC-GLZ-006).
pub(crate) fn set_glazing(txn: &mut crate::txn::Transaction<'_>, p: SetGlazingParams) -> Result<GlazingInfo> {
    let store = txn.store().clone();
    let id = store.resolve(&p.roof_id).ok_or_else(|| Error::new("element.not_found", "no such roof"))?;
    let e = store.get(id).expect("resolved");
    let mut r = super::roof_recipe(e).ok_or_else(|| Error::new("roof.not_reviera", format!("element {id} is not a Reviera roof")))?;
    if r.arch.is_none() && r.dome.is_none() {
        return Err(Error::new("roof.not_curved", "glazing grids are for arched roofs and domes; a footprint roof's glazing follows its type"));
    }
    let ty = super::glazing::of_roof(&store, e).ok_or_else(|| {
        Error::new("roof.not_glazed", "give the roof a Sloped Glazing type first (Modify | Roofs › Sloped Glazing)")
    })?;
    let mut g = r.curved_glazing.unwrap_or_default();
    if let Some(v) = &p.grid_u {
        g.u = match v {
            Value::Null => None,
            v if r.arch.is_some() => Some(length(&store, v, "the bay spacing")?),
            v => Some(v.as_f64().ok_or_else(|| Error::new("roof.bad_grid", "a dome's meridians are a count"))?),
        };
    }
    if let Some(v) = p.grid_v {
        g.v = Some(v);
    }
    if let Some(t) = p.triangulate {
        g.triangulate = t;
    }
    let (min_u, max_u) = if r.arch.is_some() { (0.2, 20.0) } else { (3.0, 400.0) };
    if g.u.is_some_and(|u| !(min_u..=max_u).contains(&u)) || g.v.is_some_and(|v| !(1.0..=200.0).contains(&v)) {
        return Err(Error::new("roof.bad_grid", "the glazing grid is out of range (a vault's bays 0.2–20 m apart, a dome's 3–400 meridians, 1–200 divisions or rings)"));
    }
    r.curved_glazing = Some(g);
    let value = serde_json::to_value(&r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(crate::walls::RECIPE_KEY.into(), value);
    })?;
    let store = txn.store().clone();
    let geo = super::regenerate(&store, store.get(id).expect("exists"))?;
    let surface = geo.surface.as_ref().ok_or_else(|| Error::new("roof.not_curved", "the roof has no curved surface"))?;
    Ok(info_of(&glazing(surface, &g, &ty)))
}

// ---- Commands ----

fn length(store: &crate::store::Store, v: &Value, what: &str) -> Result<f64> {
    super::number(store, v, "length", what)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateArchedParams {
    pub type_id: Option<ElementRef>,
    /// The footprint: four curves in one loop.
    pub curves: Vec<super::RoofCurveArg>,
    pub level_id: ElementRef,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base_offset: Option<Value>,
    /// The two opposite curves (keys) the arch springs from; default the longer pair.
    pub spring_edges: Option<[u32; 2]>,
    /// Default a quarter of the span.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub rise: Option<Value>,
    /// Or the radius of a segment at the first end.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub radius: Option<Value>,
    pub profile: Option<ArchProfile>,
    pub taper: Option<Taper>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub overhang: Option<Value>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub end_overhang: Option<Value>,
}

/// The spring edges' default: the longer pair of opposite edges.
fn default_spring(store: &crate::store::Store, e: &crate::element::Element) -> Option<[u32; 2]> {
    let curves = crate::sketch_edit::current_curves(store, e)?;
    if curves.len() != 4 {
        return None;
    }
    let l = |k: usize| {
        let c = &curves[k].1;
        (c.end[0] - c.start[0]).hypot(c.end[1] - c.start[1])
    };
    Some(if l(0) + l(2) >= l(1) + l(3) {
        [curves[0].0, curves[2].0]
    } else {
        [curves[1].0, curves[3].0]
    })
}

/// The span at an arched roof's first end, from its footprint.
fn first_span(
    store: &crate::store::Store,
    e: &crate::element::Element,
    spring: [u32; 2],
) -> Option<f64> {
    let curves = crate::sketch_edit::current_curves(store, e)?;
    let a = curves.iter().find(|c| c.0 == spring[0])?.1;
    let b = curves.iter().find(|c| c.0 == spring[1])?.1;
    // B runs the other way round the loop: its end is opposite A's start.
    Some((b.end[0] - a.start[0]).hypot(b.end[1] - a.start[1]))
}

/// The rise for a segment of `radius` over `span`.
fn rise_of_radius(radius: f64, span: f64) -> Result<f64> {
    if radius < span / 2.0 - 1e-9 {
        return Err(Error::new(
            "roof.bad_radius",
            format!(
                "the radius must be at least half the span ({:.3} m)",
                span / 2.0
            ),
        ));
    }
    Ok(radius - (radius * radius - span * span / 4.0).max(0.0).sqrt())
}

/// An arched roof over a four-sided footprint (`roof.create_arched`).
pub(crate) fn create_arched(
    txn: &mut crate::txn::Transaction<'_>,
    p: CreateArchedParams,
) -> Result<ElementId> {
    if p.curves.len() != 4 {
        return Err(Error::new(
            "roof.arch_needs_four_sides",
            "an arched roof's footprint has four sides",
        ));
    }
    let id = super::create(
        txn,
        super::CreateParams {
            type_id: p.type_id,
            curves: p.curves,
            level_id: p.level_id,
            base_offset: p.base_offset,
            slope: None,
            cutoff_level_id: None,
            cutoff_offset: None,
        },
    )?;
    set_arch(
        txn,
        SetArchParams {
            roof_id: ElementRef::Id(id),
            spring_edges: p.spring_edges,
            rise: p.rise,
            radius: p.radius,
            profile: p.profile,
            taper: p.taper,
            overhang: p.overhang,
            end_overhang: p.end_overhang,
        },
    )?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetArchParams {
    pub roof_id: ElementRef,
    pub spring_edges: Option<[u32; 2]>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub rise: Option<Value>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub radius: Option<Value>,
    pub profile: Option<ArchProfile>,
    pub taper: Option<Taper>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub overhang: Option<Value>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub end_overhang: Option<Value>,
}

/// Makes a footprint roof arched, or changes its arch (`roof.set_arch`).
pub(crate) fn set_arch(txn: &mut crate::txn::Transaction<'_>, p: SetArchParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut recipe) = super::resolve_roof(&store, &p.roof_id)?;
    crate::modify::check_unpinned(&store, id)?;
    let e = store.get(id).expect("resolved");
    if recipe.dome.is_some() {
        return Err(Error::new(
            "roof.is_dome",
            "a dome is changed with roof.set_dome",
        ));
    }
    let spring = match (p.spring_edges, &recipe.arch) {
        (Some(s), _) => s,
        (None, Some(a)) => a.spring,
        (None, None) => default_spring(&store, e).ok_or_else(|| {
            Error::new(
                "roof.arch_needs_four_sides",
                "an arched roof's footprint has four sides",
            )
        })?,
    };
    let span = first_span(&store, e, spring).unwrap_or(4.0);
    let rise = match (&p.rise, &p.radius) {
        (Some(v), _) => length(&store, v, "the rise")?,
        (None, Some(v)) => rise_of_radius(length(&store, v, "the radius")?, span)?,
        (None, None) => recipe.arch.as_ref().map_or(span / 4.0, |a| a.rise),
    };
    let old = recipe.arch.clone();
    let len = |v: &Option<Value>, what: &str, old: f64| -> Result<f64> {
        v.as_ref().map_or(Ok(old), |v| length(&store, v, what))
    };
    let arch = Arch {
        spring,
        rise,
        profile: p.profile.unwrap_or_else(|| {
            old.as_ref()
                .map_or_else(ArchProfile::default, |a| a.profile)
        }),
        taper: p
            .taper
            .unwrap_or_else(|| old.as_ref().map_or_else(Taper::default, |a| a.taper)),
        overhang: len(
            &p.overhang,
            "the overhang",
            old.as_ref().map_or(0.0, |a| a.overhang),
        )?,
        end_overhang: len(
            &p.end_overhang,
            "the end overhang",
            old.as_ref().map_or(0.0, |a| a.end_overhang),
        )?,
    };
    if arch.overhang < 0.0 || arch.end_overhang < 0.0 {
        return Err(Error::new(
            "roof.bad_overhang",
            "the overhang cannot be negative",
        ));
    }
    recipe.arch = Some(arch);
    super::write_recipe(txn, id, &recipe)?;
    super::check(txn, id)?;
    super::classify_into(txn, id)
}

/// A dome's plan as a command gives it.
/// Changes to a dome.
#[derive(Debug)]
pub struct DomeArgs {
    pub center: Option<[f64; 2]>,
    pub plan: Option<DomePlan>,
    pub rise: Option<Value>,
    pub profile: Option<DomeProfile>,
    pub ratio: Option<f64>,
    pub oculus: Option<Value>,
    pub faceted: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateDomeParams {
    pub type_id: Option<ElementRef>,
    pub level_id: ElementRef,
    /// The springing's height above the level.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base_offset: Option<Value>,
    pub center: [f64; 2],
    pub plan: DomePlan,
    /// Default the plan's radius (a hemisphere's).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub rise: Option<Value>,
    pub profile: Option<DomeProfile>,
    pub ratio: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub oculus: Option<Value>,
    #[serde(default)]
    pub faceted: bool,
}

fn apply_dome(store: &crate::store::Store, d: &mut Dome, a: DomeArgs) -> Result<()> {
    if let Some(c) = a.center {
        d.center = c;
    }
    if let Some(p) = a.plan {
        d.plan = p;
    }
    if let Some(p) = a.profile {
        d.profile = p;
    }
    if let Some(r) = a.ratio {
        d.ratio = Some(r);
    }
    if let Some(f) = a.faceted {
        d.faceted = f;
    }
    if let Some(v) = &a.oculus {
        d.oculus = length(store, v, "the oculus")?;
    }
    d.rise = match &a.rise {
        Some(v) => length(store, v, "the rise")?,
        None if d.profile == DomeProfile::Hemisphere || d.rise <= 0.0 => d.radius(),
        None => d.rise,
    };
    Ok(())
}

/// A dome roof (`roof.create_dome`).
pub(crate) fn create_dome(
    txn: &mut crate::txn::Transaction<'_>,
    p: CreateDomeParams,
) -> Result<ElementId> {
    let store = txn.store().clone();
    let level = super::resolve_level(&store, &p.level_id)?;
    let type_id = match &p.type_id {
        Some(r) => store
            .resolve(r)
            .filter(|id| store.get(*id).is_some_and(super::is_roof_type))
            .ok_or_else(|| Error::new("type.wrong_family", "not a roof type"))?,
        None => super::default_type(txn)?,
    };
    let mut d = Dome {
        center: p.center,
        plan: p.plan,
        rise: 0.0,
        profile: p.profile.unwrap_or_default(),
        ratio: None,
        oculus: 0.0,
        faceted: p.faceted,
    };
    apply_dome(
        &store,
        &mut d,
        DomeArgs {
            center: None,
            plan: None,
            rise: p.rise,
            profile: None,
            ratio: p.ratio,
            oculus: p.oculus,
            faceted: None,
        },
    )?;
    let recipe = super::RoofRecipe {
        kind: super::ROOF_RECIPE.into(),
        level,
        base_offset: p
            .base_offset
            .as_ref()
            .map(|v| length(&store, v, "the base offset"))
            .transpose()?
            .unwrap_or(0.0),
        slope: super::DEFAULT_SLOPE,
        edges: Default::default(),
        cutoff: None,
        arch: None,
        dome: Some(d),
        extrusion: None,
        joins: Default::default(),
        panes: Vec::new(),
        curved_glazing: None,
    };
    let type_guid = txn.store().get(type_id).expect("exists").guid().to_string();
    let id = txn.create(
        crate::element::NewElement::new(super::CLASS)
            .param("@PredefinedType", "DOME_ROOF")
            .param("@type", type_guid)
            .param(
                crate::walls::RECIPE_KEY,
                serde_json::to_value(&recipe).expect("serialises"),
            ),
    )?;
    super::check(txn, id)?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetDomeParams {
    pub roof_id: ElementRef,
    pub center: Option<[f64; 2]>,
    pub plan: Option<DomePlan>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub rise: Option<Value>,
    pub profile: Option<DomeProfile>,
    pub ratio: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub oculus: Option<Value>,
    pub faceted: Option<bool>,
}

/// Changes a dome (`roof.set_dome`).
pub(crate) fn set_dome(txn: &mut crate::txn::Transaction<'_>, p: SetDomeParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut recipe) = super::resolve_roof(&store, &p.roof_id)?;
    crate::modify::check_unpinned(&store, id)?;
    let Some(mut d) = recipe.dome.clone() else {
        return Err(Error::new("roof.not_dome", "the roof is not a dome"));
    };
    apply_dome(
        &store,
        &mut d,
        DomeArgs {
            center: p.center,
            plan: p.plan,
            rise: p.rise,
            profile: p.profile,
            ratio: p.ratio,
            oculus: p.oculus,
            faceted: p.faceted,
        },
    )?;
    recipe.dome = Some(d);
    super::write_recipe(txn, id, &recipe)?;
    super::check(txn, id)
}

#[cfg(test)]
mod tests {
    use super::super::Mesh;
    use super::*;

    fn shell_volume(m: &Mesh) -> f64 {
        let (p, idx) = m;
        idx.chunks(3)
            .map(|t| {
                let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
                (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                    + a[2] * (b[0] * c[1] - b[1] * c[0]))
                    / 6.0
            })
            .sum()
    }

    #[test]
    fn a_barrel_vault_and_a_tapering_one() {
        // 6 m span (y), 10 m long (x): spring edges along x.
        let ring = [[0.0, 0.0], [10.0, 0.0], [10.0, 6.0], [0.0, 6.0]];
        let a = Arch {
            spring: [1, 3],
            rise: 1.5,
            profile: ArchProfile::Segment,
            taper: Taper::ConstantRise,
            overhang: 0.0,
            end_overhang: 0.0,
        };
        let g = arch(&ring, &[1, 2, 3, 4], &a, 3.0, 0.2).unwrap();
        // The crown 1.5 m up all along.
        for x in [0.5, 5.0, 9.5] {
            assert!((g.underside([x, 3.0]).unwrap() - 4.5).abs() < 1e-3);
        }
        // Radius 3.75: at 1.5 m from the springing the height is √(3.75² − 1.5²) − 2.25.
        let z = (3.75f64 * 3.75 - 1.5 * 1.5).sqrt() - 2.25;
        assert!((g.underside([5.0, 1.5]).unwrap() - 3.0 - z).abs() < 0.01);
        let v = shell_volume(&g.faces[0].slab);
        assert!(v > 0.0);
        // About the mid-surface's arc length (radius 3.85) × thickness × length.
        let arc = 2.0 * 3.85 * (3.0f64 / 3.75).asin();
        assert!((v - arc * 0.2 * 10.0).abs() / v < 0.02, "{v}");

        // Narrowing from 6 m to 4 m along 8 m, constant rise.
        let ring = [[0.0, 0.0], [8.0, 1.0], [8.0, 5.0], [0.0, 6.0]];
        let g = arch(&ring, &[1, 2, 3, 4], &a, 0.0, 0.2).unwrap();
        assert!((g.underside([0.01, 3.0]).unwrap() - 1.5).abs() < 0.01);
        assert!((g.underside([7.99, 3.0]).unwrap() - 1.5).abs() < 0.01);
        // Constant slope at the springing: the narrow end rises 1.0 m.
        let b = Arch {
            taper: Taper::ConstantSlope,
            ..a.clone()
        };
        let g = arch(&ring, &[1, 2, 3, 4], &b, 0.0, 0.2).unwrap();
        assert!((g.underside([7.99, 3.0]).unwrap() - 1.0).abs() < 0.01);
    }

    #[test]
    fn domes() {
        let d = Dome {
            center: [0.0, 0.0],
            plan: DomePlan::Circle { radius: 4.0 },
            rise: 4.0,
            profile: DomeProfile::Hemisphere,
            ratio: None,
            oculus: 0.0,
            faceted: false,
        };
        let g = dome(&d, 10.0, 0.2).unwrap();
        assert!((g.underside([0.01, 0.0]).unwrap() - 14.0).abs() < 0.02);
        // The shell: 2/3 π (R₂³ − R₁³) within a couple of percent (faceted).
        let v = shell_volume(&g.faces[0].slab);
        let exact = 2.0 / 3.0 * std::f64::consts::PI * (4.2f64.powi(3) - 4.0f64.powi(3));
        assert!((v - exact).abs() / exact < 0.02, "{v} vs {exact}");
        // An oculus of 1 m: nothing within 0.5 m of the axis.
        let o = Dome {
            oculus: 1.0,
            ..d.clone()
        };
        let g = dome(&o, 10.0, 0.2).unwrap();
        assert!(g.underside([0.0, 0.0]).is_none());
        assert!(g.underside([0.6, 0.0]).is_some());
        // An octagonal cloister vault: straight hips.
        let p = Dome {
            plan: DomePlan::Polygon {
                radius: 4.0,
                sides: 8,
                rotation: 0.0,
            },
            ..d
        };
        let g = dome(&p, 0.0, 0.2).unwrap();
        // Along a mid-side the section is the profile over the apothem.
        let a = 4.0 * (std::f64::consts::PI / 8.0).cos();
        let mid = [
            a * (std::f64::consts::PI / 8.0).cos() * 0.5,
            a * (std::f64::consts::PI / 8.0).sin() * 0.5,
        ];
        assert!(g.underside(mid).is_some());
    }
}
