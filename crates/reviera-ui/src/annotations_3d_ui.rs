// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Annotation in 3D views (DOC-ANN-025): View › Annotation › *Annotation in 3D* turns the active
//! 3D view's floating labels on and off; *Annotation Visibility* shows or hides each class (text
//! notes, dimensions, tags by category) on its own. The labels are drawn in the HUD layer
//! (`radial::update_hud`). Also: the ribbon's toggle buttons are kept showing their settings
//! ([`Gui::sync_toggles`]).

use reviera_core::ElementId;
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};
use crate::fullscreen;

/// Panes the View tab shows and hides, by action and pane name.
const PANES: &[(&str, &str)] = &[
    ("ui.pane.properties", "properties"),
    ("ui.pane.browser", "browser"),
    ("ui.pane.model_tree", "model_tree"),
    ("ui.pane.components", "components"),
    ("ui.pane.terminal", "terminal"),
    ("ui.pane.system_browser", "system_browser"),
    ("ui.pane.sizing_calculator", "sizing_calculator"),
    ("ui.pane.clash", "clash"),
    ("ui.pane.analytical_checks", "analytical_checks"),
    ("ui.pane.results", "results"),
];

impl Gui {
    /// The annotation settings of the active saved 3D view.
    fn active_3d_settings(&self) -> Option<reviera_core::annotations_3d::Settings> {
        let view = self.active_3d_view()?;
        let doc = self.active_view()?.doc;
        let store = self.app.document(doc)?.doc.store();
        store.get(view).map(reviera_core::annotations_3d::settings)
    }

    /// The active view's saved 3D view, if it is one.
    fn active_3d_view(&self) -> Option<ElementId> {
        let v = self.active_view()?;
        if v.is_2d() {
            return None;
        }
        v.element
    }

    /// Shows each toggle button pressed or not as its setting is now: the panes, full screen,
    /// focus mode, the radial menu, and the active 3D view's annotation.
    pub(crate) fn sync_toggles(&mut self) {
        for (action, pane) in PANES {
            self.ribbon.set_toggled(action, self.aui.is_pane_shown(pane));
        }
        self.ribbon.set_toggled("ui.fullscreen.toggle", self.chrome.mode == fullscreen::Mode::Full);
        self.ribbon.set_toggled("ui.focus_mode.toggle", self.chrome.mode == fullscreen::Mode::Focus);
        self.ribbon.set_toggled("ui.radial.toggle", self.ui_settings.radial_menu);
        let shown = self.active_3d_settings().is_some_and(|s| s.shown);
        self.ribbon.set_toggled("view.toggle_3d_annotations", shown);
    }

    /// View › Annotation › Annotation in 3D.
    pub(crate) fn toggle_3d_annotations(&mut self) {
        let frame = self.frame;
        let Some(view) = self.active_3d_view() else {
            return frame.set_status_text("Annotation in 3D: open a saved 3D view (View › Default 3D View)", 0);
        };
        let shown = self.active_3d_settings().is_none_or(|s| s.shown);
        match self.run("view.set_3d_annotations", json!({ "view_id": view, "shown": !shown })) {
            Ok(_) => frame.set_status_text(if shown { "Annotation hidden in this 3D view" } else { "Annotation shown in this 3D view" }, 0),
            Err(e) => error(&frame, &e),
        }
        self.refresh_active();
    }

    /// View › Annotation › Annotation Visibility: a check box for each class.
    pub(crate) fn annotations_3d_dialog(&mut self) {
        let frame = self.frame;
        let Some(view) = self.active_3d_view() else {
            return frame.set_status_text("Annotation Visibility: open a saved 3D view (View › Default 3D View)", 0);
        };
        let info = match self.run("view.annotations_3d", json!({ "view_id": view })) {
            Ok(v) => v,
            Err(e) => return error(&frame, &e),
        };
        let dialog = Dialog::builder(&frame, "Annotation in 3D").build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let note = StaticText::builder(&dialog)
            .with_label("The plans' annotation, floating in this 3D view as labels. Each class is shown or hidden on its own.")
            .build();
        note.wrap(340);
        sizer.add(&note, 0, SizerFlag::All, 10);
        let all = CheckBox::builder(&dialog).with_label("Show annotation in this view").build();
        all.set_value(info["shown"] == true);
        sizer.add(&all, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom, 10);
        let classes = info["classes"].as_array().cloned().unwrap_or_default();
        let mut boxes = Vec::new();
        let grid = BoxSizer::builder(Orientation::Vertical).build();
        for c in &classes {
            let label = format!("{} ({})", c["label"].as_str().unwrap_or(""), c["count"]);
            let b = CheckBox::builder(&dialog).with_label(&label).build();
            b.set_value(c["visible"] == true);
            b.enable(info["shown"] == true);
            grid.add(&b, 0, SizerFlag::Left | SizerFlag::Bottom, 4);
            boxes.push((c["class"].as_str().unwrap_or("").to_owned(), b));
        }
        sizer.add_sizer(&grid, 0, SizerFlag::Left | SizerFlag::Right, 22);
        {
            let boxes: Vec<CheckBox> = boxes.iter().map(|(_, b)| *b).collect();
            all.on_toggled(move |e| {
                for b in &boxes {
                    b.enable(e.is_checked());
                }
            });
        }
        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        buttons.add_stretch_spacer(1);
        let cancel = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Cancel").build();
        let ok = Button::builder(&dialog).with_id(ID_OK).with_label("OK").build();
        buttons.add(&cancel, 0, SizerFlag::All, 6);
        buttons.add(&ok, 0, SizerFlag::All, 6);
        sizer.add_sizer(&buttons, 0, SizerFlag::Expand | SizerFlag::Top, 6);
        dialog.set_sizer_and_fit(sizer, true);
        dialog.set_escape_id(ID_CANCEL);
        let code = dialog.show_modal();
        let shown = all.get_value();
        let set: serde_json::Map<String, serde_json::Value> = boxes.iter().map(|(c, b)| (c.clone(), json!(b.get_value()))).collect();
        dialog.destroy();
        if code != ID_OK {
            return;
        }
        if let Err(e) = self.run("view.set_3d_annotations", json!({ "view_id": view, "shown": shown, "classes": set })) {
            error(&frame, &e);
        }
        self.refresh_active();
    }
}
