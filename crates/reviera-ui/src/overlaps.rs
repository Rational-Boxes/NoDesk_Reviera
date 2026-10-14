// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! View › Check Overlaps (ARC-MOD-030): runs the volume overlap engine over the model, lists the
//! overlaps largest first, and selects and frames the pair the user picks.

use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

impl Gui {
    pub(crate) fn check_overlaps(&mut self) {
        let frame = self.frame;
        let found = match self.run("overlap.find", json!({})) {
            Ok(v) => v,
            Err(e) => return error(&frame, &format!("The check did not run:\n{e}")),
        };
        let mut list = found["overlaps"].as_array().cloned().unwrap_or_default();
        // Intrusions into virtual volumes (door swings, clearances) are listed with them.
        let intrusions = self
            .run("clearance.check", json!({}))
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();
        for i in &intrusions {
            let kind = i["kind"].as_str().unwrap_or_default().replace('_', " ");
            let what = format!("{}'s {kind}", self.element_label(&i["element"]));
            list.push(json!({
                "a": i["element"], "b": i["other"],
                "a_label": what,
                "b_label": i["label"], "volume": i["volume"], "depth": i["depth"],
                "virtual": true,
            }));
        }
        list.sort_by(|x, y| {
            y["volume"]
                .as_f64()
                .unwrap_or(0.0)
                .total_cmp(&x["volume"].as_f64().unwrap_or(0.0))
        });
        let bodies = found["bodies"].as_u64().unwrap_or(0);
        if list.is_empty() {
            let d = MessageDialog::builder(
                &frame,
                &format!("No overlaps among {bodies} elements."),
                "Check Overlaps",
            )
            .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation)
            .build();
            d.show_modal();
            return;
        }
        let rows: Vec<String> = list
            .iter()
            .enumerate()
            .map(|(k, o)| {
                let volume = o["volume"].as_f64().unwrap_or(0.0);
                let depth = o["depth"].as_f64().unwrap_or(0.0);
                format!(
                    "{}. {} × {} — {} (depth {})",
                    k + 1,
                    o["a_label"].as_str().unwrap_or_default(),
                    o["b_label"].as_str().unwrap_or_default(),
                    if volume < 0.01 {
                        format!("{:.2} L", volume * 1000.0)
                    } else {
                        self.format_value(volume, "volume")
                    },
                    self.format_length(depth),
                )
            })
            .collect();
        let labels: Vec<&str> = rows.iter().map(String::as_str).collect();
        let dialog = SingleChoiceDialog::builder(
            &frame,
            &format!(
                "{} overlap(s) among {bodies} elements. Pick one to select and frame it:",
                list.len()
            ),
            "Check Overlaps",
            &labels,
        )
        .build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let picked = usize::try_from(dialog.get_selection()).ok();
        dialog.destroy();
        let Some(o) = picked.and_then(|i| list.get(i)) else {
            return;
        };
        let (a_label, b_label) = (
            o["a_label"].as_str().unwrap_or_default().to_owned(),
            o["b_label"].as_str().unwrap_or_default().to_owned(),
        );
        // How to resolve it (ARC-MOD-032).
        let is_virtual = o["virtual"] == true;
        let mut actions: Vec<(String, &str)> = vec![
            ("Select and frame them".to_owned(), "select"),
            ("Accept as intended".to_owned(), "accept"),
        ];
        if !is_virtual {
            actions.push((format!("Cut {a_label} by {b_label}"), "cut_a"));
            actions.push((format!("Cut {b_label} by {a_label}"), "cut_b"));
            actions.push(("Join (by precedence)".to_owned(), "join"));
            actions.push((
                format!("Keep only what {a_label} shares (intersect)"),
                "intersect",
            ));
            actions.push((format!("Fuse {b_label} into {a_label} (union)"), "union"));
            actions.push((format!("Move {b_label} apart"), "move_b"));
            actions.push((format!("Move {a_label} apart"), "move_a"));
        }
        let labels: Vec<&str> = actions.iter().map(|x| x.0.as_str()).collect();
        let choose = SingleChoiceDialog::builder(
            &frame,
            &format!("{a_label} × {b_label}:"),
            "Resolve Overlap",
            &labels,
        )
        .build();
        if choose.show_modal() != ID_OK {
            return;
        }
        let action = usize::try_from(choose.get_selection())
            .ok()
            .and_then(|i| actions.get(i))
            .map_or("select", |x| x.1);
        let resolve = match action {
            "accept" => Some(json!({ "a": o["a"], "b": o["b"], "mode": "accept" })),
            "move_b" => {
                Some(json!({ "a": o["a"], "b": o["b"], "mode": "move_apart", "moves": "b" }))
            }
            "move_a" => {
                Some(json!({ "a": o["a"], "b": o["b"], "mode": "move_apart", "moves": "a" }))
            }
            mode @ ("cut_a" | "cut_b" | "join" | "intersect" | "union") => {
                Some(json!({ "a": o["a"], "b": o["b"], "mode": mode }))
            }
            _ => None,
        };
        if let Some(args) = resolve {
            match self.run("overlap.resolve", args) {
                Ok(_) => {
                    self.refresh_scene(&frame);
                    let what = match action {
                        "accept" => "accepted",
                        "move_a" | "move_b" => "moved apart",
                        _ => "resolved by a boolean",
                    };
                    return frame
                        .set_status_text(&format!("Overlap {what}: {a_label} × {b_label}"), 0);
                }
                Err(e) => return error(&frame, &format!("The overlap was not resolved:\n{e}")),
            }
        }
        if is_virtual {
            self.reveal_virtual_volumes();
        }
        let _ = self.run("selection.set", json!({ "ids": [o["a"], o["b"]] }));
        self.selection_changed();
        self.zoom_to_fit();
        frame.set_status_text(
            &format!(
                "Overlap: {} × {}",
                o["a_label"].as_str().unwrap_or_default(),
                o["b_label"].as_str().unwrap_or_default()
            ),
            0,
        );
    }

    /// An element's class and id, for lists.
    fn element_label(&self, id: &serde_json::Value) -> String {
        let Some(n) = id.as_u64() else {
            return String::new();
        };
        self.doc
            .and_then(|d| self.app.document(d))
            .and_then(|d| {
                d.doc
                    .store()
                    .get(reviera_core::ElementId(n))
                    .map(|e| e.class.clone())
            })
            .map_or_else(|| format!("#{n}"), |c| format!("{c} {n}"))
    }

    /// A value in the project's units.
    fn format_value(&mut self, v: f64, data_type: &str) -> String {
        self.run(
            "units.format",
            json!({ "value": v, "data_type": data_type }),
        )
        .ok()
        .and_then(|s| s.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{v:.4}"))
    }
}

impl Gui {
    /// View › Virtual Volumes: hidden, translucent, solid, in turn, for the active view.
    pub(crate) fn cycle_virtual_volumes(&mut self) {
        let frame = self.frame;
        let Some(v) = self.active_view() else { return };
        let Some(view) = v.element else {
            return frame
                .set_status_text("Save the model's views first (this view has no element)", 0);
        };
        let next = match self.virtual_mode(v) {
            reviera_render::VirtualMode::Hidden => "translucent",
            reviera_render::VirtualMode::Translucent => "solid",
            reviera_render::VirtualMode::Solid => "hidden",
        };
        if let Err(e) = self.run(
            "view.set_virtual_volumes",
            json!({ "view_id": view, "mode": next }),
        ) {
            return error(&frame, &e);
        }
        self.refresh_scene(&frame);
        frame.set_status_text(&format!("Virtual volumes: {next}"), 0);
    }

    /// Shows virtual volumes translucent in the active view if they are hidden.
    fn reveal_virtual_volumes(&mut self) {
        let Some(v) = self.active_view() else { return };
        if self.virtual_mode(v) == reviera_render::VirtualMode::Hidden
            && let Some(view) = v.element
        {
            let _ = self.run(
                "view.set_virtual_volumes",
                json!({ "view_id": view, "mode": "translucent" }),
            );
            let frame = self.frame;
            self.refresh_scene(&frame);
        }
    }
}

impl Gui {
    /// Modify › Remove Boolean: the selected elements' booleans are removed; their own bodies
    /// return.
    pub(crate) fn remove_booleans(&mut self) {
        let frame = self.frame;
        let selected = self.selection();
        if selected.is_empty() {
            return frame.set_status_text("Select the elements to remove booleans from", 0);
        }
        let mut n = 0;
        for id in selected {
            if self
                .run("geometry.remove_boolean", json!({ "target_id": id }))
                .is_ok()
            {
                n += 1;
            }
        }
        self.refresh_scene(&frame);
        frame.set_status_text(
            &if n == 0 {
                "The selection has no booleans".to_owned()
            } else {
                format!("Removed the booleans of {n} element(s)")
            },
            0,
        );
    }
}
