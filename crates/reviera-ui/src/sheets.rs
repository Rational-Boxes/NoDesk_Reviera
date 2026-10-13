// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Sheets in the window (DOC-SHT-001…003, DOC-SCH-011):
//! - **Sheet views.** A sheet draws on white paper in paper metres: its title block, its
//!   viewports' drawings at their scale, and placed schedules.
//! - **Viewports.** A click selects a viewport or schedule; dragging moves it. Delete removes it
//!   from the sheet.
//! - **Commands.** New Sheet, Place View and Place Schedule.

use reviera_app::sheets::{ItemKind, SheetItem};
use reviera_core::ElementId;
use reviera_core::sheets::{self, SIZES};
use reviera_render::annotation::{Label, label_strokes};
use reviera_render::plan::Lines;
use reviera_render::{RenderBackend, RenderScene, SceneMesh};
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

const PAPER: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const DESK: [f32; 4] = [0.32, 0.33, 0.35, 1.0];
const SHADOW: [f32; 4] = [0.12, 0.12, 0.13, 1.0];
const SELECTED: [f32; 4] = [0.1, 0.45, 0.95, 1.0];

/// A sheet view's state.
#[derive(Debug, Clone, Default)]
pub struct SheetState {
    /// The paper's width and height, millimetres.
    pub paper: [f32; 2],
    /// Viewports and placed schedules as last drawn.
    pub items: Vec<SheetItem>,
    pub selected: Option<u32>,
    /// An item being dragged and how far it has moved (paper metres).
    pub moving: Option<(u32, [f32; 2])>,
    /// The drawing must be rebuilt.
    pub stale: bool,
}

/// Points `camera` straight down at a sheet's paper (millimetres), filling a view of `aspect`.
pub(crate) fn fit_paper(camera: &mut reviera_render::Camera, paper: [f32; 2], aspect: f32) {
    let (w, h) = (paper[0] / 1000.0, paper[1] / 1000.0);
    camera.perspective = false;
    camera.orient(glam::Vec3::Z);
    camera.target = glam::Vec3::new(w / 2.0, h / 2.0, 0.0);
    let half = (h / 2.0).max(w / 2.0 / aspect.max(0.1)) * 1.06;
    camera.distance = half / (camera.fov * 0.5).tan();
}

/// Text in the stroke font: lower-case letters draw as capitals there, so words are upper-cased;
/// units keep their case.
fn display(text: &str) -> String {
    text.split(' ')
        .map(|w| {
            if ["m", "mm", "cm", "m²", "m³"].contains(&w) {
                w.to_owned()
            } else {
                w.to_uppercase()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl Gui {
    /// The state of a sheet view element.
    pub(crate) fn sheet_info(
        &self,
        doc: reviera_app::DocumentId,
        element: Option<ElementId>,
    ) -> Option<SheetState> {
        let store = self.app.document(doc)?.doc.store();
        let data = sheets::sheet_data(store.get(element?)?)?;
        let (w, h) = data.paper();
        Some(SheetState {
            paper: [w as f32, h as f32],
            stale: true,
            ..SheetState::default()
        })
    }

    /// Whether the active view is a sheet.
    pub(crate) fn in_sheet(&self) -> bool {
        self.active_view().is_some_and(|v| v.sheet.is_some())
    }

    /// The paper of a sheet view on a grey desk, as a scene.
    fn paper_scene(paper: [f32; 2]) -> RenderScene {
        let (w, h) = (paper[0] / 1000.0, paper[1] / 1000.0);
        let rect = |x0: f32, y0: f32, x1: f32, y1: f32, z: f32, color: [f32; 4]| SceneMesh {
            element: None,
            class: "rxd:paper".into(),
            positions: vec![[x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z]],
            indices: vec![0, 1, 2, 0, 2, 3],
            color,
        };
        let m = 10.0 * w.max(h);
        RenderScene {
            meshes: vec![
                rect(-m, -m, w + m, h + m, -0.003, DESK),
                rect(0.003, -0.003, w + 0.003, h - 0.003, -0.002, SHADOW),
                rect(0.0, 0.0, w, h, -0.001, PAPER),
            ],
        }
    }

    /// Rebuilds a sheet view's drawing when stale, and draws its selection.
    pub(crate) fn update_sheet_graphics(&mut self, index: usize) {
        let v = &self.views[index];
        let Some(s) = &v.sheet else { return };
        if v.viewport.is_none() {
            return;
        }
        if s.stale {
            let (doc, element) = (v.doc, v.element);
            let drawn = element.map(|e| self.app.sheet_drawing(doc, e));
            let v = &mut self.views[index];
            let s = v.sheet.as_mut().expect("checked");
            s.stale = false;
            match drawn {
                Some(Ok(d)) => {
                    if let Some(p) = &d.drawing.paper {
                        s.paper = [p.width, p.height];
                    }
                    s.items = d.items;
                    if s.selected
                        .is_some_and(|id| !s.items.iter().any(|i| i.id == id))
                    {
                        s.selected = None;
                    }
                    let mut graphics = d.drawing.graphics;
                    let labels: Vec<Label> = d
                        .drawing
                        .labels
                        .into_iter()
                        .map(|l| Label {
                            text: display(&l.text),
                            ..l
                        })
                        .collect();
                    graphics.lines.push(Lines {
                        color: [0.0, 0.0, 0.0, 1.0],
                        width: 0.18,
                        segments: label_strokes(&labels),
                    });
                    let scene = Self::paper_scene(s.paper);
                    if let Some(vp) = &mut v.viewport {
                        vp.set_scene(&scene);
                        vp.set_highlight(&[]);
                        vp.set_plan(Some(graphics));
                    }
                }
                Some(Err(e)) => self
                    .frame
                    .set_status_text(&format!("The sheet cannot be drawn: {e}"), 0),
                None => {}
            }
        }
        // The selected or moving item's outline.
        let v = &mut self.views[index];
        let s = v.sheet.as_ref().expect("checked");
        let mut boxes = Vec::new();
        for item in &s.items {
            let offset = match s.moving {
                Some((id, d)) if id == item.id => Some(d),
                _ if s.selected == Some(item.id) => Some([0.0, 0.0]),
                _ => None,
            };
            if let Some(d) = offset {
                let (x0, y0) = (item.min[0] + d[0] - 0.002, item.min[1] + d[1] - 0.002);
                let (x1, y1) = (item.max[0] + d[0] + 0.002, item.max[1] + d[1] + 0.002);
                let p = |x: f32, y: f32| [x, y, 0.0];
                boxes.extend([
                    [p(x0, y0), p(x1, y0)],
                    [p(x1, y0), p(x1, y1)],
                    [p(x1, y1), p(x0, y1)],
                    [p(x0, y1), p(x0, y0)],
                ]);
            }
        }
        if let Some(vp) = &mut v.viewport {
            vp.set_overlay_lines(&[Lines {
                color: SELECTED,
                width: 0.35,
                segments: boxes,
            }]);
        }
    }

    /// Marks every sheet view of the model stale (the model or a sheet changed).
    pub(crate) fn sheets_stale(&mut self) {
        for v in &mut self.views {
            if let Some(s) = &mut v.sheet {
                s.stale = true;
                v.panel.refresh(false, None);
            }
        }
    }

    /// The viewport or schedule under pixel (`x`, `y`) of the active sheet view.
    fn sheet_item_at(&self, x: i32, y: i32) -> Option<u32> {
        let s = self.active_view()?.sheet.as_ref()?;
        let p = self.plan_xy(x, y)?;
        let (px, py) = (p[0] as f32, p[1] as f32);
        // Schedules are drawn last, so they are on top.
        s.items
            .iter()
            .rev()
            .find(|i| px >= i.min[0] && px <= i.max[0] && py >= i.min[1] && py <= i.max[1])
            .map(|i| i.id)
    }

    /// A press on a sheet: the item under the cursor starts to drag.
    pub(crate) fn sheet_drag_start(&mut self, x: i32, y: i32) -> Option<(u32, [f64; 2])> {
        if !self.in_sheet() {
            return None;
        }
        let item = self.sheet_item_at(x, y)?;
        let from = self.plan_xy(x, y)?;
        Some((item, from))
    }

    pub(crate) fn sheet_drag_motion(&mut self, item: u32, from: [f64; 2], x: i32, y: i32) {
        let Some(p) = self.plan_xy(x, y) else { return };
        if let Some(s) = self.active_view_mut().and_then(|v| v.sheet.as_mut()) {
            s.moving = Some((item, [(p[0] - from[0]) as f32, (p[1] - from[1]) as f32]));
        }
    }

    /// The end of a press on a sheet item: a click selects it, a drag moves it.
    pub(crate) fn sheet_drag_end(
        &mut self,
        item: u32,
        from: [f64; 2],
        x: i32,
        y: i32,
        click: bool,
    ) {
        let to = self.plan_xy(x, y);
        let Some(v) = self.active_view_mut() else {
            return;
        };
        let sheet = v.element;
        let Some(s) = v.sheet.as_mut() else { return };
        s.moving = None;
        s.selected = Some(item);
        let position = s.items.iter().find(|i| i.id == item).map(|i| i.position);
        v.panel.refresh(false, None);
        self.properties_stale = true;
        self.update_ribbon_context();
        if click {
            return;
        }
        let (Some(sheet), Some(to), Some(at)) = (sheet, to, position) else {
            return;
        };
        let to = [
            at[0] + (to[0] - from[0]) * 1000.0,
            at[1] + (to[1] - from[1]) * 1000.0,
        ];
        let args = json!({ "sheet_id": sheet, "viewport_id": item, "position": to });
        if let Err(e) = self.run("viewport.move", args) {
            error(&self.frame, &format!("The viewport was not moved:\n{e}"));
        }
        self.sheets_stale();
    }

    /// A click on an empty part of a sheet clears its selection.
    pub(crate) fn sheet_click(&mut self, x: i32, y: i32) {
        let item = self.sheet_item_at(x, y);
        if let Some(v) = self.active_view_mut()
            && let Some(s) = v.sheet.as_mut()
        {
            s.selected = item;
            v.panel.refresh(false, None);
        }
        self.properties_stale = true;
        self.update_ribbon_context();
    }

    /// The active sheet, its selected item and that item's kind and element.
    fn sheet_selection(&self) -> Option<(ElementId, Option<SheetItem>)> {
        let v = self.active_view()?;
        let s = v.sheet.as_ref()?;
        let item = s
            .selected
            .and_then(|id| s.items.iter().find(|i| i.id == id).cloned());
        Some((v.element?, item))
    }

    /// Whether a sheet item is selected in the active view.
    pub(crate) fn sheet_item_selected(&self) -> bool {
        self.sheet_selection().is_some_and(|(_, i)| i.is_some())
    }

    /// Delete on a sheet: removes the selected viewport or schedule from it. `false` when the
    /// active view is not a sheet with a selected item.
    pub(crate) fn sheet_delete(&mut self) -> bool {
        let Some((sheet, Some(item))) = self.sheet_selection() else {
            return false;
        };
        let args = json!({ "sheet_id": sheet, "viewport_ids": [item.id] });
        if let Err(e) = self.run("viewport.remove", args) {
            error(&self.frame, &format!("The viewport was not removed:\n{e}"));
        }
        self.sheets_stale();
        self.browser_stale = true;
        self.properties_stale = true;
        self.update_ribbon_context();
        true
    }

    /// Sheet › Open View: the selected viewport's view.
    pub(crate) fn open_sheet_view(&mut self) {
        match self.sheet_selection() {
            Some((_, Some(item)))
                if item.kind == ItemKind::Viewport || item.kind == ItemKind::Schedule =>
            {
                self.open_model_view(item.element);
            }
            _ => self
                .frame
                .set_status_text("Select a viewport on the sheet to open its view", 0),
        }
    }

    /// View › Sheet: a new sheet of a chosen size, opened.
    pub(crate) fn new_sheet(&mut self) {
        let frame = self.frame;
        let Some(doc) = self.doc else { return };
        let names: Vec<String> = SIZES
            .iter()
            .map(|(n, w, h)| format!("ISO {n} ({w} × {h} mm)"))
            .collect();
        let labels: Vec<&str> = names.iter().map(String::as_str).collect();
        let dialog =
            SingleChoiceDialog::builder(&frame, "Title block:", "New Sheet", &labels).build();
        dialog.set_selection(1);
        if dialog.show_modal() != ID_OK {
            return;
        }
        let Some(size) = usize::try_from(dialog.get_selection())
            .ok()
            .and_then(|i| SIZES.get(i))
        else {
            return;
        };
        match self.run("sheet.create", json!({ "size": size.0 })) {
            Ok(v) => {
                self.browser_stale = true;
                self.open_view(doc, v.as_u64().map(ElementId));
            }
            Err(e) => error(&frame, &format!("The sheet was not created:\n{e}")),
        }
    }

    /// Sheet › Place View: a plan, section or elevation not yet on a sheet.
    pub(crate) fn place_view_on_sheet(&mut self) {
        let frame = self.frame;
        let Some((sheet, _)) = self.sheet_selection() else {
            return frame.set_status_text("Open a sheet to place views on it", 0);
        };
        let Some(store) = self
            .doc
            .and_then(|d| self.app.document(d))
            .map(|d| d.doc.store().clone())
        else {
            return;
        };
        let mut views: Vec<(ElementId, String)> = store
            .iter()
            .filter(|e| {
                reviera_core::views::view_data(e).is_some_and(|d| {
                    reviera_core::plans::is_plan(&d.kind)
                        || reviera_core::sections::is_section(&d.kind)
                }) && sheets::sheet_of_view(&store, e.guid()).is_none()
            })
            .map(|e| (e.id(), e.name.clone().unwrap_or_default()))
            .collect();
        views.sort_by(|a, b| reviera_core::schedules::natural(&a.1, &b.1));
        if views.is_empty() {
            return error(
                &frame,
                "Every plan, section and elevation is already on a sheet (a view can be on one sheet only).",
            );
        }
        let labels: Vec<&str> = views.iter().map(|v| v.1.as_str()).collect();
        let dialog = SingleChoiceDialog::builder(&frame, "View:", "Place View", &labels).build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let Some((view, _)) = usize::try_from(dialog.get_selection())
            .ok()
            .and_then(|i| views.get(i))
        else {
            return;
        };
        match self.run(
            "sheet.place_view",
            json!({ "sheet_id": sheet, "view_id": view }),
        ) {
            Ok(id) => self.select_placed(id.as_u64().map(|i| i as u32)),
            Err(e) => error(&frame, &format!("The view was not placed:\n{e}")),
        }
    }

    /// Sheet › Place Schedule.
    pub(crate) fn place_schedule_on_sheet(&mut self) {
        let frame = self.frame;
        let Some((sheet, _)) = self.sheet_selection() else {
            return frame.set_status_text("Open a sheet to place schedules on it", 0);
        };
        let Some(store) = self
            .doc
            .and_then(|d| self.app.document(d))
            .map(|d| d.doc.store().clone())
        else {
            return;
        };
        let mut list: Vec<(ElementId, String)> = store
            .iter()
            .filter(|e| reviera_core::schedules::is_schedule(e))
            .map(|e| (e.id(), e.name.clone().unwrap_or_default()))
            .collect();
        list.sort_by(|a, b| reviera_core::schedules::natural(&a.1, &b.1));
        if list.is_empty() {
            return error(&frame, "The model has no schedules yet (View › Schedules).");
        }
        let labels: Vec<&str> = list.iter().map(|v| v.1.as_str()).collect();
        let dialog =
            SingleChoiceDialog::builder(&frame, "Schedule:", "Place Schedule", &labels).build();
        if dialog.show_modal() != ID_OK {
            return;
        }
        let Some((schedule, _)) = usize::try_from(dialog.get_selection())
            .ok()
            .and_then(|i| list.get(i))
        else {
            return;
        };
        let args = json!({ "sheet_id": sheet, "schedule_id": schedule });
        match self.run("sheet.place_schedule", args) {
            Ok(id) => self.select_placed(id.as_u64().map(|i| i as u32)),
            Err(e) => error(&frame, &format!("The schedule was not placed:\n{e}")),
        }
    }

    /// After placing: the new item is selected and the sheet redrawn.
    fn select_placed(&mut self, item: Option<u32>) {
        if let Some(s) = self.active_view_mut().and_then(|v| v.sheet.as_mut()) {
            s.selected = item;
        }
        self.sheets_stale();
        self.browser_stale = true;
        self.properties_stale = true;
        self.update_ribbon_context();
    }

    /// The active sheet's rows in the Properties palette; `false` when it is not a sheet.
    pub(crate) fn sheet_view_rows(&mut self, grid: &PropertyGrid) -> bool {
        let Some(v) = self.active_view().filter(|v| v.sheet.is_some()) else {
            return false;
        };
        let Some(e) = v
            .element
            .and_then(|e| self.app.document(v.doc)?.doc.store().get(e).cloned())
        else {
            return false;
        };
        let Some(data) = sheets::sheet_data(&e) else {
            return false;
        };
        let selected = v
            .sheet
            .as_ref()
            .and_then(|s| s.selected)
            .and_then(|id| data.viewports.iter().find(|vp| vp.id == id).cloned());
        if let Some(vp) = selected {
            grid.append(Property::category("Viewport", "c_viewport"));
            let name = reviera_core::GlobalId::parse(&vp.view)
                .and_then(|g| self.app.document(v.doc)?.doc.store().find_by_guid(&g))
                .and_then(|id| self.app.document(v.doc)?.doc.store().get(id)?.name.clone())
                .unwrap_or_default();
            grid.append(Property::string("View Name", "vp_view", name));
            grid.set_property_read_only("vp_view", true);
            grid.append(Property::string(
                "Detail Number",
                "vp_detail",
                vp.detail_number,
            ));
            grid.set_property_read_only("vp_detail", true);
        }
        grid.append(Property::category("Sheet", "c_sheet"));
        grid.append(Property::string(
            "Sheet Number",
            "sht_number",
            data.number.clone(),
        ));
        grid.append(Property::string(
            "Sheet Name",
            "sht_name",
            e.name.clone().unwrap_or_default(),
        ));
        let sizes: Vec<(String, i32)> = SIZES
            .iter()
            .enumerate()
            .map(|(i, s)| (s.0.to_owned(), i as i32))
            .collect();
        let index = SIZES
            .iter()
            .position(|s| s.0 == data.size)
            .map_or(-1, |i| i as i32);
        grid.append(Property::enumeration("Size", "sht_size", sizes, index));
        grid.append(Property::category("Title Block", "c_title"));
        for (i, k) in sheets::PARAMETERS.iter().enumerate() {
            let key = format!("sht_p{i}");
            let value = data.parameters.get(*k).cloned().unwrap_or_default();
            grid.append(Property::string(*k, &key, value));
        }
        grid.append(Property::string("ID", "v0", e.id().to_string()));
        grid.set_property_read_only("v0", true);
        true
    }

    /// An edit of a sheet row; `false` if `property` is not one.
    pub(crate) fn sheet_view_edit(&mut self, property: &str, text: Option<String>) -> bool {
        let Some(sheet) = self
            .active_view()
            .filter(|v| v.sheet.is_some())
            .and_then(|v| v.element)
        else {
            return false;
        };
        let text = text.unwrap_or_default();
        let args = match property {
            "sht_number" => json!({ "sheet_id": sheet, "number": text }),
            "sht_name" => json!({ "sheet_id": sheet, "name": text }),
            "sht_size" => json!({ "sheet_id": sheet, "size": text }),
            p => match p
                .strip_prefix("sht_p")
                .and_then(|i| i.parse::<usize>().ok())
                .and_then(|i| sheets::PARAMETERS.get(i))
            {
                Some(k) => json!({ "sheet_id": sheet, "parameters": { *k: text } }),
                None => return false,
            },
        };
        if let Err(e) = self.run("sheet.set_parameter", args) {
            error(&self.frame, &format!("The sheet was not changed:\n{e}"));
        }
        if property == "sht_size"
            && let Some(paper) = self
                .active_view()
                .and_then(|v| self.sheet_info(v.doc, v.element))
            && let Some(s) = self.active_view_mut().and_then(|v| v.sheet.as_mut())
        {
            s.paper = paper.paper;
        }
        self.sheets_stale();
        self.update_captions();
        self.browser_stale = true;
        self.properties_stale = true;
        true
    }
}
