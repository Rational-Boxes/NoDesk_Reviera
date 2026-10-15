// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Massing in the window (ARC-MASS-001 … 003, ARC-ROOF-003, ARC-CW-007): Massing & Site ›
//! In-Place Mass from selected model lines (one closed loop is extruded; loops on several levels
//! are lofted), Mass Floors, and the by-face tools on a selected mass (its faces chosen from a
//! list), with Update to Face.

use reviera_core::{ElementId, masses, model_lines};
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::Gui;

/// Model lines on one level plane: its elevation, its plane arguments, and the lines' points.
type LevelLoops = (f64, Value, Vec<Vec<[f64; 2]>>);

impl Gui {
    fn selected_mass(&mut self) -> Option<ElementId> {
        let store = self.store_now()?;
        self.selection()
            .into_iter()
            .find(|id| store.get(*id).and_then(masses::recipe).is_some())
    }

    /// Massing & Site › In-Place Mass.
    pub(crate) fn in_place_mass(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else {
            return;
        };
        // The selected model lines on level planes, by elevation.
        let mut by_level: Vec<LevelLoops> = Vec::new();
        for id in self.selection() {
            let Some(r) = store.get(id).and_then(model_lines::recipe) else {
                continue;
            };
            let reviera_core::work_planes::WorkPlaneRef::Level { level, offset } = &r.plane else {
                continue;
            };
            let Ok(z) = reviera_core::levels::elevation_of(&store, level) else {
                continue;
            };
            let z = z + offset;
            let pts = r.curve.points();
            match by_level.iter_mut().find(|(zz, _, _)| (zz - z).abs() < 1e-6) {
                Some(b) => b.2.push(pts),
                None => by_level.push((z, json!({ "level": level, "offset": offset }), vec![pts])),
            }
        }
        if by_level.is_empty() {
            return frame.set_status_text(
                "Draw closed loops of model lines on levels for the mass, and select them",
                0,
            );
        }
        by_level.sort_by(|a, b| a.0.total_cmp(&b.0));
        let loops: Option<Vec<Vec<[f64; 2]>>> = by_level
            .iter()
            .map(|(_, _, lines)| super::in_place::closed_loop(lines))
            .collect();
        let Some(loops) = loops else {
            return frame.set_status_text("The lines on each level must close one loop", 0);
        };
        let name = TextEntryDialog::builder(&frame, "Mass name:", "In-Place Mass")
            .with_default_value("Mass 1")
            .build();
        if name.show_modal() != ID_OK {
            return;
        }
        let Some(name) = name.get_value() else { return };
        let base = by_level[0].1.clone();
        let form = if loops.len() == 1 {
            let h = TextEntryDialog::builder(&frame, "Height:", "In-Place Mass")
                .with_default_value(&self.format_length(12.0))
                .build();
            if h.show_modal() != ID_OK {
                return;
            }
            let Some(height) = self
                .run(
                    "units.parse",
                    json!({ "text": h.get_value(), "data_type": "length" }),
                )
                .ok()
                .and_then(|v| v.as_f64())
            else {
                return frame.set_status_text("Type a height", 0);
            };
            json!({ "kind": "extrusion", "start": 0.0, "end": height, "profile": loops[0] })
        } else {
            let z0 = by_level[0].0;
            let profiles: Vec<Value> = by_level
                .iter()
                .zip(&loops)
                .map(|((z, _, _), l)| json!({ "offset": z - z0, "points": l }))
                .collect();
            json!({ "kind": "loft", "profile": loops[0], "profiles": profiles })
        };
        let mut form = form;
        if let (Some(f), Some(b)) = (form.as_object_mut(), base.as_object()) {
            f.extend(b.clone());
        }
        match self.run("mass.create", json!({ "name": name, "forms": [form] })) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text(
                    "Mass made (the model lines stay; delete them if not needed)",
                    0,
                );
            }
            Err(e) => super::error(&frame, &format!("No mass:\n{e}")),
        }
    }

    /// Massing & Site › Mass Floors.
    pub(crate) fn mass_floors(&mut self) {
        let frame = self.frame;
        let Some(m) = self.selected_mass() else {
            return frame.set_status_text("Select a mass", 0);
        };
        let Some(store) = self.store_now() else {
            return;
        };
        let levels = reviera_core::levels::list(&store);
        let r = store.get(m).and_then(masses::recipe).expect("a mass");
        let names: Vec<String> = levels
            .iter()
            .map(|l| format!("{} ({})", l.name, self.format_length(l.elevation)))
            .collect();
        let labels: Vec<&str> = names.iter().map(String::as_str).collect();
        let dialog =
            MultiChoiceDialog::builder(&frame, "Levels with mass floors:", "Mass Floors", &labels)
                .build();
        let ticked: Vec<i32> = levels
            .iter()
            .enumerate()
            .filter(|(_, l)| r.all_levels || r.floors.contains(&l.guid))
            .map(|(i, _)| i as i32)
            .collect();
        dialog.set_selections(&ticked);
        if dialog.show_modal() != ID_OK {
            return;
        }
        let ids: Vec<u64> = dialog
            .get_selections()
            .into_iter()
            .filter_map(|i| levels.get(i as usize).map(|l| l.id.0))
            .collect();
        match self.run("mass.set_floors", json!({ "mass_id": m, "level_ids": ids })) {
            Ok(_) => {
                let n = self
                    .run("mass.floors", json!({ "mass_id": m }))
                    .ok()
                    .and_then(|v| v.as_array().map(Vec::len))
                    .unwrap_or(0);
                frame.set_status_text(&format!("{n} mass floor(s)"), 0);
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
    }

    /// A by-face tool: the mass's faces chosen, then the command.
    pub(crate) fn by_face(&mut self, command: &str) {
        let frame = self.frame;
        let Some(m) = self.selected_mass() else {
            return frame.set_status_text("Select a mass", 0);
        };
        if command == "floor.create_by_mass_floor" {
            match self.run(command, json!({ "mass_id": m })) {
                Ok(_) => {
                    self.refresh_scene(&frame);
                    frame.set_status_text("Floors made on the mass floors", 0);
                }
                Err(e) => super::error(&frame, &e),
            }
            return;
        }
        let Ok(faces) = self.run("mass.faces", json!({ "mass_id": m })) else {
            return;
        };
        let faces: Vec<(String, String)> = faces
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| {
                let name = f["face"].as_str().unwrap_or_default().to_owned();
                let nz = f["normal"][2].as_f64().unwrap_or(0.0);
                let what = if nz > 0.95 {
                    "top".to_owned()
                } else if nz < -0.95 {
                    "bottom".to_owned()
                } else {
                    let (x, y) = (
                        f["normal"][0].as_f64().unwrap_or(0.0),
                        f["normal"][1].as_f64().unwrap_or(0.0),
                    );
                    let deg = (y.atan2(x).to_degrees() + 360.0) % 360.0;
                    let dir = [
                        "east",
                        "north-east",
                        "north",
                        "north-west",
                        "west",
                        "south-west",
                        "south",
                        "south-east",
                    ][((deg + 22.5) / 45.0) as usize % 8];
                    format!("facing {dir}")
                };
                (name.clone(), format!("{name} — {what}"))
            })
            .collect();
        let labels: Vec<&str> = faces.iter().map(|f| f.1.as_str()).collect();
        let dialog = MultiChoiceDialog::builder(&frame, "Faces:", "By Face", &labels).build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let chosen: Vec<String> = dialog
            .get_selections()
            .into_iter()
            .filter_map(|i| faces.get(i as usize).map(|f| f.0.clone()))
            .collect();
        if chosen.is_empty() {
            return;
        }
        match self.run(command, json!({ "mass_id": m, "faces": chosen })) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text(
                    "Made by face (Update to Face makes them follow the mass)",
                    0,
                );
            }
            Err(e) => super::error(&frame, &e),
        }
    }

    /// Massing & Site › Update to Face.
    pub(crate) fn update_to_face(&mut self) {
        let ids = self.selection();
        let frame = self.frame;
        match self.run("element.update_to_face", json!({ "element_ids": ids })) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text("Updated to the faces", 0);
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
    }
}
