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
        let list = found["overlaps"].as_array().cloned().unwrap_or_default();
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
        let Some(o) = usize::try_from(dialog.get_selection())
            .ok()
            .and_then(|i| list.get(i))
        else {
            return;
        };
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
