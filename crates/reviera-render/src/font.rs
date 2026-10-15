// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! A small built-in stroke font for labels and annotation drawn by the renderer: capital
//! letters, digits and the punctuation of lengths (`. , ' " - /`). Lower-case letters use the
//! capital shapes, except `m`. No font files are needed, so labels look the same everywhere.
//! (Annotation text with real fonts comes with the text renderer, DOC-ANN-000.)

use glam::Vec3;

use reviera_core::stroke_font::{CELL, advance, glyph};

/// The width of `text` set `height` tall.
pub fn width(text: &str, height: f32) -> f32 {
    let unit = height / CELL;
    let total: f32 = text.chars().map(advance).sum();
    (total - 1.5).max(0.0) * unit
}

/// The strokes of `text` as line segments: starting at `origin` (bottom left), reading along
/// `right`, with `up` upward, letters `height` tall.
pub fn strokes(text: &str, origin: Vec3, right: Vec3, up: Vec3, height: f32) -> Vec<[Vec3; 2]> {
    let unit = height / CELL;
    let mut out = Vec::new();
    let mut x = 0.0;
    for c in text.chars() {
        let at = origin + right * (x * unit);
        for line in glyph(c) {
            for pair in line.windows(2) {
                let p = |(gx, gy): (f32, f32)| at + right * (gx * unit) + up * (gy * unit);
                out.push([p(pair[0]), p(pair[1])]);
            }
        }
        x += advance(c);
    }
    out
}
