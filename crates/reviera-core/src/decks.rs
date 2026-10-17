// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Decks (spec Ch 3 §3.7.8a, ARC-DECK-001): the outline, the boards laid out one by one, the
//! framing beneath, and a cut list.
//!
//! A deck is a floor (its outline, level and height are the floor's, so outline grips, Edit
//! Boundary, `sketch.move_*` and railings on its edges all work) that carries deck settings in
//! `rxd:deck`. Its body is its boards and framing instead of a slab.
//!
//! **Board layout** (deterministic):
//! 1. *Picture frame* boards, `n` rows around the outline, mitred at the corners. The field is
//!    the outline set in by the frame.
//! 2. *Courses* across the board direction at board width plus gap, from the start edge (the
//!    field's lowest edge across the boards). The last course is ripped to fit; *balance courses*
//!    centres the field so both edge courses are equal.
//! 3. Each course runs where its centre line is inside the field (cut-outs break it).
//! 4. Each run is split into pieces no longer than the longest stock length, every joint on a
//!    joist. Joints are *staggered*: no joint within the minimum stagger of a joint in the course
//!    before, and none on a joist used by the last few courses (the pattern: the longest piece
//!    that fits, `stair_step`, or a seeded `random` choice).
//! 5. *Stock*: pieces are cut from the shortest stock length that holds them, longest first,
//!    reusing offcuts long enough (first-fit decreasing).
//!
//! **Framing:** joists across the boards at the joist spacing (rim joists at both ends, doubled
//! under the frame's inner edge), beams across the joists at the beam spacing, posts on the
//! beams at the post spacing, and a pad footing under each post at the level.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::store::Store;

pub const DECK_KEY: &str = "rxd:deck";

/// A closed mesh: positions and triangles (outward).
pub type Mesh = (Vec<[f64; 3]>, Vec<u32>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Stagger {
    /// The longest piece that keeps the stagger rules.
    #[default]
    Longest,
    /// Joints step along by a joist per course.
    StairStep,
    /// A seeded random choice among the joists that keep the rules.
    Random,
}

/// A deck's board and framing settings (metres, radians).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DeckSettings {
    pub board_width: f64,
    pub board_thickness: f64,
    pub gap: f64,
    /// The boards' long direction; `None` along the outline's longest edge.
    pub direction: Option<f64>,
    pub stock_lengths: Vec<f64>,
    /// Rows of picture-frame boards around the outline.
    pub picture_frame: u32,
    pub balance_courses: bool,
    /// A last course narrower than this is flagged (default a third of the board width).
    pub min_rip: Option<f64>,
    pub stagger: Stagger,
    /// Joints in neighbouring courses at least this far apart (default two joist spaces).
    pub min_stagger: Option<f64>,
    /// No joint on a joist used by any of this many courses before.
    pub repeat_after: u32,
    pub seed: u64,
    pub joist_spacing: f64,
    /// Joist width and depth.
    pub joist: [f64; 2],
    pub beam_spacing: f64,
    pub beam: [f64; 2],
    pub post_spacing: f64,
    pub post: f64,
    /// Footing pad size and depth.
    pub footing: [f64; 2],
}

impl Default for DeckSettings {
    fn default() -> Self {
        Self {
            board_width: 0.14,
            board_thickness: 0.025,
            gap: 0.005,
            direction: None,
            stock_lengths: vec![3.6, 4.2, 4.8],
            picture_frame: 0,
            balance_courses: false,
            min_rip: None,
            stagger: Stagger::Longest,
            min_stagger: None,
            repeat_after: 3,
            seed: 1,
            joist_spacing: 0.4,
            joist: [0.045, 0.19],
            beam_spacing: 1.8,
            beam: [0.09, 0.19],
            post_spacing: 1.8,
            post: 0.09,
            footing: [0.4, 0.3],
        }
    }
}

pub fn deck_settings(e: &Element) -> Option<DeckSettings> {
    serde_json::from_value(e.param(DECK_KEY)?.clone()).ok()
}

/// One board.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Board {
    /// Numbered course by course from the start edge (frame boards first).
    pub key: u32,
    /// The course (field boards), or `None` for a picture-frame board.
    pub course: Option<u32>,
    /// Its outline in plan (counter-clockwise).
    pub outline: Vec<[f64; 2]>,
    /// Its length along the board direction (centre line).
    pub length: f64,
    /// Its width (a ripped last course is narrower).
    pub width: f64,
    /// The stock length it is cut from.
    pub stock: f64,
    /// Cut from the offcut of this board (its key), not a new length.
    pub from_offcut_of: Option<u32>,
}

/// The cut list (`deck.cut_list`).
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct CutList {
    pub boards: usize,
    /// Total board length laid, metres.
    pub board_length: f64,
    /// New lengths bought: (stock length, count).
    pub stock: Vec<(f64, u32)>,
    /// The deck's area, square metres.
    pub area: f64,
    pub joists: usize,
    pub joist_length: f64,
    pub beams: usize,
    pub beam_length: f64,
    pub posts: usize,
    pub footings: usize,
    /// Face screws: two per board per joist crossed.
    pub screws: usize,
    /// Warnings (a narrow ripped course, a joint that could not keep the stagger rules).
    pub warnings: Vec<String>,
}

/// A deck as it is now.
#[derive(Debug, Clone, PartialEq)]
pub struct DeckGeometry {
    pub top: f64,
    pub boards: Vec<Board>,
    /// Joist, beam and post lines in plan, for drawing.
    pub joists: Vec<[[f64; 2]; 2]>,
    pub beams: Vec<[[f64; 2]; 2]>,
    pub posts: Vec<[f64; 2]>,
    /// Every part as a solid: boards, joists, beams, posts, footings.
    pub parts: Vec<Mesh>,
    pub cut_list: CutList,
}

// ---- Plane helpers: a frame with u along the boards, v across them ----

#[derive(Debug, Clone, Copy)]
struct Frame {
    u: [f64; 2],
    v: [f64; 2],
}

impl Frame {
    fn new(angle: f64) -> Self {
        let (s, c) = angle.sin_cos();
        Frame {
            u: [c, s],
            v: [-s, c],
        }
    }
    fn to_local(self, p: [f64; 2]) -> [f64; 2] {
        [
            p[0] * self.u[0] + p[1] * self.u[1],
            p[0] * self.v[0] + p[1] * self.v[1],
        ]
    }
    fn to_world(self, q: [f64; 2]) -> [f64; 2] {
        [
            q[0] * self.u[0] + q[1] * self.v[0],
            q[0] * self.u[1] + q[1] * self.v[1],
        ]
    }
}

/// The intervals along u where the line v = `v` is inside the rings (even-odd), sorted.
fn scan(rings: &[Vec<[f64; 2]>], v: f64) -> Vec<(f64, f64)> {
    let mut xs = Vec::new();
    for r in rings {
        for i in 0..r.len() {
            let (a, b) = (r[i], r[(i + 1) % r.len()]);
            if (a[1] > v) != (b[1] > v) {
                xs.push(a[0] + (v - a[1]) * (b[0] - a[0]) / (b[1] - a[1]));
            }
        }
    }
    xs.sort_by(f64::total_cmp);
    xs.chunks(2)
        .filter(|c| c.len() == 2 && c[1] - c[0] > 1e-9)
        .map(|c| (c[0], c[1]))
        .collect()
}

/// A ring's lines moved `d` to the left of each edge (inward for a counter-clockwise ring), met
/// at the corners.
fn offset(ring: &[[f64; 2]], d: f64) -> Vec<[f64; 2]> {
    let n = ring.len();
    let line = |i: usize| {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        let e = [b[0] - a[0], b[1] - a[1]];
        let l = e[0].hypot(e[1]).max(1e-12);
        let m = [-e[1] / l * d, e[0] / l * d];
        ([a[0] + m[0], a[1] + m[1]], [e[0] / l, e[1] / l])
    };
    (0..n)
        .map(|i| {
            let (p, u) = line((i + n - 1) % n);
            let (q, w) = line(i);
            let den = u[0] * w[1] - u[1] * w[0];
            if den.abs() < 1e-12 {
                return q;
            }
            let r = [q[0] - p[0], q[1] - p[1]];
            let t = (r[0] * w[1] - r[1] * w[0]) / den;
            [p[0] + u[0] * t, p[1] + u[1] * t]
        })
        .collect()
}

fn signed_area(r: &[[f64; 2]]) -> f64 {
    (0..r.len())
        .map(|i| {
            let (a, b) = (r[i], r[(i + 1) % r.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        / 2.0
}

/// A closed prism of the polygon `poly` (counter-clockwise) from `z0` to `z1`.
fn prism(poly: &[[f64; 2]], z0: f64, z1: f64) -> Mesh {
    let mut ring = poly.to_vec();
    if signed_area(&ring) < 0.0 {
        ring.reverse();
    }
    let n = ring.len() as u32;
    let mut positions: Vec<[f64; 3]> = ring.iter().map(|p| [p[0], p[1], z0]).collect();
    positions.extend(ring.iter().map(|p| [p[0], p[1], z1]));
    let mut tris: Vec<u32> = Vec::new();
    earcut::Earcut::new().earcut(ring.iter().copied(), &[] as &[u32], &mut tris);
    let mut indices = Vec::new();
    for t in tris.chunks(3) {
        let (a, b, c) = (
            ring[t[0] as usize],
            ring[t[1] as usize],
            ring[t[2] as usize],
        );
        let ccw = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) > 0.0;
        let (a, b, c) = if ccw {
            (t[0], t[1], t[2])
        } else {
            (t[0], t[2], t[1])
        };
        indices.extend([a, c, b, n + a, n + b, n + c]);
    }
    for i in 0..n {
        let j = (i + 1) % n;
        indices.extend([i, j, n + j, i, n + j, n + i]);
    }
    (positions, indices)
}

/// A rectangle in the frame: u from `a` to `b`, v from `c` to `d`, in plan.
fn rect(f: &Frame, a: f64, b: f64, c: f64, d: f64) -> Vec<[f64; 2]> {
    [[a, c], [b, c], [b, d], [a, d]]
        .iter()
        .map(|q| f.to_world(*q))
        .collect()
}

/// A small seeded generator (deterministic layouts).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
}

/// Lays out a deck over `outer` (counter-clockwise) with `holes`, its top at `top`, on a level at
/// `ground`.
pub fn generate(
    outer: &[[f64; 2]],
    holes: &[Vec<[f64; 2]>],
    top: f64,
    ground: f64,
    s: &DeckSettings,
) -> Result<DeckGeometry> {
    if !(s.board_width > 0.01 && s.gap >= 0.0 && s.joist_spacing > 0.05) {
        return Err(Error::new(
            "deck.bad_settings",
            "the board width, gap and joist spacing must be positive",
        ));
    }
    let max_stock = s.stock_lengths.iter().copied().fold(0.0, f64::max);
    if max_stock < s.joist_spacing {
        return Err(Error::new(
            "deck.bad_settings",
            "the stock lengths must be longer than the joist spacing",
        ));
    }
    let mut outer = outer.to_vec();
    if signed_area(&outer) < 0.0 {
        outer.reverse();
    }
    // The board direction: given, or along the longest outline edge.
    let angle = s.direction.unwrap_or_else(|| {
        let n = outer.len();
        let (a, b) = (0..n)
            .map(|i| (outer[i], outer[(i + 1) % n]))
            .max_by(|x, y| {
                let l = |p: &([f64; 2], [f64; 2])| (p.1[0] - p.0[0]).hypot(p.1[1] - p.0[1]);
                l(x).total_cmp(&l(y))
            })
            .expect("a ring");
        (b[1] - a[1]).atan2(b[0] - a[0])
    });
    let f = Frame::new(angle);
    let pitch = s.board_width + s.gap;
    let (z0, z1) = (top - s.board_thickness, top);
    let mut boards: Vec<Board> = Vec::new();
    let mut warnings = Vec::new();

    // 1. Picture frame: row i lies between the outline set in by i·pitch and by i·pitch + width.
    let n_frame = s.picture_frame;
    for row in 0..n_frame {
        let out_ring = offset(&outer, f64::from(row) * pitch);
        let in_ring = offset(&outer, f64::from(row) * pitch + s.board_width);
        let n = outer.len();
        for i in 0..n {
            let j = (i + 1) % n;
            let poly = vec![out_ring[i], out_ring[j], in_ring[j], in_ring[i]];
            let mid = |a: [f64; 2], b: [f64; 2]| [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
            let (p, q) = (mid(out_ring[i], in_ring[i]), mid(out_ring[j], in_ring[j]));
            boards.push(Board {
                key: 0,
                course: None,
                outline: poly,
                length: (q[0] - p[0]).hypot(q[1] - p[1]),
                width: s.board_width,
                stock: 0.0,
                from_offcut_of: None,
            });
        }
    }
    let field = if n_frame > 0 {
        offset(&outer, f64::from(n_frame) * pitch)
    } else {
        outer.clone()
    };
    // The field, and its cut-outs, in the frame.
    let mut rings: Vec<Vec<[f64; 2]>> = vec![field.iter().map(|p| f.to_local(*p)).collect()];
    rings.extend(
        holes
            .iter()
            .map(|h| h.iter().map(|p| f.to_local(*p)).collect()),
    );
    let (u_min, u_max, v_min, v_max) = rings[0].iter().fold(
        (f64::MAX, f64::MIN, f64::MAX, f64::MIN),
        |(a, b, c, d), p| (a.min(p[0]), b.max(p[0]), c.min(p[1]), d.max(p[1])),
    );
    // Joists across the boards, from the field's start, with rims at both ends.
    let mut joist_us: Vec<f64> = Vec::new();
    let mut u = u_min + s.joist[0] / 2.0;
    while u < u_max - s.joist[0] / 2.0 - 1e-9 {
        joist_us.push(u);
        u += s.joist_spacing;
    }
    joist_us.push(u_max - s.joist[0] / 2.0);
    joist_us.dedup_by(|a, b| (*a - *b).abs() < s.joist[0]);

    // 2. Courses.
    let span = v_max - v_min;
    let count = ((span + s.gap) / pitch).ceil().max(1.0) as u32;
    let v0 = if s.balance_courses && count > 1 {
        // Equal ripped courses at both edges.
        let used = f64::from(count) * pitch - s.gap;
        v_min - (used - span) / 2.0
    } else {
        v_min
    };
    let min_rip = s.min_rip.unwrap_or(s.board_width / 3.0);
    let min_stagger = s.min_stagger.unwrap_or(2.0 * s.joist_spacing);
    let mut rng = Lcg(s.seed);
    let mut history: Vec<Vec<f64>> = Vec::new();
    for k in 0..count {
        let lo = (v0 + f64::from(k) * pitch).max(v_min);
        let hi = (v0 + f64::from(k) * pitch + s.board_width).min(v_max);
        let width = hi - lo;
        if width < 1e-6 {
            continue;
        }
        if width < s.board_width - 1e-9 && width < min_rip {
            warnings.push(format!(
                "Course {} is ripped to {:.0} mm, below the minimum of {:.0} mm",
                k + 1,
                width * 1000.0,
                min_rip * 1000.0
            ));
        }
        // 3. Where the course's centre line is inside the field.
        let mut joints = Vec::new();
        for (a, b) in scan(&rings, (lo + hi) / 2.0) {
            // 4. Pieces no longer than the longest stock, joints on joists, staggered.
            let mut start = a;
            while b - start > max_stock + 1e-9 {
                let limit = start + max_stock;
                let candidates: Vec<f64> = joist_us
                    .iter()
                    .copied()
                    .filter(|j| *j > start + s.joist_spacing / 2.0 && *j <= limit + 1e-9)
                    .collect();
                let ok = |j: &f64| {
                    let near_last = history.last().is_some_and(|prev| {
                        prev.iter().any(|p| (p - j).abs() < min_stagger - 1e-9)
                    });
                    let reused = history
                        .iter()
                        .rev()
                        .take(s.repeat_after as usize)
                        .any(|prev| prev.iter().any(|p| (p - j).abs() < 1e-9));
                    !near_last && !reused
                };
                let valid: Vec<f64> = candidates.iter().copied().filter(ok).collect();
                let cut = match (valid.is_empty(), s.stagger) {
                    (true, _) => {
                        warnings.push(format!(
                            "Course {}: a joint could not keep the stagger rules",
                            k + 1
                        ));
                        candidates.last().copied()
                    }
                    (false, Stagger::Random) => {
                        Some(valid[(rng.next() % valid.len() as u64) as usize])
                    }
                    (false, Stagger::StairStep) => {
                        // Step back one joist per course from the longest piece.
                        let back = (k as usize) % valid.len().max(1);
                        Some(valid[valid.len() - 1 - back])
                    }
                    (false, Stagger::Longest) => valid.last().copied(),
                };
                let Some(cut) = cut else {
                    return Err(Error::new(
                        "deck.no_joist",
                        "no joist to joint boards on: the joist spacing is wider than the stock",
                    ));
                };
                boards.push(Board {
                    key: 0,
                    course: Some(k),
                    outline: rect(&f, start, cut, lo, hi),
                    length: cut - start,
                    width,
                    stock: 0.0,
                    from_offcut_of: None,
                });
                joints.push(cut);
                start = cut;
            }
            boards.push(Board {
                key: 0,
                course: Some(k),
                outline: rect(&f, start, b, lo, hi),
                length: b - start,
                width,
                stock: 0.0,
                from_offcut_of: None,
            });
        }
        history.push(joints);
    }
    // Number the boards in order.
    for (i, b) in boards.iter_mut().enumerate() {
        b.key = i as u32 + 1;
    }
    // 5. Stock: longest pieces first, from offcuts where they fit, else the shortest stock.
    let mut order: Vec<usize> = (0..boards.len()).collect();
    order.sort_by(|a, b| boards[*b].length.total_cmp(&boards[*a].length));
    let mut stocks: Vec<f64> = s.stock_lengths.clone();
    stocks.sort_by(f64::total_cmp);
    let mut offcuts: Vec<(f64, u32)> = Vec::new();
    let mut bought: Vec<(f64, u32)> = Vec::new();
    for i in order {
        let l = boards[i].length;
        if let Some(k) = offcuts
            .iter()
            .enumerate()
            .filter(|(_, o)| o.0 >= l - 1e-9)
            .min_by(|a, b| a.1.0.total_cmp(&b.1.0))
            .map(|(k, _)| k)
        {
            let (len, from) = offcuts.remove(k);
            boards[i].stock = len;
            boards[i].from_offcut_of = Some(from);
            if len - l > s.joist_spacing {
                offcuts.push((len - l, boards[i].key));
            }
            continue;
        }
        let stock = stocks
            .iter()
            .copied()
            .find(|st| *st >= l - 1e-9)
            .unwrap_or(max_stock);
        boards[i].stock = stock;
        match bought.iter_mut().find(|b| (b.0 - stock).abs() < 1e-9) {
            Some(b) => b.1 += 1,
            None => bought.push((stock, 1)),
        }
        if stock - l > s.joist_spacing {
            offcuts.push((stock - l, boards[i].key));
        }
    }
    bought.sort_by(|a, b| a.0.total_cmp(&b.0));

    // Framing.
    let joist_top = z0;
    let joist_bottom = joist_top - s.joist[1];
    let mut joists = Vec::new();
    let mut parts: Vec<Mesh> = boards.iter().map(|b| prism(&b.outline, z0, z1)).collect();
    let mut joist_length = 0.0;
    let all_rings: Vec<Vec<[f64; 2]>> = {
        let mut r = vec![outer.iter().map(|p| f.to_local(*p)).collect::<Vec<_>>()];
        r.extend(rings.iter().skip(1).cloned());
        r
    };
    // Doubled joists under the frame's inner edge sit just inside the field's ends.
    let mut joist_lines: Vec<f64> = joist_us.clone();
    if n_frame > 0 {
        joist_lines.push(u_min + s.joist[0] * 1.5);
        joist_lines.push(u_max - s.joist[0] * 1.5);
    }
    for ju in &joist_lines {
        for (a, b) in scan_across(&all_rings, *ju) {
            joists.push([f.to_world([*ju, a]), f.to_world([*ju, b])]);
            parts.push(prism(
                &rect(&f, ju - s.joist[0] / 2.0, ju + s.joist[0] / 2.0, a, b),
                joist_bottom,
                joist_top,
            ));
            joist_length += b - a;
        }
    }
    // Beams across the joists (along u), posts on them, footings under the posts.
    let mut beams = Vec::new();
    let mut posts = Vec::new();
    let mut beam_length = 0.0;
    let (bv_min, bv_max) = all_rings[0]
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p[1]), b.max(p[1])));
    let mut bv = bv_min + s.beam[0];
    let mut beam_vs = Vec::new();
    while bv < bv_max - s.beam[0] / 2.0 {
        beam_vs.push(bv);
        bv += s.beam_spacing;
    }
    beam_vs.push(bv_max - s.beam[0]);
    beam_vs.dedup_by(|a, b| (*a - *b).abs() < s.beam[0] * 2.0);
    let beam_top = joist_bottom;
    let beam_bottom = beam_top - s.beam[1];
    for v in &beam_vs {
        for (a, b) in scan(&all_rings, *v) {
            beams.push([f.to_world([a, *v]), f.to_world([b, *v])]);
            parts.push(prism(
                &rect(&f, a, b, v - s.beam[0] / 2.0, v + s.beam[0] / 2.0),
                beam_bottom,
                beam_top,
            ));
            beam_length += b - a;
            let n = ((b - a) / s.post_spacing).ceil().max(1.0) as u32;
            for k in 0..=n {
                let pu = a + s.post / 2.0 + (b - a - s.post) * f64::from(k) / f64::from(n);
                let p = f.to_world([pu, *v]);
                posts.push(p);
                let h = s.post / 2.0;
                parts.push(prism(
                    &rect(&f, pu - h, pu + h, v - h, v + h),
                    ground,
                    beam_bottom,
                ));
                let fh = s.footing[0] / 2.0;
                parts.push(prism(
                    &rect(&f, pu - fh, pu + fh, v - fh, v + fh),
                    ground - s.footing[1],
                    ground,
                ));
            }
        }
    }
    if beam_bottom <= ground + 0.05 {
        warnings.push("The deck is too low for its framing: the beams reach the ground".to_owned());
    }
    let board_length: f64 = boards.iter().map(|b| b.length).sum();
    // Two screws per board per joist it crosses.
    let screws: usize = boards
        .iter()
        .map(|b| {
            let c = f.to_local(b.outline[0]);
            let d = f.to_local(b.outline[2]);
            let (a, e) = (c[0].min(d[0]), c[0].max(d[0]));
            2 * joist_lines
                .iter()
                .filter(|j| **j >= a - 1e-9 && **j <= e + 1e-9)
                .count()
        })
        .sum();
    let area = signed_area(&outer) - holes.iter().map(|h| signed_area(h).abs()).sum::<f64>();
    let cut_list = CutList {
        boards: boards.len(),
        board_length,
        stock: bought,
        area,
        joists: joists.len(),
        joist_length,
        beams: beams.len(),
        beam_length,
        posts: posts.len(),
        footings: posts.len(),
        screws,
        warnings,
    };
    Ok(DeckGeometry {
        top,
        boards,
        joists,
        beams,
        posts,
        parts,
        cut_list,
    })
}

/// The intervals along v where the line u = `u` is inside the rings.
fn scan_across(rings: &[Vec<[f64; 2]>], u: f64) -> Vec<(f64, f64)> {
    let swapped: Vec<Vec<[f64; 2]>> = rings
        .iter()
        .map(|r| r.iter().map(|p| [p[1], p[0]]).collect())
        .collect();
    scan(&swapped, u)
}

/// Regenerates a deck from its floor and settings.
pub fn regenerate(store: &Store, e: &Element) -> Result<DeckGeometry> {
    let s = deck_settings(e).ok_or_else(|| Error::new("deck.not_a_deck", "not a deck"))?;
    let g = crate::floors::regenerate(store, e)?;
    let (_, recipe) = crate::floors::slab_recipe(e).expect("a floor");
    let ground = crate::id::GlobalId::parse(&recipe.level)
        .and_then(|l| store.get(store.find_by_guid(&l)?))
        .map(crate::levels::elevation)
        .unwrap_or(0.0);
    let p = g
        .profiles
        .first()
        .ok_or_else(|| Error::new("deck.no_outline", "the deck has no outline"))?;
    generate(&p.outer, &p.holes, g.top, ground, &s)
}

/// Every deck that regenerates.
pub fn all(store: &Store) -> Vec<(ElementId, DeckGeometry)> {
    store
        .iter()
        .filter(|e| deck_settings(e).is_some())
        .filter_map(|e| Some((e.id(), regenerate(store, e).ok()?)))
        .collect()
}

// ---- Commands ----

fn number(store: &Store, v: &Value, data_type: &str, what: &str) -> Result<f64> {
    match v {
        Value::Number(n) => Ok(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => crate::units::parse(s, data_type, &crate::units::settings_of(store)),
        _ => Err(Error::new(
            "deck.invalid_value",
            format!("{what} must be a number or text with units"),
        )),
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// The outline (and cut-outs as inner loops), as for floors.
    pub outline: Vec<Vec<crate::sketch::CurveArg>>,
    pub level_id: ElementRef,
    /// The boards' top above the level (default 600 mm).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub height: Option<Value>,
    /// Board and framing settings (any omitted take their defaults).
    #[serde(default)]
    pub settings: Option<DeckSettings>,
}

pub(crate) fn create(txn: &mut crate::txn::Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let height = p
        .height
        .as_ref()
        .map(|v| number(&store, v, "length", "the height"))
        .transpose()?
        .unwrap_or(0.6);
    let id = crate::floors::create(
        txn,
        crate::floors::CreateParams {
            type_id: None,
            loops: p.outline,
            level_id: p.level_id,
            offset: Some(height),
            structural: false,
            span_direction_edge: None,
        },
    )?;
    let settings = p.settings.unwrap_or_default();
    let value = serde_json::to_value(&settings).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(DECK_KEY.into(), value);
        e.params.insert("@ObjectType".into(), "DECK".into());
        e.name = Some("Deck".into());
    })?;
    regenerate(txn.store(), txn.store().get(id).expect("created"))?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub deck_id: ElementRef,
    /// The settings to change, as in `deck.create` (omitted fields are kept).
    pub settings: Value,
}

/// Changes a deck's board and framing settings (`deck.set_boards`, `deck.set_framing`).
pub(crate) fn set(txn: &mut crate::txn::Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, current) = resolve(&store, &p.deck_id)?;
    let mut merged = serde_json::to_value(&current).expect("serialises");
    let Value::Object(changes) = p.settings else {
        return Err(Error::new(
            "deck.invalid_value",
            "settings must be an object of the fields to change",
        ));
    };
    for (k, v) in changes {
        merged[k] = v;
    }
    let settings: DeckSettings = serde_json::from_value(merged)
        .map_err(|e| Error::new("deck.invalid_value", e.to_string()))?;
    let value = serde_json::to_value(&settings).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(DECK_KEY.into(), value);
    })?;
    regenerate(txn.store(), txn.store().get(id).expect("exists")).map(|_| ())
}

fn resolve(store: &Store, r: &ElementRef) -> Result<(ElementId, DeckSettings)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such deck"))?;
    let s = deck_settings(store.get(id).expect("resolved"))
        .ok_or_else(|| Error::new("deck.not_a_deck", format!("element {id} is not a deck")))?;
    Ok((id, s))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryParams {
    pub deck_id: ElementRef,
}

/// The boards of a deck (`deck.boards`).
pub(crate) fn boards(store: &Store, p: QueryParams) -> Result<Vec<Board>> {
    let (id, _) = resolve(store, &p.deck_id)?;
    Ok(regenerate(store, store.get(id).expect("resolved"))?.boards)
}

/// The cut list of a deck (`deck.cut_list`).
pub(crate) fn cut_list(store: &Store, p: QueryParams) -> Result<CutList> {
    let (id, _) = resolve(store, &p.deck_id)?;
    Ok(regenerate(store, store.get(id).expect("resolved"))?.cut_list)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect4(w: f64, h: f64) -> Vec<[f64; 2]> {
        vec![[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]]
    }

    #[test]
    fn the_specified_deck() {
        // ARC-DECK-001: 4 × 6 m, 140 mm boards with a 5 mm gap along the 6 m side: 28 courses,
        // the last ripped; joints on joists; no adjacent joints within 800 mm.
        let s = DeckSettings {
            direction: Some(std::f64::consts::FRAC_PI_2),
            ..DeckSettings::default()
        };
        let g = generate(&rect4(4.0, 6.0), &[], 0.6, 0.0, &s).unwrap();
        let courses: std::collections::BTreeSet<u32> =
            g.boards.iter().filter_map(|b| b.course).collect();
        assert_eq!(courses.len(), 28);
        let last = g.boards.iter().find(|b| b.course == Some(27)).unwrap();
        assert!(last.width < 0.14 && last.width > 0.0, "{}", last.width);
        // Every board fits its stock, and the joints are on joists.
        let f = Frame::new(std::f64::consts::FRAC_PI_2);
        let joist_us: Vec<f64> = g.joists.iter().map(|j| f.to_local(j[0])[0]).collect();
        let mut joints: std::collections::BTreeMap<u32, Vec<f64>> = Default::default();
        for b in &g.boards {
            assert!(b.length <= b.stock + 1e-9);
            let (a, e) = (f.to_local(b.outline[0])[0], f.to_local(b.outline[2])[0]);
            for end in [a.min(e), a.max(e)] {
                let at_edge = end.abs() < 1e-6 || (end - 6.0).abs() < 1e-6;
                if !at_edge {
                    assert!(
                        joist_us.iter().any(|j| (j - end).abs() < 1e-6),
                        "a joint at {end} is not on a joist"
                    );
                    joints.entry(b.course.unwrap()).or_default().push(end);
                }
            }
        }
        for k in 1..28u32 {
            for a in joints.get(&k).into_iter().flatten() {
                for b in joints.get(&(k - 1)).into_iter().flatten() {
                    assert!((a - b).abs() >= 0.8 - 1e-6, "courses {k}: {a} near {b}");
                }
            }
        }
        // The cut list adds up.
        let total: f64 = g.boards.iter().map(|b| b.length).sum();
        assert!((g.cut_list.board_length - total).abs() < 1e-9);
        let bought: u32 = g.cut_list.stock.iter().map(|s| s.1).sum();
        let new = g
            .boards
            .iter()
            .filter(|b| b.from_offcut_of.is_none())
            .count();
        assert_eq!(bought as usize, new);
        assert!(g.cut_list.posts > 0 && g.cut_list.screws > 0);
    }

    #[test]
    fn balanced_courses_and_a_picture_frame() {
        let s = DeckSettings {
            direction: Some(0.0),
            balance_courses: true,
            ..DeckSettings::default()
        };
        let g = generate(&rect4(6.0, 4.0), &[], 0.6, 0.0, &s).unwrap();
        let widths: Vec<f64> = (0..28)
            .filter_map(|k| {
                g.boards
                    .iter()
                    .find(|b| b.course == Some(k))
                    .map(|b| b.width)
            })
            .collect();
        assert!(
            (widths[0] - widths[widths.len() - 1]).abs() < 1e-9,
            "{widths:?}"
        );
        let framed = DeckSettings {
            picture_frame: 1,
            ..DeckSettings::default()
        };
        let g = generate(&rect4(6.0, 4.0), &[], 0.6, 0.0, &framed).unwrap();
        let frame: Vec<&Board> = g.boards.iter().filter(|b| b.course.is_none()).collect();
        assert_eq!(frame.len(), 4);
        // Mitred: each frame board is a trapezoid, its outer edge longer than its inner.
        for b in frame {
            let outer =
                (b.outline[1][0] - b.outline[0][0]).hypot(b.outline[1][1] - b.outline[0][1]);
            let inner =
                (b.outline[2][0] - b.outline[3][0]).hypot(b.outline[2][1] - b.outline[3][1]);
            assert!(outer > inner + 0.2);
        }
    }
}
