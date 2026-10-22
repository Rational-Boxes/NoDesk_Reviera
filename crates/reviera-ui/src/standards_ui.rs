// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Manage › Settings › Transfer Project Standards (DM-XFER-001): from another open project,
//! the kinds of standards to copy into the active one, what to do with items it has already
//! (Overwrite, New only, Rename), and a summary of what was copied.

use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

impl Gui {
    pub(crate) fn transfer_standards(&mut self) {
        let frame = self.frame;
        let others: Vec<_> = self.app.documents().into_iter().filter(|d| !d.active).collect();
        if others.is_empty() {
            return frame.set_status_text("Transfer Project Standards: open the source project too (File › Open), then make the target active", 0);
        }
        let titles: Vec<String> = others.iter().map(|d| d.title.clone()).collect();
        let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
        let d = SingleChoiceDialog::builder(&frame, "Copy from:", "Transfer Project Standards", &refs).build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some(src) = usize::try_from(d.get_selection()).ok().and_then(|k| others.get(k)).map(|d| d.id) else { return };
        let kinds = match self.run("standards.list", json!({ "source_document_id": src })) {
            Ok(v) => v.as_array().cloned().unwrap_or_default(),
            Err(e) => return error(&frame, &e),
        };
        let labels: Vec<String> = kinds.iter().map(|k| format!("{} ({})", k["label"].as_str().unwrap_or(""), k["items"].as_array().map_or(0, Vec::len))).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let m = MultiChoiceDialog::builder(&frame, "Kinds to copy (types bring the types they use):", "Transfer Project Standards", &refs).build();
        if m.show_modal() != ID_OK {
            return;
        }
        let chosen: Vec<&Value> = m.get_selections().iter().filter_map(|k| kinds.get(*k as usize)).collect();
        if chosen.is_empty() {
            return;
        }
        let modes = ["Overwrite (keep this project's items, take the source's values)", "New only (skip what this project has)", "Rename (copy under a new name)"];
        let c = SingleChoiceDialog::builder(&frame, "Items this project already has:", "Transfer Project Standards", &modes).build();
        if c.show_modal() != ID_OK {
            return;
        }
        let mode = ["overwrite", "new_only", "rename"][usize::try_from(c.get_selection()).unwrap_or(1).min(2)];
        let selection: Vec<Value> = chosen.iter().map(|k| json!({ "kind": k["kind"] })).collect();
        match self.run("standards.transfer", json!({ "source_document_id": src, "selection": selection, "on_conflict": mode })) {
            Ok(r) => {
                self.refresh_scene(&frame);
                self.properties_stale = true;
                let n = |k: &str| r[k].as_array().map_or(0, Vec::len);
                let mut text = format!("Copied {}, overwritten {}, renamed {}, skipped {}, matched {}.", n("copied"), n("overwritten"), n("renamed"), n("skipped"), n("matched"));
                for k in ["copied", "overwritten", "renamed", "skipped", "matched"] {
                    for item in r[k].as_array().into_iter().flatten().take(15) {
                        text.push_str(&format!("\n{k}: {}", item.as_str().unwrap_or("")));
                    }
                }
                MessageDialog::builder(&frame, &text, "Transfer Project Standards").with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation).build().show_modal();
            }
            Err(e) => error(&frame, &e),
        }
    }
}
