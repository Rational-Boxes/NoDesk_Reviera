// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Window and door shapes (ARC-WIN-002): the outline of an insert in its own plane, as a
//! counter-clockwise polygon in (x from its centre, z from its bottom), arcs sampled finely.
//! Every shape is convex, so the frame is the outline moved inward by the frame width (the
//! intersection of the edges' inner half-planes) and, for each x, the opening is one interval of
//! height.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::InsertType;

/// The outline of a door or window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    #[default]
    Rectangle,
    /// A circle (an oculus): its diameter is both width and height.
    Round,
    /// An ellipse of the width and height.
    Elliptical,
    /// A rectangle below a head arch (`head_profile`).
    RoundTop,
    /// A half round (half ellipse) fan light: the arch alone.
    Fan,
    /// A pitched head (`rise`).
    Gable,
    /// Left and right heights differ (`left_height`, `right_height`).
    Trapezoid,
    /// A regular polygon (`sides`, `rotation`) stretched to the width and height.
    Polygon,
}

/// The arch of a round-top door or window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HeadProfile {
    /// A half circle: the rise is half the width.
    #[default]
    Semicircular,
    /// An arc of a larger circle, with a `rise` below half the width.
    Segmental,
    /// A half ellipse of the `rise`.
    Elliptical,
    /// Two arcs meeting at the apex, of radius `radius_ratio` × the width.
    Gothic,
    /// Four-centred: tight arcs at the springers, flatter ones to a low apex (`rise`).
    Tudor,
}

macro_rules! names {
    ($t:ty, $($v:ident => $name:literal, $label:literal;)+) => {
        impl $t {
            pub const ALL: &'static [$t] = &[$(<$t>::$v),+];

            /// The value `type.set_param` takes.
            pub fn name(self) -> &'static str {
                match self { $(<$t>::$v => $name),+ }
            }

            /// The name shown in Properties.
            pub fn label(self) -> &'static str {
                match self { $(<$t>::$v => $label),+ }
            }

            /// By name or label, ignoring case, spaces and hyphens.
            pub fn parse(s: &str) -> Option<Self> {
                let norm = |s: &str| s.trim().to_lowercase().replace([' ', '-'], "_");
                let s = norm(s);
                Self::ALL
                    .iter()
                    .copied()
                    .find(|v| v.name() == s || norm(v.label()) == s)
            }
        }
    };
}

pub(crate) use names;

names!(Shape,
    Rectangle => "rectangle", "Rectangle";
    Round => "round", "Round";
    Elliptical => "elliptical", "Elliptical";
    RoundTop => "round_top", "Round Top";
    Fan => "fan", "Fan";
    Gable => "gable", "Gable";
    Trapezoid => "trapezoid", "Trapezoid";
    Polygon => "polygon", "Polygon";
);

names!(HeadProfile,
    Semicircular => "semicircular", "Semicircular";
    Segmental => "segmental", "Segmental";
    Elliptical => "elliptical", "Elliptical";
    Gothic => "gothic", "Gothic";
    Tudor => "tudor", "Tudor";
);

impl Shape {
    /// Doors take rectangles and arched heads.
    pub fn for_doors(self) -> bool {
        matches!(self, Shape::Rectangle | Shape::RoundTop)
    }
}

/// Points on a circle of centre `c` and radius `r` from angle `a0` to `a1` (radians, either
/// way round), both ends included, about 3° apart.
fn arc(c: [f64; 2], r: f64, a0: f64, a1: f64) -> Vec<[f64; 2]> {
    ellipse_arc(c, r, r, a0, a1)
}

fn ellipse_arc(c: [f64; 2], rx: f64, rz: f64, a0: f64, a1: f64) -> Vec<[f64; 2]> {
    let steps = ((a1 - a0).abs() / (PI / 60.0)).ceil().max(2.0) as usize;
    (0..=steps)
        .map(|k| {
            let a = a0 + (a1 - a0) * k as f64 / steps as f64;
            [c[0] + rx * a.cos(), c[1] + rz * a.sin()]
        })
        .collect()
}

/// Drops repeated points (and a last point repeating the first).
fn dedup(mut pts: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    pts.dedup_by(|a, b| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9);
    while pts.len() > 1 {
        let (a, b) = (pts[0], pts[pts.len() - 1]);
        if (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9 {
            pts.pop();
        } else {
            break;
        }
    }
    pts
}

/// The rise of a pointed (Gothic) arch per unit of span, for a radius of `k` spans.
fn gothic_rise(k: f64) -> f64 {
    (k * k - (k - 0.5) * (k - 0.5)).sqrt()
}

/// The head's rise above the spring line at width `w` (before it is fitted to the height).
pub fn rise(t: &InsertType, w: f64) -> f64 {
    match t.shape {
        Shape::RoundTop => match t.head_profile {
            HeadProfile::Semicircular => w / 2.0,
            HeadProfile::Segmental => t.rise.unwrap_or(w / 4.0).clamp(1e-3, w / 2.0),
            HeadProfile::Elliptical => t.rise.unwrap_or(w / 3.0).max(1e-3),
            HeadProfile::Gothic => w * gothic_rise(radius_ratio(t)),
            HeadProfile::Tudor => t.rise.unwrap_or(w / 3.0).clamp(1e-3, w / 2.0),
        },
        Shape::Gable => t.rise.unwrap_or(w / 4.0).max(1e-3),
        Shape::Fan => f64::INFINITY,
        _ => 0.0,
    }
}

/// A Gothic arch's radius as a multiple of its span (at least half: a semicircle).
pub fn radius_ratio(t: &InsertType) -> f64 {
    t.radius_ratio.unwrap_or(1.0).max(0.5)
}

/// The height of a round-top's or gable's straight sides (its spring height).
pub fn spring_height(t: &InsertType, w: f64, h: f64) -> f64 {
    (h - rise(t, w)).max(0.0)
}

/// The widest a shape whose head grows with its width may be at height `h`, so that it keeps
/// its head (a round window's diameter; a semicircular or Gothic head's span). `None` when the
/// shape does not tie its width to its height.
pub fn width_for_height(t: &InsertType, h: f64) -> Option<f64> {
    match (t.shape, t.head_profile) {
        (Shape::Round, _) => Some(h),
        (Shape::RoundTop, HeadProfile::Semicircular) => Some(2.0 * h),
        (Shape::RoundTop, HeadProfile::Gothic) => Some(h / gothic_rise(radius_ratio(t))),
        _ => None,
    }
}

/// The head of a round-top from the right springer `(w/2, 0)` to the left `(-w/2, 0)`,
/// relative to the spring line.
fn head(t: &InsertType, w: f64, r: f64) -> Vec<[f64; 2]> {
    let half = w / 2.0;
    match t.head_profile {
        HeadProfile::Semicircular => ellipse_arc([0.0, 0.0], half, r, 0.0, PI),
        HeadProfile::Elliptical => ellipse_arc([0.0, 0.0], half, r, 0.0, PI),
        HeadProfile::Segmental => segmental(half, r),
        HeadProfile::Gothic => {
            let radius = radius_ratio(t) * w;
            let full = w * gothic_rise(radius_ratio(t));
            // Each side is an arc about the opposite side's centre on the spring line.
            let c = half - radius;
            let a = full.atan2(-c);
            let mut pts = arc([c, 0.0], radius, 0.0, a);
            pts.extend(arc([-c, 0.0], radius, PI - a, PI));
            // Squashed when the height leaves less room than the arch needs.
            let s = r / full;
            pts.into_iter().map(|[x, z]| [x, z * s]).collect()
        }
        HeadProfile::Tudor => tudor(half, r).unwrap_or_else(|| segmental(half, r)),
    }
}

fn segmental(half: f64, r: f64) -> Vec<[f64; 2]> {
    let radius = (r * r + half * half) / (2.0 * r);
    let cz = r - radius;
    let a = (-cz).atan2(half);
    arc([0.0, cz], radius, a, PI - a)
}

/// A four-centred arch of half-span `half` and rise `r`: arcs of a quarter of the span turning
/// 60° at each springer, then arcs tangent to them meeting at the apex.
fn tudor(half: f64, r: f64) -> Option<Vec<[f64; 2]>> {
    let r1 = (half / 2.0).min(r * 0.9);
    let theta = 60f64.to_radians();
    // The right side: the small arc about (half - r1, 0) from 0 to θ; the large one about a
    // centre on the line back through the small one's centre, reaching (0, r).
    let c1x = half - r1;
    let denom = 2.0 * (c1x * theta.cos() - r * theta.sin() + r1);
    if denom.abs() < 1e-12 {
        return None;
    }
    let d = (c1x * c1x + r * r - r1 * r1) / denom;
    let r2 = r1 + d;
    if d <= 0.0 || r2 <= r1 {
        return None;
    }
    let c2 = [c1x - d * theta.cos(), -d * theta.sin()];
    let apex = (r - c2[1]).atan2(-c2[0]);
    if apex <= theta {
        return None;
    }
    let mut right = arc([c1x, 0.0], r1, 0.0, theta);
    right.extend(arc(c2, r2, theta, apex));
    let left: Vec<[f64; 2]> = right.iter().rev().map(|[x, z]| [-x, *z]).collect();
    right.extend(left);
    Some(right)
}

/// The outline of a `w` × `h` insert of type `t`: counter-clockwise, in (x from its centre, z
/// from its bottom). Outlines with a flat bottom start at its left corner.
pub fn outline(t: &InsertType, w: f64, h: f64) -> Vec<[f64; 2]> {
    let half = w / 2.0;
    let rect = vec![[-half, 0.0], [half, 0.0], [half, h], [-half, h]];
    let pts = match t.shape {
        Shape::Rectangle => rect,
        Shape::Round => {
            let r = w.min(h) / 2.0;
            let mut c = arc([0.0, h / 2.0], r, -FRAC_PI_2, 3.0 * FRAC_PI_2);
            c.pop();
            c
        }
        Shape::Elliptical => {
            let mut c = ellipse_arc([0.0, h / 2.0], half, h / 2.0, -FRAC_PI_2, 3.0 * FRAC_PI_2);
            c.pop();
            c
        }
        Shape::RoundTop => {
            let r = rise(t, w).min(h);
            let s = h - r;
            let mut pts = vec![[-half, 0.0], [half, 0.0]];
            pts.extend(head(t, w, r).into_iter().map(|[x, z]| [x, s + z]));
            pts
        }
        Shape::Fan => {
            let mut pts = vec![[-half, 0.0]];
            pts.extend(ellipse_arc([0.0, 0.0], half, h, 0.0, PI));
            pts
        }
        Shape::Gable => {
            let s = h - rise(t, w).min(h);
            vec![[-half, 0.0], [half, 0.0], [half, s], [0.0, h], [-half, s]]
        }
        Shape::Trapezoid => {
            let side = |v: Option<f64>| v.unwrap_or(h).clamp(1e-3, h);
            let (l, r) = (side(t.left_height), side(t.right_height));
            vec![[-half, 0.0], [half, 0.0], [half, r], [-half, l]]
        }
        Shape::Polygon => {
            let n = t.sides.unwrap_or(8).clamp(3, 64) as usize;
            let rot = t.rotation.unwrap_or(PI / n as f64);
            let unit: Vec<[f64; 2]> = (0..n)
                .map(|k| {
                    let a = FRAC_PI_2 + rot + TAU * k as f64 / n as f64;
                    [a.cos(), a.sin()]
                })
                .collect();
            let (lo, hi) = bounds(&unit);
            unit.into_iter()
                .map(|[x, z]| {
                    [
                        -half + (x - lo[0]) / (hi[0] - lo[0]) * w,
                        (z - lo[1]) / (hi[1] - lo[1]) * h,
                    ]
                })
                .collect()
        }
    };
    dedup(pts)
}

fn bounds(pts: &[[f64; 2]]) -> ([f64; 2], [f64; 2]) {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for p in pts {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    (lo, hi)
}

/// The part of a convex polygon where `(p - a) · n >= f`.
fn clip(poly: &[[f64; 2]], a: [f64; 2], n: [f64; 2], f: f64) -> Vec<[f64; 2]> {
    let s = |p: [f64; 2]| (p[0] - a[0]) * n[0] + (p[1] - a[1]) * n[1] - f;
    let mut out = Vec::new();
    for i in 0..poly.len() {
        let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
        let (sp, sq) = (s(p), s(q));
        if sp >= 0.0 {
            out.push(p);
        }
        if (sp >= 0.0) != (sq >= 0.0) {
            let t = sp / (sp - sq);
            out.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
        }
    }
    dedup(out)
}

/// A convex outline moved inward by `f` (each edge's line moved in; edges that vanish go). With
/// `open_bottom` the edges along z = 0 stay (a door's frame has no sill). Empty when nothing is
/// left.
pub fn inset(poly: &[[f64; 2]], f: f64, open_bottom: bool) -> Vec<[f64; 2]> {
    let mut out = poly.to_vec();
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if open_bottom && a[1].abs() < 1e-9 && b[1].abs() < 1e-9 {
            continue;
        }
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        if len < 1e-12 {
            continue;
        }
        let normal = [-(b[1] - a[1]) / len, (b[0] - a[0]) / len];
        out = clip(&out, a, normal, f);
        if out.len() < 3 {
            return Vec::new();
        }
    }
    out
}

/// The part of a convex polygon inside another convex polygon (counter-clockwise).
pub fn clip_to(poly: &[[f64; 2]], window: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut out = poly.to_vec();
    let n = window.len();
    for i in 0..n {
        let (a, b) = (window[i], window[(i + 1) % n]);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        if len < 1e-12 {
            continue;
        }
        out = clip(&out, a, [-(b[1] - a[1]) / len, (b[0] - a[0]) / len], 0.0);
        if out.len() < 3 {
            return Vec::new();
        }
    }
    out
}

/// The interval of heights a convex polygon covers at `x`; `None` outside it.
pub fn span_at(poly: &[[f64; 2]], x: f64) -> Option<[f64; 2]> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let (x0, x1) = (a[0].min(b[0]), a[0].max(b[0]));
        if x < x0 - 1e-9 || x > x1 + 1e-9 {
            continue;
        }
        if x1 - x0 < 1e-12 {
            lo = lo.min(a[1].min(b[1]));
            hi = hi.max(a[1].max(b[1]));
        } else {
            let t = ((x - a[0]) / (b[0] - a[0])).clamp(0.0, 1.0);
            let z = a[1] + (b[1] - a[1]) * t;
            lo = lo.min(z);
            hi = hi.max(z);
        }
    }
    (lo <= hi).then_some([lo, hi])
}

/// The x of a polygon's points, in order, without repeats.
pub fn stations(poly: &[[f64; 2]]) -> Vec<f64> {
    let mut xs: Vec<f64> = poly.iter().map(|p| p[0]).collect();
    xs.sort_by(f64::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    xs
}

/// The polygon starting at its bottom-left point (the lowest, then the leftmost).
pub fn from_bottom_left(poly: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let k = (0..poly.len())
        .min_by(|&i, &j| {
            let (a, b) = (poly[i], poly[j]);
            (a[1] > 1e-9)
                .cmp(&(b[1] > 1e-9))
                .then(a[0].total_cmp(&b[0]))
        })
        .unwrap_or(0);
    poly[k..].iter().chain(&poly[..k]).copied().collect()
}

/// Glazing bars (muntins) of a shaped window: `count` bars `bar` wide, radiating from the head's
/// centre (round tops, with a transom at the spring line; fans; round and elliptical windows)
/// or upright and evenly spaced (other shapes), clipped to the glass `glass`.
pub fn muntins(
    t: &InsertType,
    w: f64,
    h: f64,
    glass: &[[f64; 2]],
    count: u32,
    bar: f64,
) -> Vec<Vec<[f64; 2]>> {
    if count == 0 || glass.len() < 3 {
        return Vec::new();
    }
    let b = bar / 2.0;
    let far = 2.0 * (w + h);
    let ray = |hub: [f64; 2], a: f64| {
        let (d, n) = ([a.cos(), a.sin()], [-a.sin(), a.cos()]);
        let end = [hub[0] + d[0] * far, hub[1] + d[1] * far];
        vec![
            [hub[0] - n[0] * b, hub[1] - n[1] * b],
            [end[0] - n[0] * b, end[1] - n[1] * b],
            [end[0] + n[0] * b, end[1] + n[1] * b],
            [hub[0] + n[0] * b, hub[1] + n[1] * b],
        ]
    };
    let mut bars: Vec<Vec<[f64; 2]>> = Vec::new();
    match t.shape {
        Shape::RoundTop | Shape::Fan => {
            let spring = match t.shape {
                Shape::Fan => 0.0,
                _ => h - rise(t, w).min(h),
            };
            let hub = [0.0, spring];
            for k in 1..=count {
                bars.push(ray(hub, PI * k as f64 / (count + 1) as f64));
            }
            if spring > bar {
                let s = spring;
                bars.push(vec![
                    [-far, s - b],
                    [far, s - b],
                    [far, s + b],
                    [-far, s + b],
                ]);
            }
        }
        Shape::Round | Shape::Elliptical => {
            let hub = [0.0, h / 2.0];
            for k in 0..count {
                // Spokes through the centre, `count` of them.
                let a = PI * k as f64 / count as f64;
                let mut both = ray(hub, a);
                let back = ray(hub, a + PI);
                both = vec![back[1], both[1], both[2], back[2]];
                bars.push(both);
            }
        }
        _ => {
            for k in 1..=count {
                let x = -w / 2.0 + w * k as f64 / (count + 1) as f64;
                bars.push(vec![
                    [x - b, -far],
                    [x + b, -far],
                    [x + b, far],
                    [x - b, far],
                ]);
            }
        }
    }
    bars.into_iter()
        .map(|p| clip_to(&p, glass))
        .filter(|p| p.len() >= 3)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inserts::Kind;

    fn ty(shape: Shape) -> InsertType {
        InsertType {
            shape,
            ..InsertType::default_for(Kind::Window)
        }
    }

    fn area(p: &[[f64; 2]]) -> f64 {
        (0..p.len())
            .map(|i| {
                let (a, b) = (p[i], p[(i + 1) % p.len()]);
                a[0] * b[1] - b[0] * a[1]
            })
            .sum::<f64>()
            / 2.0
    }

    #[test]
    fn every_shape_is_a_counter_clockwise_outline_in_its_box() {
        for &s in Shape::ALL {
            for &head in HeadProfile::ALL {
                let t = InsertType {
                    head_profile: head,
                    ..ty(s)
                };
                let o = outline(&t, 1.2, 2.0);
                assert!(o.len() >= 3, "{s:?}");
                assert!(area(&o) > 0.0, "{s:?} {head:?} is clockwise");
                let (lo, hi) = bounds(&o);
                assert!(
                    lo[0] >= -0.6 - 1e-9 && hi[0] <= 0.6 + 1e-9,
                    "{s:?} {head:?}"
                );
                assert!(lo[1] >= -1e-9 && hi[1] <= 2.0 + 1e-9, "{s:?} {head:?}");
                // Convex: every turn is to the left.
                for i in 0..o.len() {
                    let (a, b, c) = (o[i], o[(i + 1) % o.len()], o[(i + 2) % o.len()]);
                    let cross = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
                    assert!(cross > -1e-9, "{s:?} {head:?} turns right at {b:?}");
                }
            }
        }
    }

    #[test]
    fn a_semicircular_head_springs_at_the_height_less_half_the_width() {
        let t = InsertType {
            head_profile: HeadProfile::Semicircular,
            ..ty(Shape::RoundTop)
        };
        assert!((spring_height(&t, 1.2, 2.0) - 1.4).abs() < 1e-12);
        let o = outline(&t, 1.2, 2.0);
        let has = |q: [f64; 2]| o.iter().any(|p| (p[0] - q[0]).hypot(p[1] - q[1]) < 1e-9);
        assert!(has([0.6, 1.4]) && has([-0.6, 1.4]));
        // The frame follows the arch at a constant width: its inner edge is 60 mm inside.
        let inner = inset(&o, 0.06, false);
        for p in inner.iter().filter(|p| p[1] > 1.4 + 1e-6) {
            let r = p[0].hypot(p[1] - 1.4);
            assert!((r - 0.54).abs() < 1e-3, "{p:?} at {r}");
        }
        let lowest = inner.iter().map(|p| p[1]).fold(f64::MAX, f64::min);
        assert!((lowest - 0.06).abs() < 1e-9);
    }

    #[test]
    fn a_gothic_arch_of_ratio_one_is_two_arcs_of_the_width_meeting_at_the_apex() {
        let t = InsertType {
            head_profile: HeadProfile::Gothic,
            radius_ratio: Some(1.0),
            ..ty(Shape::RoundTop)
        };
        let (w, h) = (1.0, 2.5);
        let s = spring_height(&t, w, h);
        let o = outline(&t, w, h);
        let apex = o
            .iter()
            .copied()
            .fold([0.0, 0.0], |m, p| if p[1] > m[1] { p } else { m });
        assert!(
            apex[0].abs() < 1e-9 && (apex[1] - h).abs() < 1e-9,
            "{apex:?}"
        );
        assert!((h - s - w * 3f64.sqrt() / 2.0).abs() < 1e-12);
        // The right side is about the left springer, the left side about the right one.
        for p in o.iter().filter(|p| p[1] > s + 1e-6) {
            let c = if p[0] > 1e-9 { -0.5 } else { 0.5 };
            if p[0].abs() < 1e-9 {
                continue;
            }
            assert!(((p[0] - c).hypot(p[1] - s) - w).abs() < 1e-9, "{p:?}");
        }
    }

    #[test]
    fn a_round_outline_is_a_circle_of_the_smaller_side() {
        let o = outline(&ty(Shape::Round), 0.9, 0.9);
        for p in &o {
            assert!((p[0].hypot(p[1] - 0.45) - 0.45).abs() < 1e-12);
        }
        // 300 mm below its centre (z = 0.15) it is 2·√(0.45² − 0.3²) wide: its edge there.
        let x = (0.45f64 * 0.45 - 0.3 * 0.3).sqrt();
        let s = span_at(&o, x).unwrap();
        assert!((s[0] - 0.15).abs() < 2e-3, "{s:?}");
    }

    #[test]
    fn a_door_frame_is_open_at_the_bottom() {
        let t = InsertType {
            shape: Shape::RoundTop,
            ..InsertType::default_for(Kind::Door)
        };
        let o = outline(&t, 0.9, 2.4);
        let inner = from_bottom_left(&inset(&o, 0.05, true));
        let near = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).hypot(p[1] - q[1]) < 1e-9;
        assert!(near(inner[0], [-0.4, 0.0]), "{:?}", inner[0]);
        assert!(inner.iter().any(|p| near(*p, [0.4, 0.0])));
    }

    #[test]
    fn tudor_heads_are_four_centred_and_reach_their_rise() {
        let t = InsertType {
            head_profile: HeadProfile::Tudor,
            rise: Some(0.4),
            ..ty(Shape::RoundTop)
        };
        let o = outline(&t, 1.2, 2.0);
        let top = o.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
        assert!((top - 2.0).abs() < 1e-9);
        assert!(tudor(0.6, 0.4).is_some());
    }

    #[test]
    fn muntins_stay_on_the_glass() {
        let t = InsertType {
            muntins: Some(3),
            ..ty(Shape::RoundTop)
        };
        let o = outline(&t, 1.2, 2.0);
        let glass = inset(&o, 0.06, false);
        let bars = muntins(&t, 1.2, 2.0, &glass, 3, 0.03);
        assert_eq!(bars.len(), 4, "three rays and a transom");
        for bar in bars {
            for p in bar {
                assert!(
                    span_at(&glass, p[0])
                        .is_some_and(|s| p[1] >= s[0] - 1e-9 && p[1] <= s[1] + 1e-9)
                );
            }
        }
    }

    #[test]
    fn names_parse_by_label_or_name() {
        assert_eq!(Shape::parse("Round Top"), Some(Shape::RoundTop));
        assert_eq!(Shape::parse("round_top"), Some(Shape::RoundTop));
        assert_eq!(HeadProfile::parse("gothic"), Some(HeadProfile::Gothic));
        assert_eq!(Shape::parse("blob"), None);
    }
}
