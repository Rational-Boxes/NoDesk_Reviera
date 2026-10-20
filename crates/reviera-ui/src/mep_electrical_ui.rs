// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Systems › Electrical (MEP-ELEC-002, MEP-ELEC-003): Create Circuit from the selected devices
//! (the kind chosen; incompatible devices skipped and listed), Add to Circuit and Remove from
//! Circuit, Select Panel (only compatible panels with free slots are offered), Disconnect Panel,
//! Panel Settings (name, distribution system, slots, naming) and Check Circuits. A selected
//! device's or panel's circuits are drawn dashed from the panel to each device.

use reviera_core::ElementId;
use reviera_core::mep_electrical::{self, CircuitInfo, CircuitKind};
use serde_json::json;
use wxdragon::prelude::*;

use super::Gui;

const KINDS: [(CircuitKind, &str); 8] = [
    (CircuitKind::Power, "power"),
    (CircuitKind::Data, "data"),
    (CircuitKind::Telephone, "telephone"),
    (CircuitKind::Security, "security"),
    (CircuitKind::FireAlarm, "fire_alarm"),
    (CircuitKind::NurseCall, "nurse_call"),
    (CircuitKind::Controls, "controls"),
    (CircuitKind::Communication, "communication"),
];

fn label(c: &CircuitInfo) -> String {
    let num = if c.name.is_empty() { "(no panel)".to_owned() } else { c.name.clone() };
    format!("{num} — {} {:.0} V {}P, {} device(s), {:.0} VA", c.kind.label(), c.voltage, c.poles, c.elements.len(), c.load)
}

impl Gui {
    pub(crate) fn choose_circuit(&mut self, title: &str, keep: &dyn Fn(&CircuitInfo) -> bool) -> Option<CircuitInfo> {
        let frame = self.frame;
        let store = self.store_now()?;
        let all: Vec<CircuitInfo> = mep_electrical::circuits(&store).into_iter().filter(|c| keep(c)).collect();
        if all.is_empty() {
            frame.set_status_text(&format!("{title}: there is no such circuit (Create Circuit makes one)"), 0);
            return None;
        }
        if all.len() == 1 {
            return all.into_iter().next();
        }
        let labels: Vec<String> = all.iter().map(label).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let d = SingleChoiceDialog::builder(&frame, "Circuit:", title, &refs).build();
        if d.show_modal() != ID_OK {
            return None;
        }
        usize::try_from(d.get_selection()).ok().and_then(|k| all.get(k)).cloned()
    }

    /// Systems › Create Circuit.
    pub(crate) fn create_circuit(&mut self) {
        let frame = self.frame;
        let ids = self.selection();
        if ids.is_empty() {
            return frame.set_status_text("Create Circuit: select the devices or fixtures first", 0);
        }
        let labels: Vec<&str> = KINDS.iter().map(|k| k.0.label()).collect();
        let d = SingleChoiceDialog::builder(&frame, "Circuit kind:", "Create Circuit", &labels).build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some((_, kind)) = usize::try_from(d.get_selection()).ok().and_then(|k| KINDS.get(k)) else { return };
        match self.app.execute("circuit.create", json!({ "kind": kind, "element_ids": ids }), &reviera_core::Caller::user()) {
            Ok(o) => {
                let r = &o.result;
                let added = r["added"].as_array().map_or(0, Vec::len);
                let mut text = format!("Circuit of {added} device(s) made: Select Panel puts it on a panel");
                if let Some(s) = r["skipped"].as_array().filter(|s| !s.is_empty()) {
                    text.push_str(&format!("; {} skipped ({})", s.len(), s[0]["reason"].as_str().unwrap_or("")));
                }
                self.refresh_active();
                self.refresh_schedules();
                frame.set_status_text(&text, 0);
            }
            Err(e) => error(&frame, &format!("No circuit was made:\n{e}")),
        }
    }

    /// Systems › Add to Circuit / Remove from Circuit for the selected devices.
    pub(crate) fn circuit_members(&mut self, add: bool) {
        let frame = self.frame;
        let ids = self.selection();
        if ids.is_empty() {
            return frame.set_status_text("Select the devices first", 0);
        }
        let title = if add { "Add to Circuit" } else { "Remove from Circuit" };
        let keep = |c: &CircuitInfo| add || ids.iter().any(|i| c.elements.contains(i));
        let Some(c) = self.choose_circuit(title, &keep) else { return };
        let command = if add { "circuit.add" } else { "circuit.remove" };
        match self.run(command, json!({ "circuit_id": c.id, "element_ids": ids })) {
            Ok(_) => {
                self.refresh_active();
                self.refresh_schedules();
                frame.set_status_text(title, 0);
            }
            Err(e) => error(&frame, &format!("{title}:\n{e}")),
        }
    }

    /// Systems › Select Panel for a circuit of the selection.
    pub(crate) fn select_circuit_panel(&mut self) {
        let frame = self.frame;
        let sel = self.selection();
        let keep = |c: &CircuitInfo| sel.is_empty() || sel.iter().any(|i| c.elements.contains(i) || c.id == *i);
        let Some(c) = self.choose_circuit("Select Panel", &keep) else { return };
        let panels = match self.run("circuit.compatible_panels", json!({ "circuit_id": c.id })) {
            Ok(v) => v,
            Err(e) => return error(&frame, &e),
        };
        let list: Vec<(u64, String)> = panels
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| {
                (
                    p["id"].as_u64().unwrap_or(0),
                    format!("{} — {}, {} free slot(s)", p["name"].as_str().unwrap_or(""), p["distribution_system"].as_str().unwrap_or(""), p["free_slots"]),
                )
            })
            .collect();
        if list.is_empty() {
            return frame.set_status_text(
                &format!("Select Panel: no panel fits a {:.0} V {}-pole circuit with free slots (Panel Settings sets a panel's distribution system)", c.voltage, c.poles),
                0,
            );
        }
        let refs: Vec<&str> = list.iter().map(|p| p.1.as_str()).collect();
        let d = SingleChoiceDialog::builder(&frame, "Compatible panels:", "Select Panel", &refs).build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some((pid, _)) = usize::try_from(d.get_selection()).ok().and_then(|k| list.get(k)) else { return };
        match self.run("circuit.select_panel", json!({ "circuit_id": c.id, "panel_id": pid })) {
            Ok(v) => {
                self.refresh_active();
                self.refresh_schedules();
                frame.set_status_text(&format!("The circuit is {} on the panel", v.as_str().unwrap_or("")), 0);
            }
            Err(e) => error(&frame, &format!("Select Panel:\n{e}")),
        }
    }

    /// Systems › Disconnect Panel.
    pub(crate) fn disconnect_circuit_panel(&mut self) {
        let frame = self.frame;
        let sel = self.selection();
        let keep = |c: &CircuitInfo| c.panel.is_some() && (sel.is_empty() || sel.iter().any(|i| c.elements.contains(i) || c.panel == Some(*i)));
        let Some(c) = self.choose_circuit("Disconnect Panel", &keep) else { return };
        match self.run("circuit.disconnect_panel", json!({ "circuit_id": c.id })) {
            Ok(_) => {
                self.refresh_active();
                self.refresh_schedules();
                frame.set_status_text("The circuit is off its panel and has no number", 0);
            }
            Err(e) => error(&frame, &format!("Disconnect Panel:\n{e}")),
        }
    }

    /// Systems › Panel Settings for the selected panel.
    pub(crate) fn panel_settings(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let panels: Vec<ElementId> = self
            .selection()
            .into_iter()
            .filter(|i| store.get(*i).is_some_and(|e| mep_electrical::is_panel(&store, e)))
            .collect();
        let [pid] = panels[..] else {
            return frame.set_status_text("Panel Settings: select one panel (a distribution board) first", 0);
        };
        let e = store.get(pid).expect("selected");
        let pn = mep_electrical::panel(e);
        let settings = mep_electrical::settings(&store);
        let f = super::reinforcement_ui::Form::new(&frame, "Panel Settings");
        let name = f.text("Name", &mep_electrical::panel_name(e), "Shown in its circuits' numbers");
        let systems: Vec<String> = settings.distribution_systems.iter().map(|d| d.name.clone()).collect();
        let at = settings.distribution_systems.iter().position(|d| d.id == pn.distribution_system).unwrap_or(0);
        let system = f.choice("Distribution system", &systems, at);
        let slots = f.text("Slots", &pn.slots.to_string(), "Numbered down two columns; a pole takes every second slot");
        let namings = [
            ("by_project", "By Project"),
            ("numerical", "Numerical (1, 3, 5)"),
            ("prefixed", "Prefixed (C1)"),
            ("panel_name_number", "Panel Name + Number (LP1-1)"),
        ];
        let current = serde_json::to_value(pn.naming).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
        let naming = f.choice(
            "Circuit naming",
            &namings.iter().map(|n| n.1.to_owned()).collect::<Vec<_>>(),
            namings.iter().position(|n| n.0 == current).unwrap_or(0),
        );
        let lugs = f.check("Feed-through lugs", "One feeder may leave from lugs, taking no slot", pn.feed_through_lugs);
        let transformer = e.class == "IfcTransformer";
        let mut secondaries = vec!["(none)".to_owned()];
        secondaries.extend(systems.iter().cloned());
        let secondary = transformer.then(|| {
            let at = pn.secondary.as_ref().and_then(|id| settings.distribution_systems.iter().position(|d| &d.id == id)).map_or(0, |k| k + 1);
            f.choice("Secondary system", &secondaries, at)
        });
        if !f.show("Circuits are renumbered when the panel changes", "OK") {
            f.dialog.destroy();
            return;
        }
        let args = json!({
            "panel_id": pid,
            "name": name.get_value(),
            "distribution_system": settings.distribution_systems[system.get_selection().map_or(0, |i| i as usize).min(systems.len() - 1)].id,
            "slots": slots.get_value().trim().parse::<u32>().unwrap_or(pn.slots),
            "naming": namings[naming.get_selection().map_or(0, |i| i as usize).min(3)].0,
            "feed_through_lugs": lugs.get_value(),
        });
        let mut args = args;
        if let Some(k) = secondary.and_then(|c| c.get_selection()).filter(|k| *k > 0) {
            args["secondary"] = json!(settings.distribution_systems[k as usize - 1].id);
        }
        f.dialog.destroy();
        match self.run("panel.set", args) {
            Ok(_) => {
                self.refresh_active();
                self.refresh_schedules();
                frame.set_status_text("Panel settings saved", 0);
            }
            Err(e) => error(&frame, &format!("The panel settings were not saved:\n{e}")),
        }
    }
}

impl Gui {
    /// The one selected panel (or transformer), or a status message.
    fn selected_panel(&mut self, title: &str) -> Option<ElementId> {
        let store = self.store_now()?;
        let panels: Vec<ElementId> = self
            .selection()
            .into_iter()
            .filter(|i| store.get(*i).is_some_and(|e| mep_electrical::is_panel(&store, e)))
            .collect();
        match panels[..] {
            [p] => Some(p),
            _ => {
                self.frame.set_status_text(&format!("{title}: select one panel or transformer first"), 0);
                None
            }
        }
    }

    /// Systems › Select Supply: the panel or transformer the selected one is fed from, on a
    /// breaker or the source's feed-through lugs. Only sources that would accept it are offered.
    pub(crate) fn select_supply(&mut self) {
        let frame = self.frame;
        let Some(pid) = self.selected_panel("Select Supply") else { return };
        let Some(store) = self.store_now() else { return };
        let me = store.get(pid).map(mep_electrical::panel_name).unwrap_or_default();
        let settings = mep_electrical::settings(&store);
        let mut options: Vec<(ElementId, &str, String)> = Vec::new();
        let mut refused = Vec::new();
        for e in store.iter().filter(|e| e.id() != pid && mep_electrical::is_panel(&store, e)) {
            let pn = mep_electrical::panel(e);
            let gives = if e.class == "IfcTransformer" { pn.secondary.clone() } else { Some(pn.distribution_system.clone()) };
            let system = gives
                .and_then(|g| settings.distribution_systems.iter().find(|d| d.id == g).map(|d| d.name.clone()))
                .unwrap_or_else(|| "no secondary system".into());
            for connection in ["breaker", "lugs"] {
                if connection == "lugs" && !pn.feed_through_lugs {
                    continue;
                }
                let args = json!({ "panel_id": pid, "source_id": e.id(), "connection": connection });
                match self.app.preview("panel.set_supply", args, &super::user()) {
                    Ok(_) => {
                        let how = if connection == "lugs" { "feed-through lugs" } else { "breaker" };
                        options.push((e.id(), connection, format!("{} — {system}, {how}", mep_electrical::panel_name(e))));
                    }
                    Err(err) if connection == "breaker" => refused.push(format!("{}: {}", mep_electrical::panel_name(e), err.message)),
                    Err(_) => {}
                }
            }
        }
        if options.is_empty() {
            let why = if refused.is_empty() { "there is no other panel".to_owned() } else { refused.join("\n") };
            return error(&frame, &format!("{me} cannot be fed from any panel:\n{why}"));
        }
        let labels: Vec<&str> = options.iter().map(|o| o.2.as_str()).collect();
        let current = mep_electrical::supply_of(&store, pid).and_then(|f| f.panel);
        let d = SingleChoiceDialog::builder(&frame, &format!("Feed {me} from:"), "Select Supply", &labels).build();
        if let Some(k) = current.and_then(|c| options.iter().position(|o| o.0 == c)) {
            d.set_selection(k as i32);
        }
        if d.show_modal() != ID_OK {
            return;
        }
        let Some((src, connection, label)) = usize::try_from(d.get_selection()).ok().and_then(|k| options.get(k)).cloned() else { return };
        match self.run("panel.set_supply", json!({ "panel_id": pid, "source_id": src, "connection": connection })) {
            Ok(_) => {
                self.refresh_active();
                self.refresh_schedules();
                frame.set_status_text(&format!("{me} is fed from {label}"), 0);
            }
            Err(e) => error(&frame, &format!("Select Supply:\n{e}")),
        }
    }

    /// Systems › Panel Hierarchy: the distribution tree with each panel's connected load.
    pub(crate) fn panel_hierarchy(&mut self) {
        let frame = self.frame;
        let tree = match self.run("panel.hierarchy", json!({})) {
            Ok(v) => v,
            Err(e) => return error(&frame, &e),
        };
        fn walk(n: &serde_json::Value, depth: usize, out: &mut String) {
            let indent = if depth == 0 { String::new() } else { format!("{}└ ", "    ".repeat(depth - 1)) };
            out.push_str(&format!("{indent}{}   {:.0} VA\n", n["name"].as_str().unwrap_or(""), n["load"].as_f64().unwrap_or(0.0)));
            for c in n["children"].as_array().into_iter().flatten() {
                walk(c, depth + 1, out);
            }
        }
        let mut text = String::new();
        for root in tree.as_array().into_iter().flatten() {
            walk(root, 0, &mut text);
        }
        if text.is_empty() {
            return frame.set_status_text("Panel Hierarchy: there are no panels", 0);
        }
        let f = super::reinforcement_ui::Form::new(&frame, "Panel Hierarchy");
        let t = f.lines("Distribution", &text, "Panels from the supplies down, with their connected load (sub-panels included)");
        t.set_min_size(Size::new(420, 260));
        f.show("Select Supply chooses what a panel is fed from.", "Close");
        f.dialog.destroy();
    }

    /// Analyze › Panel Schedules: a schedule for each selected panel (or a chosen one), opened.
    pub(crate) fn create_panel_schedules(&mut self) {
        let frame = self.frame;
        let Some(doc) = self.doc else { return };
        let Some(store) = self.store_now() else { return };
        let mut panels: Vec<ElementId> = self
            .selection()
            .into_iter()
            .filter(|i| store.get(*i).is_some_and(|e| mep_electrical::is_panel(&store, e)))
            .collect();
        if panels.is_empty() {
            let all: Vec<(ElementId, String)> =
                store.iter().filter(|e| mep_electrical::is_panel(&store, e)).map(|e| (e.id(), mep_electrical::panel_name(e))).collect();
            if all.is_empty() {
                return frame.set_status_text("Panel Schedules: there are no panels (a distribution board is one)", 0);
            }
            let labels: Vec<&str> = all.iter().map(|p| p.1.as_str()).collect();
            let d = SingleChoiceDialog::builder(&frame, "Panel:", "Panel Schedule", &labels).build();
            if d.show_modal() != ID_OK {
                return;
            }
            let Some(p) = usize::try_from(d.get_selection()).ok().and_then(|k| all.get(k)) else { return };
            panels.push(p.0);
        }
        match self.run("panel_schedule.create", json!({ "panel_ids": panels })) {
            Ok(v) => {
                self.browser_stale = true;
                if let Some(first) = v[0].as_u64() {
                    self.open_view(doc, Some(ElementId(first)));
                }
            }
            Err(e) => error(&frame, &format!("No panel schedule was made:\n{e}")),
        }
    }
}

impl Gui {
    /// Panel Schedule › Change Panel: the open panel schedule shows another panel.
    pub(crate) fn change_schedule_panel(&mut self) {
        let frame = self.frame;
        let Some(sid) = self.active_view().filter(|v| v.schedule.is_some()).and_then(|v| v.element) else { return };
        let Some(store) = self.store_now() else { return };
        let all: Vec<(ElementId, String)> = store.iter().filter(|e| mep_electrical::is_panel(&store, e)).map(|e| (e.id(), mep_electrical::panel_name(e))).collect();
        let labels: Vec<&str> = all.iter().map(|p| p.1.as_str()).collect();
        let d = SingleChoiceDialog::builder(&frame, "Show the panel:", "Change Panel", &labels).build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some(p) = usize::try_from(d.get_selection()).ok().and_then(|k| all.get(k)) else { return };
        match self.run("panel_schedule.change_panel", json!({ "view_id": sid, "panel_id": p.0 })) {
            Ok(_) => self.refresh_schedules(),
            Err(e) => error(&frame, &format!("Change Panel:\n{e}")),
        }
    }
}

fn error(frame: &Frame, message: &str) {
    let d = MessageDialog::builder(frame, message, "BIM Reviera")
        .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconWarning)
        .build();
    d.show_modal();
}
