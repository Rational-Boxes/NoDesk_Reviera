// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Sections and elevations (spec Ch 5 §5.4–5.5, DOC-VIEW-003/004): vertical views.
//!
//! A section or elevation is a view (`view.section` / `view.elevation`) with a `section` block: a
//! line in plan, and the far clip depth. The view looks to the right-hand side of the line's
//! direction (or the left when flipped). Geometry between the line and the far clip shows in
//! projection; what the line's vertical plane cuts shows as cut.
//!
//! Drawings use a *section frame*:
//! - x runs to the viewer's right;
//! - y is up (world z);
//! - z points back toward the viewer.
//!
//! So the plan machinery draws a section as a plan of the transformed model, with the cut at
//! z = 0 and the far clip at z = −far.
//!
//! Built so far: straight sections and single-direction elevations. Jogs, crop regions, markers
//! with several directions and reference sections come later.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::NewElement;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::views::{self, CLASS_PREFIX, VIEW_KEY, ViewData};

pub const KIND_SECTION: &str = "view.section";
pub const KIND_ELEVATION: &str = "view.elevation";
/// Default far clip depth of new sections and elevations, metres.
pub const DEFAULT_FAR: f64 = 30.0;

/// Whether a view kind is a section or an elevation.
pub fn is_section(kind: &str) -> bool {
    kind == KIND_SECTION || kind == KIND_ELEVATION
}

/// The line and depth of a section or elevation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SectionDef {
    /// The section line in plan, metres.
    pub start: [f64; 2],
    pub end: [f64; 2],
    /// How far beyond the line the view sees, metres.
    pub far: f64,
    /// Looking to the left of the line instead of the right.
    #[serde(default)]
    pub flipped: bool,
}

/// The frame a section is drawn in: x to the viewer's right, y up, z toward the viewer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectionFrame {
    /// The world point at the frame's origin (the line's end on the viewer's left, at z = 0).
    pub origin: [f64; 2],
    /// The viewer's right in plan (unit).
    pub right: [f64; 2],
    /// The direction the view looks (unit, in plan).
    pub look: [f64; 2],
    /// The line's length.
    pub width: f64,
    pub far: f64,
}

impl SectionDef {
    pub fn frame(&self) -> SectionFrame {
        let d = [self.end[0] - self.start[0], self.end[1] - self.start[1]];
        let l = d[0].hypot(d[1]).max(1e-12);
        let d = [d[0] / l, d[1] / l];
        // To the right of the drawn direction, or to the left when flipped.
        let look = if self.flipped {
            [-d[1], d[0]]
        } else {
            [d[1], -d[0]]
        };
        // The viewer's right: look × up.
        let right = [look[1], -look[0]];
        let dot = |p: [f64; 2]| p[0] * right[0] + p[1] * right[1];
        let origin = if dot(self.start) <= dot(self.end) {
            self.start
        } else {
            self.end
        };
        SectionFrame {
            origin,
            right,
            look,
            width: l,
            far: self.far,
        }
    }
}

impl SectionFrame {
    /// A world point in the frame.
    pub fn to_local(&self, p: [f64; 3]) -> [f64; 3] {
        let d = [p[0] - self.origin[0], p[1] - self.origin[1]];
        [
            d[0] * self.right[0] + d[1] * self.right[1],
            p[2],
            -(d[0] * self.look[0] + d[1] * self.look[1]),
        ]
    }

    /// A frame point in the world.
    pub fn to_world(&self, q: [f64; 3]) -> [f64; 3] {
        let (x, depth) = (q[0], -q[2]);
        [
            self.origin[0] + self.right[0] * x + self.look[0] * depth,
            self.origin[1] + self.right[1] * x + self.look[1] * depth,
            q[1],
        ]
    }

    /// Where a grid line shows in the view: its line extended to the view plane, as x in the
    /// frame, when part of it lies in the view's depth and it is not parallel to the plane.
    pub fn grid_crossing(&self, a: [f64; 2], b: [f64; 2]) -> Option<f64> {
        let (la, lb) = (
            self.to_local([a[0], a[1], 0.0]),
            self.to_local([b[0], b[1], 0.0]),
        );
        if (la[2] - lb[2]).abs() < 1e-9 {
            return None;
        }
        // Some of the segment between the plane (z = 0) and the far clip (z = −far).
        let (lo, hi) = (la[2].min(lb[2]), la[2].max(lb[2]));
        if hi < -self.far || lo > 0.0 {
            return None;
        }
        let t = la[2] / (la[2] - lb[2]);
        let x = la[0] + (lb[0] - la[0]) * t;
        (-1e-9..=self.width + 1e-9).contains(&x).then_some(x)
    }

    /// Where a plan line crosses the section plane, as x in the frame, if it does within the
    /// section's width.
    pub fn crossing(&self, a: [f64; 2], b: [f64; 2]) -> Option<f64> {
        let (la, lb) = (
            self.to_local([a[0], a[1], 0.0]),
            self.to_local([b[0], b[1], 0.0]),
        );
        let (za, zb) = (la[2], lb[2]);
        if (za > 0.0) == (zb > 0.0) || (za - zb).abs() < 1e-12 {
            return None;
        }
        let t = za / (za - zb);
        let x = la[0] + (lb[0] - la[0]) * t;
        (-1e-9..=self.width + 1e-9).contains(&x).then_some(x)
    }
}

/// The frame of the section or elevation view with GlobalId `guid`, and whether it is an
/// elevation; `None` for other views.
pub fn frame_of_view(store: &Store, guid: &str) -> Option<(SectionFrame, bool)> {
    let e = store.get(store.find_by_guid(&GlobalId::parse(guid)?)?)?;
    let (kind, def) = section_of(e)?;
    Some((def.frame(), kind == KIND_ELEVATION))
}

/// The section block of a view element.
pub fn section_of(e: &crate::Element) -> Option<(String, SectionDef)> {
    let v = e.param(VIEW_KEY)?;
    let kind = v.get("kind")?.as_str()?;
    if !is_section(kind) {
        return None;
    }
    Some((
        kind.to_owned(),
        serde_json::from_value(v.get("section")?.clone()).ok()?,
    ))
}

fn free_name(store: &Store, kind: &str, base: &str) -> String {
    let existing: Vec<String> = store
        .iter()
        .filter(|e| views::view_data(e).is_some_and(|d| d.kind == kind))
        .filter_map(|e| e.name.clone())
        .collect();
    if !base.is_empty() && !existing.iter().any(|n| n == base) {
        return base.to_owned();
    }
    let stem = if base.is_empty() {
        if kind == KIND_SECTION {
            "Section"
        } else {
            "Elevation"
        }
    } else {
        base
    };
    (1..)
        .map(|n| format!("{stem} {n}"))
        .find(|n| !existing.contains(n))
        .expect("a free name")
}

fn create(
    txn: &mut crate::txn::Transaction<'_>,
    kind: &str,
    name: String,
    def: SectionDef,
) -> Result<ElementId> {
    let data = ViewData {
        kind: kind.into(),
        camera: None,
        home: None,
        level: None,
        view_range: None,
        section_box: None,
        slice_planes: Vec::new(),
        section: Some(def),
        color_scheme: None,
        area_scheme: None,
        detail_level: None,
    };
    let value = serde_json::to_value(&data).expect("view data serialises");
    let guid = GlobalId::new();
    txn.create(
        NewElement::new(format!("{CLASS_PREFIX}{}", guid.as_str()))
            .guid(guid)
            .name(name)
            .param(VIEW_KEY, value),
    )
}

fn check(def: &SectionDef) -> Result<()> {
    let l = (def.end[0] - def.start[0]).hypot(def.end[1] - def.start[1]);
    if l.is_nan() || l <= 0.01 {
        return Err(Error::new(
            "view.section_too_short",
            "a section line must be longer than 10 mm",
        ));
    }
    if !(def.far > 0.0 && def.far.is_finite()) {
        return Err(Error::new(
            "view.bad_far_clip",
            "the far clip depth must be greater than 0",
        ));
    }
    Ok(())
}

fn length(store: &Store, v: &Value) -> Result<f64> {
    match v {
        Value::Number(n) => Ok(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => crate::units::parse(s, "length", &crate::units::settings_of(store)),
        _ => Err(Error::new(
            "view.invalid_value",
            "lengths are numbers or text with units",
        )),
    }
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateSectionParams {
    /// The section line's start and end in plan (metres); the view looks to its right.
    pub start: [f64; 2],
    pub end: [f64; 2],
    /// How far the view sees beyond the line (metres or text with units); default 30 m.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub far_offset: Option<Value>,
    /// Default `Section 1`, `Section 2`, …
    pub name: Option<String>,
}

pub(crate) fn create_section(
    txn: &mut crate::txn::Transaction<'_>,
    p: CreateSectionParams,
) -> Result<ElementId> {
    let store = txn.store().clone();
    let far = p
        .far_offset
        .as_ref()
        .map(|v| length(&store, v))
        .transpose()?
        .unwrap_or(DEFAULT_FAR);
    let def = SectionDef {
        start: p.start,
        end: p.end,
        far,
        flipped: false,
    };
    check(&def)?;
    let name = free_name(&store, KIND_SECTION, p.name.as_deref().unwrap_or(""));
    create(txn, KIND_SECTION, name, def)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateElevationParams {
    /// Where the view plane is, in plan (metres).
    pub point: [f64; 2],
    /// The direction the view looks, in plan (need not be unit length).
    pub look: [f64; 2],
    /// The view plane's width (metres); default 60 m.
    pub width: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub far_offset: Option<Value>,
    pub name: Option<String>,
}

pub(crate) fn create_elevation(
    txn: &mut crate::txn::Transaction<'_>,
    p: CreateElevationParams,
) -> Result<ElementId> {
    let store = txn.store().clone();
    let l = p.look[0].hypot(p.look[1]);
    if l.is_nan() || l <= 1e-9 {
        return Err(Error::new(
            "view.bad_direction",
            "the look direction is zero",
        ));
    }
    let look = [p.look[0] / l, p.look[1] / l];
    let half = p.width.unwrap_or(60.0) / 2.0;
    // The line runs so that the view looks to its right: along (−look.y, look.x).
    let d = [-look[1], look[0]];
    let def = SectionDef {
        start: [p.point[0] - d[0] * half, p.point[1] - d[1] * half],
        end: [p.point[0] + d[0] * half, p.point[1] + d[1] * half],
        far: p
            .far_offset
            .as_ref()
            .map(|v| length(&store, v))
            .transpose()?
            .unwrap_or(DEFAULT_FAR * 4.0),
        flipped: false,
    };
    check(&def)?;
    let name = free_name(&store, KIND_ELEVATION, p.name.as_deref().unwrap_or(""));
    create(txn, KIND_ELEVATION, name, def)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditParams {
    pub view_id: ElementRef,
    pub start: Option<[f64; 2]>,
    pub end: Option<[f64; 2]>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub far_offset: Option<Value>,
    /// Turn the view to look the other way.
    #[serde(default)]
    pub flip: bool,
}

/// Moves a section's line, changes its far clip, or flips it (`section.edit`, `section.flip`).
pub(crate) fn edit(txn: &mut crate::txn::Transaction<'_>, p: EditParams) -> Result<()> {
    let store = txn.store().clone();
    let id = store
        .resolve(&p.view_id)
        .ok_or_else(|| Error::new("view.not_found", "no such view"))?;
    let e = store.get(id).expect("resolved");
    let (_, mut def) = section_of(e).ok_or_else(|| {
        Error::new(
            "view.not_a_section",
            format!("view {id} is not a section or elevation"),
        )
    })?;
    if let Some(s) = p.start {
        def.start = s;
    }
    if let Some(s) = p.end {
        def.end = s;
    }
    if let Some(f) = &p.far_offset {
        def.far = length(&store, f)?;
    }
    if p.flip {
        def.flipped = !def.flipped;
    }
    check(&def)?;
    let mut value = e.param(VIEW_KEY).cloned().expect("a view");
    value["section"] = serde_json::to_value(def).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(VIEW_KEY.into(), value);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_section_drawn_east_looks_south() {
        let def = SectionDef {
            start: [0.0, 0.0],
            end: [10.0, 0.0],
            far: 5.0,
            flipped: false,
        };
        let f = def.frame();
        assert_eq!(f.look, [0.0, -1.0]);
        // Looking south, the viewer's right is west: the line's east end is on the left.
        assert_eq!(f.right, [-1.0, 0.0]);
        assert_eq!(f.origin, [10.0, 0.0]);
        // A point 2 m south (beyond the line) at x = 3 is 7 m to the right, 2 m deep.
        let q = f.to_local([3.0, -2.0, 1.5]);
        assert!(
            (q[0] - 7.0).abs() < 1e-12 && (q[1] - 1.5).abs() < 1e-12 && (q[2] + 2.0).abs() < 1e-12
        );
        let back = f.to_world(q);
        assert!((back[0] - 3.0).abs() < 1e-12 && (back[1] + 2.0).abs() < 1e-12);
        // A wall running north–south at x = 4 crosses the plane at 6 m from the left.
        assert!((f.crossing([4.0, -5.0], [4.0, 5.0]).unwrap() - 6.0).abs() < 1e-12);
        assert!(f.crossing([14.0, -5.0], [14.0, 5.0]).is_none());
        // Flipped, it looks north and its right is east.
        let g = SectionDef {
            flipped: true,
            ..def
        }
        .frame();
        assert_eq!(g.look, [0.0, 1.0]);
        assert_eq!(g.right, [1.0, 0.0]);
        // Right-handed: right × up = toward the viewer (−look).
        let (r, l) = (f.right, f.look);
        let z = [r[1], -r[0]];
        assert_eq!(z, [-l[0], -l[1]]);
    }
}
