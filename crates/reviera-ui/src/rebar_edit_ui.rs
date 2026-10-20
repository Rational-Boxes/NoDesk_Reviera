// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Editing rebar sets on their contextual tab (STR-RBR-006, STR-RBR-015): Remove Bar, Split Set
//! and Move Bar act on the bar clicked; Reset Bars brings them back; Propagate copies the set to
//! the hosts clicked (Enter finishes); Pick New Host moves it to the host clicked; Splice cuts its
//! straight bars with a lap; Coupler joins the bar ends clicked (one, or two). Segments are edited
//! in Properties.

use glam::Vec3;
use reviera_core::ElementId;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::rebar_view_ui::{BarPick, BarPurpose};
use super::{Gui, error};

/// Propagate or Pick New Host in progress: the sets and the hosts clicked so far.
#[derive(Debug, Clone)]
pub struct HostPick {
    pub propagate: bool,
    pub sets: Vec<ElementId>,
    pub hosts: Vec<ElementId>,
}

/// Coupler in progress: the bar ends that can be clicked (set, bar, end, point) and the picks.
#[derive(Debug, Clone)]
pub struct EndPick {
    pub ends: Vec<(ElementId, usize, &'static str, [f64; 3])>,
    pub picked: Vec<usize>,
}

impl Gui {
    fn one_set(&mut self, tool: &str) -> Option<ElementId> {
        let sets = self.selected_reinforcement(true);
        match sets[..] {
            [s] => Some(s),
            _ => {
                self.frame.set_status_text(&format!("{tool}: select one rebar set"), 0);
                None
            }
        }
    }

    /// Remove Bar, Split Set or Move Bar: click the bar.
    pub(crate) fn start_bar_edit(&mut self, purpose: BarPurpose) {
        let frame = self.frame;
        let Some(set) = self.one_set("Edit Bars") else { return };
        let Ok(i) = self.run("rebar.info", json!({ "rebar_ids": [set] })) else { return };
        let bars: Vec<Vec<[f64; 3]>> = i[0]["bars"].as_array().into_iter().flatten().map(|b| serde_json::from_value(b["points"].clone()).unwrap_or_default()).collect();
        let indices: Vec<usize> = serde_json::from_value(i[0]["bar_indices"].clone()).unwrap_or_default();
        let view = self.active_view().and_then(|v| v.element).unwrap_or(ElementId(0));
        let end = self.frame;
        self.end_tool(&end);
        self.bar_pick = Some(BarPick { purpose, set, view, bars, indices, picked: Vec::new() });
        frame.set_status_text(
            match purpose {
                BarPurpose::Remove => "Remove Bar: click the bar to remove (Esc ends)",
                BarPurpose::Split => "Split Set: click the first bar of the new set",
                _ => "Move Bar: click the bar to move",
            },
            0,
        );
    }

    /// The bar clicked in Remove Bar, Split Set or Move Bar.
    pub(crate) fn bar_edit(&mut self, k: usize) {
        let frame = self.frame;
        let Some(p) = self.bar_pick.clone() else { return };
        let index = p.indices.get(k).copied().unwrap_or(k);
        let result = match p.purpose {
            BarPurpose::Remove => self.run("rebar.remove_bar", json!({ "rebar_id": p.set, "index": index })),
            BarPurpose::Split => {
                self.end_bar_pick();
                self.run("rebar.split_set", json!({ "rebar_id": p.set, "at_index": k }))
            }
            BarPurpose::Move => {
                self.end_bar_pick();
                let d = TextEntryDialog::builder(&frame, "Move the bar along the set by (mm, + or −):", "Move Bar").with_default_value("50").build();
                if d.show_modal() != ID_OK {
                    return;
                }
                let Some(mm) = d.get_value().and_then(|v| v.trim().parse::<f64>().ok()) else { return };
                self.run("rebar.move_bar_in_set", json!({ "rebar_id": p.set, "bar_index": index, "delta": mm / 1000.0 }))
            }
            BarPurpose::Present => return,
        };
        match result {
            Ok(_) => {
                self.refresh_scene(&frame);
                self.properties_stale = true;
                // Remove Bar goes on with the bars left.
                if p.purpose == BarPurpose::Remove {
                    self.end_bar_pick();
                    let _ = self.run("selection.set", json!({ "ids": [p.set] }));
                    self.start_bar_edit(BarPurpose::Remove);
                } else {
                    frame.set_status_text("Done", 0);
                }
            }
            Err(e) => {
                self.end_bar_pick();
                error(&frame, &format!("The set was not changed:\n{e}"));
            }
        }
    }

    pub(crate) fn reset_rebar_bars(&mut self) {
        let frame = self.frame;
        let Some(set) = self.one_set("Reset Bars") else { return };
        match self.run("rebar.reset_bars", json!({ "rebar_id": set })) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text("Removed and moved bars are back", 0);
            }
            Err(e) => error(&frame, &format!("The bars were not reset:\n{e}")),
        }
    }

    /// Propagate (`true`) or Pick New Host: click hosts.
    pub(crate) fn start_rebar_host_pick(&mut self, propagate: bool) {
        let frame = self.frame;
        let sets = self.selected_reinforcement(true);
        if sets.is_empty() {
            return frame.set_status_text("Select rebar sets first", 0);
        }
        let end = self.frame;
        self.end_tool(&end);
        self.rebar_host_pick = Some(HostPick { propagate, sets, hosts: Vec::new() });
        frame.set_status_text(
            if propagate {
                "Propagate: click the hosts to copy the set to; Enter finishes, Esc cancels"
            } else {
                "Pick New Host: click the new host"
            },
            0,
        );
    }

    pub(crate) fn end_rebar_host_pick(&mut self) -> bool {
        self.rebar_host_pick.take().is_some()
    }

    pub(crate) fn rebar_host_click(&mut self, x: i32, y: i32, panel: &Panel) {
        let frame = self.frame;
        let Some(host) = self.host_element_at(x, y, panel) else {
            return frame.set_status_text("Click a concrete column, beam, structural floor or wall", 0);
        };
        let Some(p) = &mut self.rebar_host_pick else { return };
        if p.propagate {
            if !p.hosts.contains(&host) {
                p.hosts.push(host);
            }
            let n = p.hosts.len();
            let hosts = p.hosts.clone();
            let _ = self.run("selection.set", json!({ "ids": hosts }));
            self.selection_changed();
            return frame.set_status_text(&format!("Propagate: {n} host(s); Enter copies the set to them"), 0);
        }
        let sets = p.sets.clone();
        self.rebar_host_pick = None;
        match self.run("rebar.pick_new_host", json!({ "rebar_ids": sets, "host_id": host })) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text("The set moved to its new host", 0);
            }
            Err(e) => error(&frame, &format!("The set was not moved:\n{e}")),
        }
    }

    /// The element under the cursor, ignoring rebar drawn over it.
    fn host_element_at(&self, x: i32, y: i32, panel: &Panel) -> Option<ElementId> {
        let store = self.store_now()?;
        let id = self.element_at(x, y, panel)?;
        let e = store.get(id)?;
        reviera_core::rebar::host_check(&store, e).is_ok().then_some(id)
    }

    /// Enter finishes Propagate.
    pub(crate) fn rebar_host_key(&mut self, key: i32) -> bool {
        if !matches!(key, WXK_RETURN | WXK_NUMPAD_ENTER) || self.rebar_host_pick.as_ref().is_none_or(|p| !p.propagate) {
            return false;
        }
        let frame = self.frame;
        let p = self.rebar_host_pick.take().expect("checked");
        if p.hosts.is_empty() {
            frame.set_status_text("Propagate: no hosts picked", 0);
            return true;
        }
        match self.run("rebar.propagate", json!({ "rebar_ids": p.sets, "target_host_ids": p.hosts })) {
            Ok(out) => {
                self.refresh_scene(&frame);
                let made = out["created"].as_array().map_or(0, Vec::len);
                let failed: Vec<String> = out["failed"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|f| format!("{}: {}", f[0], f[1].as_str().unwrap_or("")))
                    .collect();
                if failed.is_empty() {
                    frame.set_status_text(&format!("{made} set(s) made"), 0);
                } else {
                    error(&frame, &format!("{made} set(s) made; not in:\n{}", failed.join("\n")));
                }
            }
            Err(e) => error(&frame, &format!("Nothing was propagated:\n{e}")),
        }
        true
    }

    /// Splice: where, lap and stagger.
    pub(crate) fn splice_rebar(&mut self) {
        let frame = self.frame;
        let Some(set) = self.one_set("Splice") else { return };
        let Ok(i) = self.run("rebar.info", json!({ "rebar_ids": [set] })) else { return };
        let full = i[0]["lengths"][0].as_f64().unwrap_or(0.0);
        let d = TextEntryDialog::builder(
            &frame,
            &format!("The bars are {:.0} mm long. Splice at (mm from their start), lap (mm, blank for 40 diameters), stagger (mm):", full * 1000.0),
            "Splice",
        )
        .with_default_value(&format!("{:.0}, , 0", full * 500.0))
        .build();
        if d.show_modal() != ID_OK {
            return;
        }
        let text = d.get_value().unwrap_or_default();
        let parts: Vec<&str> = text.split(',').map(str::trim).collect();
        let mm = |k: usize| parts.get(k).and_then(|v| v.parse::<f64>().ok()).map(|v| v / 1000.0);
        let Some(at) = mm(0) else { return error(&frame, "Give where the splice goes, in mm") };
        let mut args = json!({ "rebar_id": set, "mode": "length", "length": at, "stagger": mm(2).unwrap_or(0.0) });
        if let Some(l) = mm(1) {
            args["lap"] = json!(l);
        }
        match self.run("rebar.splice", args) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text("Spliced into two sets that overlap by the lap", 0);
            }
            Err(e) => error(&frame, &format!("The set was not spliced:\n{e}")),
        }
    }

    /// Coupler: click a bar end of the selected sets, then a second (or Enter for one).
    pub(crate) fn start_coupler(&mut self) {
        let frame = self.frame;
        let sets = self.selected_reinforcement(true);
        if sets.is_empty() {
            return frame.set_status_text("Coupler: select the rebar sets whose bar ends to join", 0);
        }
        let mut ends = Vec::new();
        for s in sets {
            let Ok(i) = self.run("rebar.info", json!({ "rebar_ids": [s] })) else { continue };
            for (k, b) in i[0]["bars"].as_array().into_iter().flatten().enumerate() {
                let pts: Vec<[f64; 3]> = serde_json::from_value(b["points"].clone()).unwrap_or_default();
                if let (Some(a), Some(z)) = (pts.first(), pts.last()) {
                    ends.push((s, k, "start", *a));
                    ends.push((s, k, "end", *z));
                }
            }
        }
        let end = self.frame;
        self.end_tool(&end);
        self.end_pick = Some(EndPick { ends, picked: Vec::new() });
        frame.set_status_text("Coupler: click a bar end, then the end to join it to (Enter for one end)", 0);
    }

    pub(crate) fn end_coupler(&mut self) -> bool {
        self.end_pick.take().is_some()
    }

    pub(crate) fn coupler_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        let Some(v) = self.active_view() else { return };
        let size = v.panel.get_client_size();
        let Some(p) = &self.end_pick else { return };
        let near = p
            .ends
            .iter()
            .enumerate()
            .filter_map(|(k, e)| {
                let s = v.camera.project(Vec3::new(e.3[0] as f32, e.3[1] as f32, e.3[2] as f32), size.width as f32, size.height as f32)?;
                let d = (s.0 - x as f32).hypot(s.1 - y as f32);
                (d < 12.0).then_some((k, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, _)| k);
        let Some(k) = near else {
            return frame.set_status_text("Coupler: click near a bar end of the selected sets", 0);
        };
        if let Some(p) = &mut self.end_pick {
            p.picked.push(k);
            if p.picked.len() < 2 {
                return frame.set_status_text("Coupler: click the other bar end, or Enter for a coupler on this end", 0);
            }
        }
        self.place_coupler();
    }

    pub(crate) fn coupler_key(&mut self, key: i32) -> bool {
        if self.end_pick.is_none() || !matches!(key, WXK_RETURN | WXK_NUMPAD_ENTER) {
            return false;
        }
        self.place_coupler();
        true
    }

    fn place_coupler(&mut self) {
        let frame = self.frame;
        let Some(p) = self.end_pick.take() else { return };
        if p.picked.is_empty() {
            return frame.set_status_text("Coupler: no bar end picked", 0);
        }
        let refs: Vec<Value> = p
            .picked
            .iter()
            .map(|k| {
                let e = &p.ends[*k];
                json!({ "rebar_id": e.0, "bar": e.1, "end": e.2 })
            })
            .collect();
        match self.run("rebar_coupler.place", json!({ "bar_end_refs": refs })) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text("Coupler placed", 0);
            }
            Err(e) => error(&frame, &format!("The coupler was not placed:\n{e}")),
        }
    }
}
