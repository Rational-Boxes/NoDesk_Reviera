// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Legend views in the window (DOC-VIEW-010): View › Legend makes one and opens it; Annotate ›
//! Legend Component places a type's drawing in it (choose the type and direction, then click
//! where its lower-left corner goes). The legend draws like a sheet, flat, with its components
//! drawn by the application (`reviera_app::legends`) and its text notes drawn and picked as in
//! plans; a component is picked inside its drawing's extent, outlined when selected, and edited
//! in Properties (direction, host length).

use reviera_core::ElementId;
use reviera_core::legends::{self, Direction, TypeKind};
use reviera_render::plan::Lines;
use reviera_render::{RenderBackend, RenderScene};
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

const SELECTED: [f32; 4] = [0.1, 0.45, 0.95, 1.0];

/// A legend view's state.
#[derive(Debug, Clone, Default)]
pub struct LegendState {
    pub scale: f32,
    /// The components' drawing must be rebuilt (the model changed).
    pub stale: bool,
    /// The notes and selection must be redrawn.
    pub redraw: bool,
    /// The components' drawing as last built.
    pub base: Option<std::rc::Rc<reviera_render::plan::PlanGraphics>>,
    /// Each component's extent, for picking.
    pub items: Vec<reviera_app::legends::Item>,
}

/// The Legend Component tool: the type and direction chosen.
#[derive(Debug, Clone)]
pub struct ComponentTool {
    pub type_id: ElementId,
    pub direction: Direction,
}

impl Gui {
    /// The state of a legend view element.
    pub(crate) fn legend_info(&self, doc: reviera_app::DocumentId, element: Option<ElementId>) -> Option<LegendState> {
        let e = self.app.document(doc)?.doc.store().get(element?)?;
        if !legends::is_legend(e) {
            return None;
        }
        let scale = e.param(reviera_core::views::VIEW_KEY).and_then(|v| v.get("scale")?.as_f64()).unwrap_or(50.0) as f32;
        Some(LegendState { scale, stale: true, redraw: true, ..LegendState::default() })
    }

    /// Rebuilds a legend view's drawing when stale, and draws its notes and selection.
    pub(crate) fn update_legend_graphics(&mut self, index: usize) {
        let v = &self.views[index];
        let Some(l) = &v.legend else { return };
        if v.viewport.is_none() || !(l.stale || l.redraw) {
            return;
        }
        let selected: Vec<ElementId> = if Some(v.doc) == self.doc { self.selection() } else { Vec::new() };
        let v = &self.views[index];
        let (doc, element) = (v.doc, v.element);
        if v.legend.as_ref().is_some_and(|l| l.stale || l.base.is_none()) {
            let drawn = (|| {
                let store = self.app.document(doc)?.doc.store();
                let view = store.get(element?)?;
                Some(reviera_app::legends::legend_drawing(store, view))
            })();
            match drawn {
                Some(Ok(d)) => {
                    if let Some((id, why)) = d.failed.first() {
                        self.frame.set_status_text(&format!("Legend component {id} cannot be drawn: {why}"), 0);
                    }
                    let v = &mut self.views[index];
                    if let Some(vp) = &mut v.viewport {
                        vp.set_scene(&RenderScene::default());
                    }
                    if let Some(l) = &mut v.legend {
                        l.base = Some(std::rc::Rc::new(d.drawing.graphics));
                        l.items = d.items;
                        l.stale = false;
                    }
                }
                Some(Err(e)) => self.frame.set_status_text(&format!("The legend cannot be drawn: {e}"), 0),
                None => {}
            }
        }
        let Some(base) = self.views[index].legend.as_ref().and_then(|l| l.base.clone()) else { return };
        let mut graphics = (*base).clone();
        graphics.lines.extend(self.note_lines(index));
        // Selected components outlined.
        let items = self.views[index].legend.as_ref().map(|l| l.items.clone()).unwrap_or_default();
        let mut boxes = Vec::new();
        for i in items.iter().filter(|i| selected.contains(&i.id)) {
            let pad = 0.05;
            let (x0, y0, x1, y1) = ((i.lo[0] - pad) as f32, (i.lo[1] - pad) as f32, (i.hi[0] + pad) as f32, (i.hi[1] + pad) as f32);
            let p = |x: f32, y: f32| [x, y, 0.004];
            boxes.extend([[p(x0, y0), p(x1, y0)], [p(x1, y0), p(x1, y1)], [p(x1, y1), p(x0, y1)], [p(x0, y1), p(x0, y0)]]);
        }
        graphics.lines.push(Lines { color: SELECTED, width: 0.35, segments: boxes });
        let v = &mut self.views[index];
        if let Some(vp) = &mut v.viewport {
            vp.set_highlight(&[]);
            vp.set_plan(Some(graphics));
        }
        if let Some(l) = &mut v.legend {
            l.redraw = false;
        }
    }

    /// The camera box of a legend: its components' extents, or a 10 × 6 m page.
    pub(crate) fn legend_bounds(&self, doc: reviera_app::DocumentId, element: Option<ElementId>) -> (glam::Vec3, glam::Vec3) {
        let items = (|| {
            let store = self.app.document(doc)?.doc.store();
            let view = store.get(element?)?;
            reviera_app::legends::legend_drawing(store, view).ok().map(|d| d.items)
        })()
        .unwrap_or_default();
        let lo = items.iter().fold([0.0f64, 0.0], |a, i| [a[0].min(i.lo[0]), a[1].min(i.lo[1])]);
        let hi = items.iter().fold([10.0f64, 6.0], |a, i| [a[0].max(i.hi[0]), a[1].max(i.hi[1])]);
        (glam::Vec3::new(lo[0] as f32 - 1.0, lo[1] as f32 - 1.0, -0.1), glam::Vec3::new(hi[0] as f32 + 1.0, hi[1] as f32 + 1.0, 0.1))
    }

    /// The legend component under pixel (`x`, `y`) of the active legend.
    pub(crate) fn legend_component_at(&self, x: i32, y: i32) -> Option<ElementId> {
        let v = self.active_view()?;
        let l = v.legend.as_ref()?;
        let p = self.plan_xy(x, y)?;
        l.items.iter().find(|i| p[0] >= i.lo[0] && p[0] <= i.hi[0] && p[1] >= i.lo[1] && p[1] <= i.hi[1]).map(|i| i.id)
    }

    /// View › Legend: a new legend at 1 : 50, opened.
    pub(crate) fn new_legend(&mut self) {
        let frame = self.frame;
        let Some(doc) = self.doc else { return };
        match self.run("view.create_legend", json!({})) {
            Ok(v) => {
                self.browser_stale = true;
                self.open_view(doc, v.as_u64().map(ElementId));
            }
            Err(e) => error(&frame, &format!("The legend was not created:\n{e}")),
        }
    }

    /// Annotate › Legend Component: choose a type and how to look at it, then click in a legend.
    pub(crate) fn start_legend_component(&mut self) {
        let frame = self.frame;
        if !self.active_view().is_some_and(|v| v.legend.is_some()) {
            return frame.set_status_text("Legend Component: open a legend (View › Legend)", 0);
        }
        let Some(store) = self.doc.and_then(|d| self.app.document(d)).map(|d| d.doc.store().clone()) else { return };
        let types = legends::drawable_types(&store);
        if types.is_empty() {
            return frame.set_status_text("Legend Component: the model has no wall, floor, door, window or component types", 0);
        }
        let kind_name = |k: TypeKind| match k {
            TypeKind::Wall => "Wall",
            TypeKind::Floor => "Floor",
            TypeKind::Door => "Door",
            TypeKind::Window => "Window",
            TypeKind::Component => "Component",
        };
        let labels: Vec<String> = types.iter().map(|(_, n, k)| format!("{}: {n}", kind_name(*k))).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let d = SingleChoiceDialog::builder(&frame, "Type:", "Legend Component", &refs).build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some(&(type_id, _, kind)) = usize::try_from(d.get_selection()).ok().and_then(|i| types.get(i)) else { return };
        let mut directions = vec![("Plan", Direction::Plan), ("Front elevation", Direction::Front), ("Back elevation", Direction::Back)];
        if kind != TypeKind::Component {
            directions.push(("Section", Direction::Section));
        }
        let names: Vec<&str> = directions.iter().map(|d| d.0).collect();
        let d = SingleChoiceDialog::builder(&frame, "View direction:", "Legend Component", &names).build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some(&(_, direction)) = usize::try_from(d.get_selection()).ok().and_then(|i| directions.get(i)) else { return };
        let f = self.frame;
        self.end_tool(&f);
        self.legend_tool = Some(ComponentTool { type_id, direction });
        frame.set_status_text("Legend Component: click where its lower-left corner goes; Esc ends", 0);
    }

    pub(crate) fn end_legend_tool(&mut self) -> bool {
        self.legend_tool.take().is_some()
    }

    /// A click with the Legend Component tool.
    pub(crate) fn legend_click(&mut self, x: i32, y: i32) -> bool {
        let Some(t) = self.legend_tool.clone() else { return false };
        let (Some(view), Some(p)) = (self.active_view().filter(|v| v.legend.is_some()).and_then(|v| v.element), self.plan_xy(x, y)) else {
            return true;
        };
        let frame = self.frame;
        match self.run("legend.place_component", json!({ "view_id": view, "type_id": t.type_id, "direction": t.direction, "point": p })) {
            Ok(_) => {
                let f = self.frame;
                self.refresh_scene(&f);
                frame.set_status_text("Legend component placed; click for another, or Esc", 0);
            }
            Err(e) => error(&frame, &format!("Nothing was placed:\n{e}")),
        }
        true
    }

    /// Properties rows of the active legend view; `false` when it is not one.
    pub(crate) fn legend_view_rows(&mut self, grid: &PropertyGrid) -> bool {
        let Some(v) = self.active_view() else { return false };
        let (Some(element), Some(l)) = (v.element, v.legend.as_ref()) else { return false };
        let (name, scale, count) = (self.view_name(v), l.scale, l.items.len());
        grid.append(Property::category("Legend", "c_view"));
        grid.append(Property::string("View Name", "view_name", name));
        for (key, label, value) in [("lgv_scale", "Scale", format!("1 : {scale}")), ("lgv_count", "Components", count.to_string()), ("lgv_id", "ID", element.to_string())] {
            grid.append(Property::string(label, key, value));
            grid.set_property_read_only(key, true);
        }
        true
    }

    /// Properties rows of a selected legend component; `false` when it is not one.
    pub(crate) fn legend_rows(&mut self, grid: &PropertyGrid, id: ElementId) -> bool {
        let Some(store) = self.doc.and_then(|d| self.app.document(d)).map(|d| d.doc.store().clone()) else { return false };
        let Some(d) = store.get(id).and_then(legends::component_data) else { return false };
        grid.append(Property::category("Legend Component", "c_legend"));
        let t = reviera_core::GlobalId::parse(&d.type_guid).and_then(|g| store.find_by_guid(&g)).and_then(|t| store.get(t));
        grid.append(Property::string("Type", "lg_type", t.and_then(|t| t.name.clone()).unwrap_or_else(|| "(deleted)".into())));
        grid.set_property_read_only("lg_type", true);
        let component = t.and_then(legends::type_kind) == Some(TypeKind::Component);
        let mut directions = vec![("Plan", Direction::Plan), ("Front Elevation", Direction::Front), ("Back Elevation", Direction::Back)];
        if !component {
            directions.push(("Section", Direction::Section));
        }
        let index = directions.iter().position(|x| x.1 == d.direction).unwrap_or(0) as i32;
        grid.append(Property::enumeration("View Direction", "lg_dir", directions.iter().enumerate().map(|(i, x)| (x.0.to_owned(), i as i32)).collect::<Vec<(String, i32)>>(), index));
        if !component {
            let length = d.host_length.map(|l| self.format_length(l)).unwrap_or_default();
            grid.append(Property::string("Host Length", "lg_length", length));
            grid.set_help_string("lg_length", "The sample's length (empty: the default)");
        }
        true
    }

    /// An edit of a legend component's row; `false` if `property` is not one.
    pub(crate) fn legend_edit(&mut self, property: &str, text: Option<String>) -> bool {
        let frame = self.frame;
        let Some(&id) = self.selection().first() else { return false };
        let text = text.unwrap_or_default();
        let result = match property {
            "lg_dir" => {
                let d = match text.as_str() {
                    "Front Elevation" => "front",
                    "Back Elevation" => "back",
                    "Section" => "section",
                    _ => "plan",
                };
                self.run("legend.set_component", json!({ "component_ids": [id], "direction": d }))
            }
            "lg_length" => self.run("legend.set_component", json!({ "component_ids": [id], "host_length": text })),
            _ => return false,
        };
        if let Err(e) = result {
            error(&frame, &format!("The legend component was not changed:\n{e}"));
        }
        let f = self.frame;
        self.refresh_scene(&f);
        true
    }
}
