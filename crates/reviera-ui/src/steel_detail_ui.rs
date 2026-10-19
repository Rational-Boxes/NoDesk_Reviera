// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Steel › Detailing: Plate, Bolts and Weld (STR-STL-005, STR-STL-006, STR-STL-007).
//!
//! - **Plate**: with one column, beam, brace or plate selected, choose its face; otherwise the
//!   view's work plane (a plan's level). Click the contour's corners; Enter (or a click on the
//!   first corner) finishes, Backspace takes the last corner back.
//! - **Bolts**: with the elements selected, choose the kind (bolts, anchors, holes, shear studs),
//!   the face the fasteners are square to and the layout, then click where the pattern's centre
//!   goes on that face.
//! - **Weld**: with the elements selected, the edges where they touch are drawn in 3D views; click
//!   one. The weld takes its type, throat and location from a dialog.
//!
//! Properties show a plate's thickness, justification and role, a pattern's diameter, grip and
//! length, and a weld's type, throat, location and review status (with *Accept*).

use glam::Vec3;
use reviera_core::ElementId;
use reviera_core::work_planes::Plane;
use reviera_render::RenderBackend;
use reviera_render::plan::Lines;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

const PREVIEW: [f32; 4] = [0.95, 0.45, 0.0, 1.0];
const EDGE: [f32; 4] = [0.95, 0.6, 0.1, 1.0];
const HOT: [f32; 4] = [0.85, 0.1, 0.1, 1.0];

#[derive(Debug, Clone)]
pub enum SteelTool {
    Plate {
        /// The placement arguments for `plate.create` (a work plane or a face).
        placement: Value,
        plane: Plane,
        points: Vec<[f64; 2]>,
        cursor: Option<[f64; 2]>,
    },
    Pattern {
        command: &'static str,
        args: Value,
        plane: Plane,
    },
    Weld {
        elements: Vec<ElementId>,
        edges: Vec<[[f64; 3]; 2]>,
        hot: Option<usize>,
    },
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn merge(mut a: Value, b: &Value) -> Value {
    if let (Some(a), Some(b)) = (a.as_object_mut(), b.as_object()) {
        for (k, v) in b {
            a.insert(k.clone(), v.clone());
        }
    }
    a
}

impl Gui {
    pub(crate) fn end_steel_tool(&mut self) -> bool {
        self.contour_target = None;
        if self.steel_tool.take().is_none() {
            return false;
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        true
    }

    /// The faces of the selected elements: (element, face, label).
    fn selected_faces(&mut self) -> Vec<(ElementId, &'static str, String)> {
        let Some(store) = self.store_now() else { return Vec::new() };
        let mut out = Vec::new();
        for id in self.selection() {
            let Some(e) = store.get(id) else { continue };
            for f in reviera_core::steel_detail::faces_of(&store, e) {
                let what = reviera_core::categories::category_of(e).map_or(e.class.clone(), |c| c.name.to_owned());
                out.push((id, f, format!("{what} {}: {}", id.0, f.replace('_', " "))));
            }
        }
        out
    }

    fn face_plane_of(&self, id: ElementId, face: &str) -> Option<Plane> {
        let store = self.store_now()?;
        reviera_core::steel_detail::face_plane(&store, store.get(id)?, face).ok()
    }

    /// Steel › Plate.
    pub(crate) fn start_plate_tool(&mut self) {
        let frame = self.frame;
        self.end_tool(&frame);
        self.sketch_on_selection("Plate", "The plate goes on the face:");
    }

    /// Steel › Contour Cut: sketch on a face of the selected element.
    pub(crate) fn start_contour_cut(&mut self) {
        let frame = self.frame;
        self.end_tool(&frame);
        let sel = self.selection();
        let [target] = sel[..] else {
            return frame.set_status_text("Contour Cut: select the one element to cut first", 0);
        };
        self.sketch_on_selection("Contour Cut", "Sketch the contour on the face:");
        if self.steel_tool.is_some() {
            self.contour_target = Some(target);
            frame.set_status_text("Contour Cut: click the contour's corners; Enter finishes, Esc ends", 0);
        }
    }

    fn sketch_on_selection(&mut self, title: &str, prompt: &str) {
        let frame = self.frame;
        let faces = self.selected_faces();
        let (placement, plane) = if !faces.is_empty() && self.selection().len() == 1 {
            let labels: Vec<&str> = faces.iter().map(|f| f.2.as_str()).collect();
            let d = SingleChoiceDialog::builder(&frame, prompt, title, &labels).build();
            if d.show_modal() != ID_OK {
                return;
            }
            let Some((id, face, _)) = usize::try_from(d.get_selection()).ok().and_then(|i| faces.get(i).cloned()) else {
                return;
            };
            let Some(plane) = self.face_plane_of(id, face) else { return };
            (json!({ "face": { "element": id, "face": face } }), plane)
        } else {
            match self.view_work_plane() {
                Some((_, r, plane)) => (crate::datum_tools::plane_args(&r), plane),
                None => {
                    return frame.set_status_text(
                        "Plate: select a column, beam or plate for its face, or set a work plane (plans use their level)",
                        0,
                    );
                }
            }
        };
        self.steel_tool = Some(SteelTool::Plate { placement, plane, points: Vec::new(), cursor: None });
        frame.set_status_text(
            "Plate: click the contour's corners; Enter or the first corner finishes, Backspace takes one back, XY types coordinates, Esc ends",
            0,
        );
    }

    /// Where pixel (`x`, `y`) meets a plane, in its coordinates.
    fn on_plane(&self, plane: &Plane, x: i32, y: i32) -> Option<[f64; 2]> {
        if let Some(p) = crate::coords::typed() {
            return Some(p);
        }
        let v = self.active_view()?;
        if v.section.is_some() {
            return None;
        }
        let size = v.panel.get_client_size();
        let (o, d) = v.camera.ray(x as f32, y as f32, size.width as f32, size.height as f32);
        let (o, d) = ([o.x as f64, o.y as f64, o.z as f64], [d.x as f64, d.y as f64, d.z as f64]);
        let den = dot(d, plane.normal);
        if den.abs() < 1e-9 {
            return None;
        }
        let t = dot(sub(plane.origin, o), plane.normal) / den;
        let p = [o[0] + d[0] * t, o[1] + d[1] * t, o[2] + d[2] * t];
        let mut q = plane.to_local(p);
        // To the millimetre.
        q = [(q[0] * 1000.0).round() / 1000.0, (q[1] * 1000.0).round() / 1000.0];
        Some(q)
    }

    fn screen(&self, p: [f64; 3]) -> Option<(f32, f32)> {
        let v = self.active_view()?;
        let size = v.panel.get_client_size();
        v.camera.project(Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32), size.width as f32, size.height as f32)
    }

    pub(crate) fn steel_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        let Some(tool) = self.steel_tool.clone() else { return };
        match tool {
            SteelTool::Plate { placement, plane, mut points, .. } => {
                let Some(q) = self.on_plane(&plane, x, y) else {
                    return frame.set_status_text("Click on the plane: in a 3D view, or a plan for a level", 0);
                };
                // A click on the first corner closes the contour.
                let closes = points.len() >= 3
                    && self
                        .screen(plane.to_world(points[0]))
                        .is_some_and(|s| (s.0 - x as f32).hypot(s.1 - y as f32) < 8.0);
                if closes {
                    return self.finish_plate(placement, points);
                }
                points.push(q);
                let n = points.len();
                self.steel_tool = Some(SteelTool::Plate { placement, plane, points, cursor: Some(q) });
                self.steel_preview();
                frame.set_status_text(&format!("Plate: {n} corner(s); Enter finishes"), 0);
            }
            SteelTool::Pattern { command, args, plane } => {
                let Some(q) = self.on_plane(&plane, x, y) else {
                    return frame.set_status_text("Click on the face the fasteners cross", 0);
                };
                let mut args = args;
                args["layout"]["centre"] = json!(q);
                match self.run(command, args) {
                    Ok(id) => {
                        self.end_steel_tool();
                        self.refresh_scene(&frame);
                        let what = self
                            .run("fastener_pattern.info", json!({ "element_ids": [id] }))
                            .ok()
                            .map(|i| {
                                let i = &i[0];
                                let what = match i["pattern"].as_str() {
                                    Some("anchor") => "Anchor pattern",
                                    Some("hole") => "Hole pattern",
                                    Some("shear_stud") => "Shear studs",
                                    _ => "Bolt pattern",
                                };
                                format!(
                                    "{what} placed: grip {:.1} mm, length {:.1} mm, {} fastener(s)",
                                    i["grip"].as_f64().unwrap_or(0.0) * 1000.0,
                                    i["length"].as_f64().unwrap_or(0.0) * 1000.0,
                                    i["fasteners"].as_array().map_or(0, Vec::len)
                                )
                            })
                            .unwrap_or_default();
                        frame.set_status_text(&format!("{what} (shown at Fine detail)"), 0);
                    }
                    Err(e) => error(&frame, &format!("The pattern was not placed:\n{e}")),
                }
            }
            SteelTool::Weld { elements, edges, .. } => {
                let Some(k) = self.nearest_edge(&edges, x, y) else {
                    return frame.set_status_text("Click one of the highlighted edges", 0);
                };
                let Some(params) = self.weld_dialog() else { return };
                let args = merge(json!({ "element_ids": elements, "edge": edges[k] }), &params);
                match self.run("weld.create", args) {
                    Ok(_) => {
                        self.end_steel_tool();
                        self.refresh_scene(&frame);
                        frame.set_status_text("Weld placed (shown at Fine detail)", 0);
                    }
                    Err(e) => error(&frame, &format!("The weld was not placed:\n{e}")),
                }
            }
        }
    }

    fn finish_plate(&mut self, placement: Value, points: Vec<[f64; 2]>) {
        let frame = self.frame;
        if points.len() < 3 {
            return frame.set_status_text("A plate needs at least three corners", 0);
        }
        // Contour Cut sketches the same way, then cuts its element.
        if let Some(target) = self.contour_target {
            let args = merge(placement, &json!({ "contour": points, "element_id": target }));
            match self.run("steel_modifier.contour_cut", args) {
                Ok(_) => {
                    self.end_steel_tool();
                    self.refresh_scene(&frame);
                    frame.set_status_text("Contour cut made", 0);
                }
                Err(e) => error(&frame, &format!("The contour was not cut:\n{e}")),
            }
            return;
        }
        let args = merge(placement, &json!({ "contour": points }));
        match self.run("plate.create", args) {
            Ok(_) => {
                self.end_steel_tool();
                self.refresh_scene(&frame);
                frame.set_status_text("Plate placed (shown at Fine detail; thickness and justification in Properties)", 0);
            }
            Err(e) => error(&frame, &format!("The plate was not placed:\n{e}")),
        }
    }

    pub(crate) fn steel_motion(&mut self, x: i32, y: i32) {
        let Some(tool) = self.steel_tool.clone() else { return };
        match tool {
            SteelTool::Plate { placement, plane, points, .. } => {
                let cursor = self.on_plane(&plane, x, y);
                self.steel_tool = Some(SteelTool::Plate { placement, plane, points, cursor });
            }
            SteelTool::Weld { elements, edges, .. } => {
                let hot = self.nearest_edge(&edges, x, y);
                self.steel_tool = Some(SteelTool::Weld { elements, edges, hot });
            }
            SteelTool::Pattern { .. } => return,
        }
        self.steel_preview();
    }

    /// Keys: Enter finishes a plate, Backspace takes its last corner back.
    pub(crate) fn steel_key(&mut self, key: i32) -> bool {
        let Some(SteelTool::Plate { placement, plane, mut points, cursor }) = self.steel_tool.clone() else {
            return false;
        };
        match key {
            WXK_RETURN | WXK_NUMPAD_ENTER => self.finish_plate(placement, points),
            WXK_BACK => {
                points.pop();
                self.steel_tool = Some(SteelTool::Plate { placement, plane, points, cursor });
                self.steel_preview();
            }
            _ => return false,
        }
        true
    }

    fn nearest_edge(&self, edges: &[[[f64; 3]; 2]], x: i32, y: i32) -> Option<usize> {
        let (x, y) = (x as f32, y as f32);
        edges
            .iter()
            .enumerate()
            .filter_map(|(k, e)| {
                let (a, b) = (self.screen(e[0])?, self.screen(e[1])?);
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let l2 = (dx * dx + dy * dy).max(1e-6);
                let t = (((x - a.0) * dx + (y - a.1) * dy) / l2).clamp(0.0, 1.0);
                let d = (x - (a.0 + dx * t)).hypot(y - (a.1 + dy * t));
                (d < 12.0).then_some((k, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, _)| k)
    }

    /// The tool's preview: a plate's corners and the edge to the cursor, a weld's edges.
    fn steel_preview(&mut self) {
        let plan_z = self.active_view().and_then(|v| v.plan.as_ref()).map(|pl| pl.range.cut as f32 + 0.004);
        let f = |p: [f64; 3]| [p[0] as f32, p[1] as f32, plan_z.unwrap_or(p[2] as f32)];
        // Paper millimetres: 3D views draw them much wider than plans.
        let thin = if plan_z.is_some() { 0.5 } else { 0.08 };
        let lines = match &self.steel_tool {
            Some(SteelTool::Plate { plane, points, cursor, .. }) => {
                let mut pts: Vec<[f64; 3]> = points.iter().map(|q| plane.to_world(*q)).collect();
                if let Some(c) = cursor {
                    pts.push(plane.to_world(*c));
                }
                let mut segs: Vec<[[f32; 3]; 2]> = pts.windows(2).map(|w| [f(w[0]), f(w[1])]).collect();
                if pts.len() >= 3 {
                    segs.push([f(pts[pts.len() - 1]), f(pts[0])]);
                }
                vec![Lines { color: PREVIEW, width: thin, segments: segs }]
            }
            Some(SteelTool::Weld { edges, hot, .. }) => {
                let mut cold = Vec::new();
                let mut warm = Vec::new();
                for (k, e) in edges.iter().enumerate() {
                    let s = [f(e[0]), f(e[1])];
                    if Some(k) == *hot { warm.push(s) } else { cold.push(s) }
                }
                vec![
                    Lines { color: EDGE, width: thin, segments: cold },
                    Lines { color: HOT, width: thin * 2.5, segments: warm },
                ]
            }
            _ => Vec::new(),
        };
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&lines);
        }
        self.refresh_active_view();
    }

    fn refresh_active_view(&self) {
        if let Some(v) = self.active_view() {
            v.panel.refresh(false, None);
        }
    }

    /// Steel › Bolts.
    pub(crate) fn start_pattern_tool(&mut self) {
        let frame = self.frame;
        self.end_tool(&frame);
        let selected = self.selection();
        let faces = self.selected_faces();
        if selected.is_empty() || faces.is_empty() {
            return frame.set_status_text("Bolts: select the steel elements to join (or one for shear studs) first", 0);
        }
        let dialog = Dialog::builder(&frame, "Fastener Pattern").build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let grid = FlexGridSizer::builder(0, 2).with_vgap(6).with_hgap(10).build();
        let label = |t: &str| StaticText::builder(&dialog).with_label(t).build();
        let kind = Choice::builder(&dialog)
            .with_choices(vec!["Bolts".into(), "Anchors".into(), "Holes".into(), "Shear studs".into()])
            .build();
        kind.set_selection(if selected.len() == 1 { 3 } else { 0 });
        let face = Choice::builder(&dialog).with_choices(faces.iter().map(|f| f.2.clone()).collect()).build();
        face.set_selection(0);
        let shape = Choice::builder(&dialog).with_choices(vec!["Rectangular".into(), "Circular".into()]).build();
        shape.set_selection(0);
        let text = |v: &str| TextCtrl::builder(&dialog).with_value(v).build();
        let (rows, cols, sx, sy) = (text("2"), text("3"), text("75"), text("75"));
        let (count, radius, dia) = (text("4"), text("60"), text(""));
        macro_rules! row {
            ($l:expr, $w:expr) => {{
                grid.add(&label($l), 0, SizerFlag::AlignCenterVertical, 0);
                grid.add(&$w, 1, SizerFlag::Expand, 0);
            }};
        }
        row!("Kind", kind);
        row!("Square to the face", face);
        row!("Layout", shape);
        row!("Rows", rows);
        row!("Columns", cols);
        row!("Spacing x (mm)", sx);
        row!("Spacing y (mm)", sy);
        row!("Count (circular)", count);
        row!("Radius (circular, mm)", radius);
        row!("Diameter (mm, blank for the default)", dia);
        sizer.add_sizer(&grid, 1, SizerFlag::All | SizerFlag::Expand, 10);
        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        let cancel = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Cancel").build();
        let ok = Button::builder(&dialog).with_id(ID_OK).with_label("Place").build();
        buttons.add_stretch_spacer(1);
        buttons.add(&cancel, 0, SizerFlag::All, 6);
        buttons.add(&ok, 0, SizerFlag::All, 6);
        sizer.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
        dialog.set_sizer_and_fit(sizer, true);
        dialog.set_affirmative_id(ID_OK);
        let go = dialog.show_modal() == ID_OK;
        let num = |t: &TextCtrl| t.get_value().trim().parse::<f64>().ok();
        let picked = (
            kind.get_selection().unwrap_or(0),
            face.get_selection().and_then(|i| faces.get(i as usize).cloned()),
            shape.get_selection().unwrap_or(0),
            [num(&rows), num(&cols), num(&sx), num(&sy), num(&count), num(&radius), num(&dia)],
        );
        dialog.destroy();
        if !go {
            return;
        }
        let (k, Some((fid, fname, _)), s, v) = picked else { return };
        let command = ["bolt_pattern.create", "anchor_pattern.create", "hole_pattern.create", "shear_stud_pattern.create"][k as usize];
        let mm = |x: Option<f64>, d: f64| x.unwrap_or(d) / 1000.0;
        let layout = if s == 0 {
            json!({ "shape": "rect", "rows": v[0].unwrap_or(2.0) as usize, "columns": v[1].unwrap_or(3.0) as usize,
                    "spacing_x": mm(v[2], 75.0), "spacing_y": mm(v[3], 75.0) })
        } else {
            json!({ "shape": "circle", "count": v[4].unwrap_or(4.0) as usize, "radius": mm(v[5], 60.0) })
        };
        let mut args = json!({ "element_ids": selected, "face": { "element": fid, "face": fname }, "layout": layout });
        if let Some(d) = v[6] {
            args["diameter"] = json!(d / 1000.0);
        }
        let Some(plane) = self.face_plane_of(fid, fname) else { return };
        self.steel_tool = Some(SteelTool::Pattern { command, args, plane });
        frame.set_status_text("Fasteners: click where the pattern's centre goes on the face (Esc ends)", 0);
    }

    /// Steel › Weld.
    pub(crate) fn start_weld_tool(&mut self) {
        let frame = self.frame;
        self.end_tool(&frame);
        let elements = self.selection();
        if elements.len() < 2 {
            return frame.set_status_text("Weld: select the elements to weld (two or more) first", 0);
        }
        if self.active_view().is_none_or(|v| v.is_2d()) {
            return frame.set_status_text("Weld: edges are picked in a 3D view", 0);
        }
        let edges: Vec<[[f64; 3]; 2]> = match self.run("weld.edges", json!({ "element_ids": elements })) {
            Ok(v) => serde_json::from_value(v).unwrap_or_default(),
            Err(e) => return error(&frame, &e),
        };
        if edges.is_empty() {
            return frame.set_status_text("Weld: these elements do not touch (within 1 mm) anywhere", 0);
        }
        let n = edges.len();
        self.steel_tool = Some(SteelTool::Weld { elements, edges, hot: None });
        self.steel_preview();
        frame.set_status_text(&format!("Weld: click one of the {n} highlighted edges where the elements touch (Esc ends)"), 0);
    }

    /// The weld's type, throat and location.
    fn weld_dialog(&mut self) -> Option<Value> {
        let frame = self.frame;
        let tables = self.run("weld.tables", json!({})).ok()?;
        let types: Vec<String> = serde_json::from_value(tables["types"].clone()).unwrap_or_default();
        let dialog = Dialog::builder(&frame, "Weld").build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let grid = FlexGridSizer::builder(0, 2).with_vgap(6).with_hgap(10).build();
        let ty = Choice::builder(&dialog).with_choices(types.iter().map(|t| t.replace('_', " ")).collect()).build();
        ty.set_selection(0);
        let a = TextCtrl::builder(&dialog).with_value("6").build();
        let loc = Choice::builder(&dialog).with_choices(vec!["Shop".into(), "Site".into()]).build();
        loc.set_selection(0);
        macro_rules! row {
            ($l:expr, $w:expr) => {{
                grid.add(&StaticText::builder(&dialog).with_label($l).build(), 0, SizerFlag::AlignCenterVertical, 0);
                grid.add(&$w, 1, SizerFlag::Expand, 0);
            }};
        }
        row!("Type", ty);
        row!("Throat a (mm)", a);
        row!("Location", loc);
        sizer.add_sizer(&grid, 1, SizerFlag::All | SizerFlag::Expand, 10);
        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        let cancel = Button::builder(&dialog).with_id(ID_CANCEL).with_label("Cancel").build();
        let ok = Button::builder(&dialog).with_id(ID_OK).with_label("Weld").build();
        buttons.add_stretch_spacer(1);
        buttons.add(&cancel, 0, SizerFlag::All, 6);
        buttons.add(&ok, 0, SizerFlag::All, 6);
        sizer.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
        dialog.set_sizer_and_fit(sizer, true);
        dialog.set_affirmative_id(ID_OK);
        let go = dialog.show_modal() == ID_OK;
        let out = json!({
            "weld_type": ty.get_selection().and_then(|i| types.get(i as usize)).cloned().unwrap_or_else(|| "fillet".into()),
            "a": a.get_value().trim().parse::<f64>().unwrap_or(6.0) / 1000.0,
            "location": if loc.get_selection() == Some(1) { "site" } else { "shop" },
        });
        dialog.destroy();
        go.then_some(out)
    }

    // ---- Properties ----

    /// The detailing element an element is or belongs to (a pattern for its fasteners).
    fn detail_of(&self, id: ElementId) -> Option<(ElementId, &'static str)> {
        let store = self.store_now()?;
        let e = store.get(id)?;
        let parent = reviera_core::steel_fab::fab_recipe(e)
            .and_then(|r| reviera_core::id::GlobalId::parse(&r.connection))
            .and_then(|g| store.find_by_guid(&g));
        let (id, e) = match parent {
            Some(p) => (p, store.get(p)?),
            None => (id, e),
        };
        if reviera_core::steel_detail::plate_recipe(e).is_some() {
            Some((id, "plate"))
        } else if reviera_core::steel_detail::pattern_recipe(e).is_some() {
            Some((id, "pattern"))
        } else if reviera_core::steel_detail::weld_recipe(e).is_some() {
            Some((id, "weld"))
        } else {
            None
        }
    }

    pub(crate) fn detail_rows(&mut self, grid: &PropertyGrid, id: ElementId) -> bool {
        let Some((did, kind)) = self.detail_of(id) else { return false };
        let mm = |v: &Value| format!("{:.1}", v.as_f64().unwrap_or(0.0) * 1000.0);
        let ro = |grid: &PropertyGrid, label: &str, name: &str, value: String| {
            grid.append(Property::string(label, name, value));
            grid.set_property_read_only(name, true);
        };
        match kind {
            "plate" => {
                let Ok(i) = self.run("plate.info", json!({ "element_ids": [did] })) else { return false };
                let i = &i[0];
                grid.append(Property::category("Plate", "c_dt_plate"));
                grid.append(Property::string("Thickness (mm)", "dt_thickness", mm(&i["thickness"])));
                let j = ["top", "centre", "bottom"];
                let at = j.iter().position(|x| i["justification"] == *x).unwrap_or(2) as i32;
                grid.append(Property::enumeration(
                    "Justification",
                    "dt_justification",
                    vec![("Top".to_owned(), 0), ("Centre".to_owned(), 1), ("Bottom".to_owned(), 2)],
                    at,
                ));
                grid.append(Property::string("Role", "dt_role", i["role"].as_str().unwrap_or("").to_owned()));
                grid.append(Property::string("Material", "dt_material", i["material"].as_str().unwrap_or("").to_owned()));
                ro(grid, "Plane", "dt_plane", i["plane"].as_str().unwrap_or("").to_owned());
                ro(grid, "Hosted", "dt_hosted", if i["hosted"] == true { "Yes".into() } else { "No (its host is gone)".into() });
                ro(grid, "Area (m²)", "dt_area", format!("{:.4}", i["area"].as_f64().unwrap_or(0.0)));
            }
            "pattern" => {
                let Ok(i) = self.run("fastener_pattern.info", json!({ "element_ids": [did] })) else { return false };
                let i = &i[0];
                grid.append(Property::category("Fastener Pattern", "c_dt_pattern"));
                ro(grid, "Kind", "dt_kind", i["pattern"].as_str().unwrap_or("").replace('_', " "));
                ro(grid, "Count", "dt_count", i["count"].to_string());
                grid.append(Property::string("Diameter (mm)", "dt_diameter", mm(&i["diameter"])));
                ro(grid, "Grip (mm)", "dt_grip", mm(&i["grip"]));
                ro(grid, "Length (mm)", "dt_length", mm(&i["length"]));
                ro(grid, "Hole diameter (mm)", "dt_hole", mm(&i["hole_diameter"]));
                let holes: Vec<String> = i["holes"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .map(|(e, n)| format!("{n} in {e}"))
                    .collect();
                ro(grid, "Holes", "dt_holes", holes.join(", "));
                let at = if i["location"] == "site" { 1 } else { 0 };
                grid.append(Property::enumeration("Location", "dt_location", vec![("Shop".to_owned(), 0), ("Site".to_owned(), 1)], at));
            }
            "weld" => {
                let Ok(i) = self.run("weld.info", json!({ "element_ids": [did] })) else { return false };
                let i = &i[0];
                grid.append(Property::category("Weld", "c_dt_weld"));
                let types: Vec<String> = self
                    .run("weld.tables", json!({}))
                    .ok()
                    .and_then(|t| serde_json::from_value(t["types"].clone()).ok())
                    .unwrap_or_default();
                let at = types.iter().position(|t| i["weld_type"] == *t).unwrap_or(0) as i32;
                grid.append(Property::enumeration(
                    "Type",
                    "dt_weld_type",
                    types.iter().enumerate().map(|(k, t)| (t.replace('_', " "), k as i32)).collect::<Vec<(String, i32)>>(),
                    at,
                ));
                grid.append(Property::string("Throat a (mm)", "dt_a", mm(&i["a"])));
                ro(grid, "Length (mm)", "dt_weld_length", mm(&i["length"]));
                let at = if i["location"] == "site" { 1 } else { 0 };
                grid.append(Property::enumeration("Location", "dt_location", vec![("Shop".to_owned(), 0), ("Site".to_owned(), 1)], at));
                let review = i["needs_review"] == true;
                ro(grid, "Needs review", "dt_review", if review { "Yes: an element changed".into() } else { "No".into() });
                if review {
                    grid.append(Property::string("Accept", "dt_accept", "Type yes to accept the weld as it is"));
                }
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn detail_edit(&mut self, property: &str, text: Option<String>) -> bool {
        if !property.starts_with("dt_") {
            return false;
        }
        let frame = self.frame;
        let Some((did, kind)) = self.selection().first().and_then(|id| self.detail_of(*id)) else {
            return true;
        };
        let text = text.unwrap_or_default();
        let mm = |t: &str| t.trim().trim_end_matches("mm").trim().parse::<f64>().ok().map(|v| v / 1000.0);
        let result = match (kind, property) {
            ("plate", "dt_thickness") => match mm(&text) {
                Some(t) => self.run("plate.set", json!({ "plate_ids": [did], "thickness": t })),
                None => Err(format!("`{text}` is not a thickness in mm")),
            },
            ("plate", "dt_justification") => {
                self.run("plate.set", json!({ "plate_ids": [did], "justification": text.to_lowercase() }))
            }
            ("plate", "dt_role") => self.run("plate.set", json!({ "plate_ids": [did], "role": text })),
            ("plate", "dt_material") => self.run("plate.set", json!({ "plate_ids": [did], "material": text })),
            ("pattern", "dt_diameter") => match mm(&text) {
                Some(d) => self.run("fastener_pattern.set", json!({ "pattern_ids": [did], "diameter": d })),
                None => Err(format!("`{text}` is not a diameter in mm")),
            },
            ("pattern", "dt_location") => {
                self.run("fastener_pattern.set", json!({ "pattern_ids": [did], "location": text.to_lowercase() }))
            }
            ("weld", "dt_weld_type") => {
                self.run("weld.set", json!({ "weld_ids": [did], "weld_type": text.replace(' ', "_") }))
            }
            ("weld", "dt_a") => match mm(&text) {
                Some(a) => self.run("weld.set", json!({ "weld_ids": [did], "a": a })),
                None => Err(format!("`{text}` is not a throat in mm")),
            },
            ("weld", "dt_location") => self.run("weld.set", json!({ "weld_ids": [did], "location": text.to_lowercase() })),
            ("weld", "dt_accept") if text.trim().eq_ignore_ascii_case("yes") => {
                self.run("weld.set", json!({ "weld_ids": [did], "accept": true }))
            }
            _ => return true,
        };
        if let Err(e) = result {
            error(&frame, &format!("The change was not made:\n{e}"));
        }
        self.properties_stale = true;
        self.refresh_scene(&frame);
        true
    }
}
