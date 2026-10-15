// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Model In-Place and Model Text in the window (ARC-INPL-001, ARC-MTXT-001).
//!
//! Model In-Place makes a form from selected model lines: a closed loop of them is the profile
//! (on their work plane), and for a revolve one more line is the axis. With an in-place element
//! in the selection, the form is added to it; otherwise a new one is made, named and given a
//! category. Model Text asks for the text, size and depth, then places it where you click in a
//! plan.

use reviera_core::{ElementId, in_place, model_lines};
use serde_json::json;
use wxdragon::prelude::*;

use super::Gui;

const KINDS: [&str; 3] = ["Extrusion", "Void Extrusion", "Revolve"];

/// The lines' points joined end to end into one closed loop, if they make one.
fn closed_loop(lines: &[Vec<[f64; 2]>]) -> Option<Vec<[f64; 2]>> {
    let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-4;
    let mut rest: Vec<Vec<[f64; 2]>> = lines.to_vec();
    let mut ring = rest.remove(0);
    while !rest.is_empty() {
        let end = *ring.last()?;
        let k = rest
            .iter()
            .position(|l| near(l[0], end) || near(*l.last().expect("points"), end))?;
        let mut next = rest.remove(k);
        if !near(next[0], end) {
            next.reverse();
        }
        ring.extend(next.into_iter().skip(1));
    }
    if !near(ring[0], *ring.last()?) || ring.len() < 4 {
        return None;
    }
    ring.pop();
    Some(ring)
}

impl Gui {
    /// Architecture › Model › Model In-Place.
    pub(crate) fn model_in_place(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else {
            return;
        };
        let ids = self.selection();
        let target = ids
            .iter()
            .copied()
            .find(|id| store.get(*id).and_then(in_place::recipe).is_some());
        let lines: Vec<(ElementId, model_lines::ModelLineRecipe)> = ids
            .iter()
            .filter_map(|id| Some((*id, store.get(*id).and_then(model_lines::recipe)?)))
            .collect();
        if lines.is_empty() {
            return frame.set_status_text(
                "Draw a closed loop of model lines for the profile and select it (and an axis line for a revolve)",
                0,
            );
        }
        let plane = lines[0].1.plane.clone();
        let pts: Vec<Vec<[f64; 2]>> = lines.iter().map(|(_, r)| r.curve.points()).collect();
        let dialog = Dialog::builder(&frame, "Model In-Place").build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let grid = FlexGridSizer::builder(0, 2)
            .with_vgap(6)
            .with_hgap(10)
            .build();
        grid.add_growable_col(1, 1);
        let text = |label: &str, v: &str| {
            let l = StaticText::builder(&dialog).with_label(label).build();
            let t = TextCtrl::builder(&dialog).with_value(v).build();
            grid.add(&l, 0, SizerFlag::AlignCenterVertical, 0);
            grid.add(&t, 1, SizerFlag::Expand, 0);
            t
        };
        let choice = |label: &str, items: Vec<String>| {
            let l = StaticText::builder(&dialog).with_label(label).build();
            let c = Choice::builder(&dialog).with_choices(items).build();
            c.set_selection(0);
            grid.add(&l, 0, SizerFlag::AlignCenterVertical, 0);
            grid.add(&c, 1, SizerFlag::Expand, 0);
            c
        };
        let name = text("Name", "In-Place 1");
        let category = choice(
            "Category",
            in_place::CATEGORIES
                .iter()
                .map(|c| c.0.to_owned())
                .collect(),
        );
        name.enable(target.is_none());
        category.enable(target.is_none());
        let kind = choice("Form", KINDS.iter().map(|k| k.to_string()).collect());
        let start = text("Start (or start angle °)", "0");
        let end = text("End (or end angle °)", &self.format_length(1.0));
        sizer.add_sizer(&grid, 0, SizerFlag::All | SizerFlag::Expand, 10);
        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        let cancel = Button::builder(&dialog)
            .with_id(ID_CANCEL)
            .with_label("Cancel")
            .build();
        let ok = Button::builder(&dialog)
            .with_id(ID_OK)
            .with_label("OK")
            .build();
        buttons.add_stretch_spacer(1);
        buttons.add(&cancel, 0, SizerFlag::All, 6);
        buttons.add(&ok, 0, SizerFlag::All, 6);
        sizer.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
        dialog.set_sizer_and_fit(sizer, true);
        dialog.set_affirmative_id(ID_OK);
        if dialog.show_modal() != ID_OK {
            dialog.destroy();
            return;
        }
        let k = kind.get_selection().unwrap_or(0) as usize;
        let revolve = k == 2;
        let (s_text, e_text) = (start.get_value(), end.get_value());
        let name_v = name.get_value();
        let cat = category
            .get_selection()
            .and_then(|i| in_place::CATEGORIES.get(i as usize))
            .map_or("Generic Models", |c| c.0);
        dialog.destroy();
        // The profile, and for a revolve the axis (the line not in the loop).
        let (profile, axis) = if revolve {
            let mut found = None;
            for skip in 0..pts.len() {
                let others: Vec<Vec<[f64; 2]>> = pts
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != skip)
                    .map(|(_, p)| p.clone())
                    .collect();
                if !others.is_empty()
                    && let Some(l) = closed_loop(&others)
                {
                    let a = &pts[skip];
                    found = Some((l, Some([a[0], *a.last().expect("points")])));
                    break;
                }
            }
            match found {
                Some(f) => f,
                None => {
                    return frame.set_status_text(
                        "A revolve needs a closed loop of lines and one more line for the axis",
                        0,
                    );
                }
            }
        } else {
            match closed_loop(&pts) {
                Some(l) => (l, None),
                None => return frame.set_status_text("The selected lines do not close a loop", 0),
            }
        };
        let number = |t: &str, length: bool, gui: &mut Gui| -> Option<f64> {
            if length {
                gui.run("units.parse", json!({ "text": t, "data_type": "length" }))
                    .ok()
                    .and_then(|v| v.as_f64())
            } else {
                t.trim()
                    .trim_end_matches('°')
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .map(f64::to_radians)
            }
        };
        let (Some(s), Some(e)) = (
            number(&s_text, !revolve, self),
            number(&e_text, !revolve, self),
        ) else {
            return frame.set_status_text("Type the start and end", 0);
        };
        let plane_args = match &plane {
            reviera_core::work_planes::WorkPlaneRef::Level { level, offset } => {
                json!({ "level": level, "offset": offset })
            }
            reviera_core::work_planes::WorkPlaneRef::Grid { grid } => json!({ "grid": grid }),
            reviera_core::work_planes::WorkPlaneRef::RefPlane { ref_plane } => {
                json!({ "ref_plane": ref_plane })
            }
            reviera_core::work_planes::WorkPlaneRef::Explicit {
                origin,
                x_dir,
                normal,
            } => json!({ "plane": { "origin": origin, "x_dir": x_dir, "normal": normal } }),
        };
        let mut form = if revolve {
            json!({ "kind": "revolve", "axis": axis, "start": s, "end": e, "profile": profile })
        } else {
            json!({ "kind": "extrusion", "start": s, "end": e, "profile": profile, "void": k == 1 })
        };
        if let (Some(f), Some(p)) = (form.as_object_mut(), plane_args.as_object()) {
            f.extend(p.clone());
        }
        let result = match target {
            Some(t) => {
                let mut args = form;
                args["element_id"] = json!(t);
                self.run("inplace.add_form", args).map(|_| ())
            }
            None => self
                .run(
                    "inplace.create",
                    json!({ "name": name_v, "category": cat, "forms": [form] }),
                )
                .map(|_| ()),
        };
        match result {
            Ok(()) => {
                self.refresh_scene(&frame);
                frame.set_status_text(
                    "Form made (the model lines stay; delete them if not needed)",
                    0,
                );
            }
            Err(e) => super::error(&frame, &format!("No form:\n{e}")),
        }
    }

    /// Architecture › Model › Model Text: the text, then a click in a plan.
    pub(crate) fn start_model_text(&mut self) {
        let frame = self.frame;
        let Some(text) = super::notes::ask_text(&frame, "Model Text", "") else {
            return;
        };
        if text.trim().is_empty() {
            return;
        }
        let size = TextEntryDialog::builder(&frame, "Letter height:", "Model Text")
            .with_default_value(&self.format_length(0.3))
            .build();
        if size.show_modal() != ID_OK {
            return;
        }
        let depth = TextEntryDialog::builder(&frame, "Depth (0 for flat):", "Model Text")
            .with_default_value(&self.format_length(0.05))
            .build();
        if depth.show_modal() != ID_OK {
            return;
        }
        let mut len = |t: Option<String>| -> Option<f64> {
            self.run("units.parse", json!({ "text": t?, "data_type": "length" }))
                .ok()
                .and_then(|v| v.as_f64())
        };
        let (Some(s), Some(d)) = (len(size.get_value()), len(depth.get_value())) else {
            return frame.set_status_text("Type lengths for the height and depth", 0);
        };
        self.text_pick = Some((text, s, d));
        frame.set_status_text("Model Text: click where it starts, in a plan (Esc ends)", 0);
    }

    pub(crate) fn model_text_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        let Some((text, size, depth)) = self.text_pick.clone() else {
            return;
        };
        let (Some(p), Some(plan)) = (
            self.plan_xy(x, y),
            self.active_view().and_then(|v| v.plan.clone()),
        ) else {
            return frame.set_status_text("Model text is placed in plans", 0);
        };
        match self.run(
            "modeltext.create",
            json!({ "text": text, "point": p, "size": size, "depth": depth, "level": plan.level }),
        ) {
            Ok(_) => {
                self.text_pick = None;
                self.refresh_scene(&frame);
                frame.set_status_text("Model text placed", 0);
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
    }
}
