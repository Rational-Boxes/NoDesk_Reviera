// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Floating labels over a 3D view (DOC-ANN-025): each a boxed block of text drawn flat to the
//! screen like a tooltip, centred above a dot at its point in the model, with a short leader up
//! to the middle of the box's bottom edge, so it reads the same from any angle. Nearer labels are placed first; one that would cover another
//! moves up a step or two, and is left out when it still would. At most [`MAX_TIPS`]. The boxes
//! placed are returned too, so a click on one picks its annotation ([`hit`]).

use glam::{Vec2, Vec3};

use crate::viewcube::{Mesh, text};
use crate::{Camera, SceneMesh};

/// The most labels drawn at once.
pub const MAX_TIPS: usize = 150;
/// Text height in pixels at scale 1.
const TEXT: f32 = 11.0;
const PAD: f32 = 4.0;
/// How far above its point a label's box stands (pixels at scale 1).
const LIFT: f32 = 14.0;

/// A label: lines of text at a point of the model.
#[derive(Debug, Clone, PartialEq)]
pub struct Tip {
    pub anchor: Vec3,
    pub lines: Vec<String>,
    /// Its annotation is selected: drawn with a heavier edge in the selection colour.
    pub selected: bool,
}

fn linear(c: [f32; 4]) -> [f32; 4] {
    let f = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    [f(c[0]), f(c[1]), f(c[2]), c[3]]
}

/// A label drawn: the index of its tip, and its box and anchor in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub tip: usize,
    pub lo: Vec2,
    pub hi: Vec2,
    pub anchor: Vec2,
}

/// The tip whose label is at pixel `p`: in its box or on its dot, the nearest label (drawn in
/// front) first.
pub fn hit(placed: &[Placed], p: Vec2) -> Option<usize> {
    const DOT: f32 = 4.0;
    placed
        .iter()
        .find(|b| (p.x >= b.lo.x && p.x <= b.hi.x && p.y >= b.lo.y && p.y <= b.hi.y) || p.distance(b.anchor) <= DOT)
        .map(|b| b.tip)
}

/// The labels' geometry in clip space (-1..1) for a `width` × `height` viewport, as the
/// window's HUD layer takes it, and the boxes placed, nearest first; `scale` is the display's
/// pixel ratio.
pub fn layout(tips: &[Tip], camera: &Camera, width: f32, height: f32, scale: f32, dark: bool) -> (Vec<SceneMesh>, Vec<Placed>) {
    let (w, h) = (width.max(1.0), height.max(1.0));
    let (paper, ink, edge) = if dark {
        (linear([0.17, 0.18, 0.20, 0.94]), linear([0.92, 0.93, 0.95, 1.0]), linear([0.45, 0.47, 0.52, 1.0]))
    } else {
        (linear([1.0, 0.99, 0.93, 0.95]), linear([0.10, 0.10, 0.12, 1.0]), linear([0.55, 0.53, 0.45, 1.0]))
    };
    let text_h = TEXT * scale;
    let pad = PAD * scale;
    // In view, nearest first.
    let mut seen: Vec<(f32, Vec2, usize, &Tip)> = tips
        .iter()
        .enumerate()
        .filter(|(_, t)| !t.lines.is_empty())
        .filter_map(|(i, t)| {
            let (x, y) = camera.project(t.anchor, w, h)?;
            let p = Vec2::new(x, y);
            (p.x >= -50.0 && p.y >= -50.0 && p.x <= w + 50.0 && p.y <= h + 50.0).then(|| (camera.eye().distance(t.anchor), p, i, t))
        })
        .collect();
    seen.sort_by(|a, b| a.0.total_cmp(&b.0));
    seen.truncate(MAX_TIPS);
    let mut placed: Vec<(Vec2, Vec2)> = Vec::new();
    let mut boxes = Vec::new();
    let mut mesh = Mesh::default();
    let at = |p: Vec2, z: f32| Vec3::new(p.x, p.y, z);
    for (k, (_, p, i, t)) in seen.iter().enumerate() {
        let lines: Vec<&str> = t.lines.iter().map(String::as_str).take(4).collect();
        let tw = lines.iter().map(|l| crate::font::width(l, text_h)).fold(0.0, f32::max);
        let size = Vec2::new(tw + 2.0 * pad, lines.len() as f32 * text_h * 1.45 + pad);
        // Centred above the anchor, moved up a step while it covers another.
        let mut corner = *p + Vec2::new(-size.x / 2.0, -LIFT * scale - size.y);
        let overlaps = |c: Vec2, placed: &[(Vec2, Vec2)]| placed.iter().any(|(a, b)| c.x < b.x && c.x + size.x > a.x && c.y < b.y && c.y + size.y > a.y);
        let mut tries = 0;
        while overlaps(corner, &placed) && tries < 3 {
            corner.y -= size.y + 2.0 * scale;
            tries += 1;
        }
        if overlaps(corner, &placed) {
            continue;
        }
        placed.push((corner, corner + size));
        boxes.push(Placed { tip: *i, lo: corner, hi: corner + size, anchor: *p });
        // Nearer labels in front: a depth per label, the box behind its text.
        let z = 0.95 - 0.1 * k as f32 / MAX_TIPS as f32;
        let (z_box, z_ink) = (z, z - 0.004);
        let (b, edge) = if t.selected { (2.0 * scale, linear(crate::HIGHLIGHT)) } else { (1.0 * scale, edge) };
        let (lo, hi) = (corner, corner + size);
        mesh.quad([at(lo - Vec2::splat(b), z_box + 0.001), at(Vec2::new(hi.x + b, lo.y - b), z_box + 0.001), at(hi + Vec2::splat(b), z_box + 0.001), at(Vec2::new(lo.x - b, hi.y + b), z_box + 0.001)], edge);
        mesh.quad([at(lo, z_box), at(Vec2::new(hi.x, lo.y), z_box), at(hi, z_box), at(Vec2::new(lo.x, hi.y), z_box)], paper);
        // The leader, from the anchor's dot up to the middle of the box's bottom edge.
        let corner_pt = Vec2::new((lo.x + hi.x) / 2.0, hi.y);
        let d = (corner_pt - *p).normalize_or_zero();
        let n = Vec2::new(-d.y, d.x) * (0.6 * scale);
        mesh.quad([at(*p - n, z_box + 0.002), at(corner_pt - n, z_box + 0.002), at(corner_pt + n, z_box + 0.002), at(*p + n, z_box + 0.002)], edge);
        let r = 2.2 * scale;
        mesh.quad([at(*p + Vec2::new(-r, -r), z_ink), at(*p + Vec2::new(r, -r), z_ink), at(*p + Vec2::new(r, r), z_ink), at(*p + Vec2::new(-r, r), z_ink)], edge);
        for (i, l) in lines.iter().enumerate() {
            let lw = crate::font::width(l, text_h);
            let c = Vec2::new(lo.x + pad + lw / 2.0, lo.y + pad * 0.5 + text_h * 1.45 * (i as f32 + 0.5));
            text(&mut mesh, l, at(c, z_ink), Vec3::X, -Vec3::Y, text_h, ink);
        }
    }
    let mut flat = Vec::new();
    mesh.into_scene(&mut flat);
    for m in &mut flat {
        for q in &mut m.positions {
            *q = [q[0] / w * 2.0 - 1.0, 1.0 - q[1] / h * 2.0, q[2]];
        }
    }
    (flat, boxes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_in_view_are_drawn_and_overlapping_ones_move_or_go() {
        let mut c = Camera::default();
        c.orient(Vec3::new(1.0, -1.0, 1.0));
        let tip = |x: f32, s: &str| Tip { anchor: Vec3::new(x, 0.0, 0.0), lines: vec![s.to_owned()], selected: false };
        // One label: its box, edge, leader, dot and text, all within clip space.
        let (one, boxes) = layout(&[tip(0.0, "D1")], &c, 800.0, 600.0, 1.0, false);
        assert!(!one.is_empty());
        // A click in its box or on its dot picks it; beside it, nothing.
        let b = boxes[0];
        assert_eq!(hit(&boxes, (b.lo + b.hi) / 2.0), Some(0));
        assert_eq!(hit(&boxes, b.anchor + Vec2::new(1.0, 1.0)), Some(0));
        assert_eq!(hit(&boxes, b.hi + Vec2::splat(20.0)), None);
        assert!(one.iter().flat_map(|m| &m.positions).all(|p| p[0].abs() <= 1.2 && p[1].abs() <= 1.2));
        // Many labels at one point: stacked up to three more times, the rest left out.
        let many: Vec<Tip> = (0..10).map(|k| tip(0.0, &format!("Door {k}"))).collect();
        let tris = |m: &[SceneMesh]| m.iter().map(|m| m.indices.len()).sum::<usize>();
        let (stacked, placed) = layout(&many, &c, 800.0, 600.0, 1.0, false);
        let (four, _) = layout(&many[..4], &c, 800.0, 600.0, 1.0, false);
        assert_eq!(tris(&stacked), tris(&four), "four placed, six left out");
        assert_eq!(placed.len(), 4);
        // Each stacked box picks its own tip.
        for b in &placed {
            assert_eq!(hit(&placed, Vec2::new(b.hi.x - 1.0, (b.lo.y + b.hi.y) / 2.0)), Some(b.tip));
        }
        // Behind the camera: nothing.
        let behind = Tip { anchor: c.eye() + (c.eye() - c.target) * 10.0, lines: vec!["x".into()], selected: false };
        assert!(layout(&[behind], &c, 800.0, 600.0, 1.0, true).0.is_empty());
    }
}
