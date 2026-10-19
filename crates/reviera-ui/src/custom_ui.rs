// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Steel › Connections: custom connections (STR-STL-003). *Customize* makes a type of the
//! selected connections and plates, patterns and welds; *Add to Custom* and *Remove from Custom*
//! change a type (and every instance, after saying how many); *Custom Parameter* exposes a
//! sub-element's parameter and sets it for every instance. Custom types are offered by the
//! Connection tool for joins with as many members.

use reviera_core::ElementId;
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

impl Gui {
    /// The custom type and instance an element belongs to (an instance or one of its parts).
    fn custom_of(&mut self, id: ElementId) -> Option<(String, ElementId)> {
        let store = self.store_now()?;
        let e = store.get(id)?;
        let inst = match reviera_core::custom_connections::part_tag(e) {
            Some(t) => reviera_core::id::GlobalId::parse(&t.instance).and_then(|g| store.find_by_guid(&g))?,
            None => id,
        };
        let r = store.get(inst).and_then(reviera_core::connections::recipe)?;
        r.connection_type.starts_with(reviera_core::custom_connections::PREFIX).then_some((r.connection_type.clone(), inst))
    }

    fn instance_count(&mut self, type_id: &str) -> usize {
        self.run("custom_connection.list", json!({}))
            .ok()
            .and_then(|l| {
                l.as_array()?
                    .iter()
                    .find(|t| t["id"] == type_id)
                    .and_then(|t| t["instances"].as_array().map(Vec::len))
            })
            .unwrap_or(0)
    }

    fn confirm_all(&mut self, type_id: &str) -> bool {
        let n = self.instance_count(type_id);
        if n <= 1 {
            return true;
        }
        let d = MessageDialog::builder(
            &self.frame,
            &format!("This changes all {n} instances of the custom connection. Go ahead?"),
            "Custom Connection",
        )
        .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
        .build();
        d.show_modal() == ID_YES
    }

    /// Steel › Connections › Customize.
    pub(crate) fn customize_connection(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let (mut conns, mut elements) = (Vec::new(), Vec::new());
        for id in self.selection() {
            let Some(e) = store.get(id) else { continue };
            if reviera_core::connections::recipe(e).is_some() {
                conns.push(id);
            } else if reviera_core::steel_detail::is_detail(e) {
                elements.push(id);
            }
        }
        if conns.is_empty() && elements.is_empty() {
            return frame.set_status_text("Customize: select the join's connections and its plates, bolts and welds first", 0);
        }
        let d = TextEntryDialog::builder(&frame, "The new connection type's name:", "Customize Connection").build();
        if d.show_modal() != ID_OK {
            return;
        }
        let name = d.get_value().unwrap_or_default();
        match self.run(
            "structural_connection.customize",
            json!({ "connection_ids": conns, "element_ids": elements, "name": name }),
        ) {
            Ok(r) => {
                self.properties_stale = true;
                frame.set_status_text(
                    &format!(
                        "Custom connection {} made ({}); place it with Connection on matching joins",
                        name,
                        r["type_id"].as_str().unwrap_or("")
                    ),
                    0,
                );
            }
            Err(e) => error(&frame, &format!("The connection was not customized:\n{e}")),
        }
    }

    /// Steel › Connections › Add to Custom: the selected loose elements go into the type of the
    /// selected instance (or one of its parts).
    pub(crate) fn add_to_custom(&mut self) {
        let frame = self.frame;
        let sel = self.selection();
        let Some((ty, inst)) = sel.iter().find_map(|id| self.custom_of(*id)) else {
            return frame.set_status_text("Add to Custom: select a custom connection (or a part of it) and the elements to add", 0);
        };
        let Some(store) = self.store_now() else { return };
        let add: Vec<ElementId> = sel
            .iter()
            .copied()
            .filter(|id| *id != inst)
            .filter(|id| {
                store.get(*id).is_some_and(|e| {
                    reviera_core::steel_detail::is_detail(e) && reviera_core::custom_connections::part_tag(e).is_none()
                })
            })
            .collect();
        if add.is_empty() {
            return frame.set_status_text("Add to Custom: select loose plates, bolts or welds to add", 0);
        }
        if !self.confirm_all(&ty) {
            return;
        }
        match self.run("custom_connection.add_elements", json!({ "type_id": ty, "element_ids": add, "confirm": true })) {
            Ok(n) => {
                self.refresh_scene(&frame);
                frame.set_status_text(&format!("Added; {n} instance(s) remade"), 0);
            }
            Err(e) => error(&frame, &format!("The elements were not added:\n{e}")),
        }
    }

    /// Steel › Connections › Remove from Custom: the selected parts leave the type.
    pub(crate) fn remove_from_custom(&mut self) {
        let frame = self.frame;
        let sel = self.selection();
        let Some((ty, _)) = sel.iter().find_map(|id| self.custom_of(*id)) else {
            return frame.set_status_text("Remove from Custom: select the custom connection's parts to remove", 0);
        };
        let Some(store) = self.store_now() else { return };
        let parts: Vec<ElementId> = sel
            .iter()
            .copied()
            .filter(|id| store.get(*id).and_then(reviera_core::custom_connections::part_tag).is_some())
            .collect();
        if parts.is_empty() {
            return frame.set_status_text("Remove from Custom: select the parts (plates, bolts, welds, nested connections)", 0);
        }
        if !self.confirm_all(&ty) {
            return;
        }
        match self.run("custom_connection.remove_elements", json!({ "type_id": ty, "element_ids": parts, "confirm": true })) {
            Ok(n) => {
                self.refresh_scene(&frame);
                frame.set_status_text(&format!("Removed; {n} instance(s) remade"), 0);
            }
            Err(e) => error(&frame, &format!("The parts were not removed:\n{e}")),
        }
    }

    /// Steel › Connections › Custom Parameter: a sub-element's parameter, exposed and set for
    /// every instance.
    pub(crate) fn custom_parameter(&mut self) {
        let frame = self.frame;
        let Some((ty, _)) = self.selection().first().copied().and_then(|id| self.custom_of(id)) else {
            return frame.set_status_text("Custom Parameter: select a custom connection first", 0);
        };
        let Ok(list) = self.run("custom_connection.list", json!({})) else { return };
        let Some(t) = list.as_array().and_then(|l| l.iter().find(|t| t["id"] == ty.as_str())).cloned() else { return };
        // The paths a type offers: a plate's thickness, a pattern's diameter, a weld's throat,
        // a nested connection's parameters.
        let mut paths: Vec<String> = Vec::new();
        let names = |k: &str| -> Vec<String> {
            t[k].as_array().into_iter().flatten().filter_map(|s| s["name"].as_str().map(str::to_owned)).collect()
        };
        paths.extend(names("plates").into_iter().map(|n| format!("{n}.thickness")));
        paths.extend(names("patterns").into_iter().map(|n| format!("{n}.diameter")));
        paths.extend(names("welds").into_iter().map(|n| format!("{n}.a")));
        let lib = self.run("connection_type.list", json!({})).unwrap_or_default();
        for n in t["nested"].as_array().into_iter().flatten() {
            let ct = n["connection_type"].as_str().unwrap_or("");
            if let Some(def) = lib.as_array().and_then(|l| l.iter().find(|d| d["id"] == ct)) {
                for p in def["params"].as_array().into_iter().flatten() {
                    paths.push(format!("{}.{}", n["name"].as_str().unwrap_or(""), p["key"].as_str().unwrap_or("")));
                }
            }
        }
        let exposed: Vec<(String, f64)> = t["exposed"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|e| (e["path"].as_str().unwrap_or("").to_owned(), e["value"].as_f64().unwrap_or(0.0)))
            .collect();
        let labels: Vec<String> = paths
            .iter()
            .map(|p| match exposed.iter().find(|e| e.0 == *p) {
                Some((_, v)) => format!("{p} = {v} (a type parameter)"),
                None => p.clone(),
            })
            .collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let d = SingleChoiceDialog::builder(&frame, "Parameter:", "Custom Parameter", &refs).build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some(path) = usize::try_from(d.get_selection()).ok().and_then(|i| paths.get(i)).cloned() else { return };
        let current = match self.run("custom_connection.expose_parameter", json!({ "type_id": ty, "path": path })) {
            Ok(v) => v.as_f64().unwrap_or(0.0),
            Err(e) => return error(&frame, &e),
        };
        let counts = path.ends_with(".rows") || path.ends_with(".bolts");
        let shown = if counts { format!("{current}") } else { format!("{}", current * 1000.0) };
        let d = TextEntryDialog::builder(
            &frame,
            &format!("{path} for every instance{}:", if counts { "" } else { " (mm)" }),
            "Custom Parameter",
        )
        .with_default_value(&shown)
        .build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some(v) = d.get_value().and_then(|t| t.trim().parse::<f64>().ok()) else { return };
        let value = if counts { v } else { v / 1000.0 };
        if !self.confirm_all(&ty) {
            return;
        }
        match self.run("custom_connection.set_parameter", json!({ "type_id": ty, "path": path, "value": value })) {
            Ok(n) => {
                self.refresh_scene(&frame);
                frame.set_status_text(&format!("{path} set; {n} instance(s) remade"), 0);
            }
            Err(e) => error(&frame, &format!("The parameter was not set:\n{e}")),
        }
    }
}

