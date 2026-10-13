// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The Project Browser (DOC-VIEW-001) and the views of the window: opening, creating, renaming,
//! duplicating and deleting views through the `view.*` commands, and keeping each tab in step
//! with the session's view state (cameras changed by commands, undo and redo).

use reviera_core::ElementId;
use reviera_core::views::{self, KIND_3D};
use reviera_render::Camera;
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, ask, error};

impl Gui {
    /// Rebuilds the Project Browser for the active model.
    pub(crate) fn rebuild_browser(&mut self) {
        self.browser_stale = false;
        self.browser.cleanup_custom_data();
        self.browser.delete_all_items();
        let Some(doc) = self.doc else { return };
        let Some(root) = self.browser.add_root("Views (all)", None, None) else {
            return;
        };
        let list = self
            .app
            .document(doc)
            .map(|d| views::list(d.doc.store()))
            .unwrap_or_default();
        let open: Vec<ElementId> = self.views.iter().filter_map(|v| v.element).collect();
        let active = self.active_view().and_then(|v| v.element);
        let plans: Vec<_> = list
            .iter()
            .filter(|v| v.kind == reviera_core::plans::KIND_PLAN)
            .collect();
        if !plans.is_empty()
            && let Some(group) = self.browser.append_item(&root, "Floor Plans", None, None)
        {
            for v in plans {
                if let Some(item) = self
                    .browser
                    .append_item_with_data(&group, &v.name, v.id.0, None, None)
                {
                    if open.contains(&v.id) {
                        self.browser.set_item_bold(&item, true);
                    }
                    if active == Some(v.id) {
                        self.browser.select_item(&item);
                    }
                }
            }
        }
        let Some(group) = self.browser.append_item(&root, "3D Views", None, None) else {
            return;
        };
        for v in list.iter().filter(|v| v.kind == KIND_3D) {
            if let Some(item) = self
                .browser
                .append_item_with_data(&group, &v.name, v.id.0, None, None)
            {
                if open.contains(&v.id) {
                    self.browser.set_item_bold(&item, true);
                }
                if active == Some(v.id) {
                    self.browser.select_item(&item);
                }
            }
        }
        if !self
            .views
            .iter()
            .any(|v| v.doc == doc && v.element.is_some())
            && self.views.iter().any(|v| v.doc == doc)
        {
            // The unsaved default view of a model without views.
            if let Some(item) = self
                .browser
                .append_item(&group, "{3D} (not saved)", None, None)
            {
                self.browser.set_item_bold(&item, true);
            }
        }
        self.browser.expand_all();
    }

    /// The view behind a browser item (its element id is the item's data).
    pub(crate) fn browser_view(&self, item: &TreeItemId) -> Option<ElementId> {
        let data = self.browser.get_custom_data(item)?;
        data.downcast_ref::<u64>().map(|id| ElementId(*id))
    }

    /// Opens a model view of the active model (or brings its tab to the front).
    pub(crate) fn open_model_view(&mut self, view: ElementId) {
        if let Some(doc) = self.doc {
            self.open_view(doc, Some(view));
        }
    }

    /// View › Default 3D View: the model's `{3D}` view, created if needed. An unsaved `{3D}` tab
    /// becomes that view.
    pub(crate) fn default_3d_view(&mut self) {
        let frame = self.frame;
        let Some(doc) = self.doc else { return };
        let id = match self.run("view.create_3d", json!({ "kind": "default" })) {
            Ok(v) => ElementId(v.as_u64().unwrap_or_default()),
            Err(e) => return error(&frame, &format!("Could not create the view:\n{e}")),
        };
        let unsaved = self
            .views
            .iter_mut()
            .find(|v| v.doc == doc && v.element.is_none());
        match unsaved {
            Some(v) => {
                v.element = Some(id);
                v.synced = None;
                let vid = v.id;
                self.activate_view(vid);
                self.refresh_active();
            }
            None => self.open_view(doc, Some(id)),
        }
        self.browser_stale = true;
        self.properties_stale = true;
    }

    /// Window › New 3D View: a new named 3D view, starting from the active view's camera.
    pub(crate) fn new_3d_view(&mut self) {
        let frame = self.frame;
        let Some(doc) = self.doc else { return };
        let camera = self.active_view().map(|v| {
            let size = v.panel.get_client_size();
            v.camera
                .to_view(size.width.max(1) as f32 / size.height.max(1) as f32)
        });
        let id = match self.run(
            "view.create_3d",
            json!({ "kind": "named", "camera": camera }),
        ) {
            Ok(v) => ElementId(v.as_u64().unwrap_or_default()),
            Err(e) => return error(&frame, &format!("Could not create the view:\n{e}")),
        };
        self.open_view(doc, Some(id));
        self.update_captions();
    }

    pub(crate) fn rename_view(&mut self, view: ElementId) {
        let frame = self.frame;
        let Some(doc) = self.doc else { return };
        let current = self
            .app
            .document(doc)
            .and_then(|d| d.doc.store().get(view)?.name.clone())
            .unwrap_or_default();
        let dialog = TextEntryDialog::builder(&frame, "New name:", "Rename View")
            .with_default_value(&current)
            .build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let Some(name) = dialog.get_value().filter(|n| *n != current) else {
            return;
        };
        if let Err(e) = self.run("view.rename", json!({ "view_id": view, "new_name": name })) {
            return error(&frame, &format!("The view was not renamed:\n{e}"));
        }
        self.after_view_change();
    }

    pub(crate) fn duplicate_view(&mut self, view: ElementId) {
        let frame = self.frame;
        let Some(doc) = self.doc else { return };
        // The copy starts from the view's current camera.
        let camera = self.app.view_camera(doc, view);
        match self.run("view.duplicate", json!({ "view_id": view })) {
            Ok(v) => {
                let copy = ElementId(v.as_u64().unwrap_or_default());
                if let Some(c) = camera {
                    self.app.set_view_camera(doc, copy, c);
                }
                self.open_view(doc, Some(copy));
            }
            Err(e) => error(&frame, &format!("The view was not duplicated:\n{e}")),
        }
        self.after_view_change();
    }

    pub(crate) fn delete_view(&mut self, view: ElementId) {
        let frame = self.frame;
        let Some(doc) = self.doc else { return };
        let name = self
            .app
            .document(doc)
            .and_then(|d| d.doc.store().get(view)?.name.clone())
            .unwrap_or_default();
        if !ask(&frame, &format!("Delete the view {name}?")) {
            return;
        }
        if let Err(e) = self.run("view.delete", json!({ "view_ids": [view] })) {
            return error(&frame, &format!("The view was not deleted:\n{e}"));
        }
        self.after_view_change();
    }

    /// After a change to views: tabs of deleted views close, captions and the browser update.
    pub(crate) fn after_view_change(&mut self) {
        self.sync_views();
        self.update_captions();
        self.browser_stale = true;
        self.properties_stale = true;
    }

    /// Keeps the tabs in step with the model and the session: closes tabs of views that no
    /// longer exist (deleted, or undone), adopts cameras set by commands, and opens views that
    /// commands asked for (`view.open`). Called when idle.
    pub(crate) fn sync_views(&mut self) {
        let gone: Vec<u32> = self
            .views
            .iter()
            .filter(|v| {
                v.element.is_some_and(|e| {
                    self.app
                        .document(v.doc)
                        .is_none_or(|d| d.doc.store().get(e).is_none())
                })
            })
            .map(|v| v.id)
            .collect();
        for id in gone {
            let doc = self.view(id).map(|v| v.doc);
            let others = self.views.iter().filter(|v| Some(v.doc) == doc).count() > 1;
            if others {
                self.close_tab(id);
            } else if let Some(v) = self.views.iter_mut().find(|v| v.id == id) {
                // The model's last tab stays, as its unsaved default view.
                v.element = None;
                v.synced = None;
            }
            self.browser_stale = true;
        }
        let mut redraw = Vec::new();
        for i in 0..self.views.len() {
            let (doc, Some(element)) = (self.views[i].doc, self.views[i].element) else {
                continue;
            };
            let current = self.app.view_camera(doc, element);
            let v = &mut self.views[i];
            if let Some(c) = current
                && current != v.synced
            {
                let size = v.panel.get_client_size();
                let aspect = size.width.max(1) as f32 / size.height.max(1) as f32;
                v.camera = Camera::from_view(&c, aspect);
                v.synced = current;
                redraw.push(v.panel);
            }
        }
        for p in redraw {
            p.refresh(false, None);
        }
        for (doc, view) in self.app.take_view_requests() {
            if self.app.document(doc).is_some() {
                self.open_view(doc, Some(view));
            }
        }
    }
}
