// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Drawing tags and text notes (DOC-ANN-009…011): their text in the stroke font, a tag's
//! outline and leader, and a note's wrapped lines and leaders with arrowheads; and spot
//! elevations and coordinates (DOC-ANN-006/007): a symbol at the point, a leader to a shoulder
//! and the values above it; spot slopes (DOC-ANN-008): an arrow pointing downhill, or a triangle,
//! with the value beside it. Sizes are paper millimetres scaled by the view scale.

use reviera_core::notes::{Attach, Frame, Side, TextLeader};

use crate::annotation::Label;
use crate::font;

/// Line spacing, as a multiple of the text height.
const LINE_SPACING: f32 = 1.6;
/// Space between a tag's text and its outline (paper mm).
const TAG_PAD_MM: f32 = 1.0;
/// Space between a note's text and where its leaders start (paper mm).
const LEADER_GAP_MM: f32 = 1.0;
/// Arrowhead length and half-width (paper mm).
const ARROW_MM: [f32; 2] = [2.5, 0.8];

/// What an annotation object draws: segments, text, and its outline (four corners, for picking
/// and highlighting).
#[derive(Debug, Clone, PartialEq)]
pub struct NoteParts {
    pub segments: Vec<[[f32; 3]; 2]>,
    pub labels: Vec<Label>,
    pub outline: [[f32; 2]; 4],
}

/// A tag to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct TagMark {
    /// The centre of the head (plan, metres).
    pub head: [f64; 2],
    pub lines: Vec<String>,
    pub frame: Frame,
    pub vertical: bool,
    /// The end of the leader, if it has one.
    pub leader: Option<[f64; 2]>,
    /// Text height on paper (mm).
    pub text_mm: f32,
}

/// A text note to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct TextMark {
    /// The top-left corner (plan, metres).
    pub position: [f64; 2],
    /// Counter-clockwise, radians.
    pub rotation: f64,
    /// Text height on paper (mm).
    pub size_mm: f32,
    /// Wrapping width on paper (mm).
    pub width_mm: Option<f32>,
    pub content: String,
    pub leaders: Vec<TextLeader>,
}

/// The mark at a spot's point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpotSymbol {
    /// A spot elevation: a triangle standing on its point.
    Triangle,
    /// A spot coordinate: a cross on it.
    Cross,
    /// A spot slope: an arrow through its point pointing downhill (a unit vector in the view),
    /// the value above its middle.
    SlopeArrow([f64; 2]),
    /// A spot slope in a section or elevation: a right triangle on the slope's line at its
    /// point, 10 mm along, with downhill and the slope (rise over run).
    SlopeTriangle([f64; 2], f64),
}

/// A spot to draw: its point and the shoulder's start in the view's 2D coordinates, and its
/// values, one per line.
#[derive(Debug, Clone, PartialEq)]
pub struct SpotMark {
    pub point: [f64; 2],
    pub text: [f64; 2],
    pub lines: Vec<String>,
    pub symbol: SpotSymbol,
    pub text_mm: f32,
}

/// A spot's symbol, leader and shoulder, and its values written above the shoulder (on the side
/// away from the point), at elevation `z` for a view at 1:`scale`.
pub fn spot_parts(s: &SpotMark, scale: f32, z: f32) -> NoteParts {
    let mm = scale / 1000.0;
    let h = s.text_mm * mm;
    let step = h * LINE_SPACING;
    let p = [s.point[0] as f32, s.point[1] as f32];
    let t = [s.text[0] as f32, s.text[1] as f32];
    let mut segments = Vec::new();
    if let SpotSymbol::SlopeArrow(d) | SpotSymbol::SlopeTriangle(d, _) = s.symbol {
        return slope_parts(s, [d[0] as f32, d[1] as f32], p, t, mm, h, z);
    }
    match s.symbol {
        SpotSymbol::SlopeArrow(_) | SpotSymbol::SlopeTriangle(..) => {}
        SpotSymbol::Triangle => {
            let (w, ht) = (1.2 * mm, 2.0 * mm);
            let (a, b) = ([p[0] - w, p[1] + ht], [p[0] + w, p[1] + ht]);
            segments.extend([seg(p, a, z), seg(a, b, z), seg(b, p, z)]);
        }
        SpotSymbol::Cross => {
            let r = 1.5 * mm;
            segments.extend([seg([p[0] - r, p[1] - r], [p[0] + r, p[1] + r], z), seg([p[0] - r, p[1] + r], [p[0] + r, p[1] - r], z)]);
        }
    }
    let width = s.lines.iter().map(|l| font::width(l, h)).fold(0.0, f32::max) + 1.0 * mm;
    // The shoulder runs away from the point.
    let dir = if t[0] >= p[0] { 1.0 } else { -1.0 };
    let end = [t[0] + dir * width, t[1]];
    segments.push(seg(p, t, z));
    segments.push(seg(t, end, z));
    let left = t[0].min(end[0]) + 0.5 * mm;
    let n = s.lines.len().max(1) as f32;
    let labels = s
        .lines
        .iter()
        .enumerate()
        .map(|(k, text)| Label {
            origin: [left, t[1] + 0.8 * mm + (n - 1.0 - k as f32) * step, z],
            right: [1.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            height: h,
            text: text.clone(),
        })
        .collect();
    let (x0, x1) = (t[0].min(end[0]), t[0].max(end[0]));
    let (y0, y1) = (t[1] - 0.5 * mm, t[1] + 0.8 * mm + (n - 1.0) * step + h + 0.5 * mm);
    NoteParts { segments, labels, outline: [[x0, y0], [x1, y0], [x1, y1], [x0, y1]] }
}

/// A spot slope: its arrow (or triangle) at `p` along downhill `d`, its value at `t`.
fn slope_parts(s: &SpotMark, d: [f32; 2], p: [f32; 2], t: [f32; 2], mm: f32, h: f32, z: f32) -> NoteParts {
    let mut segments = Vec::new();
    let text = s.lines.first().cloned().unwrap_or_default();
    let w = font::width(&text, h);
    match s.symbol {
        SpotSymbol::SlopeTriangle(_, slope) => {
            // Run 10 mm along the horizontal, rise by the slope, from the point uphill.
            let run = 10.0 * mm * d[0].signum();
            let rise = (10.0 * mm * slope as f32).min(30.0 * mm);
            let a = p;
            let b = [p[0] - run, p[1]];
            let c = [p[0] - run, p[1] + rise];
            segments.extend([seg(a, b, z), seg(b, c, z), seg(c, a, z)]);
        }
        _ => {
            // 12 mm through the point, arrowhead at the downhill end.
            let half = 6.0 * mm;
            let (a, b) = ([p[0] - d[0] * half, p[1] - d[1] * half], [p[0] + d[0] * half, p[1] + d[1] * half]);
            segments.push(seg(a, b, z));
            let (len, wid) = (ARROW_MM[0] * mm, ARROW_MM[1] * mm);
            let back = [b[0] - d[0] * len, b[1] - d[1] * len];
            let n = [-d[1] * wid, d[0] * wid];
            segments.push(seg(b, [back[0] + n[0], back[1] + n[1]], z));
            segments.push(seg(b, [back[0] - n[0], back[1] - n[1]], z));
        }
    }
    let origin = [t[0] - w / 2.0, t[1] + 0.5 * mm];
    let labels = vec![Label { origin: [origin[0], origin[1], z], right: [1.0, 0.0, 0.0], up: [0.0, 1.0, 0.0], height: h, text }];
    let (x0, x1) = (origin[0].min(p[0] - 6.0 * mm), (origin[0] + w).max(p[0] + 6.0 * mm));
    let (y0, y1) = ((p[1] - 6.0 * mm).min(origin[1]), (origin[1] + h + 0.5 * mm).max(p[1] + 6.0 * mm));
    NoteParts { segments, labels, outline: [[x0, y0], [x1, y0], [x1, y1], [x0, y1]] }
}

fn seg(a: [f32; 2], b: [f32; 2], z: f32) -> [[f32; 3]; 2] {
    [[a[0], a[1], z], [b[0], b[1], z]]
}

/// An arrowhead pointing at `tip` from `from`, as three segments (a closed triangle).
fn arrow(from: [f32; 2], tip: [f32; 2], mm: f32, z: f32) -> Vec<[[f32; 3]; 2]> {
    let d = [tip[0] - from[0], tip[1] - from[1]];
    let len = d[0].hypot(d[1]);
    if len < 1e-9 {
        return Vec::new();
    }
    let u = [d[0] / len, d[1] / len];
    let n = [-u[1], u[0]];
    let (l, w) = (ARROW_MM[0] * mm, ARROW_MM[1] * mm);
    let base = [tip[0] - u[0] * l, tip[1] - u[1] * l];
    let a = [base[0] + n[0] * w, base[1] + n[1] * w];
    let b = [base[0] - n[0] * w, base[1] - n[1] * w];
    vec![seg(tip, a, z), seg(a, b, z), seg(b, tip, z)]
}

/// Words of `paragraph` wrapped to `width` (model units) at text height `h`.
fn wrap(paragraph: &str, width: Option<f32>, h: f32) -> Vec<String> {
    let Some(width) = width else {
        return vec![paragraph.to_owned()];
    };
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in paragraph.split(' ') {
        let candidate = if line.is_empty() {
            word.to_owned()
        } else {
            format!("{line} {word}")
        };
        if !line.is_empty() && font::width(&candidate, h) > width {
            lines.push(std::mem::take(&mut line));
            line = word.to_owned();
        } else {
            line = candidate;
        }
    }
    lines.push(line);
    lines
}

/// The lines of a text note as it wraps.
pub fn text_lines(t: &TextMark, scale: f32) -> Vec<String> {
    let mm = scale / 1000.0;
    let h = t.size_mm * mm;
    t.content
        .split('\n')
        .flat_map(|p| wrap(p, t.width_mm.map(|w| w * mm), h))
        .collect()
}

/// A text note at elevation `z`, for a view at 1:`scale`.
pub fn text_parts(t: &TextMark, scale: f32, z: f32) -> NoteParts {
    let mm = scale / 1000.0;
    let h = t.size_mm * mm;
    let step = h * LINE_SPACING;
    let lines = text_lines(t, scale);
    let (s, c) = (t.rotation as f32).sin_cos();
    let (right, up) = ([c, s], [-s, c]);
    let o = [t.position[0] as f32, t.position[1] as f32];
    // Local (x right, y up from the top-left corner) to plan.
    let at = |x: f32, y: f32| {
        [
            o[0] + right[0] * x + up[0] * y,
            o[1] + right[1] * x + up[1] * y,
        ]
    };
    let width = t.width_mm.map_or_else(
        || lines.iter().map(|l| font::width(l, h)).fold(0.0, f32::max),
        |w| w * mm,
    );
    let height = (lines.len().max(1) as f32 - 1.0) * step + h;
    let labels = lines
        .iter()
        .enumerate()
        .map(|(k, text)| {
            let p = at(0.0, -h - k as f32 * step);
            Label {
                origin: [p[0], p[1], z],
                right: [right[0], right[1], 0.0],
                up: [up[0], up[1], 0.0],
                height: h,
                text: text.clone(),
            }
        })
        .collect();
    let mut segments = Vec::new();
    let gap = LEADER_GAP_MM * mm;
    for l in &t.leaders {
        let y = match l.attach {
            Attach::Top => -h / 2.0,
            Attach::Middle => -height / 2.0,
            Attach::Bottom => -height + h / 2.0,
        };
        let start = match l.side {
            Side::Left => at(-gap, y),
            Side::Right => at(width + gap, y),
        };
        let mut prev = start;
        for p in &l.points {
            let p = [p[0] as f32, p[1] as f32];
            segments.push(seg(prev, p, z));
            if l.points
                .last()
                .is_some_and(|last| [last[0] as f32, last[1] as f32] == p)
            {
                segments.extend(arrow(prev, p, mm, z));
            }
            prev = p;
        }
    }
    NoteParts {
        segments,
        labels,
        outline: [
            at(0.0, 0.0),
            at(width, 0.0),
            at(width, -height),
            at(0.0, -height),
        ],
    }
}

/// A tag at elevation `z`, for a view at 1:`scale`.
pub fn tag_parts(t: &TagMark, scale: f32, z: f32) -> NoteParts {
    let mm = scale / 1000.0;
    let h = t.text_mm * mm;
    let step = h * LINE_SPACING;
    let n = t.lines.len().max(1) as f32;
    let text_w = t
        .lines
        .iter()
        .map(|l| font::width(l, h))
        .fold(0.0, f32::max);
    let pad = TAG_PAD_MM * mm;
    let (hx, hy) = (text_w / 2.0 + pad, ((n - 1.0) * step + h) / 2.0 + pad);
    // Local (x along the text, y up) to plan; vertical tags read upward.
    let c = [t.head[0] as f32, t.head[1] as f32];
    let (right, up) = if t.vertical {
        ([0.0, 1.0], [-1.0, 0.0])
    } else {
        ([1.0, 0.0], [0.0, 1.0])
    };
    let at = |x: f32, y: f32| {
        [
            c[0] + right[0] * x + up[0] * y,
            c[1] + right[1] * x + up[1] * y,
        ]
    };
    let labels = t
        .lines
        .iter()
        .enumerate()
        .map(|(k, text)| {
            let w = font::width(text, h);
            let y = (n - 1.0) * step / 2.0 - k as f32 * step - h / 2.0;
            let p = at(-w / 2.0, y);
            Label {
                origin: [p[0], p[1], z],
                right: [right[0], right[1], 0.0],
                up: [up[0], up[1], 0.0],
                height: h,
                text: text.clone(),
            }
        })
        .collect();
    let corners = [at(-hx, -hy), at(hx, -hy), at(hx, hy), at(-hx, hy)];
    let mut segments = Vec::new();
    let tip = hy;
    let ring: Vec<[f32; 2]> = match t.frame {
        Frame::None => Vec::new(),
        Frame::Box => corners.to_vec(),
        // Pointed ends left and right.
        Frame::Hexagon => vec![
            at(-hx - tip, 0.0),
            at(-hx, -hy),
            at(hx, -hy),
            at(hx + tip, 0.0),
            at(hx, hy),
            at(-hx, hy),
        ],
    };
    for k in 0..ring.len() {
        segments.push(seg(ring[k], ring[(k + 1) % ring.len()], z));
    }
    // The leader leaves the outline toward its end.
    if let Some(end) = t.leader {
        let e = [end[0] as f32, end[1] as f32];
        let d = [e[0] - c[0], e[1] - c[1]];
        // The end in the tag's frame, to clip against the outline.
        let local = [
            d[0] * right[0] + d[1] * right[1],
            d[0] * up[0] + d[1] * up[1],
        ];
        let reach = |half: f32, v: f32| {
            if v.abs() < 1e-9 {
                f32::INFINITY
            } else {
                half / v.abs()
            }
        };
        let k = reach(hx, local[0]).min(reach(hy, local[1]));
        if k < 1.0 {
            segments.push(seg([c[0] + d[0] * k, c[1] + d[1] * k], e, z));
        }
    }
    let outline = if t.frame == Frame::Hexagon {
        [
            at(-hx - tip, -hy),
            at(hx + tip, -hy),
            at(hx + tip, hy),
            at(-hx - tip, hy),
        ]
    } else {
        corners
    };
    NoteParts {
        segments,
        labels,
        outline,
    }
}

/// Whether plan point `p` is inside an outline.
pub fn outline_contains(outline: &[[f32; 2]; 4], p: [f32; 2]) -> bool {
    // A convex quadrilateral: `p` is on the same side of every edge.
    let side =
        |a: [f32; 2], b: [f32; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
    let s: Vec<f32> = (0..4)
        .map(|k| side(outline[k], outline[(k + 1) % 4]))
        .collect();
    s.iter().all(|v| *v >= 0.0) || s.iter().all(|v| *v <= 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_wrap_at_their_width_and_leaders_end_in_arrows() {
        let t = TextMark {
            position: [0.0, 0.0],
            rotation: 0.0,
            size_mm: 2.5,
            width_mm: Some(30.0),
            content: "CONCRETE SLAB ON GRADE\nSEE DETAIL".into(),
            leaders: vec![TextLeader {
                side: Side::Left,
                attach: Attach::Top,
                points: vec![[-2.0, 1.0]],
            }],
        };
        let lines = text_lines(&t, 100.0);
        assert!(lines.len() >= 3, "{lines:?}");
        assert_eq!(lines[0], "CONCRETE SLAB");
        assert_eq!(lines.last().map(String::as_str), Some("SEE DETAIL"));
        let p = text_parts(&t, 100.0, 0.0);
        // One leader segment and a three-sided arrowhead.
        assert_eq!(p.segments.len(), 4);
        assert!(outline_contains(&p.outline, [1.0, -0.2]));
        assert!(!outline_contains(&p.outline, [1.0, 0.2]));
    }

    #[test]
    fn a_tag_leader_starts_at_its_outline() {
        let t = TagMark {
            head: [0.0, 0.0],
            lines: vec!["101".into()],
            frame: Frame::Box,
            vertical: false,
            leader: Some([3.0, 0.0]),
            text_mm: 2.5,
        };
        let p = tag_parts(&t, 100.0, 0.0);
        assert_eq!(p.segments.len(), 5);
        let leader = p.segments[4];
        assert!((leader[0][0] - p.outline[1][0]).abs() < 1e-5);
        assert_eq!(leader[1][0], 3.0);
    }
}
