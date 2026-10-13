// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Drawing annotation in views (spec Ch 5 §5.13): aligned dimensions as lines (dimension line,
//! witness lines, tick marks) and their values in the stroke font. Sizes are paper sizes, scaled
//! by the view scale, so a 2.5 mm text is 250 mm tall in the model at 1:100.

use glam::{Vec2, Vec3};

use crate::font;

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
}
