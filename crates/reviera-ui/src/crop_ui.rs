// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Crop rows in a plan's, section's or elevation's Properties (DOC-GFX-015): Crop View, Crop
//! Region Visible, Annotation Crop, and Reset Crop (fit the model). The drawing is cropped in
//! `plans`, `sections` and `inspect`.

use reviera_core::ElementId;
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

impl Gui {
    /// Appends the crop rows of view `element`.
    pub(crate) fn crop_rows(&self, grid: &PropertyGrid, element: ElementId) {
        let Some(e) = self.doc.and_then(|d| self.app.document(d)).and_then(|d| d.doc.store().get(element)) else { return };
        let crop = reviera_core::crop::crop_of(e);
        let annotation = reviera_core::crop::annotation_crop_of(e);
        grid.append(Property::category("Extents", "c_crop"));
        grid.append(Property::boolean("Crop View", "crop_on", crop.is_some_and(|c| c.enabled)));
        grid.append(Property::boolean("Crop Region Visible", "crop_visible", crop.is_some_and(|c| c.visible)));
        grid.append(Property::boolean("Annotation Crop", "crop_annotation", annotation.enabled));
        grid.set_help_string("crop_annotation", "Annotation crossing the annotation crop is clipped; annotation outside it is hidden");
        grid.append(Property::boolean("Reset Crop", "crop_reset", false));
        grid.set_help_string("crop_reset", "Tick to fit the crop region to the model again");
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
