// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Section and elevation views in the window (Ch 5 §5.4–5.5, DOC-VIEW-003/004):
//! - **Drawing.** These views draw like plans: the model turned into the view's frame, cut at
//!   the view plane, in projection up to the far clip, with levels and grids.
//! - **Section tool.** Two clicks in a plan make a section.
//! - **Building Elevations.** Makes the four elevations around the model.
//! - **Markers.** Plans show section lines and elevation markers.

use std::rc::Rc;

use reviera_core::ElementId;
use reviera_core::sections::{self, SectionDef};
use reviera_render::annotation::{Label, label_strokes};
use reviera_render::plan::Lines;
use reviera_render::{RenderBackend, RenderScene};
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

/// Section lines and elevation markers in plans.
const MARKER: [f32; 4] = [0.05, 0.05, 0.05, 1.0];
const PREVIEW: [f32; 4] = [0.95, 0.45, 0.9, 1.0];

/// A section or elevation view's state.
#[derive(Debug, Clone)]
pub struct SectionState {
    pub def: SectionDef,
    pub elevation: bool,
    pub scale: f32,
    /// The drawing must be rebuilt (model or definition changed).
    pub stale: bool,
    /// The model in the view's frame, for picking.
    pub scene: Rc<RenderScene>,
    /// The model's drawing as last built, under the annotation (which is redrawn on its own, as
    /// the selection changes).
    pub base: Option<Rc<reviera_render::plan::PlanGraphics>>,
    /// The annotation must be redrawn.
    pub redraw: bool,
    /// The cut edges with their element, for dimension references.
    pub cut_edges: Rc<Vec<reviera_render::plan::CutEdge>>,
}

/// The Section tool: the first point, once clicked.
#[derive(Debug, Default)]
pub struct SectionTool {
    start: Option<[f64; 2]>,
}

impl Gui {
    /// The state of a section or elevation view element.
    pub(crate) fn section_info(
        &self,
        doc: reviera_app::DocumentId,
        element: Option<ElementId>,
    ) -> Option<SectionState> {
        let store = self.app.document(doc)?.doc.store();
        let e = store.get(element?)?;
        let (kind, def) = sections::section_of(e)?;
        Some(SectionState {
            def,
            elevation: kind == sections::KIND_ELEVATION,
            scale: e
                .param(reviera_core::views::VIEW_KEY)
                .and_then(|v| v.get("scale")?.as_f64())
                .unwrap_or(100.0) as f32,
            stale: true,
            scene: Rc::default(),
            base: None,
            redraw: true,
            cut_edges: Rc::default(),
        })
    }

    /// Rebuilds a section view's drawing when stale: its model in the view frame, cut and
    /// projection, levels and grids; and draws its dimensions, tags and text notes over it
    /// (selected ones highlighted) when they changed.
    pub(crate) fn update_section_graphics(&mut self, index: usize) {
        let v = &self.views[index];
        let Some(s) = &v.section else { return };
        if !(s.stale || s.redraw) || v.viewport.is_none() {
            return;
        }
        let doc = v.doc;
        let highlight = if Some(doc) == self.doc {
            self.selection()
        } else {
            Vec::new()
        };
        let v = &self.views[index];
        let Some(s) = &v.section else { return };
        if s.stale || s.base.is_none() {
            let visibility = self.view_visibility(v);
            let (Some(scene), Some(store)) = (
                self.scenes.get(&doc),
                self.app.document(doc).map(|d| d.doc.store().clone()),
            ) else {
                return;
            };
            let def = s.def;
            let scale = s.scale;
            let view_guid = v
                .element
                .and_then(|e| store.get(e))
                .map(|e| e.guid().to_string());
            let parts = reviera_app::output::section_parts(
                &store,
                view_guid.as_deref(),
                scene,
                &def,
                scale,
                &|e| visibility.opacity(e).is_some(),
            );
            let mut graphics = parts.graphics;
            graphics.lines.extend(parts.annotation);
            graphics.lines.push(Lines {
                color: [0.0, 0.0, 0.0, 1.0],
                width: 0.18,
                segments: label_strokes(&parts.labels),
            });
            // The crop region (DOC-GFX-015).
            if let Some(c) = v.element.and_then(|e| store.get(e)).and_then(reviera_core::crop::rects) {
                reviera_render::crop::crop_graphics(&mut graphics, c.model);
            }
            let cut_edges = Rc::new(std::mem::take(&mut graphics.cut_edges));
            let v = &mut self.views[index];
            if let Some(vp) = &mut v.viewport {
                vp.set_scene(&parts.scene);
            }
            if let Some(s) = &mut v.section {
                s.stale = false;
                s.scene = Rc::new(parts.scene);
                s.base = Some(Rc::new(graphics));
                s.cut_edges = cut_edges;
            }
        }
        let Some(base) = self.views[index].section.as_ref().and_then(|s| s.base.clone()) else { return };
        let mut graphics = (*base).clone();
        let from = graphics.lines.len();
        graphics.lines.extend(self.annotation_lines(index));
        graphics.lines.extend(self.note_lines(index));
        let (doc, element) = (self.views[index].doc, self.views[index].element);
        let view = self.app.document(doc).and_then(|d| d.doc.store().get(element?).cloned());
        if let Some(view) = view
            && let Some(c) = reviera_core::crop::rects(&view)
        {
            if let Some(a) = c.annotation {
                reviera_render::crop::crop_lines(&mut graphics.lines[from..], a);
            }
            if reviera_core::crop::crop_of(&view).is_some_and(|k| k.visible) {
                graphics.lines.push(Lines { color: crate::plans::CROP_COLOR, width: 0.25, segments: reviera_render::crop::outline(c.model, 0.005) });
            }
        }
        let v = &mut self.views[index];
        if let Some(vp) = &mut v.viewport {
            vp.set_highlight(&highlight);
            vp.set_plan(Some(graphics));
        }
        if let Some(s) = &mut v.section {
            s.redraw = false;
        }
    }

    /// The element under pixel (`x`, `y`) of a section view: picked in the view's frame,
    /// between the view plane and the far clip.
    pub(crate) fn section_pick(&self, x: i32, y: i32) -> Option<ElementId> {
        let v = self.active_view()?;
        let s = v.section.as_ref()?;
        let size = v.panel.get_client_size();
        let (origin, dir) = v
            .camera
            .ray(x as f32, y as f32, size.width as f32, size.height as f32);
        let far = -(s.def.far as f32);
        let visibility = self.view_visibility(v);
        s.scene
            .pick_visible(origin, dir, &|p| p.z <= 1e-4 && p.z >= far, &visibility)
            .and_then(|(i, _)| s.scene.meshes[i].element)
    }

    // ---- The Section tool ----

    /// View › Section: two clicks in a plan.
    pub(crate) fn start_section_tool(&mut self) {
        let frame = self.frame;
        if !self.in_plan() {
            return frame.set_status_text("Sections are drawn in plan views", 0);
        }
        self.end_room_tool();
        self.section_tool = Some(SectionTool::default());
        frame.set_status_text(
            "Section: click the start of the section line (the view looks to its right); Esc ends",
            0,
        );
    }

    pub(crate) fn end_section_tool(&mut self) -> bool {
        if self.section_tool.take().is_none() {
            return false;
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        true
    }

    pub(crate) fn section_motion(&mut self, x: i32, y: i32) {
        let Some(start) = self.section_tool.as_ref().and_then(|t| t.start) else {
            return;
        };
        let (Some(p), Some(plan)) = (
            self.plan_xy(x, y),
            self.active_view().and_then(|v| v.plan.clone()),
        ) else {
            return;
        };
        let z = plan.range.cut as f32 + 0.004;
        let def = SectionDef {
            start,
            end: p,
            far: sections::DEFAULT_FAR,
            flipped: false,
        };
        let lines = marker_lines(&def, false, "", plan.scale, z);
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[Lines {
                color: PREVIEW,
                ..lines
            }]);
        }
    }

    pub(crate) fn section_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        let Some(p) = self.plan_xy(x, y) else { return };
        let Some(t) = &mut self.section_tool else {
            return;
        };
        let Some(start) = t.start else {
            t.start = Some(p);
            return frame.set_status_text("Section: click the end of the section line", 0);
        };
        self.end_section_tool();
        match self.run("view.create_section", json!({ "start": start, "end": p })) {
            Ok(v) => {
                let id = ElementId(v.as_u64().unwrap_or_default());
                self.browser_stale = true;
                if let Some(doc) = self.doc {
                    self.plans_redraw(doc);
                    self.open_view(doc, Some(id));
                }
            }
            Err(e) => error(&frame, &format!("The section was not created:\n{e}")),
        }
    }

    /// View › Building Elevations: North, South, East and West elevations 5 m outside the
    /// model, looking at it.
    pub(crate) fn building_elevations(&mut self) {
        let frame = self.frame;
        let Some(doc) = self.doc else { return };
        let Some((lo, hi)) = self.scene().bounds() else {
            return frame.set_status_text("The model has no geometry to elevate", 0);
        };
        let (lo, hi) = (lo.as_dvec3(), hi.as_dvec3());
        let margin = 5.0;
        let mid = [(lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0];
        let (wx, wy) = (hi.x - lo.x + 2.0 * margin, hi.y - lo.y + 2.0 * margin);
        let views = [
            ("North", [mid[0], hi.y + margin], [0.0, -1.0], wx),
            ("South", [mid[0], lo.y - margin], [0.0, 1.0], wx),
            ("East", [hi.x + margin, mid[1]], [-1.0, 0.0], wy),
            ("West", [lo.x - margin, mid[1]], [1.0, 0.0], wy),
        ];
        let mut first = None;
        for (name, point, look, width) in views {
            match self.run(
                "view.create_elevation",
                json!({ "point": point, "look": look, "width": width, "name": name }),
            ) {
                Ok(v) => {
                    first = first.or(v.as_u64().map(ElementId));
                }
                Err(e) => return error(&frame, &format!("The elevations were not created:\n{e}")),
            }
        }
        self.browser_stale = true;
        self.plans_redraw(doc);
        if let Some(id) = first {
            self.open_view(doc, Some(id));
        }
        frame.set_status_text(
            "Four building elevations created (Project Browser › Elevations)",
            0,
        );
    }

    /// Section lines and elevation markers for a plan.
    pub(crate) fn section_markers(&mut self, index: usize) -> Vec<Lines> {
        let v = &self.views[index];
        let Some(plan) = &v.plan else {
            return Vec::new();
        };
        if plan.ceiling {
            return Vec::new();
        }
        let (scale, z) = (plan.scale, plan.range.cut as f32 + 0.003);
        let Some(store) = self.app.document(v.doc).map(|d| d.doc.store().clone()) else {
            return Vec::new();
        };
        let mut lines = Lines {
            color: MARKER,
            width: 0.35,
            segments: Vec::new(),
        };
        for e in store.iter() {
            let Some((kind, def)) = sections::section_of(e) else {
                continue;
            };
            let name = e.name.clone().unwrap_or_default();
            let l = marker_lines(&def, kind == sections::KIND_ELEVATION, &name, scale, z);
            lines.segments.extend(l.segments);
        }
        vec![lines]
    }

    /// Properties rows of an open section or elevation view.
    pub(crate) fn section_view_rows(&mut self, grid: &PropertyGrid) -> bool {
        let Some(v) = self.active_view() else {
            return false;
        };
        let (Some(element), Some(s)) = (v.element, v.section.clone()) else {
            return false;
        };
        let name = self.view_name(v);
        grid.append(Property::category(
            if s.elevation { "Elevation" } else { "Section" },
            "c_view",
        ));
        grid.append(Property::string("View Name", "view_name", name));
        let far = self.format_length(s.def.far);
        grid.append(Property::string("Far Clip Offset", "sec_far", far));
        grid.set_help_string("sec_far", "How far beyond the view plane the view sees");
        grid.append(Property::boolean("Flipped", "sec_flip", s.def.flipped));
        grid.append(Property::string("ID", "v0", element.to_string()));
        grid.set_property_read_only("v0", true);
        self.crop_rows(grid, element);
        true
    }

    /// An edit of a section view row; `false` if `property` is not one.
    pub(crate) fn section_view_edit(&mut self, property: &str, text: Option<String>) -> bool {
        let Some(view) = self
            .active_view()
            .filter(|v| v.section.is_some())
            .and_then(|v| v.element)
        else {
            return false;
        };
        let args = match property {
            "sec_far" => json!({ "view_id": view, "far_offset": text.unwrap_or_default() }),
            "sec_flip" => json!({ "view_id": view, "flip": true }),
            _ => return false,
        };
        if let Err(e) = self.run("section.edit", args) {
            error(&self.frame, &format!("The view was not changed:\n{e}"));
        }
        self.sections_stale();
        self.properties_stale = true;
        true
    }

    /// Marks every section view stale (the model or a definition changed).
    pub(crate) fn sections_stale(&mut self) {
        let defs: Vec<(u32, Option<SectionDef>)> = self
            .views
            .iter()
            .map(|v| {
                (
                    v.id,
                    v.element
                        .and_then(|e| self.app.document(v.doc)?.doc.store().get(e).cloned())
                        .and_then(|e| sections::section_of(&e).map(|(_, d)| d)),
                )
            })
            .collect();
        for v in &mut self.views {
            if let Some(s) = &mut v.section {
                if let Some((_, Some(d))) = defs.iter().find(|(id, _)| *id == v.id) {
                    s.def = *d;
                }
                s.stale = true;
                v.panel.refresh(false, None);
            }
        }
    }
}

/// A section line with a head arrow at each end pointing the way the view looks, and its name;
/// or an elevation marker (a square with an arrow) at the line's middle.
fn marker_lines(def: &SectionDef, elevation: bool, name: &str, scale: f32, z: f32) -> Lines {
    let mm = f64::from(scale) / 1000.0;
    let f = def.frame();
    let seg =
        |a: [f64; 2], b: [f64; 2]| [[a[0] as f32, a[1] as f32, z], [b[0] as f32, b[1] as f32, z]];
    let at = |p: [f64; 2], along: f64, out: f64| {
        [
            p[0] + f.right[0] * along + f.look[0] * out,
            p[1] + f.right[1] * along + f.look[1] * out,
        ]
    };
    let mut out = Vec::new();
    let mut labels: Vec<Label> = Vec::new();
    let text = |p: [f64; 2]| Label {
        origin: [p[0] as f32, p[1] as f32, z],
        right: [1.0, 0.0, 0.0],
        up: [0.0, 1.0, 0.0],
        height: (2.5 * mm) as f32,
        text: name.to_uppercase(),
    };
    if elevation {
        let c = [
            (def.start[0] + def.end[0]) / 2.0,
            (def.start[1] + def.end[1]) / 2.0,
        ];
        let r = 4.0 * mm;
        let sq = [at(c, -r, -r), at(c, r, -r), at(c, r, r), at(c, -r, r)];
        for k in 0..4 {
            out.push(seg(sq[k], sq[(k + 1) % 4]));
        }
        // The arrow points the way the view looks.
        let tip = at(c, 0.0, r * 2.2);
        out.push(seg(at(c, -r, r), tip));
        out.push(seg(at(c, r, r), tip));
        // The name centred below the marker.
        let height = 2.5 * mm;
        let w = f64::from(reviera_render::font::width(
            &name.to_uppercase(),
            height as f32,
        ));
        labels.push(text([c[0] - w / 2.0, c[1] - r * 2.4 - height]));
    } else {
        out.push(seg(def.start, def.end));
        for p in [def.start, def.end] {
            let tip = at(p, 0.0, 6.0 * mm);
            out.push(seg(p, tip));
            out.push(seg(at(p, -2.0 * mm, 3.5 * mm), tip));
            out.push(seg(at(p, 2.0 * mm, 3.5 * mm), tip));
        }
        if !name.is_empty() {
            labels.push(text(at(def.start, 2.0 * mm, -5.0 * mm)));
        }
    }
    out.extend(label_strokes(&labels));
    Lines {
        color: MARKER,
        width: 0.35,
        segments: out,
    }
}
