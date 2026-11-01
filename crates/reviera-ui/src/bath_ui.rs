// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Check Bathrooms (ARC-BATH-002): runs `bath.check` under the bathroom rule packs in use, lists
//! the findings (errors first), and selects and frames the fixture and what is in its way. Place
//! Grab Bars (ARC-BATH-003) puts the bars the packs ask for at the selected fixtures, or all.

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

    pub(crate) fn place_grab_bars(&mut self) {
        let frame = self.frame;
        let selected: Vec<serde_json::Value> = self.selection().into_iter().map(|id| json!(id.0)).collect();
        let args = if selected.is_empty() { json!({}) } else { json!({ "element_ids": selected }) };
        let out = match self.run("bath.place_grab_bars", args) {
            Ok(v) => v,
            Err(e) => return error(&frame, &format!("Grab bars were not placed:\n{e}")),
        };
        self.refresh_scene(&frame);
        let placed = out["placed"].as_array().map_or(0, Vec::len);
        let skipped: Vec<String> = out["skipped"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|s| format!("{} ({}): {}", self.element_label(&s["fixture"]), s["rule"].as_str().unwrap_or_default(), s["reason"].as_str().unwrap_or_default()))
            .collect();
        if placed == 0 && skipped.is_empty() {
            return error(&frame, "No grab bars to place: the bathroom rule packs in use ask for none at these fixtures (the ADA pack places WC bars).");
        }
        if skipped.is_empty() {
            frame.set_status_text(&format!("{placed} grab bar(s) placed"), 0);
        } else {
            let d = MessageDialog::builder(&frame, &format!("{placed} grab bar(s) placed. Not placed:\n{}", skipped.join("\n")), "Place Grab Bars")
                .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation)
                .build();
            d.show_modal();
        }
    }

    /// Architecture › Build › Bathroom (ARC-BATH-004): a generic bathroom fixture or accessory,
    /// loaded the first time; each click near a wall puts one on its face at its own height.
    pub(crate) fn place_bathroom_component(&mut self) {
        let frame = self.frame;
        let components = |store: &reviera_core::Store| -> Vec<(reviera_core::ElementId, String)> {
            store.iter().filter(|e| reviera_core::bath_components::is_component(e)).map(|e| (e.id(), e.name.clone().unwrap_or_default())).collect()
        };
        let Some(store) = self.store_now() else {
            return frame.set_status_text("Open or create a project first (File > New)", 0);
        };
        let mut types = components(&store);
        if types.len() < reviera_core::bath_components::names().len() {
            if let Err(e) = self.run("bath.load_components", json!({})) {
                return error(&frame, &e);
            }
            let Some(store) = self.store_now() else { return };
            types = components(&store);
        }
        // In the catalogue's order: fixtures, then accessories.
        let order = reviera_core::bath_components::names();
        types.sort_by_key(|t| order.iter().position(|n| *n == t.1).unwrap_or(usize::MAX));
        let f = super::reinforcement_ui::Form::new(&frame, "Place Bathroom Component");
        let names: Vec<String> = types.iter().map(|t| t.1.clone()).collect();
        let ty = f.choice("Component", &names, 0);
        let height = f.text("Raise by (mm)", "0", "Above its own mounting height");
        let ok = f.show("Click near a wall to put it on that face, its back to the wall (it follows the wall); away from walls it stands on the work plane. Spacebar turns it there.", "Place");
        let k = ty.get_selection().map_or(0, |k| k as usize);
        let h = height.get_value().trim().replace(',', ".").parse::<f64>().unwrap_or(0.0) / 1000.0;
        f.dialog.destroy();
        let Some((id, name)) = types.get(k).cloned().filter(|_| ok) else { return };
        self.start_place_tool(super::components::Source::Type(id), name);
        self.set_place_offset(h);
    }

    /// Architecture › Build › Bathroom Layout (ARC-BATH-005): a template and its options, then a
    /// click inside a room closed by walls lays it out there.
    pub(crate) fn bathroom_layout(&mut self) {
        let frame = self.frame;
        let templates = match self.run("bath.layouts", json!({})) {
            Ok(v) => v.as_array().cloned().unwrap_or_default(),
            Err(e) => return error(&frame, &e),
        };
        let f = super::reinforcement_ui::Form::new(&frame, "Bathroom Layout");
        let titles: Vec<String> = templates.iter().map(|t| t["title"].as_str().unwrap_or_default().to_owned()).collect();
        let t = f.choice("Template", &titles, 0);
        let start = f.choice("Start from", &["The left end, facing the wet wall".to_owned(), "The right end".to_owned()], 0);
        let replace = f.choice("An earlier layout here", &["Replace it".to_owned(), "Keep it".to_owned()], 0);
        let ok = f.show("Then click inside a room closed by walls, in a plan: the fixtures run along its longer wall, spaced by the bathroom rule packs in use.", "Choose Room");
        let k = t.get_selection().map_or(0, |k| k as usize);
        let right = start.get_selection() == Some(1);
        let keep = replace.get_selection() == Some(1);
        f.dialog.destroy();
        let Some(id) = templates.get(k).and_then(|t| t["id"].as_str()).filter(|_| ok).map(str::to_owned) else { return };
        self.start_room_tool();
        if let Some(tool) = self.room_tool.as_mut() {
            tool.layout = Some(json!({ "template": id, "start": if right { "right" } else { "left" }, "replace": !keep }));
            frame.set_status_text("Bathroom Layout: click inside the room; Esc ends", 0);
        }
    }

    /// Lays out a bathroom where the Bathroom Layout tool was clicked, and reports the fit.
    pub(crate) fn layout_bathroom_at(&mut self, args: serde_json::Value) {
        let frame = self.frame;
        match self.app.execute("bath.layout", args, &super::user()) {
            Ok(out) => {
                let placed = out.result["placed"].as_array().map_or(0, Vec::len);
                self.refresh_scene(&frame);
                let problems = self
                    .run("bath.check", json!({ "element_ids": out.result["placed"] }))
                    .ok()
                    .and_then(|v| v.as_array().map(|a| a.iter().filter(|f| f["severity"] == "error").count()))
                    .unwrap_or(0);
                frame.set_status_text(&format!("{placed} component(s) laid out; Check Bathrooms finds {problems} error(s)"), 0);
            }
            Err(e) => error(&frame, &format!("The layout was not made:\n{}", e.message)),
        }
    }
}
