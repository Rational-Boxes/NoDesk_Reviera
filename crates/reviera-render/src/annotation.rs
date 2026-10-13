// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Drawing annotation in views (spec Ch 5 §5.13): aligned dimensions as lines (dimension line,
//! witness lines, tick marks) and their values in the stroke font. Sizes are paper sizes, scaled
//! by the view scale, so a 2.5 mm text is 250 mm tall in the model at 1:100.

use glam::{Vec2, Vec3};

use crate::font;
use crate::plan::Lines;

/// Paper sizes in millimetres.
pub const TEXT_HEIGHT_MM: f32 = 2.5;
const TICK_MM: f32 = 3.0;
const WITNESS_GAP_MM: f32 = 1.5;
const WITNESS_EXTENSION_MM: f32 = 2.0;
const TEXT_OFFSET_MM: f32 = 1.0;

/// One dimension to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct Dimension {
    /// Points on the references (plan, metres), in order.
    pub points: Vec<[f64; 2]>,
    /// Unit direction of the references (witness lines run along it).
    pub along: [f64; 2],
    /// A point on the dimension line.
    pub line: [f64; 2],
    /// The text of each segment.
    pub texts: Vec<String>,
}

/// The line segments of a dimension at elevation `z`, for a view at 1:`scale`.
/// A text label: its baseline start, reading and up directions (unit), and height, in model
/// units.
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub origin: [f32; 3],
    pub right: [f32; 3],
    pub up: [f32; 3],
    pub height: f32,
    pub text: String,
}

/// The line segments of a dimension at elevation `z`, for a view at 1:`scale`, with its values
/// drawn in the stroke font.
pub fn dimension_lines(d: &Dimension, scale: f32, z: f32) -> Vec<[[f32; 3]; 2]> {
    let (mut lines, labels) = dimension_parts(d, scale, z);
    for l in labels {
        for [p, q] in font::strokes(
            &l.text,
            l.origin.into(),
            l.right.into(),
            l.up.into(),
            l.height,
        ) {
            lines.push([p.to_array(), q.to_array()]);
        }
    }
    lines
}

/// The lines of a dimension and its value labels, separately (output with real text).
pub fn dimension_parts(d: &Dimension, scale: f32, z: f32) -> (Vec<[[f32; 3]; 2]>, Vec<Label>) {
    let mut out: Vec<[[f32; 3]; 2]> = Vec::new();
    let mut push = |a: Vec3, b: Vec3| out.push([a.to_array(), b.to_array()]);
    let mm = scale / 1000.0;
    let v = |p: [f64; 2]| Vec2::new(p[0] as f32, p[1] as f32);
    let along = v(d.along).normalize_or_zero();
    let across = Vec2::new(-along.y, along.x);
    let line = v(d.line);
    let at = |p: Vec2| Vec3::new(p.x, p.y, z);
    // Feet of the references on the dimension line, in order across.
    let feet: Vec<Vec2> = d
        .points
        .iter()
        .map(|p| {
            let p = v(*p);
            p + along * (line - p).dot(along)
        })
        .collect();
    let (Some(first), Some(last)) = (feet.first(), feet.last()) else {
        return (Vec::new(), Vec::new());
    };
    // The dimension line, past the outer ticks.
    let dir = (*last - *first).normalize_or_zero();
    let dir = if dir == Vec2::ZERO { across } else { dir };
    let tick = TICK_MM * mm;
    push(at(*first - dir * tick * 0.6), at(*last + dir * tick * 0.6));
    for (p, foot) in d.points.iter().map(|p| v(*p)).zip(&feet) {
        // Witness line: from a gap off the reference to a little past the dimension line.
        let toward = (*foot - p).normalize_or_zero();
        let length = (*foot - p).length();
        if length > WITNESS_GAP_MM * mm {
            push(
                at(p + toward * WITNESS_GAP_MM * mm),
                at(*foot + toward * WITNESS_EXTENSION_MM * mm),
            );
        }
        // Architectural tick: a short diagonal through the foot.
        let diagonal = (dir + along).normalize_or_zero();
        let diagonal = if diagonal == Vec2::ZERO {
            dir
        } else {
            diagonal
        };
        push(
            at(*foot - diagonal * tick / 2.0),
            at(*foot + diagonal * tick / 2.0),
        );
    }
    // Values: centred on each segment, above the line, reading from the bottom or the right.
    let height = TEXT_HEIGHT_MM * mm;
    let mut reading = dir;
    if reading.x < -1e-6 || (reading.x.abs() <= 1e-6 && reading.y < 0.0) {
        reading = -reading;
    }
    let up = reading.perp();
    let mut labels = Vec::new();
    for (k, text) in d.texts.iter().enumerate() {
        let (Some(a), Some(b)) = (feet.get(k), feet.get(k + 1)) else {
            break;
        };
        let mid = (*a + *b) / 2.0;
        let w = font::width(text, height);
        let origin = mid - reading * (w / 2.0) + up * (TEXT_OFFSET_MM * mm);
        labels.push(Label {
            origin: at(origin).to_array(),
            right: [reading.x, reading.y, 0.0],
            up: [up.x, up.y, 0.0],
            height,
            text: text.clone(),
        });
    }
    (out, labels)
}

/// Grid bubble diameter and name height, in paper millimetres.
pub const BUBBLE_MM: f32 = 8.0;
const GRID_TEXT_MM: f32 = 3.5;
const BUBBLE_SEGMENTS: usize = 32;

/// Dash and gap lengths of a line pattern in paper millimetres (dash first); empty for solid.
pub fn pattern_mm(name: &str) -> &'static [f32] {
    match name {
        "dash" => &[6.0, 3.0],
        "center" => &[12.0, 3.0, 3.0, 3.0],
        "dash_dot" => &[6.0, 2.0, 0.5, 2.0],
        "dot" => &[0.5, 1.5],
        _ => &[],
    }
}

/// How one part of a grid line is drawn: colour, width and dash pattern on paper.
#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    pub color: [f32; 4],
    /// Paper millimetres.
    pub width: f32,
    /// Dash and gap lengths, paper millimetres; empty for solid.
    pub pattern: Vec<f32>,
}

impl Stroke {
    pub fn solid(color: [f32; 4], width: f32) -> Self {
        Self {
            color,
            width,
            pattern: Vec::new(),
        }
    }
}

/// How a grid is drawn (from its grid type, ARC-GRID-001).
#[derive(Debug, Clone, PartialEq)]
pub struct GridStyle {
    /// Bubbles are drawn at all (the type's symbol).
    pub symbol: bool,
    /// The ends of the line; the whole line when `center` is `Continuous`.
    pub end: Stroke,
    pub center: CenterStroke,
    /// Length of each end segment, paper millimetres.
    pub end_length_mm: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CenterStroke {
    Continuous,
    /// A gap between the end segments.
    None,
    Custom(Stroke),
}

impl Default for GridStyle {
    fn default() -> Self {
        Self::of(&reviera_core::grids::GridType::default(), &|_| 0.18)
    }
}

/// `#RRGGBB` to linear RGBA (black when malformed).
pub fn hex_color(hex: &str) -> [f32; 4] {
    let c = |i: usize| {
        let v = hex
            .get(i..i + 2)
            .and_then(|h| u8::from_str_radix(h, 16).ok())
            .unwrap_or(0);
        (f32::from(v) / 255.0).powf(2.2)
    };
    [c(1), c(3), c(5), 1.0]
}

impl GridStyle {
    /// The style of a grid type; `pen_width` gives an annotation pen's paper width.
    pub fn of(t: &reviera_core::grids::GridType, pen_width: &dyn Fn(u8) -> f32) -> Self {
        use reviera_core::grids::{CenterSegment, SegmentStyle, Symbol};
        let stroke = |s: &SegmentStyle| Stroke {
            color: hex_color(&s.colour),
            width: pen_width(s.pen),
            pattern: pattern_mm(&s.pattern).to_vec(),
        };
        Self {
            symbol: t.symbol == Symbol::Bubble,
            end: stroke(&t.end),
            center: match t.center_segment {
                CenterSegment::Continuous => CenterStroke::Continuous,
                CenterSegment::None => CenterStroke::None,
                CenterSegment::Custom => CenterStroke::Custom(stroke(&t.center)),
            },
            end_length_mm: t.end_length_mm as f32,
        }
    }
}

/// One grid line to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct GridMark {
    pub start: [f64; 2],
    pub end: [f64; 2],
    pub name: String,
    /// A bubble with the name at the start and at the end.
    pub bubbles: [bool; 2],
    pub style: GridStyle,
}

/// The segments of `a`–`b` drawn with a dash pattern of model lengths (solid when empty). The
/// pattern starts at `a`.
fn dashed(a: Vec2, b: Vec2, pattern: &[f32], z: f32) -> Vec<[[f32; 3]; 2]> {
    let at = |p: Vec2| [p.x, p.y, z];
    let length = (b - a).length();
    let total: f32 = pattern.iter().sum();
    if pattern.is_empty() || total <= f32::EPSILON || length <= f32::EPSILON {
        return vec![[at(a), at(b)]];
    }
    let dir = (b - a) / length;
    let mut out = Vec::new();
    let mut t = 0.0;
    'outer: loop {
        for (k, len) in pattern.iter().enumerate() {
            let next = (t + len).min(length);
            if k % 2 == 0 && next > t {
                out.push([at(a + dir * t), at(a + dir * next)]);
            }
            t = next;
            if t >= length {
                break 'outer;
            }
        }
    }
    out
}

/// The lines of a grid, grouped by stroke (line, bubbles), and its name labels, at elevation
/// `z`, for a view at 1:`scale` (ARC-GRID-001). Each bubble sits beyond its end of the line,
/// with the name centred in it.
pub fn grid_parts(g: &GridMark, scale: f32, z: f32) -> (Vec<Lines>, Vec<Label>) {
    let v = |p: [f64; 2]| Vec2::new(p[0] as f32, p[1] as f32);
    let (start, end) = (v(g.start), v(g.end));
    let length = (end - start).length();
    if length <= f32::EPSILON {
        return (Vec::new(), Vec::new());
    }
    let dir = (end - start) / length;
    let at = |p: Vec2| [p.x, p.y, z];
    let mm = scale / 1000.0;
    let model = |pattern: &[f32]| pattern.iter().map(|l| l * mm).collect::<Vec<f32>>();
    let st = &g.style;
    let mut ends = Lines {
        color: st.end.color,
        width: st.end.width,
        segments: Vec::new(),
    };
    let mut lines = Vec::new();
    match &st.center {
        CenterStroke::Continuous => {
            ends.segments
                .extend(dashed(start, end, &model(&st.end.pattern), z));
        }
        center => {
            let l = (st.end_length_mm * mm).min(length / 2.0);
            let pattern = model(&st.end.pattern);
            ends.segments
                .extend(dashed(start, start + dir * l, &pattern, z));
            ends.segments
                .extend(dashed(end, end - dir * l, &pattern, z));
            if let CenterStroke::Custom(c) = center
                && length > 2.0 * l
            {
                lines.push(Lines {
                    color: c.color,
                    width: c.width,
                    segments: dashed(start + dir * l, end - dir * l, &model(&c.pattern), z),
                });
            }
        }
    }
    let radius = BUBBLE_MM * mm / 2.0;
    let mut labels = Vec::new();
    for (shown, point, outward) in [(g.bubbles[0], start, -dir), (g.bubbles[1], end, dir)] {
        if !shown || !st.symbol {
            continue;
        }
        let centre = point + outward * radius;
        let ring: Vec<Vec2> = (0..=BUBBLE_SEGMENTS)
            .map(|k| {
                let a = k as f32 / BUBBLE_SEGMENTS as f32 * std::f32::consts::TAU;
                centre + Vec2::new(a.cos(), a.sin()) * radius
            })
            .collect();
        ends.segments
            .extend(ring.windows(2).map(|w| [at(w[0]), at(w[1])]));
        // Names read horizontally, centred in the bubble.
        let height = GRID_TEXT_MM * mm;
        let width = font::width(&g.name, height);
        let origin = centre - Vec2::new(width / 2.0, height / 2.0);
        labels.push(Label {
            origin: at(origin),
            right: [1.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            height,
            text: g.name.clone(),
        });
    }
    lines.insert(0, ends);
    (lines, labels)
}

/// The lines of a grid with its names drawn in the stroke font (with the end stroke).
pub fn grid_lines(g: &GridMark, scale: f32, z: f32) -> Vec<Lines> {
    let (mut lines, labels) = grid_parts(g, scale, z);
    for l in labels {
        let text = font::strokes(
            &l.text,
            l.origin.into(),
            l.right.into(),
            l.up.into(),
            l.height,
        );
        if let Some(first) = lines.first_mut() {
            first
                .segments
                .extend(text.into_iter().map(|[p, q]| [p.to_array(), q.to_array()]));
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dimension_has_its_line_witnesses_ticks_and_text() {
        let d = Dimension {
            points: vec![[0.0, 0.1], [0.0, 3.9]],
            along: [1.0, 0.0],
            line: [5.0, 0.0],
            texts: vec!["3800".into()],
        };
        let lines = dimension_lines(&d, 100.0, 1.2);
        // Dimension line along x = 5 from y ≈ 0.1 to 3.9, extended past the ticks.
        let dim = lines[0];
        assert!((dim[0][0] - 5.0).abs() < 1e-5 && (dim[1][0] - 5.0).abs() < 1e-5);
        assert!(dim[0][1] < 0.1 && dim[1][1] > 3.9);
        // 1 dimension line, 2 witnesses, 2 ticks, then the strokes of "3800".
        assert!(lines.len() > 5 + 8, "{}", lines.len());
        // The text is 250 mm tall at 1:100 and sits beside the line.
        let text = &lines[5..];
        let xs: Vec<f32> = text.iter().flat_map(|s| [s[0][0], s[1][0]]).collect();
        let (lo, hi) = xs
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(*x), b.max(*x)));
        assert!((hi - lo - 0.25).abs() < 0.02, "{lo} {hi}");
        assert!(lines.iter().all(|s| s[0][2] == 1.2));
    }

    #[test]
    fn a_grid_has_its_line_a_bubble_beyond_the_end_and_its_name() {
        let g = GridMark {
            start: [0.0, 0.0],
            end: [0.0, 10.0],
            name: "A".into(),
            bubbles: [false, true],
            style: GridStyle::default(),
        };
        let (lines, labels) = grid_parts(&g, 100.0, 0.5);
        assert_eq!(lines.len(), 1);
        let segs = &lines[0].segments;
        assert_eq!(segs[0], [[0.0, 0.0, 0.5], [0.0, 10.0, 0.5]]);
        assert_eq!(segs.len(), 1 + BUBBLE_SEGMENTS);
        // At 1:100 the bubble is 800 mm across, centred 400 mm past the end.
        let ys: Vec<f32> = segs[1..].iter().map(|s| s[0][1]).collect();
        let (lo, hi) = ys
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), y| (a.min(*y), b.max(*y)));
        assert!(
            (lo - 10.0).abs() < 1e-4 && (hi - 10.8).abs() < 1e-4,
            "{lo} {hi}"
        );
        assert_eq!(labels.len(), 1);
        assert!((labels[0].origin[1] + labels[0].height / 2.0 - 10.4).abs() < 1e-4);
        assert!(grid_lines(&g, 100.0, 0.5)[0].segments.len() > segs.len());
    }

    #[test]
    fn grid_types_gap_or_restyle_the_middle_and_can_drop_the_bubble() {
        let mut t = reviera_core::grids::GridType {
            center_segment: reviera_core::grids::CenterSegment::None,
            symbol: reviera_core::grids::Symbol::None,
            ..Default::default()
        };
        let g = GridMark {
            start: [0.0, 0.0],
            end: [0.0, 10.0],
            name: "1".into(),
            bubbles: [true, true],
            style: GridStyle::of(&t, &|_| 0.25),
        };
        // 1:100: two 1.5 m end segments, no bubbles or labels.
        let (lines, labels) = grid_parts(&g, 100.0, 0.0);
        assert!(labels.is_empty());
        assert_eq!(lines.len(), 1);
        let total: f32 = lines[0]
            .segments
            .iter()
            .map(|s| (s[1][1] - s[0][1]).abs())
            .sum();
        assert!((total - 3.0).abs() < 1e-4, "{total}");

        // Custom: a dashed red middle on its own pen.
        t.center_segment = reviera_core::grids::CenterSegment::Custom;
        t.center.colour = "#FF0000".into();
        t.center.pattern = "dash".into();
        t.center.pen = 3;
        let g = GridMark {
            style: GridStyle::of(&t, &|p| f32::from(p) / 10.0),
            ..g
        };
        let (lines, _) = grid_parts(&g, 100.0, 0.0);
        assert_eq!(lines.len(), 2);
        let middle = &lines[1];
        assert_eq!(middle.color, [1.0, 0.0, 0.0, 1.0]);
        assert!((middle.width - 0.3).abs() < 1e-6);
        // 7 m of 0.6 m dashes and 0.3 m gaps.
        assert_eq!(middle.segments.len(), 8);
    }
}
