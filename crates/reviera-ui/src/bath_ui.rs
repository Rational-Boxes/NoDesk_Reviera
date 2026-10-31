// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Check Bathrooms (ARC-BATH-002): runs `bath.check` under the bathroom rule packs in use, lists
//! the findings (errors first), and selects and frames the fixture and what is in its way.

use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

impl Gui {
    pub(crate) fn check_bathrooms(&mut self) {
        let frame = self.frame;
        let in_use: Vec<String> = self
            .run("bath.rule_packs", json!({}))
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter(|p| p["in_use"] == true)
            .filter_map(|p| p["title"].as_str().map(str::to_owned))
            .collect();
        if in_use.is_empty() {
            return error(
                &frame,
                "No bathroom rule packs are in use. Choose them in Manage › Model Properties, or set a building code or accessibility rule set they follow.",
            );
        }
        let found = match self.run("bath.check", json!({})) {
            Ok(v) => v.as_array().cloned().unwrap_or_default(),
            Err(e) => return error(&frame, &format!("The check did not run:\n{e}")),
        };
        let packs = in_use.join("; ");
        if found.is_empty() {
            let d = MessageDialog::builder(&frame, &format!("No problems found under: {packs}."), "Check Bathrooms")
                .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation)
                .build();
            d.show_modal();
            return;
        }
        let rows: Vec<String> = found
            .iter()
            .enumerate()
            .map(|(k, f)| {
                let severity = if f["severity"] == "note" { "Note" } else { "Error" };
                let mut row = format!("{}. {severity}: {}", k + 1, f["message"].as_str().unwrap_or_default());
                if f["confidence"] != "verified" {
                    row.push_str(&format!(" [{}]", f["confidence"].as_str().unwrap_or_default()));
                }
                row
            })
            .collect();
        let labels: Vec<&str> = rows.iter().map(String::as_str).collect();
        let errors = found.iter().filter(|f| f["severity"] == "error").count();
        let dialog = SingleChoiceDialog::builder(
            &frame,
            &format!(
                "{errors} error(s) and {} note(s) under: {packs}.\nPick one to select and frame it:",
                found.len() - errors
            ),
            "Check Bathrooms",
            &labels,
        )
        .build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let picked = usize::try_from(dialog.get_selection()).ok();
        dialog.destroy();
        let Some(f) = picked.and_then(|i| found.get(i)) else {
            return;
        };
        // Show the clear spaces, then the fixture and what is in its way.
        if let Some(v) = self.active_view()
            && self.virtual_mode(v) == reviera_render::VirtualMode::Hidden
            && let Some(view) = v.element
        {
            let _ = self.run("view.set_virtual_volumes", json!({ "view_id": view, "mode": "translucent" }));
            self.refresh_scene(&frame);
        }
        let mut ids = vec![f["element"].clone()];
        if !f["other"].is_null() {
            ids.push(f["other"].clone());
        }
        let _ = self.run("selection.set", json!({ "ids": ids }));
        self.selection_changed();
        self.zoom_to_fit();
        frame.set_status_text(
            &format!(
                "{} — {} ({}, {})",
                f["rule"].as_str().unwrap_or_default(),
                f["description"].as_str().unwrap_or_default(),
                f["edition"].as_str().unwrap_or_default(),
                f["source"].as_str().unwrap_or_default()
            ),
            0,
        );
    }
}
