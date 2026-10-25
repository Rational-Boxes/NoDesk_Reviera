// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Duct and pipe colour fills in plans (MEP-DUCT-010): Analyze › Color Fill › **Duct Legend**
//! and **Pipe Legend** place a legend with a click, then ask for the scheme (an existing duct or
//! pipe scheme, or a new one: name, parameter, by value or by range). The plan colours each
//! segment (and fitting) on its level by the legend's scheme, drawn over its footprint, and
//! draws the legend: its title, then a swatch and a caption per row in use. A legend is picked
//! by clicking it, and deleted like any element, which ends the fill in that view.

use reviera_core::ElementId;
use reviera_core::color_schemes;
use reviera_core::mep_color_fill::{self as fill, Kind};
use reviera_render::SceneMesh;
use reviera_render::annotation::{Label, label_strokes, labels_extent};
use reviera_render::plan::Lines;
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

/// Text height and row step on paper (mm).
const TEXT_MM: f32 = 2.5;
const ROW_MM: f32 = 5.0;

fn quad_mesh(element: Option<ElementId>, q: &[[f64; 2]; 4], z: f32, color: [f32; 4]) -> SceneMesh {
    SceneMesh {
        element,
        class: "IfcAnnotation".into(),
        positions: q.iter().map(|p| [p[0] as f32, p[1] as f32, z]).collect(),
        indices: vec![0, 1, 2, 0, 2, 3],
        color,
    }
}

/// A legend's drawing: its labels, swatches (corner, size, colour) and extent.
struct LegendDrawing {
    id: ElementId,
    labels: Vec<Label>,
    swatches: Vec<([f32; 2], f32, [f32; 4])>,
}

impl Gui {
    fn legend_drawings(&mut self, index: usize) -> Vec<LegendDrawing> {
        let v = &self.views[index];
        let (Some(e), Some(plan)) = (v.element, v.plan.as_ref()) else { return Vec::new() };
        let Some(store) = self.app.document(v.doc).map(|d| d.doc.store().clone()) else { return Vec::new() };
        let Some(view) = store.get(e) else { return Vec::new() };
        let mm = plan.scale / 1000.0;
        let z = plan.range.cut as f32 + 0.004;
        let mut out = Vec::new();
        for (id, l) in fill::legends(&store, view) {
            let Some(s) = color_schemes::find(&store, &l.scheme) else { continue };
            let rows = color_schemes::rows_now(&store, &s);
            let (x, mut y) = (l.point[0] as f32, l.point[1] as f32);
            let h = TEXT_MM * mm;
            let mut labels = vec![Label { origin: [x, y - h, z], right: [1.0, 0.0, 0.0], up: [0.0, 1.0, 0.0], height: h * 1.2, text: s.title.to_uppercase() }];
            let mut swatches = Vec::new();
            y -= ROW_MM * mm * 1.4;
            let mut row = |caption: String, color: [f32; 4], y: f32| {
                swatches.push(([x, y - h], h, color));
                labels.push(Label { origin: [x + h * 1.8, y - h, z], right: [1.0, 0.0, 0.0], up: [0.0, 1.0, 0.0], height: h, text: caption.to_uppercase() });
            };
            for r in rows.iter().filter(|r| r.visible && (r.in_use || s.mode == color_schemes::Mode::Range)) {
                if let Some(c) = color_schemes::rgba(&r.color) {
                    row(r.caption.clone(), c, y);
                    y -= ROW_MM * mm;
                }
            }
            if let Some(c) = s.no_value_color.as_deref().and_then(color_schemes::rgba) {
                row("No value".into(), c, y);
            }
            out.push(LegendDrawing { id, labels, swatches });
        }
        out
    }

    /// The coloured footprints of the plan's ducts and pipes, and the legends' swatches.
    pub(crate) fn mep_fill_meshes(&mut self, index: usize) -> Vec<SceneMesh> {
        let v = &self.views[index];
        let (Some(e), Some(plan)) = (v.element, v.plan.clone()) else { return Vec::new() };
        let Some(store) = self.app.document(v.doc).map(|d| d.doc.store().clone()) else { return Vec::new() };
        let Some(view) = store.get(e) else { return Vec::new() };
        let Some(level) = store.get(plan.level).map(|l| l.guid().to_string()) else { return Vec::new() };
        let mut out = Vec::new();
        for (id, color) in fill::fills(&store, view) {
            let Some(el) = store.get(id) else { continue };
            if fill::level_of(el).as_deref() != Some(level.as_str()) {
                continue;
            }
            let (quads, top) = fill::footprint(el);
            for q in &quads {
                out.push(quad_mesh(Some(id), q, top as f32 + 0.002, color));
            }
        }
        let z = plan.range.cut as f32 + 0.004;
        for d in self.legend_drawings(index) {
            for (c, s, color) in d.swatches {
                let q = [[c[0] as f64, c[1] as f64], [(c[0] + s) as f64, c[1] as f64], [(c[0] + s) as f64, (c[1] + s) as f64], [c[0] as f64, (c[1] + s) as f64]];
                out.push(quad_mesh(Some(d.id), &q, z, color));
            }
        }
        out
    }

    /// The legends' text and swatch outlines; selected ones lit.
    pub(crate) fn mep_legend_lines(&mut self, index: usize) -> Vec<Lines> {
        let selected: std::collections::HashSet<ElementId> = self.selection().into_iter().collect();
        let z = self.views[index].plan.as_ref().map_or(0.0, |p| p.range.cut as f32 + 0.004);
        let mut normal = Lines { color: [0.15, 0.15, 0.15, 1.0], width: 0.18, segments: Vec::new() };
        let mut lit = Lines { color: reviera_render::HIGHLIGHT, width: 0.3, segments: Vec::new() };
        for d in self.legend_drawings(index) {
            let target = if selected.contains(&d.id) { &mut lit } else { &mut normal };
            target.segments.extend(label_strokes(&d.labels));
            for (c, s, _) in &d.swatches {
                let p = [[c[0], c[1]], [c[0] + s, c[1]], [c[0] + s, c[1] + s], [c[0], c[1] + s]];
                for i in 0..4 {
                    let (a, b) = (p[i], p[(i + 1) % 4]);
                    target.segments.push([[a[0], a[1], z], [b[0], b[1], z]]);
                }
            }
        }
        vec![normal, lit]
    }

    /// The legend under pixel (`x`, `y`) in the active plan.
    pub(crate) fn mep_legend_at(&mut self, x: i32, y: i32) -> Option<ElementId> {
        let index = self.views.iter().position(|v| Some(v.id) == self.active)?;
        self.views[index].plan.as_ref()?;
        let p = self.plan_xy(x, y)?;
        let (px, py) = (p[0] as f32, p[1] as f32);
        self.legend_drawings(index).into_iter().find_map(|d| {
            let (mut lo, mut hi) = labels_extent(&d.labels)?;
            for (c, s, _) in &d.swatches {
                lo = [lo[0].min(c[0]), lo[1].min(c[1])];
                hi = [hi[0].max(c[0] + s), hi[1].max(c[1] + s)];
            }
            (px >= lo[0] && px <= hi[0] && py >= lo[1] && py <= hi[1]).then_some(d.id)
        })
    }

    /// The scheme for a new legend: an existing one of the kind, or a new one; its key.
    pub(crate) fn choose_mep_scheme(&mut self, kind: Kind) -> Option<String> {
        let frame = self.frame;
        let store = self.store_now()?;
        let schemes: Vec<color_schemes::ColorScheme> = color_schemes::all(&store).into_iter().filter(|s| s.category == kind.category()).collect();
        let what = if kind == Kind::Duct { "Duct" } else { "Pipe" };
        let mut labels: Vec<String> = schemes.iter().map(|s| format!("{} ({})", s.name, s.parameter)).collect();
        labels.push("New scheme…".into());
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let d = SingleChoiceDialog::builder(&frame, &format!("Colour {} runs by:", what.to_lowercase()), "Choose Colour Scheme", &refs).build();
        let chosen = (d.show_modal() == ID_OK).then(|| usize::try_from(d.get_selection()).ok()).flatten();
        d.destroy();
        let k = chosen?;
        if let Some(s) = schemes.get(k) {
            return Some(s.id.clone());
        }
        let mut params: Vec<String> = fill::DUCT_PARAMETERS.iter().map(|s| s.to_string()).collect();
        if kind == Kind::Pipe {
            params.extend(fill::PIPE_PARAMETERS.iter().map(|s| s.to_string()));
        }
        let f = super::reinforcement_ui::Form::new(&frame, &format!("New {what} Colour Scheme"));
        let name = f.text("Name", &format!("{what} Velocity"), "");
        let param = f.choice("Colour by", &params, 1);
        let modes = vec!["By range (5 equal bands)".to_owned(), "By value".to_owned()];
        let mode = f.choice("Kind", &modes, 0);
        let fittings = f.check("Fittings", "Fill fittings and accessories too", true);
        if !f.show("Calculated values (flow, velocity, friction, pressure drop) come from the last network calculation.", "OK") {
            f.dialog.destroy();
            return None;
        }
        let args = json!({
            "name": name.get_value().trim(),
            "category": kind.category(),
            "parameter": params[param.get_selection().map_or(1, |i| i as usize)],
            "mode": if mode.get_selection() == Some(1) { "value" } else { "range" },
            "include_fittings": fittings.get_value(),
        });
        f.dialog.destroy();
        match self.run("colorscheme.create", args) {
            Ok(v) => v.as_str().map(str::to_owned),
            Err(e) => {
                error(&frame, &e);
                None
            }
        }
    }
}
