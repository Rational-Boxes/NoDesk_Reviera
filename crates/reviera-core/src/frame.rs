// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! A linear static 3D frame solver (ANA-FEA-001, the built-in structural analysis provider):
//! the direct stiffness method on Euler–Bernoulli members with six degrees of freedom per node.
//!
//! The model is the neutral structural model (`reviera.structural.v1.Model`, also exported as
//! `reviera-structural-model.json`): nodes with supports, members with their section properties,
//! moduli, end releases and the direction of their local z, load cases and combinations, and point
//! and line loads. Units are SI: metres, newtons, pascals.
//!
//! Released end degrees of freedom are condensed out of each member's stiffness and loads. A
//! degree of freedom nothing restrains (a mechanism, for example a member's rotation about its
//! own axis between two pins) is held, with a warning. Results per case, and per combination by
//! superposition: node displacements, support reactions, and member internal forces at stations.

// Matrix code reads best with explicit indices.
#![allow(clippy::needless_range_loop)]

use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::loads::Dof;

/// The neutral structural model.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Model {
    /// Always `SI` (metres, newtons, pascals).
    pub units: String,
    pub nodes: Vec<Node>,
    pub members: Vec<Member>,
    pub cases: Vec<Case>,
    pub combinations: Vec<Combination>,
    pub loads: Vec<Load>,
    /// Analytical panels (STR-AM-002), for providers that analyse surfaces; the built-in frame
    /// solver does not.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub panels: Vec<Panel>,
    /// What the distiller wants the results to say (for example area loads it could not carry).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// An analytical panel: its corner nodes, outline (metres; arcs sampled), openings, thickness,
/// moduli where its material is known, and how it is analysed. A cladding panel transfers loads
/// but must be left out of the stiffness matrix.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Panel {
    pub key: String,
    pub nodes: Vec<String>,
    pub outline: Vec<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub openings: Vec<Vec<[f64; 3]>>,
    pub thickness: f64,
    /// Young's modulus (Pa) and Poisson's ratio, when the material is in the library.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub e: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nu: Option<f64>,
    /// `shell`, `membrane` or `bending_plate`.
    pub analyze_as: String,
    /// On a cylinder (a curved panel).
    #[serde(default)]
    pub curved: bool,
    /// Stiffness excluded.
    #[serde(default)]
    pub cladding: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Node {
    pub key: String,
    pub xyz: [f64; 3],
    /// Translations x, y, z, rotations x, y, z.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub support: Option<[Dof; 6]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Member {
    pub key: String,
    pub nodes: [String; 2],
    /// Young's and shear moduli (Pa).
    pub e: f64,
    pub g: f64,
    /// Area (m²), second moments about local y and z (m⁴), torsion constant (m⁴).
    pub a: f64,
    pub iy: f64,
    pub iz: f64,
    pub j: f64,
    /// Each end's six fixities (true is fixed to the node).
    pub releases: [[bool; 6]; 2],
    /// The direction its local z leans toward (up for beams).
    pub up: [f64; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Case {
    pub key: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Combination {
    pub key: String,
    pub name: String,
    /// Case keys and factors.
    pub factors: Vec<(String, f64)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Load {
    /// At a node, or at `at` (0 to 1) along a member: Fx, Fy, Fz (N), Mx, My, Mz (N·m), global.
    Point {
        case: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        node: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        member: Option<String>,
        #[serde(default)]
        at: f64,
        forces: [f64; 6],
    },
    /// Along a member, varying linearly from `start` to `end` (N/m, global).
    Line {
        case: String,
        member: String,
        start: [f64; 3],
        end: [f64; 3],
        #[serde(default)]
        projected: bool,
        /// Carried from this area load (its key): a provider that analyses panels takes the area
        /// load itself and leaves these out.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from_area: Option<String>,
    },
    /// A uniform force per area (N/m², global) on a panel, for providers that analyse surfaces;
    /// the built-in solver takes the line loads carried from it instead.
    Area {
        key: String,
        case: String,
        panel: String,
        value: [f64; 3],
        #[serde(default)]
        projected: bool,
    },
}

/// The results.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Results {
    /// One per case, then one per combination.
    pub results: Vec<CaseResult>,
    pub warnings: Vec<String>,
    /// Free degrees of freedom solved.
    pub dof: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CaseResult {
    pub key: String,
    pub name: String,
    pub combination: bool,
    /// Node key and displacements (m) and rotations (rad), global.
    pub displacements: Vec<(String, [f64; 6])>,
    /// Supported node key and reactions (N, N·m), global.
    pub reactions: Vec<(String, [f64; 6])>,
    pub members: Vec<MemberResult>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemberResult {
    pub key: String,
    /// Internal forces along the member, local axes: N (tension positive), Vy, Vz, T, My, Mz.
    pub stations: Vec<Station>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Station {
    /// From the member's start (m).
    pub x: f64,
    pub forces: [f64; 6],
}

/// How far along a member internal forces are reported: this many intervals.
const INTERVALS: usize = 10;

type M3 = [[f64; 3]; 3];

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn unit(a: [f64; 3]) -> [f64; 3] {
    let l = dot(a, a).sqrt();
    [a[0] / l, a[1] / l, a[2] / l]
}

/// A member's length and rotation (rows: local x, y, z in global axes).
fn frame(a: [f64; 3], b: [f64; 3], up: [f64; 3]) -> (f64, M3) {
    let d = sub(b, a);
    let l = dot(d, d).sqrt();
    let x = unit(d);
    let mut z_hint = up;
    if dot(z_hint, z_hint) < 1e-12 || cross(x, z_hint).iter().all(|v| v.abs() < 1e-9) {
        z_hint = if x[2].abs() > 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 0.0, 1.0]
        };
    }
    let y = unit(cross(z_hint, x));
    let z = cross(x, y);
    (l, [x, y, z])
}

fn to_local(r: &M3, v: [f64; 3]) -> [f64; 3] {
    [dot(r[0], v), dot(r[1], v), dot(r[2], v)]
}

/// The local 12 × 12 stiffness of a member.
fn local_stiffness(m: &Member, l: f64) -> [[f64; 12]; 12] {
    let mut k = [[0.0; 12]; 12];
    let (ea, gj) = (m.e * m.a / l, m.g * m.j / l);
    let (eiz, eiy) = (m.e * m.iz, m.e * m.iy);
    let set = |k: &mut [[f64; 12]; 12], i: usize, j: usize, v: f64| {
        k[i][j] = v;
        k[j][i] = v;
    };
    set(&mut k, 0, 0, ea);
    set(&mut k, 6, 6, ea);
    set(&mut k, 0, 6, -ea);
    set(&mut k, 3, 3, gj);
    set(&mut k, 9, 9, gj);
    set(&mut k, 3, 9, -gj);
    let (l2, l3) = (l * l, l * l * l);
    // Bending in the local xy plane (v, θz), about local z.
    set(&mut k, 1, 1, 12.0 * eiz / l3);
    set(&mut k, 1, 5, 6.0 * eiz / l2);
    set(&mut k, 1, 7, -12.0 * eiz / l3);
    set(&mut k, 1, 11, 6.0 * eiz / l2);
    set(&mut k, 5, 5, 4.0 * eiz / l);
    set(&mut k, 5, 7, -6.0 * eiz / l2);
    set(&mut k, 5, 11, 2.0 * eiz / l);
    set(&mut k, 7, 7, 12.0 * eiz / l3);
    set(&mut k, 7, 11, -6.0 * eiz / l2);
    set(&mut k, 11, 11, 4.0 * eiz / l);
    // Bending in the local xz plane (w, θy), about local y.
    set(&mut k, 2, 2, 12.0 * eiy / l3);
    set(&mut k, 2, 4, -6.0 * eiy / l2);
    set(&mut k, 2, 8, -12.0 * eiy / l3);
    set(&mut k, 2, 10, -6.0 * eiy / l2);
    set(&mut k, 4, 4, 4.0 * eiy / l);
    set(&mut k, 4, 8, 6.0 * eiy / l2);
    set(&mut k, 4, 10, 2.0 * eiy / l);
    set(&mut k, 8, 8, 12.0 * eiy / l3);
    set(&mut k, 8, 10, 6.0 * eiy / l2);
    set(&mut k, 10, 10, 4.0 * eiy / l);
    k
}

/// A member's loads in local axes: point forces (position, force) and a linear distributed load.
#[derive(Debug, Clone, Default)]
struct MemberLoads {
    points: Vec<(f64, [f64; 3])>,
    q1: [f64; 3],
    q2: [f64; 3],
}

impl MemberLoads {
    /// The consistent nodal loads (local, 12) of these loads on a fixed-ended member.
    fn nodal(&self, l: f64) -> [f64; 12] {
        let mut q = [0.0; 12];
        for &(a, f) in &self.points {
            let b = l - a;
            q[0] += f[0] * b / l;
            q[6] += f[0] * a / l;
            let (s1, s2) = (
                b * b * (3.0 * a + b) / l.powi(3),
                a * a * (a + 3.0 * b) / l.powi(3),
            );
            let (m1, m2) = (a * b * b / (l * l), a * a * b / (l * l));
            q[1] += f[1] * s1;
            q[7] += f[1] * s2;
            q[5] += f[1] * m1;
            q[11] -= f[1] * m2;
            q[2] += f[2] * s1;
            q[8] += f[2] * s2;
            q[4] -= f[2] * m1;
            q[10] += f[2] * m2;
        }
        let (p1, p2) = (self.q1, self.q2);
        q[0] += l * (2.0 * p1[0] + p2[0]) / 6.0;
        q[6] += l * (p1[0] + 2.0 * p2[0]) / 6.0;
        let f1 = |k: usize| l * (7.0 * p1[k] + 3.0 * p2[k]) / 20.0;
        let f2 = |k: usize| l * (3.0 * p1[k] + 7.0 * p2[k]) / 20.0;
        let m1 = |k: usize| l * l * (3.0 * p1[k] + 2.0 * p2[k]) / 60.0;
        let m2 = |k: usize| l * l * (2.0 * p1[k] + 3.0 * p2[k]) / 60.0;
        q[1] += f1(1);
        q[7] += f2(1);
        q[5] += m1(1);
        q[11] -= m2(1);
        q[2] += f1(2);
        q[8] += f2(2);
        q[4] -= m1(2);
        q[10] += m2(2);
        q
    }
}

/// Condenses released local dofs out of `k` and `q`: the stiffness and loads the member's
/// fixed dofs see (released rows and columns zero).
fn condense(
    k: &[[f64; 12]; 12],
    q: &[f64; 12],
    released: &[usize],
) -> ([[f64; 12]; 12], [f64; 12]) {
    if released.is_empty() {
        return (*k, *q);
    }
    let r = released.len();
    // k_RR⁻¹ by Gauss–Jordan (small).
    let mut a: Vec<Vec<f64>> = released
        .iter()
        .map(|&i| released.iter().map(|&j| k[i][j]).collect())
        .collect();
    let mut inv: Vec<Vec<f64>> = (0..r)
        .map(|i| (0..r).map(|j| f64::from(u8::from(i == j))).collect())
        .collect();
    for c in 0..r {
        let p = (c..r)
            .max_by(|x, y| a[*x][c].abs().total_cmp(&a[*y][c].abs()))
            .unwrap_or(c);
        a.swap(c, p);
        inv.swap(c, p);
        let d = a[c][c];
        if d.abs() < 1e-30 {
            continue;
        }
        for j in 0..r {
            a[c][j] /= d;
            inv[c][j] /= d;
        }
        for i in 0..r {
            if i != c {
                let f = a[i][c];
                for j in 0..r {
                    a[i][j] -= f * a[c][j];
                    inv[i][j] -= f * inv[c][j];
                }
            }
        }
    }
    let mut kc = *k;
    let mut qc = *q;
    for i in 0..12 {
        for j in 0..12 {
            let mut s = 0.0;
            for (ri, &ra) in released.iter().enumerate() {
                for (rj, &rb) in released.iter().enumerate() {
                    s += k[i][ra] * inv[ri][rj] * k[rb][j];
                }
            }
            kc[i][j] -= s;
        }
        let mut s = 0.0;
        for (ri, &ra) in released.iter().enumerate() {
            for (rj, &rb) in released.iter().enumerate() {
                s += k[i][ra] * inv[ri][rj] * q[rb];
            }
        }
        qc[i] -= s;
    }
    for &i in released {
        for j in 0..12 {
            kc[i][j] = 0.0;
            kc[j][i] = 0.0;
        }
        qc[i] = 0.0;
    }
    (kc, qc)
}

fn transform(r: &M3) -> [[f64; 12]; 12] {
    let mut t = [[0.0; 12]; 12];
    for b in 0..4 {
        for i in 0..3 {
            for j in 0..3 {
                t[3 * b + i][3 * b + j] = r[i][j];
            }
        }
    }
    t
}

/// A member's prepared data.
struct Prepared {
    l: f64,
    r: M3,
    k: [[f64; 12]; 12],
    released: Vec<usize>,
    dofs: [usize; 12],
}

/// Solves the model: each case, then each combination.
pub fn solve(model: &Model) -> std::result::Result<Results, String> {
    let index: HashMap<&str, usize> = model
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.key.as_str(), i))
        .collect();
    let n = model.nodes.len() * 6;
    if n == 0 {
        return Err("the model has no nodes".into());
    }
    if n > 6000 {
        return Err(format!(
            "the built-in solver takes up to 1000 nodes; this model has {}",
            model.nodes.len()
        ));
    }
    let mut warnings = model.notes.clone();
    if !model.panels.is_empty() {
        warnings.push(format!(
            "{} analytical panel(s) are not analysed by the built-in frame solver: members only",
            model.panels.len()
        ));
    }
    let mut prepared: Vec<Prepared> = Vec::new();
    for m in &model.members {
        let (Some(&a), Some(&b)) = (
            index.get(m.nodes[0].as_str()),
            index.get(m.nodes[1].as_str()),
        ) else {
            return Err(format!(
                "member {} names a node that is not in the model",
                m.key
            ));
        };
        if m.e <= 0.0 || m.a <= 0.0 {
            return Err(format!("member {} has no stiffness (E or A)", m.key));
        }
        let (l, r) = frame(model.nodes[a].xyz, model.nodes[b].xyz, m.up);
        if l < 1e-6 {
            return Err(format!("member {} has no length", m.key));
        }
        let mut dofs = [0usize; 12];
        for k in 0..6 {
            dofs[k] = a * 6 + k;
            dofs[6 + k] = b * 6 + k;
        }
        let released: Vec<usize> = (0..12).filter(|&i| !m.releases[i / 6][i % 6]).collect();
        prepared.push(Prepared {
            l,
            r,
            k: local_stiffness(m, l),
            released,
            dofs,
        });
    }
    let member_index: HashMap<&str, usize> = model
        .members
        .iter()
        .enumerate()
        .map(|(i, m)| (m.key.as_str(), i))
        .collect();
    // Each member's loads per case, in local axes.
    let mut loads: HashMap<(usize, &str), MemberLoads> = HashMap::new();
    let mut node_loads: HashMap<&str, Vec<f64>> = HashMap::new();
    for load in &model.loads {
        match load {
            Load::Point {
                case,
                node,
                member,
                at,
                forces,
            } => {
                let node_key = node.clone().or_else(|| {
                    let mi = member_index.get(member.as_deref()?)?;
                    let m = &model.members[*mi];
                    if *at <= 1e-9 {
                        Some(m.nodes[0].clone())
                    } else if *at >= 1.0 - 1e-9 {
                        Some(m.nodes[1].clone())
                    } else {
                        None
                    }
                });
                if let Some(nk) = node_key {
                    let Some(&ni) = index.get(nk.as_str()) else {
                        continue;
                    };
                    let v = node_loads
                        .entry(case.as_str())
                        .or_insert_with(|| vec![0.0; n]);
                    for k in 0..6 {
                        v[ni * 6 + k] += forces[k];
                    }
                } else if let Some(&mi) = member.as_deref().and_then(|m| member_index.get(m)) {
                    let p = &prepared[mi];
                    if forces[3..].iter().any(|v| v.abs() > 0.0) {
                        warnings.push(format!(
                            "moments of a point load inside member {} are left out",
                            model.members[mi].key
                        ));
                    }
                    let f = to_local(&p.r, [forces[0], forces[1], forces[2]]);
                    loads
                        .entry((mi, case.as_str()))
                        .or_default()
                        .points
                        .push((at * p.l, f));
                }
            }
            Load::Area { .. } => {}
            Load::Line {
                case,
                member,
                start,
                end,
                projected,
                ..
            } => {
                let Some(&mi) = member_index.get(member.as_str()) else {
                    continue;
                };
                let p = &prepared[mi];
                let scale = |v: [f64; 3]| -> [f64; 3] {
                    if !projected {
                        return v;
                    }
                    let x = p.r[0];
                    let plan = (x[0] * x[0] + x[1] * x[1]).sqrt();
                    [v[0] * x[2].abs(), v[1] * x[2].abs(), v[2] * plan]
                };
                let e = loads.entry((mi, case.as_str())).or_default();
                let (a, b) = (to_local(&p.r, scale(*start)), to_local(&p.r, scale(*end)));
                for k in 0..3 {
                    e.q1[k] += a[k];
                    e.q2[k] += b[k];
                }
            }
        }
    }
    // Assemble the stiffness (shared by all cases).
    let mut kg = vec![0.0; n * n];
    for p in &prepared {
        let (kc, _) = condense(&p.k, &[0.0; 12], &p.released);
        let t = transform(&p.r);
        let kgl = mul_t_k_t(&t, &kc);
        for i in 0..12 {
            for j in 0..12 {
                kg[p.dofs[i] * n + p.dofs[j]] += kgl[i][j];
            }
        }
    }
    // Supports.
    let mut fixed = vec![false; n];
    for (i, node) in model.nodes.iter().enumerate() {
        if let Some(s) = &node.support {
            for k in 0..6 {
                match s[k] {
                    Dof::Fixed => fixed[i * 6 + k] = true,
                    Dof::Released => {}
                    Dof::Spring(v) => kg[(i * 6 + k) * n + i * 6 + k] += v,
                }
            }
        }
    }
    // Factor the free part (LDLᵀ, symmetric); unrestrained dofs are held.
    let free: Vec<usize> = (0..n).filter(|i| !fixed[*i]).collect();
    let nf = free.len();
    let mut a = vec![0.0; nf * nf];
    for (fi, &i) in free.iter().enumerate() {
        for (fj, &j) in free.iter().enumerate() {
            a[fi * nf + fj] = kg[i * n + j];
        }
    }
    let scale = (0..nf)
        .map(|i| a[i * nf + i].abs())
        .fold(0.0, f64::max)
        .max(1.0);
    let mut held = vec![false; nf];
    let mut mechanisms = 0usize;
    for c in 0..nf {
        let d = a[c * nf + c];
        if d.abs() <= 1e-10 * scale {
            held[c] = true;
            mechanisms += 1;
            for j in 0..nf {
                a[c * nf + j] = 0.0;
                a[j * nf + c] = 0.0;
            }
            a[c * nf + c] = 1.0;
            continue;
        }
        for i in c + 1..nf {
            let f = a[i * nf + c] / d;
            if f == 0.0 {
                continue;
            }
            for j in c..nf {
                a[i * nf + j] -= f * a[c * nf + j];
            }
            a[i * nf + c] = f;
        }
    }
    if mechanisms > 0 {
        warnings.push(format!(
            "{mechanisms} degrees of freedom nothing restrains were held (a mechanism, such as a member turning about its own axis between pins)"
        ));
    }
    let solve_free = |rhs: &[f64]| -> Vec<f64> {
        let mut y: Vec<f64> = free.iter().map(|&i| rhs[i]).collect();
        for (c, h) in held.iter().enumerate() {
            if *h {
                y[c] = 0.0;
            }
        }
        // Forward (unit lower), then the diagonal and back substitution.
        for i in 0..nf {
            let mut s = y[i];
            for k in 0..i {
                let l = if k < i { a[i * nf + k] } else { 0.0 };
                if !held[k] {
                    s -= l * y[k];
                }
            }
            y[i] = s;
        }
        for i in (0..nf).rev() {
            if held[i] {
                y[i] = 0.0;
                continue;
            }
            let mut s = y[i];
            for j in i + 1..nf {
                s -= a[i * nf + j] * y[j];
            }
            y[i] = s / a[i * nf + i];
        }
        let mut u = vec![0.0; n];
        for (fi, &i) in free.iter().enumerate() {
            u[i] = y[fi];
        }
        u
    };
    // Each case.
    let mut results = Vec::new();
    for case in &model.cases {
        let mut f = node_loads
            .get(case.key.as_str())
            .cloned()
            .unwrap_or_else(|| vec![0.0; n]);
        let mut member_q: Vec<[f64; 12]> = Vec::new();
        for (mi, p) in prepared.iter().enumerate() {
            let q = loads
                .get(&(mi, case.key.as_str()))
                .map(|ml| ml.nodal(p.l))
                .unwrap_or([0.0; 12]);
            member_q.push(q);
            let (_, qc) = condense(&p.k, &q, &p.released);
            let t = transform(&p.r);
            for i in 0..12 {
                let mut s = 0.0;
                for k in 0..12 {
                    s += t[k][i] * qc[k];
                }
                f[p.dofs[i]] += s;
            }
        }
        let u = solve_free(&f);
        // Reactions at supported nodes: K u − F at their dofs.
        let mut reactions = Vec::new();
        for (i, node) in model.nodes.iter().enumerate() {
            if node.support.is_none() {
                continue;
            }
            let mut r = [0.0; 6];
            for k in 0..6 {
                let row = i * 6 + k;
                let mut s = 0.0;
                for j in 0..n {
                    s += kg[row * n + j] * u[j];
                }
                r[k] = s - f[row];
            }
            reactions.push((node.key.clone(), r));
        }
        let members = prepared
            .iter()
            .enumerate()
            .map(|(mi, p)| {
                let ml = loads
                    .get(&(mi, case.key.as_str()))
                    .cloned()
                    .unwrap_or_default();
                MemberResult {
                    key: model.members[mi].key.clone(),
                    stations: stations(p, &u, &member_q[mi], &ml),
                }
            })
            .collect();
        results.push(CaseResult {
            key: case.key.clone(),
            name: case.name.clone(),
            combination: false,
            displacements: model
                .nodes
                .iter()
                .enumerate()
                .map(|(i, node)| (node.key.clone(), [0, 1, 2, 3, 4, 5].map(|k| u[i * 6 + k])))
                .collect(),
            reactions,
            members,
        });
    }
    // Combinations by superposition.
    for c in &model.combinations {
        let mut out = CaseResult {
            key: c.key.clone(),
            name: c.name.clone(),
            combination: true,
            ..Default::default()
        };
        let mut first = true;
        for (case, factor) in &c.factors {
            let Some(r) = results.iter().find(|r| !r.combination && r.key == *case) else {
                warnings.push(format!(
                    "combination {} names a case not in the model: {case}",
                    c.name
                ));
                continue;
            };
            if first {
                out.displacements = r
                    .displacements
                    .iter()
                    .map(|(k, _)| (k.clone(), [0.0; 6]))
                    .collect();
                out.reactions = r
                    .reactions
                    .iter()
                    .map(|(k, _)| (k.clone(), [0.0; 6]))
                    .collect();
                out.members = r
                    .members
                    .iter()
                    .map(|m| MemberResult {
                        key: m.key.clone(),
                        stations: m
                            .stations
                            .iter()
                            .map(|s| Station {
                                x: s.x,
                                forces: [0.0; 6],
                            })
                            .collect(),
                    })
                    .collect();
                first = false;
            }
            for (o, s) in out.displacements.iter_mut().zip(&r.displacements) {
                for k in 0..6 {
                    o.1[k] += factor * s.1[k];
                }
            }
            for (o, s) in out.reactions.iter_mut().zip(&r.reactions) {
                for k in 0..6 {
                    o.1[k] += factor * s.1[k];
                }
            }
            for (om, sm) in out.members.iter_mut().zip(&r.members) {
                for (o, s) in om.stations.iter_mut().zip(&sm.stations) {
                    for k in 0..6 {
                        o.forces[k] += factor * s.forces[k];
                    }
                }
            }
        }
        results.push(out);
    }
    Ok(Results {
        results,
        warnings,
        dof: nf,
    })
}

fn mul_t_k_t(t: &[[f64; 12]; 12], k: &[[f64; 12]; 12]) -> [[f64; 12]; 12] {
    let mut kt = [[0.0; 12]; 12];
    for i in 0..12 {
        for j in 0..12 {
            let mut s = 0.0;
            for m in 0..12 {
                s += k[i][m] * t[m][j];
            }
            kt[i][j] = s;
        }
    }
    let mut out = [[0.0; 12]; 12];
    for i in 0..12 {
        for j in 0..12 {
            let mut s = 0.0;
            for m in 0..12 {
                s += t[m][i] * kt[m][j];
            }
            out[i][j] = s;
        }
    }
    out
}

/// Internal forces at stations along a member from its end forces and loads.
fn stations(p: &Prepared, u: &[f64], q: &[f64; 12], ml: &MemberLoads) -> Vec<Station> {
    let t = transform(&p.r);
    let mut ul = [0.0; 12];
    for i in 0..12 {
        for k in 0..12 {
            ul[i] += t[i][k] * u[p.dofs[k]];
        }
    }
    // Released local displacements from the member's own equilibrium.
    if !p.released.is_empty() {
        let kept: Vec<usize> = (0..12).filter(|i| !p.released.contains(i)).collect();
        let r = &p.released;
        let n = r.len();
        let mut a: Vec<Vec<f64>> = r
            .iter()
            .map(|&i| r.iter().map(|&j| p.k[i][j]).collect())
            .collect();
        let mut b: Vec<f64> = r
            .iter()
            .map(|&i| q[i] - kept.iter().map(|&j| p.k[i][j] * ul[j]).sum::<f64>())
            .collect();
        for c in 0..n {
            let piv = (c..n)
                .max_by(|x, y| a[*x][c].abs().total_cmp(&a[*y][c].abs()))
                .unwrap_or(c);
            a.swap(c, piv);
            b.swap(c, piv);
            let d = a[c][c];
            if d.abs() < 1e-30 {
                continue;
            }
            for i in 0..n {
                if i != c {
                    let f = a[i][c] / d;
                    for j in 0..n {
                        a[i][j] -= f * a[c][j];
                    }
                    b[i] -= f * b[c];
                }
            }
        }
        for (c, &i) in r.iter().enumerate() {
            ul[i] = if a[c][c].abs() < 1e-30 {
                0.0
            } else {
                b[c] / a[c][c]
            };
        }
    }
    // End forces on the member: k u − q.
    let mut f1 = [0.0; 6];
    for (i, f) in f1.iter_mut().enumerate() {
        let mut s = -q[i];
        for k in 0..12 {
            s += p.k[i][k] * ul[k];
        }
        *f = s;
    }
    let l = p.l;
    (0..=INTERVALS)
        .map(|s| {
            let x = l * s as f64 / INTERVALS as f64;
            // Loads on [0, x]: their resultant and moment about the cut (lever s − x).
            let mut sum = [0.0; 3];
            let mut lever = [0.0; 3];
            for &(a, f) in &ml.points {
                if a <= x + 1e-12 {
                    for k in 0..3 {
                        sum[k] += f[k];
                        lever[k] += (a - x) * f[k];
                    }
                }
            }
            for k in 0..3 {
                let (q1, dq) = (ml.q1[k], ml.q2[k] - ml.q1[k]);
                sum[k] += q1 * x + dq * x * x / (2.0 * l);
                lever[k] += -q1 * x * x / 2.0 - dq * x.powi(3) / (6.0 * l);
            }
            let forces = [
                -(f1[0] + sum[0]),
                -(f1[1] + sum[1]),
                -(f1[2] + sum[2]),
                -f1[3],
                -(f1[4] + x * f1[2] - lever[2]),
                -(f1[5] - x * f1[1] + lever[1]),
            ];
            Station { x, forces }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beam(releases: [[bool; 6]; 2]) -> Member {
        Member {
            key: "m".into(),
            nodes: ["a".into(), "b".into()],
            e: 200e9,
            g: 77e9,
            a: 0.01,
            iy: 1e-4,
            iz: 2e-5,
            j: 1e-6,
            releases,
            up: [0.0, 0.0, 1.0],
        }
    }

    fn pinned() -> [Dof; 6] {
        [
            Dof::Fixed,
            Dof::Fixed,
            Dof::Fixed,
            Dof::Released,
            Dof::Released,
            Dof::Released,
        ]
    }

    #[test]
    fn simply_supported_beam_under_a_line_load() {
        // 6 m, 10 kN/m down: mid-span moment wL²/8 = 45 kN·m, deflection 5wL⁴/384EI.
        let model = Model {
            units: "SI".into(),
            panels: Vec::new(),
            notes: Vec::new(),
            nodes: vec![
                Node {
                    key: "a".into(),
                    xyz: [0.0, 0.0, 0.0],
                    support: Some(pinned()),
                },
                Node {
                    key: "b".into(),
                    xyz: [6.0, 0.0, 0.0],
                    support: Some(pinned()),
                },
            ],
            members: vec![beam([[true; 6]; 2])],
            cases: vec![Case {
                key: "c".into(),
                name: "D".into(),
            }],
            combinations: vec![Combination {
                key: "k".into(),
                name: "1.5D".into(),
                factors: vec![("c".into(), 1.5)],
            }],
            loads: vec![Load::Line {
                case: "c".into(),
                member: "m".into(),
                start: [0.0, 0.0, -10_000.0],
                end: [0.0, 0.0, -10_000.0],
                projected: false,
                from_area: None,
            }],
        };
        let r = solve(&model).unwrap();
        let st = &r.results[0].members[0].stations;
        let mid = st[5].forces[4];
        assert!((mid.abs() - 45_000.0).abs() < 45.0 * 0.5 * 10.0, "{mid}");
        // Reactions: 30 kN up at each support.
        for (_, re) in &r.results[0].reactions {
            assert!((re[2] - 30_000.0).abs() < 1.0, "{re:?}");
        }
        // Combination 1.5 × D.
        assert!((r.results[1].members[0].stations[5].forces[4] - 1.5 * mid).abs() < 1e-6);
        // Pinned supports leave the beam's own rotation free: held, with a warning.
        assert!(!r.warnings.is_empty());
    }

    #[test]
    fn cantilever_with_a_point_load_and_pinned_ends() {
        // A 3 m cantilever, 5 kN down at its tip: tip deflection PL³/3EI, root moment PL.
        let fixed = [Dof::Fixed; 6];
        let mut m = beam([[true; 6]; 2]);
        m.nodes = ["a".into(), "b".into()];
        let model = Model {
            units: "SI".into(),
            panels: Vec::new(),
            notes: Vec::new(),
            nodes: vec![
                Node {
                    key: "a".into(),
                    xyz: [0.0, 0.0, 0.0],
                    support: Some(fixed),
                },
                Node {
                    key: "b".into(),
                    xyz: [3.0, 0.0, 0.0],
                    support: None,
                },
            ],
            members: vec![m],
            cases: vec![Case {
                key: "c".into(),
                name: "P".into(),
            }],
            combinations: vec![],
            loads: vec![Load::Point {
                case: "c".into(),
                node: Some("b".into()),
                member: None,
                at: 0.0,
                forces: [0.0, 0.0, -5000.0, 0.0, 0.0, 0.0],
            }],
        };
        let r = solve(&model).unwrap();
        let tip = r.results[0].displacements[1].1[2];
        let expected = -5000.0 * 27.0 / (3.0 * 200e9 * 1e-4);
        assert!((tip - expected).abs() < 1e-9, "{tip} vs {expected}");
        let root = r.results[0].members[0].stations[0].forces[4];
        assert!((root.abs() - 15_000.0).abs() < 1e-6, "{root}");
        // A beam pinned at its start into a fixed support carries no moment there.
        let mut released = beam([[true, true, true, true, false, false], [true; 6]]);
        released.nodes = ["a".into(), "b".into()];
        let model2 = Model {
            nodes: vec![
                Node {
                    key: "a".into(),
                    xyz: [0.0, 0.0, 0.0],
                    support: Some(fixed),
                },
                Node {
                    key: "b".into(),
                    xyz: [6.0, 0.0, 0.0],
                    support: Some(pinned()),
                },
            ],
            members: vec![released],
            loads: vec![Load::Line {
                case: "c".into(),
                member: "m".into(),
                start: [0.0, 0.0, -10_000.0],
                end: [0.0, 0.0, -10_000.0],
                projected: false,
                from_area: None,
            }],
            ..model
        };
        let r2 = solve(&model2).unwrap();
        let st = &r2.results[0].members[0].stations;
        assert!(st[0].forces[4].abs() < 1e-3, "{}", st[0].forces[4]);
        assert!((st[5].forces[4].abs() - 45_000.0).abs() < 1.0);
    }
}
