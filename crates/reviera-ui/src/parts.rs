// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Parts in the window (ARC-PART-001 … 004): Modify › Parts › Create Parts on the selection,
//! Divide Parts (by selected model lines, swept along their work plane's normal, or by
//! intersecting levels, grids and reference planes chosen from a list, with a gap), Merge,
//! Exclude and Restore, and the view's Parts Visibility; a selected part's properties, with its
//! Material editable.

use reviera_core::{ElementId, model_lines, parts, work_planes};
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::Gui;

impl Gui {
    fn selected_parts(&mut self) -> Vec<ElementId> {
        let Some(store) = self.store_now() else {
            return Vec::new();
        };
        self.selection()
            .into_iter()
            .filter(|id| store.get(*id).and_then(parts::recipe).is_some())
            .collect()
    }

    fn parts_run(&mut self, command: &str, args: Value, done: &str) -> Option<Value> {
        let frame = self.frame;
        match self.app.execute(command, args, &super::user()) {
            Ok(out) => {
                self.refresh_scene(&frame);
                let note = out
                    .warnings
                    .first()
                    .map_or(done.to_owned(), |w| format!("{done} — {}", w.message));
                frame.set_status_text(&note, 0);
                Some(out.result)
            }
            Err(e) => {
                super::error(&frame, &e.message);
                None
            }
        }
    }

    /// Modify › Parts › Create Parts.
    pub(crate) fn create_parts(&mut self) {
        let ids = self.selection();
        if ids.is_empty() {
            return self
                .frame
                .set_status_text("Select walls, floors, roofs, ceilings, columns or beams", 0);
        }
        if let Some(v) = self.parts_run("parts.create", json!({ "host_ids": ids }), "Parts created")
        {
            let n = v.as_array().map_or(0, Vec::len);
            self.frame.set_status_text(
                &format!("{n} part(s) created (Parts Visibility shows them)"),
                0,
            );
        }
    }

    /// Modify › Parts › Divide Parts.
    pub(crate) fn divide_parts(&mut self) {
        let frame = self.frame;
        let chosen = self.selected_parts();
        if chosen.is_empty() {
            return frame.set_status_text("Select parts to divide", 0);
        }
        let Some(store) = self.store_now() else {
            return;
        };
        // Selected model lines cut along their work plane's normal.
        let selection = self.selection();
        let lines: Vec<_> = model_lines::all(&store)
            .into_iter()
            .filter(|l| selection.contains(&l.id))
            .collect();
        let mut args = json!({ "part_ids": chosen });
        if lines.is_empty() {
            // Else intersecting references.
            let mut refs: Vec<(ElementId, String)> = reviera_core::levels::list(&store)
                .into_iter()
                .map(|l| (l.id, format!("Level: {}", l.name)))
                .collect();
            refs.extend(
                reviera_core::grids::list(&store)
                    .into_iter()
                    .map(|g| (g.id, format!("Grid: {}", g.name))),
            );
            refs.extend(reviera_core::ref_planes::list(&store).into_iter().map(|r| {
                (
                    r.id,
                    format!(
                        "Reference Plane: {}",
                        r.name.clone().unwrap_or_else(|| r.id.to_string())
                    ),
                )
            }));
            let labels: Vec<&str> = refs.iter().map(|r| r.1.as_str()).collect();
            let d = MultiChoiceDialog::builder(
                &frame,
                "Divide by the intersecting references (or select model lines on the parts first):",
                "Divide Parts",
                &labels,
            )
            .build();
            if d.show_modal() != ID_OK {
                return;
            }
            let picked: Vec<Value> = d
                .get_selections()
                .into_iter()
                .filter_map(|i| refs.get(i as usize))
                .map(|r| json!({ "ref_id": r.0 }))
                .collect();
            if picked.is_empty() {
                return;
            }
            args["refs"] = json!(picked);
        } else {
            let normal = selection
                .iter()
                .filter_map(|id| store.get(*id).and_then(model_lines::recipe))
                .find_map(|r| work_planes::resolve(&store, &r.plane).ok())
                .map(|p| p.normal);
            let segs: Vec<Value> = lines
                .iter()
                .flat_map(|l| {
                    l.points
                        .windows(2)
                        .map(|w| json!({ "start": w[0], "end": w[1] }))
                })
                .collect();
            args["lines"] = json!(segs);
            if let Some(n) = normal {
                args["normal"] = json!(n);
            }
        }
        let gap = TextEntryDialog::builder(&frame, "Divider gap:", "Divide Parts")
            .with_default_value(&self.format_length(0.0))
            .build();
        if gap.show_modal() != ID_OK {
            return;
        }
        let Some(gap) = self
            .run(
                "units.parse",
                json!({ "text": gap.get_value(), "data_type": "length" }),
            )
            .ok()
            .and_then(|v| v.as_f64())
        else {
            return frame.set_status_text("Type a gap", 0);
        };
        args["gap"] = json!(gap);
        if let Some(v) = self.parts_run("parts.divide", args, "Divided") {
            let n = v["parts"].as_array().map_or(0, Vec::len);
            frame.set_status_text(&format!("Divided into {n} piece(s)"), 0);
        }
    }

    /// Modify › Parts › Merge Parts.
    pub(crate) fn merge_parts(&mut self) {
        let ids = self.selected_parts();
        self.parts_run("parts.merge", json!({ "part_ids": ids }), "Parts merged");
    }

    /// Modify › Parts › Exclude Parts.
    pub(crate) fn exclude_parts(&mut self) {
        let ids = self.selected_parts();
        if ids.is_empty() {
            return self.frame.set_status_text("Select parts to exclude", 0);
        }
        self.parts_run(
            "parts.exclude",
            json!({ "part_ids": ids }),
            "Excluded (Restore Parts brings them back)",
        );
    }

    /// Modify › Parts › Restore Parts: the excluded parts of the selected elements' originals,
    /// or every excluded part.
    pub(crate) fn restore_parts(&mut self) {
        let Some(store) = self.store_now() else {
            return;
        };
        let selection = self.selection();
        let sources: Vec<String> = selection
            .iter()
            .filter_map(|id| store.get(*id))
            .map(|e| parts::recipe(e).map_or_else(|| e.guid().to_string(), |r| r.source))
            .collect();
        let ids: Vec<ElementId> = store
            .iter()
            .filter(|e| {
                parts::recipe(e).is_some_and(|r| {
                    r.excluded && (sources.is_empty() || sources.contains(&r.source))
                })
            })
            .map(|e| e.id())
            .collect();
        if ids.is_empty() {
            return self.frame.set_status_text("No excluded parts", 0);
        }
        let n = ids.len();
        self.parts_run(
            "parts.restore",
            json!({ "part_ids": ids }),
            &format!("{n} part(s) restored"),
        );
    }

    /// View › Parts Visibility.
    pub(crate) fn parts_visibility(&mut self) {
        let frame = self.frame;
        let Some(view) = self.active_view().and_then(|v| v.element) else {
            return frame
                .set_status_text("Save the model's views first (this view has no element)", 0);
        };
        let Some(store) = self.store_now() else {
            return;
        };
        let modes = ["Show Parts", "Show Original", "Show Both"];
        let d = SingleChoiceDialog::builder(&frame, "This view shows:", "Parts Visibility", &modes)
            .build();
        d.set_selection(match parts::view_mode(&store, Some(view)) {
            parts::Visibility::ShowParts => 0,
            parts::Visibility::ShowOriginal => 1,
            parts::Visibility::ShowBoth => 2,
        });
        if d.show_modal() != ID_OK {
            return;
        }
        let mode =
            ["show_parts", "show_original", "show_both"][d.get_selection().clamp(0, 2) as usize];
        self.parts_run(
            "view.set_parts_visibility",
            json!({ "view_id": view, "mode": mode }),
            modes[d.get_selection().clamp(0, 2) as usize],
        );
    }

    /// A part's rows; `false` if it is not one.
    pub(crate) fn part_rows(&mut self, grid: &PropertyGrid, id: ElementId) -> bool {
        let Some(store) = self.store_now() else {
            return false;
        };
        if store.get(id).and_then(parts::recipe).is_none() {
            return false;
        }
        let Ok(i) = self.run("parts.info", json!({ "part_id": id })) else {
            return false;
        };
        grid.append(Property::category("Part", "c_part"));
        let s = |k: &str| i[k].as_str().unwrap_or_default().to_owned();
        let own = if i["material_by_original"].as_bool().unwrap_or(true) {
            String::new()
        } else {
            s("material")
        };
        grid.append(Property::string("Material", "pt_material", own));
        grid.set_help_string(
            "pt_material",
            "The part's own material; empty for Material By Original",
        );
        let level = i["base_level"]
            .as_str()
            .map(|g| {
                if g.is_empty() {
                    "Non Associated".to_owned()
                } else {
                    reviera_core::GlobalId::parse(g)
                        .and_then(|g| store.find_by_guid(&g))
                        .and_then(|l| store.get(l))
                        .and_then(|l| l.name.clone())
                        .unwrap_or_default()
                }
            })
            .unwrap_or_default();
        let volume = format!("{:.4} m³", i["volume"].as_f64().unwrap_or(0.0));
        let area = format!("{:.3} m²", i["area"].as_f64().unwrap_or(0.0));
        let rows = [
            ("Material (in use)", s("material")),
            ("Construction", s("construction")),
            ("Original Category", s("original_category")),
            ("Original Type", s("original_type")),
            ("Base Level", level),
            ("Volume", volume),
            ("Area", area),
            (
                "Excluded",
                if i["excluded"].as_bool().unwrap_or(false) {
                    "Yes"
                } else {
                    "No"
                }
                .to_owned(),
            ),
        ];
        for (k, (label, value)) in rows.into_iter().enumerate() {
            let key = format!("pt_r{k}");
            grid.append(Property::string(label, &key, value));
            grid.set_property_read_only(&key, true);
        }
        true
    }

    /// An edit of a part's row; `false` if `property` is not one.
    pub(crate) fn part_edit(&mut self, property: &str, text: Option<String>) -> bool {
        if property != "pt_material" {
            return property.starts_with("pt_");
        }
        let ids = self.selected_parts();
        self.parts_run(
            "parts.set_overrides",
            json!({ "part_ids": ids, "material": text.unwrap_or_default() }),
            "Material changed",
        );
        true
    }
}
