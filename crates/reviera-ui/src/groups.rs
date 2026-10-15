// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Groups and saved selections in the window (ARC-GRP-001 … 003, ARC-SEL-002).
//!
//! Architecture › Model: Create Group from the selection, Place Group (choose the type, then
//! click where its origin goes, in a plan), Edit Group on a selected member's group with Add,
//! Remove, Finish and Cancel, Exclude a selected member from its instance and Restore Excluded,
//! and Ungroup. Manage › Selection: Save Selection and Load Selection.

use reviera_core::{ElementId, GlobalId, groups};
use serde_json::json;
use wxdragon::prelude::*;

use super::Gui;
use super::select::Combine;

impl Gui {
    /// The group instance of the selected members (or a selected instance).
    fn selected_group(&mut self) -> Option<ElementId> {
        let store = self.store_now()?;
        self.selection().into_iter().find_map(|id| {
            let e = store.get(id)?;
            if groups::recipe(e).is_some() {
                return Some(id);
            }
            store.find_by_guid(&GlobalId::parse(&groups::instance_of(e)?)?)
        })
    }

    fn group_run(&mut self, command: &str, args: serde_json::Value, done: &str) {
        let frame = self.frame;
        match self.run(command, args) {
            Ok(_) => {
                self.properties_stale = true;
                self.refresh_scene(&frame);
                frame.set_status_text(done, 0);
            }
            Err(e) => super::error(&frame, &e),
        }
    }

    /// Architecture › Model › Create Group.
    pub(crate) fn create_group(&mut self) {
        let frame = self.frame;
        let ids = self.selection();
        if ids.is_empty() {
            return frame.set_status_text("Select the elements to group", 0);
        }
        let n = self.store_now().map_or(0, |s| groups::types(&s).len()) + 1;
        let dialog = TextEntryDialog::builder(&frame, "Group name:", "Create Group")
            .with_default_value(&format!("Group {n}"))
            .build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let Some(name) = dialog.get_value() else {
            return;
        };
        self.group_run(
            "group.create",
            json!({ "element_ids": ids, "name": name }),
            "Group created",
        );
    }

    /// Architecture › Model › Place Group: the type, then a click.
    pub(crate) fn start_place_group(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else {
            return;
        };
        let types = groups::types(&store);
        if types.is_empty() {
            return frame.set_status_text("There are no groups yet: Create Group first", 0);
        }
        let labels: Vec<&str> = types.iter().map(|t| t.name.as_str()).collect();
        let dialog = SingleChoiceDialog::builder(&frame, "Group:", "Place Group", &labels).build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let Some(t) = usize::try_from(dialog.get_selection())
            .ok()
            .and_then(|i| types.get(i))
        else {
            return;
        };
        self.group_pick = Some(t.id.clone());
        frame.set_status_text(
            "Place Group: click where its origin goes in a plan (Esc ends)",
            0,
        );
    }

    pub(crate) fn group_click(&mut self, x: i32, y: i32) {
        let Some(t) = self.group_pick.clone() else {
            return;
        };
        let Some(p) = self.plan_xy(x, y) else {
            return self.frame.set_status_text("Groups are placed in plans", 0);
        };
        self.group_run(
            "group.place",
            json!({ "group_type_id": t, "point": p }),
            "Group placed; click again, or Esc",
        );
    }

    /// Architecture › Model › Edit Group.
    pub(crate) fn edit_group(&mut self) {
        let Some(g) = self.selected_group() else {
            return self
                .frame
                .set_status_text("Select a member of the group to edit", 0);
        };
        self.group_run(
            "group.edit",
            json!({ "instance_id": g }),
            "Editing the group: change its members, Add or Remove, then Finish or Cancel",
        );
    }

    pub(crate) fn group_add(&mut self) {
        let ids = self.selection();
        self.group_run(
            "group.add_members",
            json!({ "element_ids": ids }),
            "Added to the group",
        );
    }

    pub(crate) fn group_remove(&mut self) {
        let ids = self.selection();
        self.group_run(
            "group.remove_members",
            json!({ "element_ids": ids }),
            "Removed from the group",
        );
    }

    pub(crate) fn group_finish(&mut self) {
        self.group_run(
            "group.finish",
            json!({}),
            "Group edited: every instance follows",
        );
    }

    pub(crate) fn group_cancel(&mut self) {
        self.group_run("group.cancel", json!({}), "Group editing cancelled");
    }

    /// Exclude the selected member from its instance.
    pub(crate) fn group_exclude(&mut self) {
        let Some(g) = self.selected_group() else {
            return self
                .frame
                .set_status_text("Select a group member to exclude", 0);
        };
        let Some(&m) = self.selection().first() else {
            return;
        };
        self.group_run(
            "group.exclude_member",
            json!({ "instance_id": g, "element_id": m }),
            "Excluded from this instance",
        );
    }

    pub(crate) fn group_restore(&mut self) {
        let Some(g) = self.selected_group() else {
            return self
                .frame
                .set_status_text("Select a member of the group", 0);
        };
        self.group_run(
            "group.restore_member",
            json!({ "instance_id": g }),
            "Excluded members restored",
        );
    }

    pub(crate) fn ungroup(&mut self) {
        let Some(g) = self.selected_group() else {
            return self
                .frame
                .set_status_text("Select a member of the group", 0);
        };
        self.group_run("group.ungroup", json!({ "instance_ids": [g] }), "Ungrouped");
    }

    /// Manage › Selection › Save Selection.
    pub(crate) fn save_selection(&mut self) {
        let frame = self.frame;
        let ids = self.selection();
        if ids.is_empty() {
            return frame.set_status_text("Select the elements to save", 0);
        }
        let n = self
            .store_now()
            .map_or(0, |s| reviera_core::selection_sets::all(&s).len())
            + 1;
        let dialog = TextEntryDialog::builder(&frame, "Name:", "Save Selection")
            .with_default_value(&format!("Selection {n}"))
            .build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let Some(name) = dialog.get_value() else {
            return;
        };
        let result = match self.run(
            "selectionset.save",
            json!({ "name": name, "element_ids": ids }),
        ) {
            Err(e) if e.contains("replace it") => {
                let ask = MessageDialog::builder(
                    &frame,
                    &format!("A selection set is called {name}. Replace it?"),
                    "Save Selection",
                )
                .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
                .build();
                if ask.show_modal() != ID_YES {
                    return;
                }
                self.run(
                    "selectionset.save",
                    json!({ "name": name, "element_ids": ids, "replace": true }),
                )
            }
            other => other,
        };
        match result {
            Ok(_) => frame.set_status_text("Selection saved", 0),
            Err(e) => super::error(&frame, &e),
        }
    }

    /// Manage › Selection › Load Selection.
    pub(crate) fn load_selection(&mut self) {
        let frame = self.frame;
        let Ok(list) = self.run("selectionset.list", json!({})) else {
            return;
        };
        let sets: Vec<(String, String)> = list
            .as_array()
            .into_iter()
            .flatten()
            .map(|s| {
                (
                    s["id"].as_str().unwrap_or_default().to_owned(),
                    format!(
                        "{} ({})",
                        s["name"].as_str().unwrap_or_default(),
                        s["count"].as_u64().unwrap_or(0)
                    ),
                )
            })
            .collect();
        if sets.is_empty() {
            return frame.set_status_text("There are no saved selections", 0);
        }
        let labels: Vec<&str> = sets.iter().map(|s| s.1.as_str()).collect();
        let dialog =
            SingleChoiceDialog::builder(&frame, "Saved selection:", "Load Selection", &labels)
                .build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let Some((id, _)) = usize::try_from(dialog.get_selection())
            .ok()
            .and_then(|i| sets.get(i))
        else {
            return;
        };
        if let Ok(v) = self.run("selectionset.load", json!({ "id": id })) {
            let ids: Vec<ElementId> = v
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_u64)
                .map(ElementId)
                .collect();
            self.select_ids(&ids, Combine::Replace);
            frame.set_status_text(&format!("{} elements selected", ids.len()), 0);
        }
    }
}
