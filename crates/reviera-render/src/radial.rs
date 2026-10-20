// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The radial menu's ring drawn over a view (UI-SEL-007): its layout in pixels, which wedge a
//! pointer touches, and the screen-space geometry the renderer draws on top of the scene.
//!
//! Wedges are spread evenly round the centre, the first at the top, then clockwise. Each has a
//! label box at [`REACH`] from the centre. Inside [`DEAD`] nothing is touched; past it the
//! pointer's direction highlights a wedge, and reaching the label box's radius touches it.

use glam::{Mat4, Vec2, Vec3};

use crate::Gizmo;
use crate::viewcube::{Mesh, text};

/// Radius of the centre dead zone, logical pixels.
pub const DEAD: f32 = 24.0;
/// Distance from the centre to the label boxes, logical pixels: reaching it touches the wedge.
pub const REACH: f32 = 110.0;
/// Label text height, logical pixels. Labels are drawn as given (mnemonics already removed).
pub const TEXT: f32 = 11.0;
/// How far, in pixels at the pointer, a highlighted wedge holds past its boundary.
pub const HYSTERESIS: f32 = 4.0;
/// Label box padding and height.
const PAD: f32 = 8.0;
const BOX_H: f32 = 24.0;

/// One wedge as drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct Wedge {
    pub label: String,
    /// It opens a further tier.
    pub submenu: bool,
    pub enabled: bool,
}

/// The ring as drawn: centre in logical pixels, wedges, the highlighted one, the centre title
/// and the path of tiers above.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Ring {
    pub centre: [f32; 2],
    pub wedges: Vec<Wedge>,
    pub hot: Option<usize>,
    pub title: String,
    pub breadcrumb: String,
    /// A tier below the first: the centre returns.
    pub back: bool,
    pub dark: bool,
    /// Larger hit targets (Accessibility): the ring is drawn 1.25 times larger.
    pub large: bool,
}

/// The ring's scale.
pub fn scale(large: bool) -> f32 {
    if large { 1.25 } else { 1.0 }
}

/// The screen direction of wedge `k` of `n` (y down): the first up, then clockwise.
pub fn direction(k: usize, n: usize) -> Vec2 {
    let a = std::f32::consts::TAU * k as f32 / n.max(1) as f32;
    Vec2::new(a.sin(), -a.cos())
}

/// Half the angular width of a wedge of `n`.
fn half_width(n: usize) -> f32 {
    std::f32::consts::PI / n.max(1) as f32
}

/// The label as the stroke font draws it: menu mnemonics (`&`) dropped, `&&` kept as `&`.
pub fn plain(label: &str) -> String {
    label.replace("&&", "\u{1}").replace('&', "").replace('\u{1}', "&")
}

/// The width of a wedge's label box.
fn box_width(w: &Wedge, s: f32) -> f32 {
    crate::font::width(&w.label, TEXT * s) + 2.0 * PAD * s + if w.submenu { 10.0 * s } else { 0.0 }
}

/// The centre of wedge `k`'s label box (`half` its half-width): boxes to the sides grow outward
/// from the ring, so neighbours on the diagonals do not overlap.
fn box_centre(centre: Vec2, k: usize, n: usize, s: f32, half: f32) -> Vec2 {
    let d = direction(k, n);
    centre + d * REACH * s + Vec2::new(d.x * half, 0.0)
}

/// The extent of the ring round its centre (left, right, up, down), logical pixels.
fn extent(wedges: &[Wedge], s: f32) -> [f32; 4] {
    let n = wedges.len();
    let mut e = [REACH * s * 0.5; 4];
    for (k, w) in wedges.iter().enumerate() {
        let half = box_width(w, s) / 2.0;
        let c = box_centre(Vec2::ZERO, k, n, s, half);
        e[0] = e[0].max(-(c.x - half));
        e[1] = e[1].max(c.x + half);
        e[2] = e[2].max(-(c.y - BOX_H * s / 2.0));
        e[3] = e[3].max(c.y + BOX_H * s / 2.0);
    }
    // The title above and the breadcrumb above that.
    e[2] = e[2].max(REACH * s + BOX_H * s + 2.0 * TEXT * s + 12.0);
    e
}

/// Where the ring is drawn for a pointer at `p`: the pointer, shifted so the whole ring fits in a
/// `width` × `height` view (the pointer is never moved).
pub fn fit(p: [f32; 2], wedges: &[Wedge], large: bool, width: f32, height: f32) -> [f32; 2] {
    let [l, r, u, d] = extent(wedges, scale(large));
    let clamp = |v: f32, lo: f32, hi: f32| if lo > hi { (lo + hi) / 2.0 } else { v.clamp(lo, hi) };
    [clamp(p[0], l + 4.0, width - r - 4.0), clamp(p[1], u + 4.0, height - d - 4.0)]
}

/// What the pointer at `p` does to a ring of `n` wedges centred at `centre`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Touch {
    /// In the dead zone.
    Centre,
    /// Pointing at wedge `k`; `reached` once the pointer is as far out as the label box.
    Wedge { k: usize, reached: bool },
}

/// Which wedge the pointer points at; `hot` holds for [`HYSTERESIS`] pixels past its boundary.
pub fn touch(centre: [f32; 2], p: [f32; 2], n: usize, hot: Option<usize>, large: bool) -> Touch {
    let s = scale(large);
    let d = Vec2::from(p) - Vec2::from(centre);
    let r = d.length();
    if r < DEAD * s || n == 0 {
        return Touch::Centre;
    }
    let reached = r >= (REACH - BOX_H / 2.0) * s;
    // Angle from the top, clockwise.
    let a = d.x.atan2(-d.y).rem_euclid(std::f32::consts::TAU);
    let off = |k: usize| {
        let mid = std::f32::consts::TAU * k as f32 / n as f32;
        ((a - mid + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI)
            .abs()
    };
    let half = half_width(n);
    if let Some(k) = hot.filter(|k| *k < n)
        && off(k) <= half + HYSTERESIS / r
    {
        return Touch::Wedge { k, reached };
    }
    let k = (0..n)
        .min_by(|i, j| off(*i).total_cmp(&off(*j)))
        .unwrap_or(0);
    Touch::Wedge { k, reached }
}

/// Colours of the ring, light and dark.
struct Palette {
    disc: [f32; 4],
    sector: [f32; 4],
    hot: [f32; 4],
    boxed: [f32; 4],
    hot_box: [f32; 4],
    ink: [f32; 4],
    hot_ink: [f32; 4],
    dim: [f32; 4],
    crumb: [f32; 4],
}

/// The palette, given in sRGB, as the linear colours the renderer blends.
fn palette(dark: bool) -> Palette {
    let p = srgb_palette(dark);
    let l = |c: [f32; 4]| {
        let f = |v: f32| {
            if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        [f(c[0]), f(c[1]), f(c[2]), c[3]]
    };
    Palette {
        disc: l(p.disc),
        sector: l(p.sector),
        hot: l(p.hot),
        boxed: l(p.boxed),
        hot_box: l(p.hot_box),
        ink: l(p.ink),
        hot_ink: l(p.hot_ink),
        dim: l(p.dim),
        crumb: l(p.crumb),
    }
}

fn srgb_palette(dark: bool) -> Palette {
    if dark {
        Palette {
            disc: [0.12, 0.13, 0.15, 0.72],
            sector: [0.30, 0.32, 0.36, 0.55],
            hot: [0.20, 0.50, 0.90, 0.85],
            boxed: [0.17, 0.18, 0.21, 0.95],
            hot_box: [0.20, 0.50, 0.90, 1.0],
            ink: [0.92, 0.93, 0.95, 1.0],
            hot_ink: [1.0, 1.0, 1.0, 1.0],
            dim: [0.55, 0.56, 0.60, 1.0],
            crumb: [0.85, 0.86, 0.88, 1.0],
        }
    } else {
        Palette {
            disc: [0.98, 0.98, 0.99, 0.78],
            sector: [0.80, 0.82, 0.86, 0.55],
            hot: [0.16, 0.45, 0.85, 0.85],
            boxed: [1.0, 1.0, 1.0, 0.97],
            hot_box: [0.16, 0.45, 0.85, 1.0],
            ink: [0.10, 0.11, 0.13, 1.0],
            hot_ink: [1.0, 1.0, 1.0, 1.0],
            dim: [0.62, 0.63, 0.66, 1.0],
            crumb: [0.15, 0.16, 0.18, 1.0],
        }
    }
}

/// The ring as screen-space geometry over a `width` × `height` view (logical pixels). The
/// caller scales the gizmo's rectangle to physical pixels.
pub fn gizmo(ring: &Ring, width: f32, height: f32) -> Gizmo {
    let s = scale(ring.large);
    let pal = palette(ring.dark);
    let (w, h) = (width.max(1.0), height.max(1.0));
    // Built in pixels (y down) with a depth per layer (lower is in front), then mapped to clip
    // space.
    let at = |p: Vec2, z: f32| Vec3::new(p.x, p.y, z);
    let (px, py) = (Vec3::X, -Vec3::Y);
    let mut mesh = Mesh::default();
    let c = Vec2::from(ring.centre);
    let n = ring.wedges.len();
    let annulus = |mesh: &mut Mesh, r0: f32, r1: f32, a0: f32, a1: f32, z: f32, col: [f32; 4]| {
        let steps = (((a1 - a0).abs() / 0.12).ceil() as usize).max(2);
        for i in 0..steps {
            let (t0, t1) = (
                a0 + (a1 - a0) * i as f32 / steps as f32,
                a0 + (a1 - a0) * (i + 1) as f32 / steps as f32,
            );
            let d = |t: f32| Vec2::new(t.sin(), -t.cos());
            mesh.quad(
                [
                    at(c + d(t0) * r0, z),
                    at(c + d(t0) * r1, z),
                    at(c + d(t1) * r1, z),
                    at(c + d(t1) * r0, z),
                ],
                col,
            );
        }
    };
    let tau = std::f32::consts::TAU;
    let (r0, r1) = (DEAD * s, (REACH - BOX_H / 2.0 - 4.0) * s);
    // The ring's band, and the highlighted wedge's sector over it.
    annulus(&mut mesh, r0, r1, 0.0, tau, 0.8, pal.disc);
    if n > 0 {
        let half = half_width(n);
        for k in 0..n {
            let mid = tau * k as f32 / n as f32;
            // Thin dividers between wedges.
            annulus(&mut mesh, r0, r1, mid + half - 0.012, mid + half + 0.012, 0.75, pal.sector);
        }
        if let Some(k) = ring.hot.filter(|k| *k < n) {
            let mid = tau * k as f32 / n as f32;
            annulus(&mut mesh, r0, r1, mid - half, mid + half, 0.7, pal.hot);
        }
    }
    // The centre: a dot, or a back mark in tiers below the first.
    annulus(&mut mesh, 0.0, r0 - 3.0 * s, 0.0, tau, 0.8, pal.disc);
    if ring.back {
        text(&mut mesh, "<", at(c, 0.3), px, py, TEXT * s, pal.ink);
    }
    // Label boxes.
    for (k, wedge) in ring.wedges.iter().enumerate() {
        let half = Vec2::new(box_width(wedge, s) / 2.0, BOX_H * s / 2.0);
        let b = box_centre(c, k, n, s, half.x);
        let hot = ring.hot == Some(k);
        let col = if hot && wedge.enabled { pal.hot_box } else { pal.boxed };
        mesh.quad(
            [
                at(b + Vec2::new(-half.x, -half.y), 0.5),
                at(b + Vec2::new(half.x, -half.y), 0.5),
                at(b + Vec2::new(half.x, half.y), 0.5),
                at(b + Vec2::new(-half.x, half.y), 0.5),
            ],
            col,
        );
        let ink = if !wedge.enabled {
            pal.dim
        } else if hot {
            pal.hot_ink
        } else {
            pal.ink
        };
        let label = &wedge.label;
        let shift = if wedge.submenu { -5.0 * s } else { 0.0 };
        text(&mut mesh, label, at(b + Vec2::new(shift, 0.0), 0.3), px, py, TEXT * s, ink);
        if wedge.submenu {
            let tip = b + Vec2::new(half.x - PAD * s * 0.9, 0.0);
            let t = 3.5 * s;
            mesh.tri(
                [
                    at(tip + Vec2::new(-t, -t), 0.3),
                    at(tip + Vec2::new(t * 0.6, 0.0), 0.3),
                    at(tip + Vec2::new(-t, t), 0.3),
                ],
                ink,
            );
        }
    }
    // The title above the ring, the breadcrumb above it.
    let top = c.y - REACH * s - BOX_H * s;
    for (line, y, col) in [
        (&ring.title, top - TEXT * s, pal.ink),
        (&ring.breadcrumb, top - 2.4 * TEXT * s - 4.0, pal.crumb),
    ] {
        if line.is_empty() {
            continue;
        }
        let half = crate::font::width(line, TEXT * s) / 2.0 + PAD * s * 0.6;
        let mid = Vec2::new(c.x, y);
        let hh = TEXT * s * 0.85;
        mesh.quad(
            [
                at(mid + Vec2::new(-half, -hh), 0.5),
                at(mid + Vec2::new(half, -hh), 0.5),
                at(mid + Vec2::new(half, hh), 0.5),
                at(mid + Vec2::new(-half, hh), 0.5),
            ],
            pal.boxed,
        );
        text(&mut mesh, line, at(mid, 0.3), px, py, TEXT * s, col);
    }
    let mut flat = Vec::new();
    mesh.into_scene(&mut flat);
    for m in &mut flat {
        for p in &mut m.positions {
            *p = [p[0] / w * 2.0 - 1.0, 1.0 - p[1] / h * 2.0, p[2]];
        }
    }
    Gizmo {
        rect: [0.0, 0.0, w, h],
        view_proj: Mat4::IDENTITY,
        eye: Vec3::Z,
        meshes: Vec::new(),
        flat,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wedges(n: usize) -> Vec<Wedge> {
        (0..n)
            .map(|k| Wedge {
                label: format!("Item {k}"),
                submenu: k % 2 == 0,
                enabled: true,
            })
            .collect()
    }

    #[test]
    fn wedges_start_at_the_top_and_go_clockwise() {
        let c = [400.0, 300.0];
        assert_eq!(touch(c, [400.0, 290.0], 8, None, false), Touch::Centre);
        assert_eq!(touch(c, [400.0, 250.0], 8, None, false), Touch::Wedge { k: 0, reached: false });
        assert_eq!(touch(c, [460.0, 300.0], 8, None, false), Touch::Wedge { k: 2, reached: false });
        assert_eq!(touch(c, [400.0, 400.0], 8, None, false), Touch::Wedge { k: 4, reached: true });
        assert_eq!(touch(c, [300.0, 300.0], 8, None, false), Touch::Wedge { k: 6, reached: true });
    }

    #[test]
    fn a_highlighted_wedge_holds_past_its_boundary() {
        let c = [0.0, 0.0];
        // Four wedges: the boundary between 0 (up) and 1 (right) is at 45°.
        let r = 60.0;
        let just_past = |deg: f32| {
            let a = deg.to_radians();
            [a.sin() * r, -a.cos() * r]
        };
        // 2 px past the boundary on wedge 1's side: wedge 0 holds while it is hot.
        let p = just_past(45.0 + (2.0 / r).to_degrees());
        assert_eq!(touch(c, p, 4, Some(0), false), Touch::Wedge { k: 0, reached: false });
        assert_eq!(touch(c, p, 4, None, false), Touch::Wedge { k: 1, reached: false });
        // 6 px past it, wedge 1 takes over.
        let p = just_past(45.0 + (6.0 / r).to_degrees());
        assert_eq!(touch(c, p, 4, Some(0), false), Touch::Wedge { k: 1, reached: false });
    }

    #[test]
    fn the_ring_fits_inside_the_view() {
        let w = wedges(8);
        let c = fit([5.0, 5.0], &w, false, 1200.0, 800.0);
        let e = extent(&w, 1.0);
        assert!(c[0] - e[0] >= 0.0 && c[1] - e[2] >= 0.0, "{c:?}");
        // Away from the edges the ring is at the pointer.
        assert_eq!(fit([600.0, 400.0], &w, false, 1200.0, 800.0), [600.0, 400.0]);
    }

    #[test]
    fn the_gizmo_covers_the_view_in_clip_space() {
        let ring = Ring {
            centre: [300.0, 300.0],
            wedges: wedges(5),
            hot: Some(1),
            title: "Basic Wall".into(),
            breadcrumb: "Architecture › Build".into(),
            back: true,
            ..Default::default()
        };
        let g = gizmo(&ring, 800.0, 600.0);
        assert_eq!(g.rect, [0.0, 0.0, 800.0, 600.0]);
        assert!(!g.flat.is_empty());
        for m in &g.flat {
            for p in &m.positions {
                assert!(p[0].abs() <= 1.0 && p[1].abs() <= 1.0, "{p:?}");
            }
        }
        assert_eq!(plain("Mirror – &Pick Axis"), "Mirror – Pick Axis");
        assert_eq!(plain("Save && Close"), "Save & Close");
    }
}
