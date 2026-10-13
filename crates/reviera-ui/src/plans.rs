// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Plan views in the window (spec Ch 5 §5.3): a top-down parallel view cut at the plan's cut
//! plane, drawn with cut poché and lines (`reviera_render::plan`), navigated by pan and zoom only.

use std::rc::Rc;

use reviera_app::DocumentId;
use reviera_core::ElementId;
use reviera_core::plans::{self, KIND_PLAN};
use reviera_core::views;
use reviera_render::RenderBackend;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

/// Below the bottom of a plan's range, floors, slabs and the like still show within this depth
/// (DOC-VR-002 step 6, floor tolerance).
pub const FLOOR_TOLERANCE: f64 = 1.22;

/// What the window knows of a plan view.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanState {
    pub level: ElementId,
    pub level_name: String,
    pub elevation: f64,
    pub range: plans::Resolved,
    /// The plan graphics must be rebuilt (model or range changed).
    pub stale: bool,
    /// Cut edges of the current graphics with their element (snapping, dimension references).
    pub cut_edges: Rc<Vec<reviera_render::plan::CutEdge>>,
    /// The drawing scale denominator (100 for 1:100).
    pub scale: f32,
}

impl PlanState {
    /// Elevations drawn: from the bottom (with the floor tolerance) to the cut plane.
    pub fn drawn(&self) -> (f32, f32) {
        let bottom = (self.range.bottom - FLOOR_TOLERANCE).min(self.range.depth);
        (bottom.max(-1e6) as f32, self.range.cut as f32)
    }
}

impl Gui {
    /// The plan state of a view element, if it is a plan.
    pub(crate) fn plan_info(
        &self,
        doc: DocumentId,
        element: Option<ElementId>,
    ) -> Option<PlanState> {
        let store = self.app.document(doc)?.doc.store();
        let data = views::view_data(store.get(element?)?)?;
        if data.kind != KIND_PLAN {
            return None;
        }
        let guid = data.level.clone()?;
        let level = plans::levels(store).into_iter().find(|l| l.1 == guid)?;
        let range = plans::resolve(store, &guid, data.view_range.as_ref()?).ok()?;
        Some(PlanState {
            level: level.2,
            level_name: store.get(level.2)?.name.clone().unwrap_or_default(),
            elevation: level.0,
            range,
            stale: true,
            cut_edges: Rc::default(),
            scale: store
                .get(element?)
                .and_then(|e| e.param(views::VIEW_KEY)?.get("scale")?.as_f64())
                .unwrap_or(100.0) as f32,
        })
    }

    /// Whether the active view is a plan.
    pub(crate) fn in_plan(&self) -> bool {
        self.active_view().is_some_and(|v| v.plan.is_some())
    }

    /// Rebuilds the plan graphics of view `index` when stale.
    pub(crate) fn update_plan_graphics(&mut self, index: usize) {
        let v = &self.views[index];
        let Some(plan) = &v.plan else { return };
        if !plan.stale || v.viewport.is_none() {
            return;
        }
        let doc = v.doc;
        let (bottom, cut) = plan.drawn();
        let Some(scene) = self.scenes.get(&doc) else {
            return;
        };
        let edges = self
            .plan_edges
            .entry(doc)
            .or_insert_with(|| Rc::new(reviera_render::plan::feature_edges(scene)))
            .clone();
        let scale = plan.scale;
        // Pens of each element's category (DOC-GFX-002, DOC-GFX-009).
        let Some(store) = self.app.document(doc).map(|d| d.doc.store().clone()) else {
            return;
        };
        let pens = reviera_core::standards::pen_set(&store);
        let styles = reviera_core::standards::object_styles(&store);
        let weights = |element: Option<ElementId>| {
            let (cut, projection) = reviera_core::standards::element_widths(
                &store,
                &pens,
                &styles,
                element,
                f64::from(scale),
            );
            reviera_render::plan::Weights {
                cut: cut.map(|w| w as f32),
                projection: projection as f32,
            }
        };
        let mut graphics =
            reviera_render::plan::plan_graphics(scene, &edges, cut, bottom, scale, &weights);
        let cut_edges = Rc::new(std::mem::take(&mut graphics.cut_edges));
        graphics.lines.extend(self.annotation_lines(index));
        let v = &mut self.views[index];
        if let Some(viewport) = &mut v.viewport {
            viewport.set_plan(Some(graphics));
        }
        if let Some(p) = &mut v.plan {
            p.stale = false;
            p.cut_edges = cut_edges;
        }
    }

    /// Redraws the plans of `doc` (annotation or selection changed; the model did not).
    pub(crate) fn plans_redraw(&mut self, doc: DocumentId) {
        for v in self.views.iter_mut().filter(|v| v.doc == doc) {
            if let Some(p) = &mut v.plan {
                p.stale = true;
                v.panel.refresh(false, None);
            }
        }
    }

    /// Marks the plans of `doc` for rebuilding (the model changed).
    pub(crate) fn plans_stale(&mut self, doc: DocumentId) {
        self.plan_edges.remove(&doc);
        for v in self.views.iter_mut().filter(|v| v.doc == doc) {
            if let Some(p) = &mut v.plan {
                p.stale = true;
            }
        }
    }

    /// Re-reads the plan state of every view (levels, ranges); changed plans are rebuilt.
    pub(crate) fn refresh_plan_states(&mut self) {
        for i in 0..self.views.len() {
            let (doc, element) = (self.views[i].doc, self.views[i].element);
            let Some(mut fresh) = self.plan_info(doc, element) else {
                continue;
            };
            let v = &mut self.views[i];
            if let Some(old) = &v.plan {
                fresh.cut_edges = old.cut_edges.clone();
                let same = old.range == fresh.range
                    && old.elevation == fresh.elevation
                    && old.scale == fresh.scale;
                fresh.stale = old.stale || !same;
                if same && !old.stale {
                    continue;
                }
            }
            v.plan = Some(fresh);
            v.panel.refresh(false, None);
        }
    }

    /// View › New Floor Plan: plans for the chosen levels, then the last one opens.
    pub(crate) fn new_floor_plans(&mut self) {
        let frame = self.frame;
        let Some(doc) = self.doc else { return };
        let Some(store) = self.app.document(doc).map(|d| d.doc.store().clone()) else {
            return;
        };
        let planned: Vec<String> = store
            .iter()
            .filter_map(views::view_data)
            .filter(|d| d.kind == KIND_PLAN)
            .filter_map(|d| d.level)
            .collect();
        // Levels that already have a plan are not listed (Do not duplicate existing views).
        let levels: Vec<(ElementId, String)> = plans::levels(&store)
            .into_iter()
            .filter(|l| !planned.contains(&l.1))
            .map(|l| {
                (
                    l.2,
                    store
                        .get(l.2)
                        .and_then(|e| e.name.clone())
                        .unwrap_or_default(),
                )
            })
            .collect();
        if levels.is_empty() {
            return frame.set_status_text("Every level already has a floor plan", 0);
        }
        let names: Vec<&str> = levels.iter().map(|l| l.1.as_str()).collect();
        let dialog =
            MultiChoiceDialog::builder(&frame, "Levels:", "New Floor Plan", &names).build();
        dialog.set_selections(&(0..names.len() as i32).collect::<Vec<_>>());
        if dialog.show_modal() != ID_OK {
            return;
        }
        let ids: Vec<u64> = dialog
            .get_selections()
            .into_iter()
            .filter_map(|i| levels.get(i as usize).map(|l| l.0.0))
            .collect();
        if ids.is_empty() {
            return;
        }
        match self.run(
            "view.create_plan",
            json!({ "kind": "floor_plan", "level_ids": ids }),
        ) {
            Ok(v) => {
                if let Some(last) = v.as_array().and_then(|a| a.last()).and_then(Value::as_u64) {
                    self.open_view(doc, Some(ElementId(last)));
                }
            }
            Err(e) => error(&frame, &format!("The plans were not created:\n{e}")),
        }
        self.after_view_change();
    }

    /// Sets the cut plane of the active plan to `text` above its reference level.
    pub(crate) fn set_cut_plane(&mut self, text: &str, frame: &Frame) {
        let Some(view) = self.active_view().and_then(|v| v.element) else {
            return;
        };
        let offset = match self.run(
            "units.parse",
            json!({ "text": text, "data_type": "length" }),
        ) {
            Ok(v) => v.as_f64().unwrap_or_default(),
            Err(e) => return error(frame, &format!("Invalid length:\n{e}")),
        };
        let Ok(range) = self.run("view.get_view_range", json!({ "view_id": view })) else {
            return;
        };
        let plane = |name: &str, offset: Option<f64>| {
            let p = &range["planes"][name];
            json!({ "level_ref": p["level"], "offset": offset.unwrap_or(p["offset"].as_f64().unwrap_or(0.0)) })
        };
        let args = json!({
            "view_id": view,
            "top": plane("top", None),
            "cut": plane("cut", Some(offset)),
            "bottom": plane("bottom", None),
            "depth": plane("depth", None),
        });
        if let Err(e) = self.run("view.set_view_range", args) {
            error(frame, &format!("The view range was not changed:\n{e}"));
        }
        self.refresh_plan_states();
        self.properties_stale = true;
    }
}
