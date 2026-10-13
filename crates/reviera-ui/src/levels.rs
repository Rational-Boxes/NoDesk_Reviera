// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Levels in the window (ARC-LVL-001): Architecture › Level, the Levels group of the Project
//! Browser, renaming and moving levels, and the confirmation before deleting elements that
//! others depend on.

use std::collections::BTreeMap;

use reviera_core::levels::{self, LevelInfo};
use reviera_core::{ElementId, Store};
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, ask, error};

/// Storey height offered for a new level when the model has only one.
const DEFAULT_STOREY: f64 = 3.0;

impl Gui {
    fn store(&self) -> Option<Store> {
        Some(self.app.document(self.doc?)?.doc.store().clone())
    }

    /// The model's levels, lowest first.
    pub(crate) fn levels(&self) -> Vec<LevelInfo> {
        self.store().map(|s| levels::list(&s)).unwrap_or_default()
    }

    pub(crate) fn is_level(&self, id: ElementId) -> bool {
        self.store()
            .and_then(|s| s.get(id).map(|e| e.class == levels::CLASS))
            .unwrap_or(false)
    }

    /// Architecture › Level: asks for the elevation of a new level (one storey above the top
    /// level by default) and creates it with its floor plan.
    pub(crate) fn new_level(&mut self) {
        let frame = self.frame;
        if self.doc.is_none() {
            return;
        }
        let all = self.levels();
        let suggested = match all.as_slice() {
            [] => 0.0,
            [only] => only.elevation + DEFAULT_STOREY,
            [.., below, top] => top.elevation + (top.elevation - below.elevation).max(0.1),
        };
        let default = self.format_length(suggested);
        let dialog = TextEntryDialog::builder(
            &frame,
            "Elevation of the new level (a floor plan is created with it):",
            "New Level",
        )
        .with_default_value(&default)
        .build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let Some(text) = dialog.get_value().filter(|t| !t.trim().is_empty()) else {
            return;
        };
        match self.run("level.create", json!({ "elevation": text })) {
            Ok(v) => {
                let id = ElementId(v.as_u64().unwrap_or_default());
                let name = self
                    .store()
                    .and_then(|s| s.get(id)?.name.clone())
                    .unwrap_or_default();
                frame.set_status_text(&format!("Created {name} and its floor plan"), 0);
                self.select_level(id);
            }
            Err(e) => error(&frame, &format!("The level was not created:\n{e}")),
        }
        // A level in between changes the ranges of the plans around it.
        self.refresh_scene(&frame);
    }

    /// Selects a level, so its Name and Elevation show in Properties.
    pub(crate) fn select_level(&mut self, id: ElementId) {
        if self.run("selection.set", json!({ "ids": [id] })).is_ok() {
            self.selection_changed();
        }
    }

    /// Renames a level, asking whether its plan views follow.
    pub(crate) fn rename_level(&mut self, id: ElementId, name: Option<String>) {
        let frame = self.frame;
        let current = self
            .store()
            .and_then(|s| s.get(id)?.name.clone())
            .unwrap_or_default();
        let name = match name {
            Some(n) => n,
            None => {
                let dialog = TextEntryDialog::builder(&frame, "New name:", "Rename Level")
                    .with_default_value(&current)
                    .build();
                if dialog.show_modal() != ID_OK {
                    return;
                }
                match dialog.get_value() {
                    Some(n) => n,
                    None => return,
                }
            }
        };
        if name == current {
            return;
        }
        let rename_views = ask(
            &frame,
            &format!("Would you like to rename the views of {current} to {name} as well?"),
        );
        match self.run(
            "level.rename",
            json!({ "level_id": id, "name": name, "rename_views": rename_views }),
        ) {
            Ok(_) => self.refresh_scene(&frame),
            Err(e) => {
                error(&frame, &format!("The level was not renamed:\n{e}"));
                self.properties_stale = true;
            }
        }
    }

    /// Asks for a level's new elevation.
    pub(crate) fn move_level(&mut self, id: ElementId) {
        let frame = self.frame;
        let Some(level) = self.levels().into_iter().find(|l| l.id == id) else {
            return;
        };
        let default = self.format_length(level.elevation);
        let dialog = TextEntryDialog::builder(
            &frame,
            &format!("Elevation of {}:", level.name),
            "Level Elevation",
        )
        .with_default_value(&default)
        .build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let Some(text) = dialog.get_value().filter(|t| *t != default) else {
            return;
        };
        match self.run(
            "level.set_elevation",
            json!({ "level_id": id, "elevation": text }),
        ) {
            Ok(_) => self.refresh_scene(&frame),
            Err(e) => error(&frame, &format!("The level was not moved:\n{e}")),
        }
    }

    /// Deletes elements after a confirmation that lists, by category, the other elements that
    /// go with them (a level's walls and plans, for example). `false` if the user declined.
    pub(crate) fn confirm_delete(&mut self, ids: &[ElementId]) -> bool {
        let Some(store) = self.store() else {
            return false;
        };
        let all = reviera_core::builtin::delete_closure(&store, ids);
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for id in all.iter().filter(|id| !ids.contains(id)) {
            let Some(e) = store.get(*id) else { continue };
            // The grid system goes with its last line, unseen.
            if reviera_core::grids::is_system(e) {
                continue;
            }
            let label = if reviera_core::views::view_data(e).is_some() {
                "Views".to_owned()
            } else if reviera_core::annotations::dimension_data(e).is_some() {
                "Dimensions".to_owned()
            } else {
                reviera_core::categories::category_of(e)
                    .map_or_else(|| e.class.clone(), |c| c.name.to_owned())
            };
            *counts.entry(label).or_default() += 1;
        }
        if counts.is_empty() {
            return true;
        }
        let list: Vec<String> = counts
            .into_iter()
            .map(|(label, n)| format!("    {label}: {n}"))
            .collect();
        ask(
            &self.frame,
            &format!(
                "These elements will be deleted with the selection:\n\n{}\n\nDelete them?",
                list.join("\n")
            ),
        )
    }

    /// Deletes a level (from the Project Browser) after the confirmation.
    pub(crate) fn delete_level(&mut self, id: ElementId) {
        let frame = self.frame;
        if !self.confirm_delete(&[id]) {
            return;
        }
        match self.run("element.delete", json!({ "element_ids": [id] })) {
            Ok(_) => {
                let _ = self.run("selection.clear", json!({}));
                self.refresh_scene(&frame);
                self.selection_changed();
            }
            Err(e) => error(&frame, &format!("The level was not deleted:\n{e}")),
        }
    }
}
