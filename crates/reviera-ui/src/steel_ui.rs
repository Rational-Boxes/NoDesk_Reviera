// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Steel connections in Properties (STR-STL-002): a connection's (or one of its plates', bolts'
//! or welds') rows show its type and each parameter of the type by tab. Changing the type gives
//! the connection that type; changing a parameter overrides it for this connection; *Type
//! Parameters…* changes the type's values for the whole project.

use reviera_core::{ElementId, connections, steel_fab};
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

impl Gui {
    /// The connection an element is or belongs to.
    fn connection_of(&self, id: ElementId) -> Option<ElementId> {
        let store = self.store_now()?;
        let e = store.get(id)?;
        if connections::recipe(e).is_some() {
            return Some(id);
        }
        let r = steel_fab::fab_recipe(e)?;
        reviera_core::GlobalId::parse(&r.connection).and_then(|g| store.find_by_guid(&g))
    }

    /// The rows of a connection or a fabrication element; `false` if `id` is neither.
    pub(crate) fn connection_rows(&mut self, grid: &PropertyGrid, id: ElementId) -> bool {
        let Some(cid) = self.connection_of(id) else {
            return false;
        };
        let Some(store) = self.store_now() else { return false };
        let Some(c) = store.get(cid).and_then(connections::recipe) else {
            return false;
        };
        grid.append(Property::category("Connection", "c_conn"));
        let lib = steel_fab::library();
        let mut names = vec![("Generic".to_owned(), 0)];
        names.extend(lib.iter().enumerate().map(|(i, t)| (t.name.to_owned(), i as i32 + 1)));
        let at = lib
            .iter()
            .position(|t| t.id == c.connection_type)
            .map_or(0, |i| i as i32 + 1);
        grid.append(Property::enumeration("Connection Type", "cn_type", names, at));
        let values = steel_fab::params(&store, &c);
        if let Some(t) = steel_fab::connection_type(&c.connection_type) {
            let mut tab = "";
            for d in &t.params {
                if d.tab != tab {
                    tab = d.tab;
                    grid.append(Property::category(tab, format!("c_conn_{tab}")));
                }
                let v = values.get(d.key).copied().unwrap_or(0.0);
                let text = if d.kind == "count" {
                    format!("{}", v.round() as i64)
                } else {
                    self.format_length(v)
                };
                // An override is marked: the label column is narrow, so with a dot, and the
                // row's help says it.
                let own = c.params.contains_key(d.key);
                let label = if own { format!("● {}", d.label) } else { d.label.to_owned() };
                let name = format!("cn_p_{}", d.key);
                grid.append(Property::string(&label, name.clone(), text));
                if own {
                    grid.set_help_string(&name, "● Set for this connection only, overriding the type's value");
                }
            }
            grid.append(Property::category("Type", "c_conn_type"));
            grid.append(Property::string(
                "Type Parameters…",
                "cn_type_params",
                "Edit for the whole project",
            ));
            let keys: Vec<&str> = t.params.iter().map(|d| d.key).collect();
            grid.set_help_string(
                "cn_type_params",
                &format!(
                    "Type key=value (for example bolt_diameter=7/8\") to change a parameter for every connection of this type. Keys: {}",
                    keys.join(", ")
                ),
            );
        }
        if let Ok(i) = self.run("structural_connection.info", json!({ "connection_ids": [cid] })) {
            let n = |k: &str| i[0][k].as_array().map_or(0, Vec::len);
            grid.append(Property::string(
                "Fabrication",
                "cn_fab",
                format!("{} plate(s), {} bolt(s), {} weld(s) (shown at Fine detail)", n("plates"), n("bolts"), n("welds")),
            ));
            grid.set_property_read_only("cn_fab", true);
        }
        true
    }

    /// An edit of a connection row; `false` if `property` is not one.
    pub(crate) fn connection_edit(&mut self, property: &str, text: Option<String>) -> bool {
        if !property.starts_with("cn_") {
            return false;
        }
        let frame = self.frame;
        let text = text.unwrap_or_default();
        let Some(cid) = self.selection().first().and_then(|id| self.connection_of(*id)) else {
            return true;
        };
        let result = if property == "cn_type" {
            let ty = steel_fab::library()
                .into_iter()
                .find(|t| t.name == text)
                .map_or("generic", |t| t.id);
            self.run(
                "structural_connection.set_type",
                json!({ "connection_ids": [cid], "connection_type": ty }),
            )
        } else if let Some(key) = property.strip_prefix("cn_p_") {
            let value = self.parse_param(&text, self.is_count(cid, key));
            match value {
                Some(v) => self.run(
                    "structural_connection.override",
                    json!({ "connection_id": cid, "params": { key: v } }),
                ),
                None => Err(format!("`{text}` is not a number or a length")),
            }
        } else if property == "cn_type_params" {
            // `key=value` changes the type's value for the project.
            let Some((k, v)) = text.split_once('=') else {
                return true;
            };
            let ty = self
                .store_now()
                .and_then(|s| s.get(cid).and_then(connections::recipe))
                .map(|c| c.connection_type)
                .unwrap_or_default();
            let count = steel_fab::connection_type(&ty)
                .is_some_and(|t| t.params.iter().any(|d| d.key == k.trim() && d.kind == "count"));
            match self.parse_param(v, count) {
                Some(x) => self.run(
                    "connection_type.edit_parameters",
                    json!({ "type_id": ty, "params": { k.trim(): x } }),
                ),
                None => Err(format!("`{v}` is not a number or a length")),
            }
        } else {
            return true;
        };
        if let Err(e) = result {
            error(&frame, &format!("The change was not made:\n{e}"));
        }
        self.properties_stale = true;
        self.refresh_scene(&frame);
        true
    }

    /// Whether a connection's parameter is a count (else a length).
    fn is_count(&self, cid: ElementId, key: &str) -> bool {
        self.store_now()
            .and_then(|s| s.get(cid).and_then(connections::recipe))
            .and_then(|c| steel_fab::connection_type(&c.connection_type))
            .is_some_and(|t| t.params.iter().any(|d| d.key == key && d.kind == "count"))
    }

    /// A count, or a length in the model's units.
    fn parse_param(&mut self, text: &str, count: bool) -> Option<Value> {
        let t = text.trim();
        if count {
            return t.parse::<i64>().ok().map(|n| json!(n));
        }
        self.run("units.parse", json!({ "text": t, "data_type": "length" }))
            .ok()
            .filter(|v| v.is_number())
    }
}
