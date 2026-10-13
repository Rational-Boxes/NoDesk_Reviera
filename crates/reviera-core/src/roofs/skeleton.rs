// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The weighted straight skeleton of a simple polygon (spec Ch 3 ARC-ROOF-001, step 4), by
//! wavefront propagation.
//!
//! Each edge of a counter-clockwise polygon moves inward at its own speed (metres per unit of
//! time; a gable moves at 0). At time `t` the edge lies on the line `n · p = c + w t`, where `n`
//! is its inward normal. Vertices travel where neighbouring edges meet. Events, in time order:
//! - an *edge event*: an edge shrinks to nothing and its neighbours meet;
//! - a *split event*: a reflex vertex reaches an opposite edge and the wavefront splits in two.
//!
//! Opposite edges that meet head-on (a rectangle's long sides, the two sides of a wing) leave a
//! ridge; collinear edges that meet are joined. Propagation ends when every wavefront has
//! collapsed, or at a cutoff time, where what remains is a flat top.
//!
//! The result is one face per edge: the region its wavefront swept, with each point's time as
//! its third coordinate. With time as height above the plate, the faces are the roof's planes.
//! The algorithm follows the published straight-skeleton literature (Aichholzer and Aurenhammer;
//! the weighted generalisation; Kelly and Wonka's procedural extrusions); this implementation is
//! Reviera's own.

/// Points closer than this (metres) are one point.
const TOL: f64 = 1e-7;
/// Events closer in time than this happen together.
const TOL_T: f64 = 1e-9;

/// An input edge: from `a` to `b` (the polygon is counter-clockwise), moving inward at `speed`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkelEdge {
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub speed: f64,
}

/// What lies across a face's boundary segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Across {
    /// The face of another input edge.
    Face(usize),
    /// Outside the polygon: the input edge itself.
    Base,
    /// The flat top left at the cutoff.
    Top,
}

/// A face's boundary: points `[x, y, t]` in order (counter-clockwise seen from above), and what
/// lies across the segment from each point to the next.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    pub points: Vec<[f64; 3]>,
    pub across: Vec<Across>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Skeleton {
    /// One face per input edge, in input order (empty for an edge that sweeps nothing).
    pub faces: Vec<Face>,
    /// Flat top polygons at the cutoff (counter-clockwise, at `t` = the cutoff), with the face
    /// across each edge.
    pub tops: Vec<Face>,
}

#[derive(Debug, Clone)]
struct Vert {
    /// The edge arriving at this vertex and the edge leaving it.
    left: usize,
    right: usize,
    /// Where it was at time `t0`, and how it moves.
    p0: [f64; 2],
    t0: f64,
    vel: [f64; 2],
    prev: usize,
    next: usize,
    alive: bool,
}

/// A skeleton segment from `a` to `b` between two faces (`usize::MAX` is the top).
#[derive(Debug, Clone, Copy)]
struct Arc {
    a: [f64; 3],
    b: [f64; 3],
    faces: [usize; 2],
}

const TOP: usize = usize::MAX;

struct Wave {
    lines: Vec<([f64; 2], f64, f64)>,
    verts: Vec<Vert>,
    arcs: Vec<Arc>,
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}
fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn len(a: [f64; 2]) -> f64 {
    a[0].hypot(a[1])
}

enum Meet {
    /// The lines cross: the vertex moves with velocity `v`.
    Moving { v: [f64; 2] },
    /// The lines are parallel, facing the same way.
    Collinear,
    /// The lines face each other.
    Opposite,
}

impl Wave {
    fn line(&self, e: usize, t: f64) -> ([f64; 2], f64) {
        let (n, c, w) = self.lines[e];
        (n, c + w * t)
    }

    /// Where edges `a` and `b` meet over time.
    fn meet(&self, a: usize, b: usize) -> Meet {
        let (na, _, wa) = self.lines[a];
        let (nb, _, wb) = self.lines[b];
        let det = cross(na, nb);
        if det.abs() < 1e-12 {
            return if dot(na, nb) > 0.0 {
                Meet::Collinear
            } else {
                Meet::Opposite
            };
        }
        // The velocity solves [na; nb] v = [wa; wb].
        let solve = |x: f64, y: f64| [(x * nb[1] - y * na[1]) / det, (na[0] * y - nb[0] * x) / det];
        Meet::Moving { v: solve(wa, wb) }
    }

    fn pos(&self, v: usize, t: f64) -> [f64; 2] {
        let x = &self.verts[v];
        [
            x.p0[0] + x.vel[0] * (t - x.t0),
            x.p0[1] + x.vel[1] * (t - x.t0),
        ]
    }

    /// A new vertex between `left` and `right` at `p` at time `t`.
    fn add(&mut self, left: usize, right: usize, p: [f64; 2], t: f64) -> usize {
        let vel = match self.meet(left, right) {
            Meet::Moving { v, .. } => v,
            // Resolved by `settle` before it moves.
            _ => [0.0, 0.0],
        };
        self.verts.push(Vert {
            left,
            right,
            p0: p,
            t0: t,
            vel,
            prev: usize::MAX,
            next: usize::MAX,
            alive: true,
        });
        self.verts.len() - 1
    }

    fn link(&mut self, a: usize, b: usize) {
        self.verts[a].next = b;
        self.verts[b].prev = a;
    }

    /// Ends a vertex at time `t`, recording its path.
    fn kill(&mut self, v: usize, t: f64) {
        let x = self.verts[v].clone();
        if !x.alive {
            return;
        }
        let p = self.pos(v, t);
        self.verts[v].alive = false;
        self.arc([x.p0[0], x.p0[1], x.t0], [p[0], p[1], t], [x.left, x.right]);
    }

    fn arc(&mut self, a: [f64; 3], b: [f64; 3], faces: [usize; 2]) {
        if len(sub([a[0], a[1]], [b[0], b[1]])) > TOL || (a[2] - b[2]).abs() > TOL {
            self.arcs.push(Arc { a, b, faces });
        }
    }

    fn cycle(&self, start: usize) -> Vec<usize> {
        let mut out = vec![start];
        let mut v = self.verts[start].next;
        while v != start && out.len() <= self.verts.len() {
            out.push(v);
            v = self.verts[v].next;
        }
        out
    }

    /// Resolves degenerate vertices around `v` at time `t`: edges that meet head-on leave a
    /// ridge and close; collinear edges join. Small cycles collapse.
    fn settle(&mut self, v: usize, t: f64) {
        let mut queue = vec![v];
        while let Some(v) = queue.pop() {
            if !self.verts[v].alive {
                continue;
            }
            let cyc = self.cycle(v);
            if cyc.len() <= 2 {
                // Two edges left facing each other: the ridge between the two vertices.
                if cyc.len() == 2 {
                    let (a, b) = (cyc[0], cyc[1]);
                    let (pa, pb) = (self.pos(a, t), self.pos(b, t));
                    let faces = [self.verts[a].left, self.verts[a].right];
                    self.kill(a, t);
                    self.kill(b, t);
                    self.arc([pa[0], pa[1], t], [pb[0], pb[1], t], faces);
                } else {
                    self.kill(v, t);
                }
                continue;
            }
            let x = self.verts[v].clone();
            match self.meet(x.left, x.right) {
                Meet::Moving { .. } => {}
                Meet::Collinear => {
                    // The edges are one line now: the vertex is not a corner any more. The
                    // next vertex continues the arriving edge.
                    let (p, n) = (x.prev, x.next);
                    let pv = self.pos(v, t);
                    let pn = self.pos(n, t);
                    self.kill(v, t);
                    let nx = self.verts[n].clone();
                    self.kill(n, t);
                    // The leaving edge's face is closed along the line, next to the arriving
                    // edge's face.
                    self.arc([pv[0], pv[1], t], [pn[0], pn[1], t], [x.left, x.right]);
                    let y = self.add(x.left, nx.right, pn, t);
                    self.link(p, y);
                    self.link(y, nx.next);
                    queue.push(y);
                }
                Meet::Opposite => {
                    let (lx, cx) = self.line(x.left, t);
                    let (_, cy) = self.line(x.right, t);
                    let _ = lx;
                    if (cx + cy).abs() > TOL {
                        // Facing edges that do not touch yet cannot share a vertex; leave it
                        // standing (bad input).
                        continue;
                    }
                    // The two edges lie on one line: the stretch between this vertex and the
                    // nearer neighbour is closed, and leaves a ridge.
                    let (p, n) = (x.prev, x.next);
                    let (px, pp, pn) = (self.pos(v, t), self.pos(p, t), self.pos(n, t));
                    let (dp, dn) = (len(sub(pp, px)), len(sub(pn, px)));
                    self.kill(v, t);
                    if (dp - dn).abs() <= TOL {
                        let (xp, xn) = (self.verts[p].clone(), self.verts[n].clone());
                        self.arc([px[0], px[1], t], [pp[0], pp[1], t], [x.left, x.right]);
                        self.kill(p, t);
                        self.kill(n, t);
                        if xp.prev == n {
                            continue;
                        }
                        let y = self.add(xp.left, xn.right, pp, t);
                        self.link(xp.prev, y);
                        self.link(y, xn.next);
                        queue.push(y);
                    } else if dp < dn {
                        let xp = self.verts[p].clone();
                        self.arc([px[0], px[1], t], [pp[0], pp[1], t], [x.left, x.right]);
                        self.kill(p, t);
                        let y = self.add(xp.left, x.right, pp, t);
                        self.link(xp.prev, y);
                        self.link(y, n);
                        queue.push(y);
                    } else {
                        let xn = self.verts[n].clone();
                        self.arc([px[0], px[1], t], [pn[0], pn[1], t], [x.left, x.right]);
                        self.kill(n, t);
                        let y = self.add(x.left, xn.right, pn, t);
                        self.link(p, y);
                        self.link(y, xn.next);
                        queue.push(y);
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Event {
    /// The edge leaving vertex `v` shrinks to nothing.
    Edge { v: usize },
    /// Reflex vertex `v` reaches the edge leaving vertex `u`, at point `p`.
    Split { v: usize, u: usize, p: [f64; 2] },
}

/// The skeleton of a counter-clockwise polygon whose edges move at the given speeds, stopping at
/// `cutoff` (a time) if given. Consecutive edges must meet (`a` of each is `b` of the one before).
pub fn skeleton(edges: &[SkelEdge], cutoff: Option<f64>) -> Result<Skeleton, String> {
    skeleton_loops(&[edges.len()], edges, cutoff)
}

/// The skeleton of a polygon with holes: `edges` holds the loops one after another, their
/// lengths in `loops`. The outer loop runs counter-clockwise and each hole clockwise, so every
/// edge moves into the polygon (a courtyard's edges move away from the courtyard). Faces come
/// out one per edge, in input order, as for [`skeleton`].
pub fn skeleton_loops(
    loops: &[usize],
    edges: &[SkelEdge],
    cutoff: Option<f64>,
) -> Result<Skeleton, String> {
    let n = edges.len();
    if n < 3 || loops.iter().any(|l| *l < 3) || loops.iter().sum::<usize>() != n {
        return Err("a roof footprint needs loops of at least three edges".into());
    }
    // The previous and next edge of each edge, within its loop.
    let mut prev = vec![0; n];
    let mut next = vec![0; n];
    let mut start = 0;
    for &len in loops {
        for k in 0..len {
            let i = start + k;
            prev[i] = start + (k + len - 1) % len;
            next[i] = start + (k + 1) % len;
        }
        start += len;
    }
    let mut wave = Wave {
        lines: Vec::with_capacity(n),
        verts: Vec::new(),
        arcs: Vec::new(),
    };
    for (i, e) in edges.iter().enumerate() {
        let d = sub(e.b, e.a);
        let l = len(d);
        if l < TOL {
            return Err(format!("edge {} has no length", i + 1));
        }
        if !(e.speed >= 0.0 && e.speed.is_finite()) {
            return Err(format!("edge {} has an invalid slope", i + 1));
        }
        let normal = [-d[1] / l, d[0] / l];
        wave.lines.push((normal, dot(normal, e.a), e.speed));
    }
    for (i, e) in edges.iter().enumerate() {
        wave.add(prev[i], i, e.a, 0.0);
    }
    for (i, &nx) in next.iter().enumerate() {
        wave.link(i, nx);
    }
    for i in 0..n {
        wave.settle(i, 0.0);
    }
    let mut now = 0.0f64;
    let mut steps = 0;
    loop {
        steps += 1;
        if steps > 50 * n + 1000 {
            return Err("the roof's skeleton does not converge".into());
        }
        let alive: Vec<usize> = (0..wave.verts.len())
            .filter(|v| wave.verts[*v].alive)
            .collect();
        if alive.is_empty() {
            break;
        }
        let mut best: Option<(f64, u8, usize, Event)> = None;
        let mut consider = |t: f64, rank: u8, key: usize, e: Event| {
            let better = match &best {
                None => true,
                Some((bt, br, bk, _)) => {
                    t < bt - TOL_T || (t <= bt + TOL_T && (rank, key) < (*br, *bk))
                }
            };
            if better {
                best = Some((t, rank, key, e));
            }
        };
        for &v in &alive {
            let nx = wave.verts[v].next;
            // Edge event: the vertex and the next one meet.
            let (pv, pn) = (wave.pos(v, now), wave.pos(nx, now));
            let d0 = sub(pn, pv);
            let dv = sub(wave.verts[nx].vel, wave.verts[v].vel);
            let dv2 = dot(dv, dv);
            if len(d0) <= TOL {
                consider(now, 0, v, Event::Edge { v });
            } else if dv2 > 1e-18 {
                let tau = -dot(d0, dv) / dv2;
                let gap = len([d0[0] + dv[0] * tau, d0[1] + dv[1] * tau]);
                if tau >= -TOL_T && gap <= TOL.max(1e-9 * len(d0)) * 10.0 {
                    consider(now + tau.max(0.0), 0, v, Event::Edge { v });
                }
            }
            // Split events of a reflex vertex.
            let x = &wave.verts[v];
            let (dl, dr) = (
                [wave.lines[x.left].0[1], -wave.lines[x.left].0[0]],
                [wave.lines[x.right].0[1], -wave.lines[x.right].0[0]],
            );
            if cross(dl, dr) >= -1e-12 {
                continue;
            }
            // Edges of the vertex's own wavefront, and of the others (a courtyard's wavefront
            // meets the outer one).
            let own = wave.cycle(v);
            for &u in &alive {
                let other = !own.contains(&u);
                let e = wave.verts[u].right;
                if e == x.left || e == x.right || u == v || wave.verts[u].next == v {
                    continue;
                }
                let (ne, ce) = wave.line(e, now);
                let dist = dot(ne, pv) - ce;
                let rate = dot(ne, x.vel) - wave.lines[e].2;
                if dist < -TOL || rate >= -1e-15 {
                    continue;
                }
                let tau = (-dist / rate).max(0.0);
                let t = now + tau;
                let h = [pv[0] + x.vel[0] * tau, pv[1] + x.vel[1] * tau];
                let (a, b) = (wave.pos(u, t), wave.pos(wave.verts[u].next, t));
                let ab = sub(b, a);
                let l2 = dot(ab, ab);
                if l2 < TOL * TOL {
                    continue;
                }
                let s = dot(sub(h, a), ab) / l2;
                let slack = TOL / l2.sqrt();
                // Another wavefront is joined where the vertex reaches it, ends included.
                let within = if other {
                    s > -slack && s < 1.0 + slack
                } else {
                    s > slack && s < 1.0 - slack
                };
                if within {
                    consider(t, 1, v * n + u, Event::Split { v, u, p: h });
                }
            }
        }
        let Some((t, _, _, event)) = best else {
            match cutoff {
                Some(_) => break,
                None => {
                    return Err(
                        "the roof does not close: some edges never meet (too many gables?)".into(),
                    );
                }
            }
        };
        if let Some(c) = cutoff
            && t >= c - TOL_T
        {
            now = c;
            break;
        }
        now = t.max(now);
        match event {
            Event::Edge { v } => {
                let nx = wave.verts[v].next;
                let cyc = wave.cycle(v);
                let p = wave.pos(v, now);
                if cyc.len() <= 3 {
                    // The wavefront collapses to a point.
                    for c in cyc {
                        let q = wave.pos(c, now);
                        let x = wave.verts[c].clone();
                        wave.verts[c].alive = false;
                        wave.arc(
                            [x.p0[0], x.p0[1], x.t0],
                            [q[0], q[1], now],
                            [x.left, x.right],
                        );
                        wave.arc([q[0], q[1], now], [p[0], p[1], now], [x.left, x.right]);
                    }
                    continue;
                }
                let (xv, xn) = (wave.verts[v].clone(), wave.verts[nx].clone());
                wave.kill(v, now);
                wave.kill(nx, now);
                let y = wave.add(xv.left, xn.right, p, now);
                wave.link(xv.prev, y);
                wave.link(y, xn.next);
                wave.settle(y, now);
            }
            Event::Split { v, u, p } => {
                let x = wave.verts[v].clone();
                let w = wave.verts[u].next;
                let e = wave.verts[u].right;
                wave.kill(v, now);
                let y1 = wave.add(x.left, e, p, now);
                let y2 = wave.add(e, x.right, p, now);
                wave.link(x.prev, y1);
                wave.link(y1, w);
                wave.link(u, y2);
                wave.link(y2, x.next);
                wave.settle(y1, now);
                wave.settle(y2, now);
            }
        }
    }
    // At the cutoff, what is left is the flat top.
    let mut tops = Vec::new();
    let remaining: Vec<usize> = (0..wave.verts.len())
        .filter(|v| wave.verts[*v].alive)
        .collect();
    let mut seen = std::collections::HashSet::new();
    for v in remaining {
        if seen.contains(&v) {
            continue;
        }
        let cyc = wave.cycle(v);
        let pts: Vec<[f64; 2]> = cyc.iter().map(|c| wave.pos(*c, now)).collect();
        for (i, c) in cyc.iter().enumerate() {
            seen.insert(*c);
            let e = wave.verts[*c].right;
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            wave.arc([a[0], a[1], now], [b[0], b[1], now], [e, TOP]);
        }
        for c in &cyc {
            wave.kill(*c, now);
        }
        if pts.len() >= 3 && crate::sketch::signed_area(&pts) > TOL {
            tops.push(Face {
                points: pts.iter().map(|p| [p[0], p[1], now]).collect(),
                across: cyc
                    .iter()
                    .map(|c| Across::Face(wave.verts[*c].right))
                    .collect(),
            });
        }
    }
    // Faces: the input edge, then the arcs around it.
    let mut faces = Vec::with_capacity(n);
    for (i, e) in edges.iter().enumerate() {
        let mut segs: Vec<([f64; 3], [f64; 3], Across)> = Vec::new();
        for a in &wave.arcs {
            if a.faces[0] == i || a.faces[1] == i {
                let other = if a.faces[0] == i {
                    a.faces[1]
                } else {
                    a.faces[0]
                };
                let across = match other {
                    TOP => Across::Top,
                    o if o == i => continue,
                    o => Across::Face(o),
                };
                segs.push((a.a, a.b, across));
            }
        }
        faces.push(chain(i, e, segs)?);
    }
    Ok(Skeleton { faces, tops })
}

/// Chains a face's boundary from its base edge through the arcs around it.
fn chain(
    i: usize,
    e: &SkelEdge,
    mut segs: Vec<([f64; 3], [f64; 3], Across)>,
) -> Result<Face, String> {
    let same = |a: [f64; 3], b: [f64; 3]| {
        (a[0] - b[0]).abs() <= 1e-6 && (a[1] - b[1]).abs() <= 1e-6 && (a[2] - b[2]).abs() <= 1e-6
    };
    let start = [e.a[0], e.a[1], 0.0];
    let mut points = vec![start];
    let mut across = vec![Across::Base];
    let mut cur = [e.b[0], e.b[1], 0.0];
    let limit = segs.len() + 2;
    let mut guard = 0;
    while !same(cur, start) {
        guard += 1;
        if guard > limit {
            return Err(format!("the roof face of edge {} does not close", i + 1));
        }
        let k = segs
            .iter()
            .position(|(a, b, _)| same(*a, cur) || same(*b, cur))
            .ok_or_else(|| format!("the roof face of edge {} does not close", i + 1))?;
        let (a, b, x) = segs.swap_remove(k);
        let next = if same(a, cur) { b } else { a };
        points.push(cur);
        across.push(x);
        cur = next;
    }
    Ok(Face { points, across })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face_area(f: &Face) -> f64 {
        let q = &f.points;
        (0..q.len())
            .map(|k| {
                let (a, b) = (q[k], q[(k + 1) % q.len()]);
                a[0] * b[1] - b[0] * a[1]
            })
            .sum::<f64>()
            .abs()
            / 2.0
    }

    #[test]
    fn a_courtyard_ring_has_its_ridge_halfway() {
        // A 10 m square (counter-clockwise) with a 4 m courtyard (clockwise) in the middle.
        let mut edges = poly(
            &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            &[1.0],
        );
        edges.extend(poly(
            &[[3.0, 3.0], [3.0, 7.0], [7.0, 7.0], [7.0, 3.0]],
            &[1.0],
        ));
        let s = skeleton_loops(&[4, 4], &edges, None).unwrap();
        assert_eq!(s.faces.len(), 8);
        // The ring is covered once: 100 − 16.
        let area: f64 = s.faces.iter().map(face_area).sum();
        assert!((area - 84.0).abs() < 1e-6, "{area}");
        // The ridge runs halfway across the 3 m ring, 1.5 up; eaves at 0 on both sides.
        for p in [[1.5, 5.0], [5.0, 1.5], [8.5, 5.0], [5.0, 8.5]] {
            assert!((height(&s, p).unwrap() - 1.5).abs() < 1e-6, "{p:?}");
        }
        for p in [[0.01, 5.0], [2.99, 5.0], [5.0, 7.01]] {
            assert!(height(&s, p).unwrap() < 0.02, "{p:?}");
        }
        // Nothing over the courtyard.
        assert!(height(&s, [5.0, 5.0]).is_none());
        // An off-centre courtyard: the narrow side's ridge is lower.
        let mut edges = poly(
            &[[0.0, 0.0], [12.0, 0.0], [12.0, 10.0], [0.0, 10.0]],
            &[1.0],
        );
        edges.extend(poly(
            &[[2.0, 3.0], [2.0, 7.0], [7.0, 7.0], [7.0, 3.0]],
            &[1.0],
        ));
        let s = skeleton_loops(&[4, 4], &edges, None).unwrap();
        let area: f64 = s.faces.iter().map(face_area).sum();
        assert!((area - 100.0).abs() < 1e-6, "{area}");
        assert!((height(&s, [1.0, 5.0]).unwrap() - 1.0).abs() < 1e-6);
        assert!((height(&s, [9.5, 5.0]).unwrap() - 2.5).abs() < 1e-6);
    }

    fn poly(pts: &[[f64; 2]], speeds: &[f64]) -> Vec<SkelEdge> {
        (0..pts.len())
            .map(|i| SkelEdge {
                a: pts[i],
                b: pts[(i + 1) % pts.len()],
                speed: speeds[i % speeds.len()],
            })
            .collect()
    }

    /// The height (time) of the skeleton surface at `p`, from the face containing it.
    fn height(s: &Skeleton, p: [f64; 2]) -> Option<f64> {
        for f in &s.faces {
            let ring: Vec<[f64; 2]> = f.points.iter().map(|q| [q[0], q[1]]).collect();
            let mut inside = false;
            for k in 0..ring.len() {
                let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
                if (a[1] > p[1]) != (b[1] > p[1])
                    && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
                {
                    inside = !inside;
                }
            }
            if inside {
                // The face's plane through three of its points.
                let q = &f.points;
                for k in 0..q.len() {
                    let (a, b, c) = (q[0], q[(k + 1) % q.len()], q[(k + 2) % q.len()]);
                    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                    let nz = u[0] * v[1] - u[1] * v[0];
                    if nz.abs() > 1e-9 {
                        let nx = u[1] * v[2] - u[2] * v[1];
                        let ny = u[2] * v[0] - u[0] * v[2];
                        return Some(a[2] - (nx * (p[0] - a[0]) + ny * (p[1] - a[1])) / nz);
                    }
                }
            }
        }
        None
    }

    fn area(f: &Face) -> f64 {
        let ring: Vec<[f64; 2]> = f.points.iter().map(|q| [q[0], q[1]]).collect();
        crate::sketch::signed_area(&ring)
    }

    #[test]
    fn a_rectangle_gives_a_hip_roof_with_a_ridge() {
        let s = skeleton(
            &poly(&[[0.0, 0.0], [10.0, 0.0], [10.0, 6.0], [0.0, 6.0]], &[1.0]),
            None,
        )
        .unwrap();
        assert_eq!(s.faces.len(), 4);
        // Two trapezoids and two triangles, covering the rectangle.
        assert_eq!(s.faces[0].points.len(), 4);
        assert_eq!(s.faces[1].points.len(), 3);
        let total: f64 = s.faces.iter().map(area).sum();
        assert!((total - 60.0).abs() < 1e-6, "{total}");
        assert!(s.faces.iter().all(|f| area(f) > 0.0));
        // The ridge is at height 3 from x = 3 to 7.
        assert!((height(&s, [5.0, 2.999]).unwrap() - 2.999).abs() < 1e-6);
        assert!((height(&s, [1.0, 3.0]).unwrap() - 1.0).abs() < 1e-6);
        assert!(
            s.faces[0]
                .points
                .iter()
                .any(|p| (p[0] - 3.0).abs() < 1e-6 && (p[2] - 3.0).abs() < 1e-6)
        );
    }

    #[test]
    fn gable_ends_do_not_move() {
        // The short ends are gables: two faces meet at a ridge along the full length.
        let s = skeleton(
            &poly(
                &[[0.0, 0.0], [10.0, 0.0], [10.0, 6.0], [0.0, 6.0]],
                &[1.0, 0.0, 1.0, 0.0],
            ),
            None,
        )
        .unwrap();
        assert!((area(&s.faces[0]) - 30.0).abs() < 1e-6);
        assert!((area(&s.faces[2]) - 30.0).abs() < 1e-6);
        assert!(area(&s.faces[1]).abs() < 1e-6, "a gable is vertical");
        assert!((height(&s, [0.5, 2.0]).unwrap() - 2.0).abs() < 1e-6);
        // The gable face rises to the ridge at the middle of the end.
        assert!(
            s.faces[1]
                .points
                .iter()
                .any(|p| (p[1] - 3.0).abs() < 1e-6 && (p[2] - 3.0).abs() < 1e-6)
        );
    }

    #[test]
    fn convex_footprints_match_the_lower_envelope() {
        // ARC-ROOF-001: for convex footprints the roof is the lowest of the edge planes.
        let pts = [
            [0.0, 0.0],
            [8.0, -1.0],
            [11.0, 4.0],
            [6.0, 9.0],
            [-1.0, 6.0],
        ];
        let speeds = [1.0, 0.5, 1.5, 1.0, 0.8];
        let edges = poly(&pts, &speeds);
        let s = skeleton(&edges, None).unwrap();
        let envelope = |p: [f64; 2]| {
            edges
                .iter()
                .map(|e| {
                    let d = sub(e.b, e.a);
                    let l = len(d);
                    let nrm = [-d[1] / l, d[0] / l];
                    dot(nrm, sub(p, e.a)) / e.speed
                })
                .fold(f64::INFINITY, f64::min)
        };
        for p in [
            [2.0, 1.0],
            [5.0, 4.0],
            [8.0, 3.0],
            [3.0, 6.0],
            [6.0, 7.5],
            [9.5, 3.5],
        ] {
            let h = height(&s, p).expect("inside");
            assert!(
                (h - envelope(p)).abs() < 1e-6,
                "{p:?}: {h} vs {}",
                envelope(p)
            );
        }
        let total: f64 = s.faces.iter().map(area).sum();
        assert!((total - crate::sketch::signed_area(&pts)).abs() < 1e-6);
    }

    #[test]
    fn a_thousand_convex_footprints_match_the_lower_envelope() {
        // ARC-ROOF-001 acceptance: random convex polygons with random slopes, checked at random
        // inside points within 0.01 mm.
        let mut seed = 777u64;
        let mut rnd = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as f64) / (1u64 << 31) as f64
        };
        for case in 0..1000 {
            let n = 3 + (rnd() * 9.0) as usize;
            // Points on an ellipse at increasing angles are convex.
            let (a, b) = (4.0 + 6.0 * rnd(), 3.0 + 5.0 * rnd());
            let mut angles: Vec<f64> = (0..n).map(|_| rnd() * std::f64::consts::TAU).collect();
            angles.sort_by(f64::total_cmp);
            angles.dedup_by(|x, y| (*x - *y).abs() < 0.05);
            if angles.len() < 3 {
                continue;
            }
            let pts: Vec<[f64; 2]> = angles.iter().map(|t| [a * t.cos(), b * t.sin()]).collect();
            if crate::sketch::signed_area(&pts) < 0.5 {
                continue;
            }
            let speeds: Vec<f64> = (0..pts.len()).map(|_| 0.3 + 2.0 * rnd()).collect();
            let edges = poly(&pts, &speeds);
            let s = skeleton(&edges, None).unwrap_or_else(|e| panic!("case {case}: {e}"));
            let envelope = |p: [f64; 2]| {
                edges
                    .iter()
                    .map(|e| {
                        let d = sub(e.b, e.a);
                        let l = len(d);
                        dot([-d[1] / l, d[0] / l], sub(p, e.a)) / e.speed
                    })
                    .fold(f64::INFINITY, f64::min)
            };
            for _ in 0..5 {
                // A random point inside: a convex combination of three corners.
                let (i, j, k) = (
                    (rnd() * pts.len() as f64) as usize % pts.len(),
                    (rnd() * pts.len() as f64) as usize % pts.len(),
                    (rnd() * pts.len() as f64) as usize % pts.len(),
                );
                let (u, v) = (rnd(), rnd());
                let (u, v) = if u + v > 1.0 {
                    (1.0 - u, 1.0 - v)
                } else {
                    (u, v)
                };
                let p = [
                    pts[i][0] + (pts[j][0] - pts[i][0]) * u + (pts[k][0] - pts[i][0]) * v,
                    pts[i][1] + (pts[j][1] - pts[i][1]) * u + (pts[k][1] - pts[i][1]) * v,
                ];
                if envelope(p) < 1e-3 {
                    continue;
                }
                let h = height(&s, p).unwrap_or_else(|| panic!("case {case}: {p:?} not covered"));
                assert!(
                    (h - envelope(p)).abs() < 1e-5 * (1.0 + h),
                    "case {case}: {h} vs {}",
                    envelope(p)
                );
            }
        }
    }

    #[test]
    fn an_l_shape_splits_and_has_a_valley() {
        // An L: the reflex corner at (4, 4) sends a valley inward.
        let pts = [
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 4.0],
            [4.0, 4.0],
            [4.0, 10.0],
            [0.0, 10.0],
        ];
        let s = skeleton(&poly(&pts, &[1.0]), None).unwrap();
        let total: f64 = s.faces.iter().map(area).sum();
        assert!((total - 64.0).abs() < 1e-6, "{total}");
        // Each wing is 4 wide: ridges at height 2.
        assert!((height(&s, [8.0, 2.0]).unwrap() - 2.0).abs() < 1e-6);
        assert!((height(&s, [2.0, 8.0]).unwrap() - 2.0).abs() < 1e-6);
        // Past the reflex corner, below the valley, the south face continues.
        assert!((height(&s, [3.5, 3.0]).unwrap() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_t_shape_and_a_plus_shape_close() {
        let t = [
            [0.0, 0.0],
            [12.0, 0.0],
            [12.0, 4.0],
            [8.0, 4.0],
            [8.0, 10.0],
            [4.0, 10.0],
            [4.0, 4.0],
            [0.0, 4.0],
        ];
        let s = skeleton(&poly(&t, &[1.0]), None).unwrap();
        let total: f64 = s.faces.iter().map(area).sum();
        assert!(
            (total - crate::sketch::signed_area(&t)).abs() < 1e-6,
            "{total}"
        );
        assert!((height(&s, [6.0, 8.0]).unwrap() - 2.0).abs() < 1e-6);
        let plus = [
            [4.0, 0.0],
            [8.0, 0.0],
            [8.0, 4.0],
            [12.0, 4.0],
            [12.0, 8.0],
            [8.0, 8.0],
            [8.0, 12.0],
            [4.0, 12.0],
            [4.0, 8.0],
            [0.0, 8.0],
            [0.0, 4.0],
            [4.0, 4.0],
        ];
        let s = skeleton(&poly(&plus, &[1.0]), None).unwrap();
        let total: f64 = s.faces.iter().map(area).sum();
        assert!((total - 80.0).abs() < 1e-6, "{total}");
        assert!((height(&s, [6.0, 6.0]).unwrap() - 2.0).abs() < 1e-6);
    }

    #[test]
    fn a_cutoff_leaves_a_flat_top() {
        let s = skeleton(
            &poly(&[[0.0, 0.0], [10.0, 0.0], [10.0, 6.0], [0.0, 6.0]], &[1.0]),
            Some(1.0),
        )
        .unwrap();
        assert_eq!(s.tops.len(), 1);
        assert!((area(&s.tops[0]) - 32.0).abs() < 1e-6);
        assert!(s.tops[0].points.iter().all(|p| (p[2] - 1.0).abs() < 1e-12));
        let sloped: f64 = s.faces.iter().map(area).sum();
        assert!((sloped - 28.0).abs() < 1e-6);
        assert!(s.faces[0].across.contains(&Across::Top));
    }

    #[test]
    fn different_slopes_shift_the_ridge() {
        // The south edge twice as fast (half as steep): the ridge sits nearer the north.
        let s = skeleton(
            &poly(
                &[[0.0, 0.0], [10.0, 0.0], [10.0, 6.0], [0.0, 6.0]],
                &[2.0, 0.0, 1.0, 0.0],
            ),
            None,
        )
        .unwrap();
        assert!((area(&s.faces[0]) - 40.0).abs() < 1e-6);
        assert!((area(&s.faces[2]) - 20.0).abs() < 1e-6);
        assert!((height(&s, [5.0, 4.0]).unwrap() - 2.0).abs() < 1e-6);
    }

    #[test]
    fn random_footprints_are_covered_exactly() {
        // Star-shaped polygons with random radii and slopes: the faces close and tile the
        // footprint, and the surface rises inward.
        let mut seed = 12345u64;
        let mut rnd = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as f64) / (1u64 << 31) as f64
        };
        let mut failures = Vec::new();
        for case in 0..300 {
            let n = 4 + (rnd() * 12.0) as usize;
            let pts: Vec<[f64; 2]> = (0..n)
                .map(|i| {
                    let a = (i as f64 + 0.15 + 0.7 * rnd()) / n as f64 * std::f64::consts::TAU;
                    let r = 3.0 + 7.0 * rnd();
                    [r * a.cos(), r * a.sin()]
                })
                .collect();
            let mut speeds: Vec<f64> = (0..n)
                .map(|_| {
                    if case % 3 == 0 {
                        1.0
                    } else {
                        0.4 + 1.6 * rnd()
                    }
                })
                .collect();
            if case % 4 == 1 {
                // A gable.
                let k = (rnd() * n as f64) as usize % n;
                speeds[k] = 0.0;
            }
            match skeleton(&poly(&pts, &speeds), None) {
                Ok(s) => {
                    let total: f64 = s.faces.iter().map(area).sum();
                    let want = crate::sketch::signed_area(&pts);
                    if (total - want).abs() > 1e-6 * want.max(1.0)
                        || s.faces.iter().flat_map(|f| &f.points).any(|p| p[2] < -1e-9)
                    {
                        failures.push(format!("case {case}: area {total} vs {want}"));
                    }
                }
                Err(e) => failures.push(format!("case {case}: {e}")),
            }
        }
        assert!(
            failures.is_empty(),
            "{} failures: {:?}",
            failures.len(),
            &failures[..failures.len().min(8)]
        );
    }

    #[test]
    fn a_shed_roof_rises_to_its_high_side() {
        let s = skeleton(
            &poly(
                &[[0.0, 0.0], [10.0, 0.0], [10.0, 6.0], [0.0, 6.0]],
                &[1.0, 0.0, 0.0, 0.0],
            ),
            None,
        )
        .unwrap();
        assert!((area(&s.faces[0]) - 60.0).abs() < 1e-6);
        assert!((height(&s, [3.0, 5.0]).unwrap() - 5.0).abs() < 1e-6);
    }
}
