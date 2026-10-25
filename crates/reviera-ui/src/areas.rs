// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Areas and colour schemes in the window (ARC-AREA-001, ARC-CLR-001): area plans, boundaries
//! and areas; colour fills in plans and the Color Schemes dialog.
//!
//! - **Area Plan** makes an area plan of a scheme for the active plan's level (with boundaries
//!   on the outside walls, if asked) and opens it.
//! - **Area Boundary** picks the selected walls into the active area plan, placed by the
//!   scheme's rules.
//! - **Area** places areas where you click in an area plan, of the type chosen first, until Esc.
//! - **Color Schemes** edits a scheme's rows and shows it in the active plan.

use reviera_core::ElementId;
use reviera_core::color_schemes::{self, Mode};
use reviera_render::SceneMesh;
use reviera_render::annotation::{label_strokes, room_tag_labels};
use reviera_render::plan::Lines;
use serde_json::json;
use wxdragon::prelude::*;

use super::Gui;

const BOUNDARY_COLOR: [f32; 4] = [0.55, 0.15, 0.6, 1.0];

impl Gui {
    /// The active view's area scheme and level, when it is an area plan.
    fn area_plan_of(&self, index: usize) -> Option<(ElementId, String, String)> {
        let v = &self.views[index];
        let e = v.element?;
        let store = self.app.document(v.doc)?.doc.store().clone();
        let data = reviera_core::views::view_data(store.get(e)?)?;
        Some((e, data.area_scheme?, data.level?))
    }

    /// An area plan's boundaries and area labels.
    pub(crate) fn area_lines(&mut self, index: usize) -> Vec<Lines> {
        let Some((_, scheme, level)) = self.area_plan_of(index) else {
            return Vec::new();
        };
        let Some(plan) = self.views[index].plan.clone() else {
            return Vec::new();
        };
        let Some(store) = self
            .app
            .document(self.views[index].doc)
            .map(|d| d.doc.store().clone())
        else {
            return Vec::new();
        };
        let z = plan.range.cut as f32 + 0.004;
        let mut boundaries = Lines {
            color: BOUNDARY_COLOR,
            width: 0.35,
            segments: Vec::new(),
        };
        for e in store.iter() {
            let Some(b) = reviera_core::areas::boundary_recipe(e) else {
                continue;
            };
            if b.scheme != scheme || b.level != level {
                continue;
            }
            if let Some(l) = reviera_core::areas::boundary_line(&store, e) {
                boundaries.segments.push([
                    [l.start[0] as f32, l.start[1] as f32, z],
                    [l.end[0] as f32, l.end[1] as f32, z],
                ]);
            }
        }
        let mut labels = Lines {
            color: BOUNDARY_COLOR,
            width: 0.18,
            segments: Vec::new(),
        };
        for a in reviera_core::areas::all(&store)
            .into_iter()
            .filter(|a| a.scheme == scheme && a.level == level)
        {
            let Some(e) = store.get(a.id) else { continue };
            let (number, name) = reviera_core::rooms::number_and_name(e);
            let point = a.region.as_ref().map_or_else(
                || reviera_core::areas::area_recipe(e).map_or([0.0; 2], |r| r.point),
                |r| {
                    let ring = r.outer_ring();
                    let n = ring.len().max(1) as f64;
                    [
                        ring.iter().map(|p| p[0]).sum::<f64>() / n,
                        ring.iter().map(|p| p[1]).sum::<f64>() / n,
                    ]
                },
            );
            let area = if a.region.is_some() {
                format!("{:.2} m", a.area)
            } else {
                "NOT ENCLOSED".into()
            };
            let lines = vec![name.to_uppercase(), number.to_uppercase(), area];
            labels.segments.extend(label_strokes(&room_tag_labels(
                point, &lines, plan.scale, z,
            )));
        }
        vec![boundaries, labels]
    }

    /// The colour fills of the active plan's colour scheme, on its level.
    pub(crate) fn color_fills(&mut self, index: usize) -> Vec<SceneMesh> {
        let v = &self.views[index];
        let (Some(e), Some(plan)) = (v.element, v.plan.as_ref()) else {
            return Vec::new();
        };
        let Some(store) = self.app.document(v.doc).map(|d| d.doc.store().clone()) else {
            return Vec::new();
        };
        let Some(view) = store.get(e) else {
            return Vec::new();
        };
        let z = plan.elevation + 0.002;
        color_schemes::fills(&store, view)
            .into_iter()
            .filter_map(|(id, rings, color)| {
                let (outer, holes) = rings.split_first()?;
                let (pts, idx) = reviera_core::bodies::extrude(outer, holes, z, z + 0.0005);
                Some(SceneMesh {
                    element: Some(id),
                    class: "IfcSpace".into(),
                    positions: pts.iter().map(|p| p.map(|c| c as f32)).collect(),
                    indices: idx,
                    color,
                })
            })
            .collect()
    }

    /// Room & Area › Area Plan.
    pub(crate) fn area_plan_dialog(&mut self) {
        let frame = self.frame;
        let Some(doc) = self.doc else { return };
        let Some(plan) = self.active_view().and_then(|v| v.plan.clone()) else {
            return frame.set_status_text("Open a plan of the level first", 0);
        };
        let Some(store) = self.store_now() else {
            return;
        };
        let schemes = reviera_core::areas::schemes(&store);
        let dialog = Dialog::builder(&frame, "New Area Plan").build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let text = StaticText::builder(&dialog)
            .with_label(&format!(
                "An area plan of {} in the scheme:",
                plan.level_name
            ))
            .build();
        sizer.add(&text, 0, SizerFlag::All, 10);
        let choice = Choice::builder(&dialog)
            .with_choices(schemes.iter().map(|s| s.name.clone()).collect())
            .build();
        choice.set_selection(0);
        sizer.add(
            &choice,
            0,
            SizerFlag::Left | SizerFlag::Right | SizerFlag::Expand,
            10,
        );
        let auto = CheckBox::builder(&dialog)
            .with_label("Boundaries on the walls round the outside")
            .build();
        auto.set_value(true);
        sizer.add(&auto, 0, SizerFlag::All, 10);
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
        if dialog.show_modal() == ID_OK
            && let Some(s) = choice.get_selection().and_then(|i| schemes.get(i as usize))
        {
            match self.run(
                "areaplan.create",
                json!({ "scheme_id": s.id, "level_ids": [plan.level], "auto_boundaries": auto.get_value() }),
            ) {
                Ok(v) => {
                    self.rebuild_browser();
                    if let Some(id) = v.get(0).and_then(serde_json::Value::as_u64) {
                        self.open_view(doc, Some(ElementId(id)));
                    }
                }
                Err(e) => super::error(&frame, &format!("No area plan:\n{e}")),
            }
        }
        dialog.destroy();
    }

    /// Room & Area › Area Boundary: the selected walls, placed by the scheme's rules.
    pub(crate) fn area_boundaries(&mut self) {
        let frame = self.frame;
        let Some(index) = self
            .active
            .and_then(|id| self.views.iter().position(|v| v.id == id))
        else {
            return;
        };
        let Some((view, _, _)) = self.area_plan_of(index) else {
            return frame.set_status_text("Open an area plan first", 0);
        };
        let Some(store) = self.store_now() else {
            return;
        };
        let walls: Vec<ElementId> = self
            .selection()
            .into_iter()
            .filter(|id| {
                store
                    .get(*id)
                    .and_then(reviera_core::walls::wall_recipe)
                    .is_some()
            })
            .collect();
        if walls.is_empty() {
            return frame.set_status_text("Select the walls to pick as area boundaries", 0);
        }
        match self.run(
            "areaboundary.create",
            json!({ "view_id": view, "wall_ids": walls, "apply_rules": true }),
        ) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text("Area boundaries added (they follow the walls)", 0);
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
    }

    /// Room & Area › Area: choose the type, then click inside boundaries.
    pub(crate) fn start_area_tool(&mut self) {
        let frame = self.frame;
        let Some(index) = self
            .active
            .and_then(|id| self.views.iter().position(|v| v.id == id))
        else {
            return;
        };
        let Some((view, scheme, _)) = self.area_plan_of(index) else {
            return frame.set_status_text("Open an area plan first", 0);
        };
        let Some(store) = self.store_now() else {
            return;
        };
        let Some(s) = reviera_core::areas::scheme(&store, &scheme) else {
            return;
        };
        let labels: Vec<&str> = s.types.iter().map(String::as_str).collect();
        let dialog = SingleChoiceDialog::builder(&frame, "Area type:", "Area", &labels).build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let Some(t) = usize::try_from(dialog.get_selection())
            .ok()
            .and_then(|i| s.types.get(i).cloned())
        else {
            return;
        };
        self.area_pick = Some((view, t));
        frame.set_status_text("Click inside area boundaries to place areas (Esc ends)", 0);
    }

    pub(crate) fn area_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        let Some((view, t)) = self.area_pick.clone() else {
            return;
        };
        let Some(p) = self.plan_xy(x, y) else { return };
        match self.run(
            "area.place",
            json!({ "view_id": view, "point": p, "area_type": t }),
        ) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text("Area placed; click another, or Esc", 0);
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
    }

    /// Room & Area › Color Schemes.
    pub(crate) fn color_scheme_dialog(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else {
            return;
        };
        let view = self
            .active_view()
            .filter(|v| v.plan.is_some())
            .and_then(|v| v.element);
        let schemes = color_schemes::all(&store);
        let current = view
            .and_then(|v| store.get(v))
            .and_then(reviera_core::views::view_data)
            .and_then(|d| d.color_scheme);
        let dialog = Dialog::builder(&frame, "Edit Color Scheme")
            .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
            .build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let grid = FlexGridSizer::builder(0, 2)
            .with_vgap(6)
            .with_hgap(10)
            .build();
        grid.add_growable_col(1, 1);
        let mut names: Vec<String> = schemes.iter().map(|s| s.name.clone()).collect();
        names.push("New scheme…".into());
        let l = StaticText::builder(&dialog).with_label("Scheme").build();
        let pick = Choice::builder(&dialog).with_choices(names.clone()).build();
        let start = current
            .as_ref()
            .and_then(|c| schemes.iter().position(|s| &s.id == c))
            .unwrap_or(if schemes.is_empty() {
                names.len() - 1
            } else {
                0
            });
        pick.set_selection(start as u32);
        grid.add(&l, 0, SizerFlag::AlignCenterVertical, 0);
        grid.add(&pick, 1, SizerFlag::Expand, 0);
        let field = |label: &str, value: &str| {
            let l = StaticText::builder(&dialog).with_label(label).build();
            let t = TextCtrl::builder(&dialog).with_value(value).build();
            grid.add(&l, 0, SizerFlag::AlignCenterVertical, 0);
            grid.add(&t, 1, SizerFlag::Expand, 0);
            t
        };
        let first = schemes.get(start);
        let name = field("New scheme's name", "Scheme 1");
        let parameter = field("Color by", first.map_or("Name", |s| s.parameter.as_str()));
        let l = StaticText::builder(&dialog).with_label("Category").build();
        let category = Choice::builder(&dialog)
            .with_choices(vec!["Rooms".into(), "Areas".into()])
            .build();
        let in_area_plan = view
            .and_then(|v| store.get(v))
            .and_then(reviera_core::views::view_data)
            .is_some_and(|d| d.area_scheme.is_some());
        category.set_selection(u32::from(match first {
            Some(s) => s.category == "cat.areas",
            None => in_area_plan,
        }));
        grid.add(&l, 0, SizerFlag::AlignCenterVertical, 0);
        grid.add(&category, 1, SizerFlag::Expand, 0);
        let l = StaticText::builder(&dialog).with_label("Mode").build();
        let mode = Choice::builder(&dialog)
            .with_choices(vec!["By value".into(), "By range".into()])
            .build();
        mode.set_selection(u32::from(first.is_some_and(|s| s.mode == Mode::Range)));
        grid.add(&l, 0, SizerFlag::AlignCenterVertical, 0);
        grid.add(&mode, 1, SizerFlag::Expand, 0);
        sizer.add_sizer(&grid, 0, SizerFlag::All | SizerFlag::Expand, 10);
        let note = StaticText::builder(&dialog)
            .with_label(
                "Rows: Value (by value) or At least (by range, in m² or m), Color #rrggbb, Visible. \
                 Values with no row are coloured from the palette.",
            )
            .build();
        note.wrap(460);
        sizer.add(&note, 0, SizerFlag::Left | SizerFlag::Right, 10);
        let rows = Grid::builder(&dialog).build();
        rows.create_grid(0, 3, GridSelectionMode::Cells);
        for (i, h) in ["Value / At least", "Color", "Visible"].iter().enumerate() {
            rows.set_col_label_value(i as i32, h);
        }
        rows.set_col_format_bool(2);
        rows.set_col_size(0, 200);
        rows.set_col_size(1, 100);
        rows.set_min_size(Size::new(460, 220));
        let fill_rows = move |store: &reviera_core::Store,
                              s: Option<&color_schemes::ColorScheme>| {
            let n = rows.get_number_rows();
            if n > 0 {
                rows.delete_rows(0, n, true);
            }
            let Some(s) = s else { return };
            let now = color_schemes::rows_now(store, s);
            rows.append_rows(now.len() as i32, true);
            for (k, r) in now.iter().enumerate() {
                let k = k as i32;
                let v = r
                    .value
                    .clone()
                    .or_else(|| r.at_least.map(|a| a.to_string()))
                    .unwrap_or_default();
                rows.set_cell_value(k, 0, &v);
                rows.set_cell_value(k, 1, &r.color);
                rows.set_cell_value(k, 2, if r.visible { "1" } else { "" });
            }
        };
        fill_rows(&store, first);
        sizer.add(&rows, 1, SizerFlag::All | SizerFlag::Expand, 10);
        let add = Button::builder(&dialog).with_label("Add Row").build();
        sizer.add(&add, 0, SizerFlag::Left, 10);
        add.on_click(move |_| {
            let k = rows.get_number_rows();
            rows.append_rows(1, true);
            rows.set_cell_value(k, 1, &color_schemes::palette(k as usize));
            rows.set_cell_value(k, 2, "1");
        });
        let show = CheckBox::builder(&dialog)
            .with_label("Show in this plan")
            .build();
        show.set_value(view.is_some());
        show.enable(view.is_some());
        sizer.add(&show, 0, SizerFlag::All, 10);
        let store2 = store.clone();
        let schemes2 = schemes.clone();
        pick.on_selection_changed(move |_| {
            let i = pick.get_selection().unwrap_or(0) as usize;
            fill_rows(&store2, schemes2.get(i));
        });
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
        loop {
            if dialog.show_modal() != ID_OK {
                break;
            }
            rows.save_edit_control_value();
            let i = pick.get_selection().unwrap_or(0) as usize;
            let by_range = mode.get_selection() == Some(1);
            let scheme_id = match schemes.get(i) {
                Some(s) => Ok(s.id.clone()),
                None => self
                    .run(
                        "colorscheme.create",
                        json!({
                            "name": name.get_value(),
                            "parameter": parameter.get_value(),
                            "category": if category.get_selection() == Some(1) { "cat.areas" } else { "cat.rooms" },
                            "mode": if by_range { "range" } else { "value" },
                        }),
                    )
                    .map(|v| v.as_str().unwrap_or_default().to_owned()),
            };
            let mut list = Vec::new();
            for k in 0..rows.get_number_rows() {
                let v = rows.get_cell_value(k, 0).trim().to_owned();
                if v.is_empty() {
                    continue;
                }
                let color = rows.get_cell_value(k, 1).trim().to_owned();
                let visible = rows.get_cell_value(k, 2).trim() == "1";
                list.push(if by_range {
                    json!({ "at_least": v.parse::<f64>().unwrap_or(0.0), "color": color, "visible": visible })
                } else {
                    json!({ "value": v, "color": color, "visible": visible })
                });
            }
            let result = scheme_id.and_then(|id| {
                self.run(
                    "colorscheme.set_rows",
                    json!({
                        "scheme_id": id, "rows": list, "parameter": parameter.get_value(),
                        "mode": if by_range { "range" } else { "value" },
                    }),
                )?;
                match view {
                    Some(v) if show.get_value() => self
                        .run(
                            "view.set_color_scheme",
                            json!({ "view_id": v, "scheme_id": id }),
                        )
                        .map(|_| ()),
                    Some(v) => self
                        .run("view.set_color_scheme", json!({ "view_id": v }))
                        .map(|_| ()),
                    None => Ok(()),
                }
            });
            match result {
                Ok(()) => {
                    self.refresh_scene(&frame);
                    break;
                }
                Err(e) => super::error(&frame, &format!("The scheme was not changed:\n{e}")),
            }
        }
        dialog.destroy();
    }

    /// Room separation lines on a plan's level (ARC-ROOM-001).
    pub(crate) fn separator_lines(&mut self, index: usize) -> Vec<Lines> {
        let v = &self.views[index];
        let Some(plan) = v.plan.clone() else {
            return Vec::new();
        };
        let Some(store) = self.app.document(v.doc).map(|d| d.doc.store().clone()) else {
            return Vec::new();
        };
        let Some(level) = store.get(plan.level).map(|l| l.guid().to_string()) else {
            return Vec::new();
        };
        let z = plan.range.cut as f32 + 0.003;
        let mut lines = Lines {
            color: [0.2, 0.2, 0.2, 1.0],
            width: 0.13,
            segments: Vec::new(),
        };
        // Space separation lines in the spaces' blue.
        let mut space = Lines {
            color: crate::spaces_ui::SPACE_COLOR,
            width: 0.13,
            segments: Vec::new(),
        };
        for e in store.iter() {
            let Some(r) = reviera_core::room_separators::recipe(e) else {
                continue;
            };
            if r.level != level {
                continue;
            }
            let target = if reviera_core::room_separators::is_space(&r) {
                &mut space
            } else {
                &mut lines
            };
            for w in reviera_core::room_separators::points(&r).windows(2) {
                target.segments.push([
                    [w[0][0] as f32, w[0][1] as f32, z],
                    [w[1][0] as f32, w[1][1] as f32, z],
                ]);
            }
        }
        vec![lines, space]
    }
}
