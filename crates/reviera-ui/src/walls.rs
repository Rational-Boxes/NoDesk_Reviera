// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Wall rows of the Properties palette (ARC-WALL-001): the base and top constraints
//! (`wall.set_constraints`), and the type: chosen, duplicated, renamed and its width set
//! (`element.set_type`, `type.duplicate`, `type.rename`, `type.set_param`). With the Wall tool
//! running and nothing selected, the type row chooses the type new walls get.

use reviera_core::ElementId;
use reviera_core::walls;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

const DUPLICATE: &str = "Duplicate Type…";
const UNCONNECTED: &str = "Unconnected";

impl Gui {
    fn wall_store(&self) -> Option<reviera_core::Store> {
        Some(self.app.document(self.doc?)?.doc.store().clone())
    }

    /// Reviera wall types with a width, by name.
    fn wall_types(store: &reviera_core::Store) -> Vec<(ElementId, String, f64)> {
        let mut types: Vec<_> = store
            .iter()
            .filter(|e| e.class == "IfcWallType")
            .filter_map(|e| {
                Some((
                    e.id(),
                    e.name.clone().unwrap_or_default(),
                    walls::type_width(e)?,
                ))
            })
            .collect();
        types.sort_by(|a, b| a.1.cmp(&b.1));
        types
    }

    /// The type the Wall tool gives new walls: the one chosen, else the default (when it exists).
    pub(crate) fn tool_wall_type(&self) -> Option<ElementId> {
        let store = self.wall_store()?;
        self.wall_type
            .filter(|t| store.get(*t).is_some())
            .or_else(|| {
                Self::wall_types(&store)
                    .into_iter()
                    .find(|t| t.1 == walls::DEFAULT_TYPE_NAME)
                    .map(|t| t.0)
            })
    }

    /// The type selector and type values; `current` is the selected wall's type, or the tool's.
    fn wall_type_rows(&mut self, grid: &PropertyGrid, current: Option<ElementId>) {
        let Some(store) = self.wall_store() else {
            return;
        };
        let types = Self::wall_types(&store);
        let mut names: Vec<(String, i32)> = types
            .iter()
            .enumerate()
            .map(|(i, t)| (t.1.clone(), i as i32))
            .collect();
        let index = current
            .and_then(|c| types.iter().position(|t| t.0 == c))
            .map_or(-1, |i| i as i32);
        if current.is_some() {
            names.push((DUPLICATE.to_owned(), names.len() as i32));
        }
        grid.append(Property::enumeration("Type", "wt_type", names, index));
        grid.set_help_string(
            "wt_type",
            "The wall type; Duplicate Type… makes a new one from it",
        );
        let Some((id, name, width)) = current.and_then(|c| types.into_iter().find(|t| t.0 == c))
        else {
            return;
        };
        let used = reviera_core::types::instances(&store, store.get(id).expect("listed")).len();
        grid.append(Property::category(
            format!("Type Parameters ({used} walls)"),
            "c_walltype",
        ));
        grid.append(Property::string("Type Name", "wt_name", name));
        let w = self.format_length(width);
        grid.append(Property::string("Width", "wt_width", w));
        grid.set_help_string("wt_width", "Changes every wall of this type");
        self.wall_type_row = Some(id);
    }

    /// The palette while the Wall tool runs with nothing selected.
    pub(crate) fn wall_tool_properties(&mut self, grid: &PropertyGrid) -> bool {
        if self.tool.is_none() {
            return false;
        }
        grid.append(Property::category("Wall tool", "c_walltool"));
        let current = self.tool_wall_type();
        self.wall_type_rows(grid, current);
        true
    }

    /// Constraint and type rows of a selected Reviera wall; `false` for anything else.
    pub(crate) fn wall_rows(&mut self, grid: &PropertyGrid, id: ElementId) -> bool {
        let Some(store) = self.wall_store() else {
            return false;
        };
        let Some(e) = store.get(id) else { return false };
        let Some(r) = walls::wall_recipe(e) else {
            return false;
        };
        let levels = reviera_core::levels::list(&store);
        let level_names: Vec<(String, i32)> = levels
            .iter()
            .enumerate()
            .map(|(i, l)| (l.name.clone(), i as i32 + 1))
            .collect();
        grid.append(Property::category("Constraints", "c_wall"));
        let base = levels
            .iter()
            .position(|l| l.guid == r.base_level)
            .map_or(-1, |i| i as i32 + 1);
        grid.append(Property::enumeration(
            "Base Constraint",
            "wl_base",
            level_names.clone(),
            base,
        ));
        let off = self.format_length(r.base_offset);
        grid.append(Property::string("Base Offset", "wl_base_off", off));
        let top_level = r.top.get("level").and_then(Value::as_str);
        let mut tops = vec![(UNCONNECTED.to_owned(), 0)];
        tops.extend(level_names);
        let top = match top_level {
            Some(g) => levels
                .iter()
                .position(|l| l.guid == g)
                .map_or(-1, |i| i as i32 + 1),
            None => 0,
        };
        grid.append(Property::enumeration("Top Constraint", "wl_top", tops, top));
        let height = walls::regenerate(&store, e).map_or(0.0, |g| g.height);
        let h = self.format_length(height);
        grid.append(Property::string("Unconnected Height", "wl_height", h));
        let t = self.format_length(r.top_offset);
        grid.append(Property::string("Top Offset", "wl_top_off", t));
        if top_level.is_some() {
            grid.set_property_read_only("wl_height", true);
            grid.set_help_string(
                "wl_height",
                "Follows the top level; choose Unconnected to set it",
            );
        } else {
            grid.set_property_read_only("wl_top_off", true);
            grid.set_help_string("wl_top_off", "Applies when the top is a level");
        }
        let current = e
            .param("@type")
            .and_then(Value::as_str)
            .and_then(reviera_core::GlobalId::parse)
            .and_then(|g| store.find_by_guid(&g));
        grid.append(Property::category("Type", "c_wall_type"));
        self.wall_type_rows(grid, current);
        false
    }

    /// An edit of a wall row; `false` if `property` is not one.
    pub(crate) fn wall_edit(&mut self, property: &str, text: Option<String>) -> bool {
        if !property.starts_with("wl_") && !property.starts_with("wt_") {
            return false;
        }
        let frame = self.frame;
        let text = text.unwrap_or_default();
        let Some(store) = self.wall_store() else {
            return true;
        };
        let selected: Vec<ElementId> = self
            .selection()
            .into_iter()
            .filter(|i| store.get(*i).and_then(walls::wall_recipe).is_some())
            .collect();
        let level = |name: &str| {
            reviera_core::levels::list(&store)
                .into_iter()
                .find(|l| l.name == name)
                .map(|l| l.id)
        };
        let constraints = |g: &mut Gui, args: Value| -> Result<(), String> {
            for w in &selected {
                let mut a = args.clone();
                a["wall_id"] = json!(w);
                g.run("wall.set_constraints", a)?;
            }
            Ok(())
        };
        let result: Option<Result<(), String>> = match property {
            "wl_base" => level(&text).map(|l| constraints(self, json!({ "base_level": l }))),
            "wl_base_off" => Some(constraints(self, json!({ "base_offset": text }))),
            "wl_top" if text == UNCONNECTED => {
                // Keep the current height.
                let height = selected
                    .first()
                    .and_then(|w| walls::regenerate(&store, store.get(*w)?).ok())
                    .map_or(3.0, |g| g.height);
                Some(constraints(
                    self,
                    json!({ "top_constraint": { "unconnected": height } }),
                ))
            }
            "wl_top" => {
                level(&text).map(|l| constraints(self, json!({ "top_constraint": { "level": l } })))
            }
            "wl_height" => Some(constraints(
                self,
                json!({ "top_constraint": { "unconnected": text } }),
            )),
            "wl_top_off" => Some(constraints(self, json!({ "top_offset": text }))),
            "wt_type" => self.choose_wall_type(&text, &selected),
            "wt_name" => self.wall_type_row.map(|t| {
                self.run("type.rename", json!({ "type_id": t, "name": text }))
                    .map(|_| ())
            }),
            "wt_width" => self.wall_type_row.map(|t| {
                self.run(
                    "type.set_param",
                    json!({ "type_id": t, "param": "width", "value": text }),
                )
                .map(|_| ())
            }),
            _ => None,
        };
        if let Some(Err(e)) = result {
            error(&frame, &format!("The change was not made:\n{e}"));
        }
        self.properties_stale = true;
        self.refresh_scene(&frame);
        true
    }

    fn choose_wall_type(
        &mut self,
        choice: &str,
        selected: &[ElementId],
    ) -> Option<Result<(), String>> {
        let frame = self.frame;
        let store = self.wall_store()?;
        let chosen = if choice == DUPLICATE {
            let current = self.wall_type_row?;
            let proposed = reviera_core::types::duplicate_name(&store, current);
            let dialog =
                TextEntryDialog::builder(&frame, "Name of the new type:", "Duplicate Type")
                    .with_default_value(&proposed)
                    .build();
            if dialog.show_modal() != ID_OK {
                return None;
            }
            let name = dialog.get_value()?;
            match self.run(
                "type.duplicate",
                json!({ "type_id": current, "new_name": name }),
            ) {
                Ok(v) => ElementId(v.as_u64().unwrap_or_default()),
                Err(e) => return Some(Err(e)),
            }
        } else {
            Self::wall_types(&store)
                .into_iter()
                .find(|t| t.1 == choice)?
                .0
        };
        if selected.is_empty() {
            self.wall_type = Some(chosen);
            return Some(Ok(()));
        }
        Some(
            self.run(
                "element.set_type",
                json!({ "element_ids": selected, "type_id": chosen }),
            )
            .map(|_| ()),
        )
    }
}
