// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Roof by Footprint in the window (Ch 3 §3.6.4, ARC-ROOF-001): the footprint sketch session,
//! Edit Footprint, and the roof's Properties rows.

use reviera_core::ElementId;
use reviera_core::roofs;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::sketch::{Curve, DUPLICATE, SketchKind};
use super::{Gui, ask, error};

impl Gui {
    /// Architecture › Roof: a footprint sketch in a plan. In the lowest level's plan the roof
    /// can go to the level above, where walls end.
    pub(crate) fn start_roof_sketch(&mut self) {
        let frame = self.frame;
        if !self.ensure_plan(false) {
            return;
        }
        let Some(level) = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map(|p| p.level)
        else {
            return frame
                .set_status_text("Roofs are sketched in plan views (View › Floor Plan)", 0);
        };
        let levels = self.levels();
        let mut target = level;
        if levels.first().map(|l| l.id) == Some(level)
            && let Some(above) = levels.get(1)
            && ask(
                &frame,
                &format!(
                    "The roof would be at the lowest level. Move it to {} instead?",
                    above.name
                ),
            )
        {
            target = above.id;
        }
        self.begin_sketch(SketchKind::Roof, None, target, Vec::new());
    }

    /// Modify | Roofs › Edit Footprint.
    pub(crate) fn edit_roof_footprint(&mut self) {
        let frame = self.frame;
        if !self.in_plan() {
            return frame.set_status_text("Edit Footprint works in plan views", 0);
        }
        let Some(store) = self
            .doc
            .and_then(|d| self.app.document(d))
            .map(|d| d.doc.store().clone())
        else {
            return;
        };
        let Some((roof, recipe, sketch)) = self.selection().into_iter().find_map(|id| {
            let e = store.get(id)?;
            Some((
                id,
                roofs::roof_recipe(e)?,
                reviera_core::sketch::sketch_of(e)?,
            ))
        }) else {
            return frame.set_status_text("Select a roof to edit its footprint", 0);
        };
        let level = reviera_core::GlobalId::parse(&recipe.level)
            .and_then(|g| store.find_by_guid(&g))
            .unwrap_or(ElementId(0));
        // Curves in loop order, keeping each one's slope data.
        let order: Vec<u32> = sketch.loops.iter().flatten().copied().collect();
        let curves = order
            .iter()
            .filter_map(|k| sketch.curves.iter().find(|c| c.key == *k))
            .map(|c| {
                let edge = recipe.edge(c.key);
                Curve {
                    gable: !edge.defines_slope,
                    overhang: edge.overhang,
                    slope: edge.slope,
                    ..Curve::new(
                        c.line,
                        c.lock.as_ref().and_then(|l| {
                            let id = reviera_core::GlobalId::parse(&l.element)
                                .and_then(|g| store.find_by_guid(&g))?;
                            Some((id, l.sub.clone()))
                        }),
                    )
                }
            })
            .collect();
        self.begin_sketch(SketchKind::Roof, Some(roof), level, curves);
    }

    /// The roof sketch's Overhang option: the overhang of walls picked from now on.
    pub(crate) fn set_roof_overhang(&mut self) {
        let frame = self.frame;
        let Some(current) = self
            .sketch
            .as_ref()
            .filter(|s| s.kind == SketchKind::Roof)
            .map(|s| s.overhang)
        else {
            return frame.set_status_text("Overhang applies while sketching a roof footprint", 0);
        };
        let text = self.format_length(current);
        let dialog = TextEntryDialog::builder(
            &frame,
            "Overhang of walls picked from now on:",
            "Roof Overhang",
        )
        .with_default_value(&text)
        .build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let value = dialog.get_value().unwrap_or_default();
        match self.run(
            "units.parse",
            json!({ "text": value, "data_type": "length" }),
        ) {
            Ok(v) if v.as_f64().is_some_and(|o| o >= 0.0) => {
                if let Some(s) = &mut self.sketch {
                    s.overhang = v.as_f64().unwrap_or(0.0);
                }
                let shown = self.format_length(v.as_f64().unwrap_or(0.0));
                frame.set_status_text(&format!("Overhang for picked walls: {shown}"), 0);
            }
            _ => error(&frame, "The overhang must be a length of 0 or more"),
        }
    }

    /// Properties of a selected roof: base level and offset, slope, cutoff, dimensions, type.
    pub(crate) fn roof_rows(&mut self, grid: &PropertyGrid, id: ElementId) {
        let Some(store) = self
            .doc
            .and_then(|d| self.app.document(d))
            .map(|d| d.doc.store().clone())
        else {
            return;
        };
        let Some(e) = store.get(id) else { return };
        let Some(recipe) = roofs::roof_recipe(e) else {
            return;
        };
        let level_name = |guid: &str| {
            reviera_core::GlobalId::parse(guid)
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|l| store.get(l)?.name.clone())
                .unwrap_or_default()
        };
        let angle = |g: &mut Gui, a: f64| {
            g.run("units.format", json!({ "value": a, "data_type": "angle" }))
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_else(|| format!("{:.2}°", a.to_degrees()))
        };
        grid.append(Property::category("Constraints", "c_roof"));
        grid.append(Property::string(
            "Base Level",
            "rf_level",
            level_name(&recipe.level),
        ));
        grid.set_property_read_only("rf_level", true);
        let offset = self.format_length(recipe.base_offset);
        grid.append(Property::string(
            "Base Offset From Level",
            "rf_offset",
            offset,
        ));
        let cutoff = recipe
            .cutoff
            .as_ref()
            .map_or("None".to_owned(), |c| level_name(&c.level));
        grid.append(Property::string("Cutoff Level", "rf_cutoff", cutoff));
        grid.set_property_read_only("rf_cutoff", true);
        if let Some(c) = &recipe.cutoff {
            let o = self.format_length(c.offset);
            grid.append(Property::string("Cutoff Offset", "rf_cutoff_offset", o));
        }
        grid.append(Property::category("Dimensions", "c_roof_dims"));
        let slope = angle(self, recipe.slope);
        grid.append(Property::string("Slope", "rf_slope", slope));
        grid.set_help_string(
            "rf_slope",
            "The slope of every slope-defining footprint curve",
        );
        match roofs::regenerate(&store, e) {
            Ok(g) => {
                let fmt = |g: &mut Gui, v: f64, t: &str| {
                    g.run("units.format", json!({ "value": v, "data_type": t }))
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .unwrap_or_else(|| format!("{v:.3}"))
                };
                let rows = [
                    ("Thickness", self.format_length(g.thickness)),
                    ("Maximum Ridge Height", self.format_length(g.max_height())),
                    ("Area", fmt(self, g.area(), "area")),
                    ("Volume", fmt(self, g.volume(), "volume")),
                    ("Roof Shape", g.predefined.replace('_', " ").to_lowercase()),
                ];
                for (k, (label, value)) in rows.into_iter().enumerate() {
                    let name = format!("rf_q{k}");
                    grid.append(Property::string(label, &name, value));
                    grid.set_property_read_only(&name, true);
                }
            }
            Err(err) => {
                grid.append(Property::string("Problem", "rf_problem", err.message));
                grid.set_property_read_only("rf_problem", true);
            }
        }
        // The type selector and the type's thickness.
        let mut types: Vec<(ElementId, String)> = store
            .iter()
            .filter(|t| roofs::is_roof_type(t))
            .map(|t| (t.id(), t.name.clone().unwrap_or_default()))
            .collect();
        types.sort_by(|a, b| a.1.cmp(&b.1));
        let current = e
            .param("@type")
            .and_then(Value::as_str)
            .and_then(reviera_core::GlobalId::parse)
            .and_then(|g| store.find_by_guid(&g));
        let mut choices: Vec<(String, i32)> = types
            .iter()
            .enumerate()
            .map(|(i, t)| (t.1.clone(), i as i32))
            .collect();
        choices.push((DUPLICATE.to_owned(), choices.len() as i32));
        let index = current
            .and_then(|c| types.iter().position(|t| t.0 == c))
            .map_or(-1, |i| i as i32);
        grid.append(Property::category("Type", "c_roof_type"));
        grid.append(Property::enumeration("Type", "rf_type", choices, index));
        let thickness = current
            .and_then(|t| store.get(t))
            .and_then(roofs::type_thickness)
            .map(|t| self.format_length(t))
            .unwrap_or_default();
        grid.append(Property::string(
            "Thickness (type)",
            "rf_thickness",
            thickness,
        ));
        self.type_rows = current;
    }

    /// An edit of a roof row; `false` if `property` is not one.
    pub(crate) fn roof_edit(&mut self, property: &str, text: Option<String>) -> bool {
        let frame = self.frame;
        let text = text.unwrap_or_default();
        let selected: Vec<ElementId> = {
            let store = self
                .doc
                .and_then(|d| self.app.document(d))
                .map(|d| d.doc.store().clone());
            self.selection()
                .into_iter()
                .filter(|i| {
                    store
                        .as_ref()
                        .and_then(|s| s.get(*i))
                        .and_then(roofs::roof_recipe)
                        .is_some()
                })
                .collect()
        };
        let result = match property {
            "rf_offset" => self.run(
                "roof.set_params",
                json!({ "roof_ids": selected, "base_offset": text }),
            ),
            "rf_slope" => self.run(
                "roof.set_params",
                json!({ "roof_ids": selected, "slope": text }),
            ),
            "rf_cutoff_offset" => self.run(
                "roof.set_params",
                json!({ "roof_ids": selected, "cutoff_offset": text }),
            ),
            "rf_thickness" => match self.type_rows {
                Some(t) => self.run(
                    "type.set_param",
                    json!({ "type_id": t, "param": "thickness", "value": text }),
                ),
                None => return true,
            },
            "rf_type" if text == DUPLICATE => {
                let Some(current) = self.type_rows else {
                    return true;
                };
                let proposed = self
                    .doc
                    .and_then(|d| self.app.document(d))
                    .map(|d| reviera_core::types::duplicate_name(d.doc.store(), current))
                    .unwrap_or_default();
                let dialog = TextEntryDialog::builder(
                    &frame,
                    "Name of the new roof type:",
                    "Duplicate Type",
                )
                .with_default_value(&proposed)
                .build();
                if dialog.show_modal() != ID_OK {
                    return true;
                }
                let name = dialog.get_value().unwrap_or(proposed);
                match self.run(
                    "type.duplicate",
                    json!({ "type_id": current, "new_name": name }),
                ) {
                    Ok(t) => self.run(
                        "element.set_type",
                        json!({ "element_ids": selected, "type_id": t }),
                    ),
                    Err(e) => Err(e),
                }
            }
            "rf_type" => {
                let chosen = self.doc.and_then(|d| self.app.document(d)).and_then(|d| {
                    d.doc
                        .store()
                        .iter()
                        .find(|t| {
                            roofs::is_roof_type(t) && t.name.as_deref() == Some(text.as_str())
                        })
                        .map(|t| t.id())
                });
                match chosen {
                    Some(t) => self.run(
                        "element.set_type",
                        json!({ "element_ids": selected, "type_id": t }),
                    ),
                    None => return true,
                }
            }
            _ => return false,
        };
        if let Err(e) = result {
            error(&frame, &format!("The roof was not changed:\n{e}"));
        }
        self.properties_stale = true;
        self.refresh_scene(&frame);
        true
    }

    /// After a roof is made from picked walls, offers to attach those walls to it.
    pub(crate) fn offer_attach(&mut self, roof: ElementId, walls: Vec<ElementId>) {
        let frame = self.frame;
        if walls.is_empty()
            || !ask(
                &frame,
                &format!("Attach the {} walls under the roof to it?", walls.len()),
            )
        {
            return;
        }
        match self.run(
            "wall.attach",
            json!({ "wall_ids": walls, "target_id": roof }),
        ) {
            Ok(_) => frame.set_status_text("Walls attached to the roof", 0),
            Err(e) => error(&frame, &format!("The walls were not attached:\n{e}")),
        }
        self.refresh_scene(&frame);
    }

    /// Modify | Walls › Attach Top/Base: the next click picks the roof.
    pub(crate) fn start_attach(&mut self) {
        let walls = self.selected_walls();
        if walls.is_empty() {
            return self
                .frame
                .set_status_text("Select the walls to attach first", 0);
        }
        self.attach_pick = Some(walls);
        self.frame.set_status_text(
            "Attach Top: click the roof the walls attach to (Esc cancels)",
            0,
        );
    }

    pub(crate) fn attach_click(&mut self, x: i32, y: i32, panel: &Panel) {
        let frame = self.frame;
        let Some(walls) = self.attach_pick.take() else {
            return;
        };
        let Some(target) = self.element_at(x, y, panel) else {
            return frame.set_status_text("Nothing to attach to there", 0);
        };
        match self.run(
            "wall.attach",
            json!({ "wall_ids": walls, "target_id": target }),
        ) {
            Ok(_) => frame.set_status_text("Walls attached", 0),
            Err(e) => error(&frame, &format!("The walls were not attached:\n{e}")),
        }
        self.refresh_scene(&frame);
    }

    /// Modify | Walls › Detach Top/Base: the selected walls return to their own tops.
    pub(crate) fn detach_selection(&mut self) {
        let frame = self.frame;
        let walls = self.selected_walls();
        if walls.is_empty() {
            return frame.set_status_text("Select the walls to detach first", 0);
        }
        match self.run("wall.detach", json!({ "wall_ids": walls })) {
            Ok(_) => frame.set_status_text("Walls detached", 0),
            Err(e) => error(&frame, &format!("The walls were not detached:\n{e}")),
        }
        self.refresh_scene(&frame);
    }

    fn selected_walls(&mut self) -> Vec<ElementId> {
        let ids = self.selection();
        let Some(store) = self
            .doc
            .and_then(|d| self.app.document(d))
            .map(|d| d.doc.store().clone())
        else {
            return Vec::new();
        };
        ids.into_iter()
            .filter(|i| {
                store
                    .get(*i)
                    .and_then(reviera_core::walls::wall_recipe)
                    .is_some()
            })
            .collect()
    }
}
