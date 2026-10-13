// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Snapping while drawing (spec Ch 1, snaps): the point under the cursor moves to a nearby wall end,
//! a wall midpoint, a point on a wall's location line, or the nearest 15° direction from the
//! previous point. Distances are measured on screen, so snapping feels the same at any zoom.

/// What the point snapped to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SnapKind {
    Endpoint,
    Midpoint,
    /// On a location line, away from its ends (a T junction).
    Nearest,
    /// Along a direction from the previous point, in degrees from +X.
    Angle(f64),
    None,
}

impl SnapKind {
    pub fn label(&self) -> String {
        match self {
            SnapKind::Endpoint => "Endpoint".into(),
            SnapKind::Midpoint => "Midpoint".into(),
            SnapKind::Nearest => "Nearest".into(),
            // `+ 0.0` turns -0 into 0.
            SnapKind::Angle(a) => format!("{:.0}°", a.round() + 0.0),
            SnapKind::None => String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snap {
    pub point: [f64; 2],
    pub kind: SnapKind,
}

/// Snaps to points closer than this many pixels.
pub const RADIUS_PX: f32 = 10.0;
/// Directions are snapped in steps of this many degrees…
const ANGLE_STEP: f64 = 15.0;
/// …when the cursor's direction is within this many degrees of one.
const ANGLE_TOLERANCE: f64 = 2.0;

/// Snaps `raw` (the point on the drawing plane under `cursor`). `lines` are the location lines of
/// walls; `to_screen` gives the pixel position of a plane point.
pub fn snap(
    raw: [f64; 2],
    cursor: (f32, f32),
    last: Option<[f64; 2]>,
    lines: &[([f64; 2], [f64; 2])],
    to_screen: impl Fn([f64; 2]) -> Option<(f32, f32)>,
) -> Snap {
    let pixels =
        |p: [f64; 2]| to_screen(p).map_or(f32::MAX, |(x, y)| (x - cursor.0).hypot(y - cursor.1));
    let closest = |points: &mut dyn Iterator<Item = [f64; 2]>| {
        points
            .map(|p| (pixels(p), p))
            .filter(|(d, _)| *d <= RADIUS_PX)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, p)| p)
    };
    if let Some(p) = closest(&mut lines.iter().flat_map(|(a, b)| [*a, *b])) {
        return Snap {
            point: p,
            kind: SnapKind::Endpoint,
        };
    }
    let mid = |(a, b): &([f64; 2], [f64; 2])| [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    if let Some(p) = closest(&mut lines.iter().map(mid)) {
        return Snap {
            point: p,
            kind: SnapKind::Midpoint,
        };
    }
    if let Some(p) = closest(&mut lines.iter().map(|(a, b)| nearest_on(raw, *a, *b))) {
        return Snap {
            point: p,
            kind: SnapKind::Nearest,
        };
    }
    if let Some(l) = last {
        let (dx, dy) = (raw[0] - l[0], raw[1] - l[1]);
        let length = dx.hypot(dy);
        if length > 1e-9 {
            let angle = dy.atan2(dx).to_degrees();
            let snapped = (angle / ANGLE_STEP).round() * ANGLE_STEP;
            if (angle - snapped).abs() <= ANGLE_TOLERANCE {
                // Keep the distance along the snapped direction.
                let r = snapped.to_radians();
                let along = dx * r.cos() + dy * r.sin();
                let shown = if snapped <= -180.0 {
                    snapped + 360.0
                } else {
                    snapped
                };
                return Snap {
                    point: [l[0] + along * r.cos(), l[1] + along * r.sin()],
                    kind: SnapKind::Angle(shown.rem_euclid(360.0)),
                };
            }
        }
    }
    Snap {
        point: raw,
        kind: SnapKind::None,
    }
}

/// The point of segment `a`–`b` nearest to `p`.
fn nearest_on(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-18 {
        return a;
    }
    let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0);
    [a[0] + t * dx, a[1] + t * dy]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plan view at 100 pixels per metre.
    fn screen(p: [f64; 2]) -> Option<(f32, f32)> {
        Some(((p[0] * 100.0) as f32, (-p[1] * 100.0) as f32))
    }

    fn at(p: [f64; 2]) -> ([f64; 2], (f32, f32)) {
        (p, screen(p).unwrap())
    }

    const WALL: ([f64; 2], [f64; 2]) = ([0.0, 0.0], [4.0, 0.0]);

    #[test]
    fn ends_win_over_midpoints_and_lines() {
        let (raw, cursor) = at([4.05, 0.03]);
        let s = snap(raw, cursor, None, &[WALL], screen);
        assert_eq!(
            s,
            Snap {
                point: [4.0, 0.0],
                kind: SnapKind::Endpoint
            }
        );
        let (raw, cursor) = at([2.02, 0.05]);
        assert_eq!(
            snap(raw, cursor, None, &[WALL], screen).kind,
            SnapKind::Midpoint
        );
        let (raw, cursor) = at([3.0, 0.05]);
        let s = snap(raw, cursor, None, &[WALL], screen);
        assert_eq!(s.kind, SnapKind::Nearest);
        assert!((s.point[0] - 3.0).abs() < 1e-12 && s.point[1] == 0.0);
    }

    #[test]
    fn far_points_snap_to_angles_from_the_last_point() {
        let (raw, cursor) = at([3.0, 5.03]);
        let s = snap(raw, cursor, Some([3.0, 1.0]), &[WALL], screen);
        assert_eq!(s.kind, SnapKind::Angle(90.0));
        assert!((s.point[0] - 3.0).abs() < 1e-12 && (s.point[1] - 5.03).abs() < 1e-12);
        let (raw, cursor) = at([-2.0, 1.02]);
        assert_eq!(
            snap(raw, cursor, Some([0.0, 1.0]), &[], screen).kind,
            SnapKind::Angle(180.0)
        );
        // 24° is more than 2° from any 15° step: no snap.
        let (raw, cursor) = at([1.0, 1.0 + 0.45]);
        let s = snap(raw, cursor, Some([0.0, 1.0]), &[], screen);
        assert_eq!(
            s,
            Snap {
                point: raw,
                kind: SnapKind::None
            }
        );
    }
}
