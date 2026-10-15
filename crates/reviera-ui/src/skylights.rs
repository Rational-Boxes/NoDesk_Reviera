// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Skylights in the window: Architecture › Opening › Skylight, and a selected skylight's
//! Properties.
//!
//! The tool places a skylight where the roof under the cursor is clicked, in a plan or a 3D view:
//! *Automatic* picks a roof window in a pitched roof and a ridge lantern on a flat one; the
//! tool's Properties choose another style and the sizes. It repeats until Esc.

use reviera_core::ElementId;
use reviera_core::skylights::{self, SkylightStyle};
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

const AUTOMATIC: &str = "Automatic";

/// The Skylight tool's settings.
#[derive(Debug, Clone, Default)]
pub struct SkylightTool {
    /// `None`: by the roof (a roof window in a slope, a lantern on a flat roof).
    pub style: Option<SkylightStyle>,
    pub width: Option<f64>,
    pub length: Option<f64>,
}

impl Gui {
    /// Architecture › Opening › Skylight.
    pub(crate) fn start_skylight_tool(&mut self) {
        let frame = self.frame;
        if self.doc.is_none() {
            return frame.set_status_text("Open or create a project first (File > New)", 0);
        }
        let keep = self.skylight_tool.clone().unwrap_or_default();
        self.end_tool(&frame);
        self.end_insert_tool();
        self.end_modify();
        self.clear_selection();
        self.skylight_tool = Some(keep);
        self.properties_stale = true;
        frame.set_status_text(
            "Skylight: click a roof where its middle goes (in a plan or 3D); Esc ends the tool",
            0,
        );
    }

    pub(crate) fn end_skylight_tool(&mut self) -> bool {
        if self.skylight_tool.take().is_none() {
            return false;
        }
        self.properties_stale = true;
        true
    }

    /// The roof under pixel (`x`, `y`) and the plan point: in 3D the roof face hit; in a plan
    /// the roof over the point (the highest, where roofs overlap).
    fn roof_under(&self, x: i32, y: i32) -> Option<(ElementId, [f64; 2])> {
        let v = self.active_view()?;
        let store = self.app.document(v.doc)?.doc.store();
        let size = v.panel.get_client_size();
        let (o, d) = v
            .camera
            .ray(x as f32, y as f32, size.width as f32, size.height as f32);
        if v.plan.is_none() {
            let (i, t) = self
                .scene()
                .pick_within(o, d, f32::NEG_INFINITY, f32::INFINITY)?;
            let id = self.scene().meshes[i].element?;
            reviera_core::roofs::roof_recipe(store.get(id)?)?;
            let hit = o + d * t;
            return Some((id, [f64::from(hit.x), f64::from(hit.y)]));
        }
        let p = [f64::from(o.x), f64::from(o.y)];
        reviera_core::roofs::all(store)
            .into_iter()
            .filter_map(|(id, g)| Some((id, g.underside(p)?)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| (id, p))
    }

    pub(crate) fn skylight_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        let Some(t) = self.skylight_tool.clone() else {
            return;
        };
        let Some((roof, point)) = self.roof_under(x, y) else {
            return frame.set_status_text("Skylight: click on a roof", 0);
        };
        let mut args = json!({ "roof_id": roof, "point": point });
        if let Some(s) = t.style {
            args["style"] = serde_json::to_value(s).expect("serialises");
        }
        if let Some(w) = t.width {
            args["width"] = json!(w);
        }
        if let Some(l) = t.length {
            args["length"] = json!(l);
        }
        match self.run("skylight.create", args) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text("Skylight placed; click another place, or Esc", 0);
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
    }

    /// The palette while the Skylight tool runs.
    pub(crate) fn skylight_tool_properties(&mut self, grid: &PropertyGrid) -> bool {
        let Some(t) = self.skylight_tool.clone() else {
            return false;
        };
        grid.append(Property::category("Skylight tool", "c_skytool"));
        let mut styles = vec![(AUTOMATIC.to_owned(), 0)];
        styles.extend(
            SkylightStyle::ALL
                .iter()
                .enumerate()
                .map(|(i, s)| (s.label().to_owned(), i as i32 + 1)),
        );
        let at = t
            .style
            .and_then(|s| SkylightStyle::ALL.iter().position(|x| *x == s))
            .map_or(0, |i| i as i32 + 1);
        grid.append(Property::enumeration("Style", "skt_style", styles, at));
        let f = |g: &mut Gui, v: Option<f64>| v.map(|v| g.format_length(v)).unwrap_or_default();
        let (w, l) = (f(self, t.width), f(self, t.length));
        grid.append(Property::string("Width", "skt_width", w));
        grid.append(Property::string("Length", "skt_length", l));
        grid.set_help_string("skt_width", "Empty: the style's usual size");
        true
    }

    /// The rows of a selected skylight; `false` if `id` is not one.
    pub(crate) fn skylight_rows(&mut self, grid: &PropertyGrid, id: ElementId) -> bool {
        let Some(store) = self.store_now() else {
            return false;
        };
        let Some(s) = store.get(id).and_then(skylights::skylight_recipe) else {
            return false;
        };
        grid.append(Property::category("Skylight", "c_sky"));
        let styles: Vec<(String, i32)> = SkylightStyle::ALL
            .iter()
            .enumerate()
            .map(|(i, x)| (x.label().to_owned(), i as i32))
            .collect();
        let at = SkylightStyle::ALL
            .iter()
            .position(|x| *x == s.style)
            .unwrap_or(0) as i32;
        grid.append(Property::enumeration("Style", "sk_style", styles, at));
        let rows = [
            ("Width", "sk_width", self.format_length(s.width)),
            ("Length", "sk_length", self.format_length(s.length)),
            (
                "Rotation",
                "sk_rotation",
                format!("{:.1}°", s.rotation.to_degrees()),
            ),
            ("Curb Height", "sk_curb", self.format_length(s.curb)),
            (
                "Glass Slope",
                "sk_pitch",
                format!("{:.1}°", s.pitch.to_degrees()),
            ),
            (
                "Largest Pane",
                "sk_max_pane",
                self.format_length(s.max_pane),
            ),
        ];
        for (label, name, value) in rows {
            grid.append(Property::string(label, name, value));
        }
        if let Ok(g) = skylights::regenerate(&store, store.get(id).expect("found"))
            && s.style == SkylightStyle::RidgeLantern
        {
            grid.append(Property::string("Panes", "sk_panes", g.panes.to_string()));
            grid.set_property_read_only("sk_panes", true);
            grid.set_help_string(
                "sk_panes",
                "From the length: as many equal panes as keep each within the largest pane",
            );
        }
        true
    }

    /// An edit of a skylight or Skylight tool row; `false` if `property` is not one.
    pub(crate) fn skylight_edit(&mut self, property: &str, text: Option<String>) -> bool {
        if !property.starts_with("sk_") && !property.starts_with("skt_") {
            return false;
        }
        let frame = self.frame;
        let text = text.unwrap_or_default();
        let style_of = |label: &str| SkylightStyle::ALL.into_iter().find(|s| s.label() == label);
        if let Some(row) = property.strip_prefix("skt_") {
            let length = self
                .run(
                    "units.parse",
                    json!({ "text": text, "data_type": "length" }),
                )
                .ok()
                .and_then(|v| v.as_f64())
                .filter(|v| *v > 0.0);
            if let Some(t) = &mut self.skylight_tool {
                match row {
                    "style" => t.style = style_of(&text),
                    "width" => t.width = length,
                    "length" => t.length = length,
                    _ => {}
                }
            }
            self.properties_stale = true;
            return true;
        }
        let ids: Vec<ElementId> = {
            let ids = self.selection();
            let store = self.store_now();
            ids.into_iter()
                .filter(|i| {
                    store
                        .as_ref()
                        .and_then(|s| s.get(*i))
                        .and_then(skylights::skylight_recipe)
                        .is_some()
                })
                .collect()
        };
        let angle = |t: &str| {
            if t.trim().ends_with('°') {
                t.to_owned()
            } else {
                format!("{}°", t.trim())
            }
        };
        let args: Option<Value> = match property {
            "sk_style" => style_of(&text).map(|s| json!({ "style": s })),
            "sk_width" => Some(json!({ "width": text })),
            "sk_length" => Some(json!({ "length": text })),
            "sk_rotation" => Some(json!({ "rotation": angle(&text) })),
            "sk_curb" => Some(json!({ "curb": text })),
            "sk_pitch" => Some(json!({ "pitch": angle(&text) })),
            "sk_max_pane" => Some(json!({ "max_pane": text })),
            _ => None,
        };
        if let Some(mut a) = args {
            a["skylight_ids"] = json!(ids);
            if let Err(e) = self.run("skylight.set", a) {
                error(&frame, &format!("The change was not made:\n{e}"));
            }
        }
        self.properties_stale = true;
        self.refresh_scene(&frame);
        true
    }
}
