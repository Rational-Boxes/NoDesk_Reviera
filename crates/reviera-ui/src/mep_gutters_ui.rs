// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Systems › Plumbing › Gutter and Gutter Outlet (MEP-PIPE-010). Gutter: for the selected roof,
//! the eaves to run along (all by default, in order round the roof), the profile, width, depth
//! and fall. Gutter Outlet: for the selected gutter, where along it, the downspout's diameter
//! and the level it drops to; then a size check at a design rainfall.

use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

fn num(t: &TextCtrl) -> Option<f64> {
    t.get_value().trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite())
}

impl Gui {
    /// Systems › Gutter for the selected roof.
    pub(crate) fn create_gutter(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let roof = self.selection().into_iter().find(|i| store.get(*i).and_then(reviera_core::roofs::roof_recipe).is_some());
        let Some(roof) = roof else {
            return frame.set_status_text("Gutter: select a roof first", 0);
        };
        let eaves: Vec<u32> = match self.run("roof.eaves", json!({ "roof_id": roof })) {
            Ok(v) => v.as_array().into_iter().flatten().filter_map(|r| r["key"].as_u64().map(|k| k as u32)).collect(),
            Err(e) => return error(&frame, &e),
        };
        let mut keys: Vec<u32> = Vec::new();
        if eaves.len() > 1 {
            let labels: Vec<String> = eaves.iter().map(|k| format!("Eave {k}")).collect();
            let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
            let d = MultiChoiceDialog::builder(&frame, "Eaves the gutter runs along, in order round the roof (all when none is ticked):", "Gutter", &refs).build();
            d.set_selections(&(0..eaves.len() as i32).collect::<Vec<_>>());
            if d.show_modal() != ID_OK {
                return;
            }
            let sel = d.get_selections();
            if sel.len() < eaves.len() {
                keys = sel.iter().filter_map(|k| eaves.get(*k as usize).copied()).collect();
            }
        }
        let f = super::reinforcement_ui::Form::new(&frame, "Gutter");
        let profiles = [("half_round", "Half-round"), ("k_style", "K-style"), ("box", "Box")];
        let profile = f.choice("Profile", &profiles.iter().map(|p| p.1.to_owned()).collect::<Vec<_>>(), 0);
        let width = f.text("Width (mm)", "125", "");
        let depth = f.text("Depth (mm)", "", "Empty: half the width");
        let fall = f.text("Fall (1 in)", "500", "The gutter falls toward its outlets");
        let ok = f.show("The gutter hangs on the fascia (or at the eave) and follows the roof.", "Make");
        let mut args = json!({
            "roof_id": roof,
            "edges": keys,
            "profile": profiles[profile.get_selection().map_or(0, |k| k as usize).min(2)].0,
            "width": num(&width).map(|v| v / 1000.0),
        });
        if let Some(d) = num(&depth) {
            args["depth"] = json!(d / 1000.0);
        }
        if let Some(n) = num(&fall).filter(|n| *n > 0.0) {
            args["fall"] = json!(1.0 / n);
        }
        f.dialog.destroy();
        if !ok {
            return;
        }
        match self.run("gutter.create", args) {
            Ok(v) => {
                self.refresh_scene(&frame);
                if let Some(g) = v.as_u64() {
                    self.select_ids(&[reviera_core::ElementId(g)], super::select::Combine::Replace);
                }
                frame.set_status_text("Gutter made: Gutter Outlet adds outlets and downspouts", 0);
            }
            Err(e) => error(&frame, &format!("No gutter was made:\n{e}")),
        }
    }

    /// Systems › Gutter Outlet for the selected gutter.
    pub(crate) fn gutter_outlet(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let gutter = self.selection().into_iter().find(|i| store.get(*i).and_then(reviera_core::mep_gutters::gutter_recipe).is_some());
        let Some(gutter) = gutter else {
            return frame.set_status_text("Gutter Outlet: select a gutter first", 0);
        };
        let r = store.get(gutter).and_then(reviera_core::mep_gutters::gutter_recipe).expect("a gutter");
        let length = reviera_core::mep_gutters::geometry(&store, &r).map_or(0.0, |g| g.length);
        let mut levels: Vec<(reviera_core::ElementId, String, f64)> = store
            .iter()
            .filter(|e| e.class == "IfcBuildingStorey")
            .map(|e| (e.id(), e.name.clone().unwrap_or_default(), reviera_core::levels::elevation_of(&store, e.guid().as_str()).unwrap_or(0.0)))
            .collect();
        levels.sort_by(|a, b| a.2.total_cmp(&b.2));
        let f = super::reinforcement_ui::Form::new(&frame, "Gutter Outlet");
        let at = f.text("Along the gutter (m)", &format!("{:.2}", length / 2.0), &format!("0 to {length:.2} m from its start"));
        let dia = f.text("Downspout (mm)", "68", "");
        let mut labels = vec!["No downspout".to_owned()];
        labels.extend(levels.iter().map(|l| format!("Down to {} ({:.2} m)", l.1, l.2)));
        let to = f.choice("Downspout", &labels, if levels.is_empty() { 0 } else { 1 });
        let ok = f.show("The downspout drops from the outlet, bends back to the wall and runs down to the level.", "Add");
        let mut args = json!({ "gutter_id": gutter, "at": num(&at), "diameter": num(&dia).map(|v| v / 1000.0) });
        if let Some(l) = to.get_selection().and_then(|k| (k as usize).checked_sub(1)).and_then(|k| levels.get(k)) {
            args["downspout_to"] = json!({ "level": l.0 });
        }
        f.dialog.destroy();
        if !ok {
            return;
        }
        match self.run("gutter.add_outlet", args) {
            Ok(_) => {
                self.refresh_scene(&frame);
                let check: Value = self.run("gutter.size", json!({ "gutter_id": gutter })).unwrap_or(Value::Null);
                let warnings: Vec<String> = check["warnings"].as_array().into_iter().flatten().filter_map(|w| w.as_str().map(str::to_owned)).collect();
                if warnings.is_empty() {
                    frame.set_status_text("Outlet and downspout added; sizes check at 75 mm/h", 0);
                } else {
                    error(&frame, &format!("Outlet added. At 75 mm/h:\n{}", warnings.join("\n")));
                }
            }
            Err(e) => error(&frame, &format!("No outlet was added:\n{e}")),
        }
    }
}
