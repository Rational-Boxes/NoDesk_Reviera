// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! File › Import BCF… and Export BCF… (COL-BCF-001): the model's own topics as `.bcf`
//! archives. The issues themselves are in the docked Issues pane ([`super::issues_pane`]).

use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

impl Gui {
    /// File › Import BCF…: topics from a .bcf archive, merged by GUID.
    pub(crate) fn bcf_import(&mut self) {
        let frame = self.frame;
        let Some(path) = self.open_target("Import BCF", "BCF (*.bcf;*.bcfzip)|*.bcf;*.bcfzip") else { return };
        match self.run("bcf.import", json!({ "path": path })) {
            Ok(r) => {
                let mut msg = format!(
                    "Read {} issues (BCF {}): {} new, {} updated, {} unchanged; {} viewpoints, {} snapshots.",
                    r["topics"],
                    r["version"].as_str().unwrap_or("?"),
                    r["added"],
                    r["updated"],
                    r["unchanged"],
                    r["viewpoints"],
                    r["snapshots"]
                );
                for p in r["problems"].as_array().into_iter().flatten() {
                    msg.push_str(&format!("\n• {}", p.as_str().unwrap_or("")));
                }
                MessageDialog::builder(&frame, &msg, "Import BCF").with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation).build().show_modal();
            }
            Err(e) => error(&frame, &e),
        }
    }

    /// File › Export BCF…: every topic as a .bcf archive, BCF 3.0 or 2.1.
    pub(crate) fn bcf_export(&mut self) {
        let frame = self.frame;
        let versions = ["BCF 3.0".to_owned(), "BCF 2.1".to_owned()];
        let f = super::reinforcement_ui::Form::new(&frame, "Export BCF");
        let v = f.choice("Version", &versions, 0);
        let ok = f.show("2.1 for tools that do not read 3.0 yet.", "Next…");
        let version = if v.get_selection() == Some(1) { "2.1" } else { "3.0" };
        f.dialog.destroy();
        if !ok {
            return;
        }
        let Some(path) = self.save_target("Export BCF", "BCF (*.bcf)|*.bcf|BCF zip (*.bcfzip)|*.bcfzip", "issues.bcf") else { return };
        match self.run("bcf.export", json!({ "path": path, "version": version })) {
            Ok(r) => frame.set_status_text(&format!("Wrote {} issues, {} comments, {} viewpoints and {} snapshots (BCF {version}) to {path}", r["topics"], r["comments"], r["viewpoints"], r["snapshots"]), 0),
            Err(e) => error(&frame, &e),
        }
    }
}
