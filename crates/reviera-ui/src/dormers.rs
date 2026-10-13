// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The Dormer tool (ARC-ROOF-007): with a roof selected, Modify | Roofs › Dormer asks for a
//! style (gable, hipped, shed or flat), then each click on the roof (in a plan, or on the roof
//! in 3D) places a dormer there with its gap walls (`dormer.create`). Esc ends the tool.
//! Selecting a dormer's roof or walls shows the dormer's own properties, which rebuild it
//! (`dormer.set`).

use reviera_core::ElementId;
use reviera_core::dormers::{self, DormerStyle};
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

const STYLES: &[(DormerStyle, &str, &str)] = &[
    (DormerStyle::Gable, "Gable (pitched)", "gable"),
    (DormerStyle::Hip, "Hipped (pitched)", "hip"),
    (DormerStyle::Shed, "Shed (flat lift)", "shed"),
    (DormerStyle::Flat, "Flat", "flat"),
];

/// The Dormer tool: the roof dormers go on, and their style.
#[derive(Debug, Clone, Copy)]
pub struct DormerTool {
    roof: ElementId,
    style: DormerStyle,
}

impl Gui {
    /// Modify | Roofs › Dormer.
    pub(crate) fn start_dormer_tool(&mut self) {
        let frame = self.frame;
        let Some(store) = self
            .doc
            .and_then(|d| self.app.document(d))
            .map(|d| d.doc.store().clone())
        else {
            return;
        };
        let Some(roof) = self.selection().into_iter().find(|id| {
            store
                .get(*id)
                .and_then(reviera_core::roofs::roof_recipe)
                .is_some()
        }) else {
            return frame.set_status_text("Select the roof to add dormers to", 0);
        };
        let labels: Vec<&str> = STYLES.iter().map(|s| s.1).collect();
        let dialog =
            SingleChoiceDialog::builder(&frame, "Dormer style:", "Dormer", &labels).build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let Some(&(style, _, _)) = usize::try_from(dialog.get_selection())
            .ok()
            .and_then(|i| STYLES.get(i))
        else {
            return;
        };
        self.end_tool(&frame);
        self.dormer_tool = Some(DormerTool { roof, style });
        frame.set_status_text(
            "Dormer: click a sloped face of the roof where the dormer's front goes (in a plan, or on the roof in 3D); Esc ends",
            0,
        );
    }

    pub(crate) fn end_dormer_tool(&mut self) -> bool {
        self.dormer_tool.take().is_some()
    }

    /// The plan point under the cursor: in a plan, where it points; in 3D, where it hits the
    /// model.
    fn dormer_point(&self, x: i32, y: i32, panel: &Panel) -> Option<[f64; 2]> {
        if self.in_plan() {
            return self.plan_xy(x, y);
        }
        let v = self.active_view()?;
        let size = panel.get_client_size();
        let (origin, dir) = v
            .camera
            .ray(x as f32, y as f32, size.width as f32, size.height as f32);
        let (_, t) = self
            .scene()
            .pick_visible(origin, dir, &|_| true, &self.view_visibility(v))?;
        let p = origin + dir * t;
        Some([f64::from(p.x), f64::from(p.y)])
    }

    pub(crate) fn dormer_click(&mut self, x: i32, y: i32, panel: &Panel) {
        let frame = self.frame;
        let Some(t) = self.dormer_tool else { return };
        let Some(p) = self.dormer_point(x, y, panel) else {
            return frame.set_status_text("Click on the roof", 0);
        };
        let style = STYLES
            .iter()
            .find(|s| s.0 == t.style)
            .map_or("gable", |s| s.2);
        match self.run(
            "dormer.create",
            json!({ "roof_id": t.roof, "point": p, "style": style }),
        ) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text("Dormer placed; click for another, or Esc", 0);
            }
            Err(e) => error(&frame, &format!("The dormer was not placed:\n{e}")),
        }
    }

    /// The dormer of a selected element: the dormer itself or one of its parts.
    fn dormer_of_selection(&self, id: ElementId) -> Option<(ElementId, dormers::DormerRecipe)> {
        let store = self.doc.and_then(|d| self.app.document(d))?.doc.store();
        let e = store.get(id)?;
        let dormer = match dormers::dormer_of(e) {
            Some(g) => store.find_by_guid(&reviera_core::GlobalId::parse(g)?)?,
            None => id,
        };
        Some((dormer, dormers::dormer_recipe(store.get(dormer)?)?))
    }

    /// The dormer's rows, for a selected dormer or dormer part.
    pub(crate) fn dormer_rows(&mut self, grid: &PropertyGrid, id: ElementId) {
        let Some((_, r)) = self.dormer_of_selection(id) else {
            return;
        };
        grid.append(Property::category("Dormer", "c_dormer"));
        let index = STYLES
            .iter()
            .position(|s| s.0 == r.style)
            .map_or(-1, |i| i as i32);
        grid.append(Property::enumeration(
            "Style",
            "dm_style",
            STYLES
                .iter()
                .enumerate()
                .map(|(i, s)| (s.1.to_owned(), i as i32))
                .collect::<Vec<(String, i32)>>(),
            index,
        ));
        let rows = [
            ("Width", "dm_width", self.format_length(r.width)),
            (
                "Front Height",
                "dm_front",
                self.format_length(r.front_height),
            ),
            ("Overhang", "dm_overhang", self.format_length(r.overhang)),
        ];
        for (label, key, value) in rows {
            grid.append(Property::string(label, key, value));
        }
        grid.set_help_string(
            "dm_front",
            "The dormer's eave above the main roof's surface",
        );
        if r.style != DormerStyle::Flat {
            let slope = self
                .run(
                    "units.format",
                    json!({ "value": r.slope, "data_type": "angle" }),
                )
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_else(|| format!("{:.1}°", r.slope.to_degrees()));
            grid.append(Property::string("Slope", "dm_slope", slope));
        }
    }

    /// An edit of a dormer row; `false` if `property` is not one.
    pub(crate) fn dormer_edit(&mut self, property: &str, text: Option<String>) -> bool {
        if !property.starts_with("dm_") {
            return false;
        }
        let frame = self.frame;
        let Some((dormer, _)) = self
            .selection()
            .first()
            .and_then(|id| self.dormer_of_selection(*id))
        else {
            return true;
        };
        let text = text.unwrap_or_default();
        let mut args = json!({ "dormer_id": dormer });
        match property {
            "dm_style" => match STYLES.iter().find(|s| s.1 == text) {
                Some(s) => args["style"] = json!(s.2),
                None => return true,
            },
            "dm_width" => args["width"] = json!(text),
            "dm_front" => args["front_height"] = json!(text),
            "dm_overhang" => args["overhang"] = json!(text),
            "dm_slope" => args["slope"] = json!(text),
            _ => return false,
        }
        if let Err(e) = self.run("dormer.set", args) {
            error(&frame, &format!("The dormer was not changed:\n{e}"));
        }
        // The selected part may have been rebuilt: select the dormer's new roof.
        let roof = self.doc.and_then(|d| self.app.document(d)).and_then(|d| {
            let store = d.doc.store();
            let r = dormers::dormer_recipe(store.get(dormer)?)?;
            r.parts.iter().find_map(|g| {
                let id = store.find_by_guid(&reviera_core::GlobalId::parse(g)?)?;
                reviera_core::roofs::roof_recipe(store.get(id)?).map(|_| id)
            })
        });
        if let Some(roof) = roof {
            let _ = self.run("selection.set", json!({ "ids": [roof] }));
            self.selection_changed();
        }
        self.refresh_scene(&frame);
        true
    }
}
