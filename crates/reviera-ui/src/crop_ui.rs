// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Crop rows in a plan's, section's or elevation's Properties (DOC-GFX-015): Crop View, Crop
//! Region Visible, Annotation Crop, and Reset Crop (fit the model). The drawing is cropped in
//! `plans`, `sections` and `inspect`.

use reviera_core::ElementId;
use reviera_render::RenderBackend;
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

impl Gui {
    /// Appends the crop rows of view `element`.
    pub(crate) fn crop_rows(&self, grid: &PropertyGrid, element: ElementId) {
        let Some(store) = self.doc.and_then(|d| self.app.document(d)).map(|d| d.doc.store()) else { return };
        let Some(e) = store.get(element) else { return };
        let crop = reviera_core::crop::effective(store, e);
        let boxed = reviera_core::crop::follows_scope_box(store, e);
        let annotation = reviera_core::crop::annotation_crop_of(e);
        grid.append(Property::category("Extents", "c_crop"));
        grid.append(Property::boolean("Crop View", "crop_on", crop.is_some_and(|c| c.enabled)));
        grid.append(Property::boolean("Crop Region Visible", "crop_visible", crop.is_some_and(|c| c.visible)));
        grid.append(Property::boolean("Annotation Crop", "crop_annotation", annotation.enabled));
        grid.set_help_string("crop_annotation", "Annotation crossing the annotation crop is clipped; annotation outside it is hidden");
        if boxed {
            grid.set_help_string("crop_on", "The crop follows the view's scope box; move or resize the box to change it");
        } else {
            grid.append(Property::boolean("Reset Crop", "crop_reset", false));
            grid.set_help_string("crop_reset", "Tick to fit the crop region to the model again");
        }
    }

    /// An edit of a crop row of the active view; `false` if `property` is not one.
    pub(crate) fn crop_edit(&mut self, property: &str, text: Option<String>) -> bool {
        if !property.starts_with("crop_") {
            return false;
        }
        let Some(view) = self.active_view().filter(|v| v.plan.is_some() || v.section.is_some()).and_then(|v| v.element) else { return false };
        let on = text.as_deref().is_some_and(|t| matches!(t, "True" | "true" | "1" | "Yes" | "yes"));
        let result = match property {
            "crop_on" => self.run("view.set_crop", json!({ "view_id": view, "enabled": on })),
            "crop_visible" => self.run("view.set_crop", json!({ "view_id": view, "visible": on })),
            "crop_annotation" => self.run("view.set_annotation_crop", json!({ "view_id": view, "enabled": on })),
            "crop_reset" if on => self.run("view.reset_crop", json!({ "view_id": view })),
            _ => return true,
        };
        if let Err(e) = result {
            error(&self.frame, &format!("The crop was not changed:\n{e}"));
        }
        let f = self.frame;
        self.refresh_scene(&f);
        self.properties_stale = true;
        true
    }
}

/// How near a crop edge a press grabs it (pixels).
const EDGE_PX: f32 = 6.0;
const GHOST: [f32; 4] = [0.1, 0.45, 0.95, 1.0];

/// Dragging a visible crop region's edges: which sides move (left, right, bottom, top; two at a
/// corner), the region at the start, and the point the drag started from (the view's 2D
/// coordinates).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CropDrag {
    pub view: ElementId,
    pub sides: [bool; 4],
    pub start: ([f64; 2], [f64; 2]),
    pub from: [f64; 2],
}

impl CropDrag {
    /// The region dragged to `to`, kept at least 50 mm each way.
    fn region(&self, to: [f64; 2]) -> ([f64; 2], [f64; 2]) {
        let (d, (mut lo, mut hi)) = ([to[0] - self.from[0], to[1] - self.from[1]], self.start);
        const MIN: f64 = 0.05;
        if self.sides[0] {
            lo[0] = (lo[0] + d[0]).min(hi[0] - MIN);
        }
        if self.sides[1] {
            hi[0] = (hi[0] + d[0]).max(lo[0] + MIN);
        }
        if self.sides[2] {
            lo[1] = (lo[1] + d[1]).min(hi[1] - MIN);
        }
        if self.sides[3] {
            hi[1] = (hi[1] + d[1]).max(lo[1] + MIN);
        }
        (lo, hi)
    }
}

impl Gui {
    /// A press at (`x`, `y`) on an edge of the active view's visible crop region (not one that
    /// follows a scope box): the drag that moves it.
    pub(crate) fn crop_grip_at(&self, x: i32, y: i32) -> Option<CropDrag> {
        let v = self.active_view().filter(|v| v.plan.is_some() || v.section.is_some())?;
        let view = v.element?;
        let store = self.app.document(v.doc)?.doc.store();
        let e = store.get(view)?;
        if reviera_core::crop::follows_scope_box(store, e) {
            return None;
        }
        let c = reviera_core::crop::effective(store, e).filter(|c| c.enabled && c.visible)?;
        let (_, z) = self.active_annotation_space()?;
        let (lo, hi) = (c.min, c.max);
        let near = |seg: [[f64; 2]; 2]| self.pixel_distance(x, y, seg, z) <= EDGE_PX;
        let sides = [
            near([[lo[0], lo[1]], [lo[0], hi[1]]]),
            near([[hi[0], lo[1]], [hi[0], hi[1]]]),
            near([[lo[0], lo[1]], [hi[0], lo[1]]]),
            near([[lo[0], hi[1]], [hi[0], hi[1]]]),
        ];
        if !sides.iter().any(|s| *s) {
            return None;
        }
        Some(CropDrag { view, sides, start: (lo, hi), from: self.plan_xy(x, y)? })
    }

    /// The region as dragged, drawn over the view.
    pub(crate) fn crop_drag_motion(&mut self, d: CropDrag, x: i32, y: i32) {
        let (Some(to), Some((_, z))) = (self.plan_xy(x, y), self.active_annotation_space()) else { return };
        let segments = reviera_render::crop::outline(d.region(to), z + 0.006);
        if let Some(vp) = self.viewport() {
            vp.set_overlay_lines(&[reviera_render::plan::Lines { color: GHOST, width: 0.5, segments }]);
        }
    }

    /// The drag's end: the crop region set (one undo step), unless it was a click.
    pub(crate) fn crop_drag_end(&mut self, d: CropDrag, x: i32, y: i32, click: bool) {
        if let Some(vp) = self.viewport() {
            vp.set_overlay_lines(&[]);
        }
        let Some(to) = self.plan_xy(x, y).filter(|_| !click) else { return };
        let (lo, hi) = d.region(to);
        if let Err(e) = self.run("view.set_crop", json!({ "view_id": d.view, "shape": { "min": lo, "max": hi } })) {
            error(&self.frame, &format!("The crop was not changed:\n{e}"));
        }
        let f = self.frame;
        self.refresh_scene(&f);
        self.properties_stale = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dragging_crop_edges_moves_only_them() {
        let d = CropDrag { view: ElementId(1), sides: [false, true, false, true], start: ([0.0, 0.0], [10.0, 5.0]), from: [10.0, 5.0] };
        assert_eq!(d.region([12.0, 4.0]), ([0.0, 0.0], [12.0, 4.0]));
        // Never past the opposite side.
        assert_eq!(d.region([-3.0, -3.0]), ([0.0, 0.0], [0.05, 0.05]));
        let left = CropDrag { sides: [true, false, false, false], from: [0.0, 2.0], ..d };
        assert_eq!(left.region([1.5, 9.0]), ([1.5, 0.0], [10.0, 5.0]));
    }
}
