// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Concrete › Reinforcement: area, path and fabric reinforcement (STR-RBR-010, 011, 012). Each
//! tool works on the selected structural floor or wall; boundaries, paths and points are entered
//! in the host's plane (plan x, y for floors; along the wall and up from its base for walls),
//! starting from the host's outline. A selected system's Properties show its layers or sheets.

use glam::Vec3;
use reviera_core::ElementId;
use reviera_core::rebar::HostKind;
use reviera_core::work_planes::Plane;
use reviera_render::RenderBackend;
use reviera_render::plan::Lines;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::rebar_ui::row;
use super::{Gui, error};

/// A dialog of labelled fields.
pub(crate) struct Form {
    pub(crate) dialog: Dialog,
    sizer: BoxSizer,
    grid: FlexGridSizer,
}

impl Form {
    pub(crate) fn new(frame: &Frame, title: &str) -> Self {
        let dialog = Dialog::builder(frame, title).with_style(DialogStyle::DefaultDialogStyle).build();
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        let grid = FlexGridSizer::builder(0, 2).with_vgap(6).with_hgap(10).build();
        grid.add_growable_col(1, 1);
        Form { dialog, sizer, grid }
    }

    pub(crate) fn text(&self, label: &str, value: &str, tip: &str) -> TextCtrl {
        row(&self.dialog, &self.grid, label);
        let t = TextCtrl::builder(&self.dialog).with_value(value).build();
        if !tip.is_empty() {
            t.set_tooltip(tip);
        }
        t.set_min_size(Size::new(260, -1));
        self.grid.add(&t, 1, SizerFlag::Expand, 0);
        t
    }

    /// A text box with a drop-down of suggestions (any value may be typed).
    pub(crate) fn combo(&self, label: &str, items: &[String], value: &str, tip: &str) -> ComboBox {
        row(&self.dialog, &self.grid, label);
        let c = ComboBox::builder(&self.dialog).with_value(value).with_choices(items.to_vec()).build();
        if !tip.is_empty() {
            c.set_tooltip(tip);
        }
        c.set_min_size(Size::new(260, -1));
        self.grid.add(&c, 1, SizerFlag::Expand, 0);
        c
    }

    /// A masked text box (a secret).
    pub(crate) fn password(&self, label: &str, tip: &str) -> TextCtrl {
        row(&self.dialog, &self.grid, label);
        let t = TextCtrl::builder(&self.dialog).with_style(TextCtrlStyle::Password).build();
        if !tip.is_empty() {
            t.set_tooltip(tip);
        }
        t.set_min_size(Size::new(260, -1));
        self.grid.add(&t, 1, SizerFlag::Expand, 0);
        t
    }

    pub(crate) fn choice(&self, label: &str, items: &[String], selected: usize) -> Choice {
        row(&self.dialog, &self.grid, label);
        let c = Choice::builder(&self.dialog).with_choices(items.to_vec()).build();
        c.set_selection(selected.min(items.len().saturating_sub(1)) as u32);
        self.grid.add(&c, 1, SizerFlag::Expand, 0);
        c
    }

    /// A text box of several lines.
    pub(crate) fn lines(&self, label: &str, value: &str, tip: &str) -> TextCtrl {
        row(&self.dialog, &self.grid, label);
        let t = TextCtrl::builder(&self.dialog).with_value(value).with_style(TextCtrlStyle::MultiLine).build();
        if !tip.is_empty() {
            t.set_tooltip(tip);
        }
        t.set_min_size(Size::new(260, 110));
        self.grid.add(&t, 1, SizerFlag::Expand, 0);
        t
    }

    pub(crate) fn check(&self, label: &str, text: &str, value: bool) -> CheckBox {
        row(&self.dialog, &self.grid, label);
        let c = CheckBox::builder(&self.dialog).with_label(text).build();
        c.set_value(value);
        self.grid.add(&c, 1, SizerFlag::Expand, 0);
        c
    }

    /// Shows the form; `true` when accepted (destroy the dialog after reading it).
    pub(crate) fn show(&self, note: &str, ok_label: &str) -> bool {
        self.sizer.add_sizer(&self.grid, 1, SizerFlag::All | SizerFlag::Expand, 12);
        if !note.is_empty() {
            let n = StaticText::builder(&self.dialog).with_label(note).build();
            self.sizer.add(&n, 0, SizerFlag::Left | SizerFlag::Right, 12);
        }
        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        buttons.add_stretch_spacer(1);
        let cancel = Button::builder(&self.dialog).with_id(ID_CANCEL).with_label("Cancel").build();
        let ok = Button::builder(&self.dialog).with_id(ID_OK).with_label(ok_label).build();
        buttons.add(&cancel, 0, SizerFlag::All, 6);
        buttons.add(&ok, 0, SizerFlag::All, 6);
        self.sizer.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
        self.dialog.set_sizer_and_fit(self.sizer, true);
        self.dialog.set_affirmative_id(ID_OK);
        ok.set_default();
        self.dialog.show_modal() == ID_OK
    }
}

fn at(c: &Choice) -> usize {
    c.get_selection().map_or(0, |i| i as usize)
}

fn mm(t: &TextCtrl) -> Option<f64> {
    t.get_value().trim().parse::<f64>().ok().map(|v| v / 1000.0)
}

const SKETCH: &str = "Sketch in the view";

const PREVIEW: [f32; 4] = [0.85, 0.0, 0.75, 1.0];
const MAJOR: [f32; 4] = [0.1, 0.45, 0.95, 1.0];

/// What a reinforcement sketch makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SketchWhat {
    Area,
    Path,
    FabricArea,
    FabricSheet,
    /// A bar's centreline (STR-RBR-007).
    Bar,
}

/// A boundary, path or sheet placement sketched on a floor's or wall's plane (the same
/// coordinates the commands take). Points come from clicks (snapping to the host's outline) or
/// the Coordinates dialog.
#[derive(Debug, Clone)]
pub struct RebarSketch {
    pub what: SketchWhat,
    pub args: Value,
    pub plane: Plane,
    pub outline: Vec<[f64; 2]>,
    pub points: Vec<[f64; 2]>,
    pub cursor: Option<[f64; 2]>,
    /// Two corners make a rectangle.
    pub rectangle: bool,
    /// The edge the major direction follows.
    pub major: usize,
    /// A fabric sheet's length and width, for its preview.
    pub sheet: Option<(f64, f64)>,
}

impl RebarSketch {
    /// The points as they will be sent: a rectangle's two corners give its four.
    pub fn shape(&self) -> Vec<[f64; 2]> {
        match (self.rectangle, &self.points[..]) {
            (true, [a, b, ..]) => vec![*a, [b[0], a[1]], *b, [a[0], b[1]]],
            _ => self.points.clone(),
        }
    }

    pub fn closed(&self) -> bool {
        matches!(self.what, SketchWhat::Area | SketchWhat::FabricArea)
    }
}

impl Gui {
    /// The selected structural floor or wall that can hold rebar, with its outline in its plane.
    fn layer_host(&mut self, tool: &str) -> Option<(ElementId, HostKind, Vec<[f64; 2]>)> {
        let frame = self.frame;
        let store = self.store_now()?;
        let found = self.selection().into_iter().find_map(|id| {
            let e = store.get(id)?;
            let kind = reviera_core::rebar::host_check(&store, e).ok().filter(|k| matches!(k, HostKind::Floor | HostKind::Wall))?;
            Some((id, kind, reviera_core::rebar_systems::outline(&store, e)?))
        });
        if found.is_none() {
            frame.set_status_text(&format!("{tool}: select a structural concrete floor or wall first"), 0);
        }
        found
    }

    fn faces_of(kind: HostKind) -> [&'static str; 2] {
        if kind == HostKind::Wall { ["exterior", "interior"] } else { ["top", "bottom"] }
    }

    fn bar_list(&mut self) -> (Vec<String>, Vec<String>) {
        let bars = self.run("rebar_bar_type.list", json!({})).unwrap_or_default();
        let list = bars.as_array().cloned().unwrap_or_default();
        (
            list.iter().filter_map(|b| b["id"].as_str().map(str::to_owned)).collect(),
            list.iter().filter_map(|b| b["name"].as_str().map(str::to_owned)).collect(),
        )
    }

    fn placed(&mut self, what: &str, result: Result<Value, String>) {
        let frame = self.frame;
        match result {
            Ok(v) => {
                self.refresh_scene(&frame);
                self.properties_stale = true;
                frame.set_status_text(&format!("{what}: placed as element {v}"), 0);
            }
            Err(e) => error(&frame, &format!("{what} was not placed:\n{e}")),
        }
    }

    /// Concrete › Reinforcement › Area.
    pub(crate) fn place_area_reinforcement(&mut self) {
        let Some((host, kind, outline)) = self.layer_host("Area Reinforcement") else { return };
        let (bar_ids, bar_names) = self.bar_list();
        let hooks = self.run("rebar_hook_type.list", json!({})).unwrap_or_default();
        let hook_ids: Vec<String> = hooks.as_array().into_iter().flatten().filter_map(|h| h["id"].as_str().map(str::to_owned)).collect();
        let mut hook_names = vec!["None".to_owned()];
        hook_names.extend(hooks.as_array().into_iter().flatten().filter_map(|h| h["name"].as_str().map(str::to_owned)));
        let [outer, inner] = Self::faces_of(kind);
        let frame = self.frame;
        let f = Form::new(&frame, "Area Reinforcement");
        let whole = f.choice("Boundary", &[SKETCH.into(), "The whole host".into()], 0);
        let bar = f.choice("Bar type", &bar_names, 3);
        let rule = f.choice("Layout", &["Maximum spacing".into(), "Fixed number".into()], 0);
        let spacing = f.text("Spacing (mm) or number", "200", "");
        let layers = [
            f.check("Layers", &format!("{outer} major"), true),
            f.check("", &format!("{outer} minor"), true),
            f.check("", &format!("{inner} major"), true),
            f.check("", &format!("{inner} minor"), true),
        ];
        let hook = f.choice("Hooks", &hook_names, 0);
        let extra = f.text("Additional offsets (mm)", "0, 0", &format!("{outer}, {inner}: further in than the cover, to stack layers"));
        if !f.show("Openings and the boundary cut the bars; each face's minor layer sits inside its major one", "Place") {
            f.dialog.destroy();
            return;
        }
        let bar_id = bar_ids.get(at(&bar)).cloned().unwrap_or_default();
        let fixed = at(&rule) == 1;
        let amount = spacing.get_value().trim().parse::<f64>().unwrap_or(200.0);
        let hook_id = at(&hook).checked_sub(1).and_then(|i| hook_ids.get(i).cloned());
        let specs = [(outer, "major"), (outer, "minor"), (inner, "major"), (inner, "minor")];
        let layers: Vec<Value> = specs
            .iter()
            .zip(&layers)
            .map(|((face, dir), on)| {
                let mut l = json!({ "face": face, "direction": dir, "bar_type": bar_id, "on": on.get_value() });
                if fixed {
                    l["n"] = json!(amount.round() as usize);
                } else {
                    l["spacing"] = json!(amount / 1000.0);
                }
                if let Some(h) = &hook_id {
                    l["hook"] = json!(h);
                }
                l
            })
            .collect();
        let offsets: Vec<f64> = extra.get_value().split(',').map(|v| v.trim().parse::<f64>().unwrap_or(0.0) / 1000.0).collect();
        let args = json!({
            "host_id": host,
            "layout_rule": if fixed { "fixed_number" } else { "maximum_spacing" },
            "layers": layers,
            "additional_top": offsets.first().copied().unwrap_or(0.0),
            "additional_bottom": offsets.get(1).copied().unwrap_or(0.0),
        });
        let sketch = at(&whole) == 0;
        f.dialog.destroy();
        if sketch {
            return self.start_rebar_sketch(SketchWhat::Area, host, outline, args);
        }
        let r = self.run("area_reinforcement.create", args);
        self.placed("Area reinforcement", r);
    }

    /// Concrete › Reinforcement › Sketch Rebar (STR-RBR-007): the bar's options, then its
    /// corners in the host's plane.
    pub(crate) fn sketch_rebar(&mut self) {
        let frame = self.frame;
        let Some(&(host, kind)) = self.rebar_hosts().first() else {
            return frame.set_status_text("Sketch Rebar: select a concrete column, beam, structural floor or wall first", 0);
        };
        if kind == HostKind::Roof {
            return frame.set_status_text("Sketch Rebar: a roof's faces are not planes; use Free Form Rebar", 0);
        }
        let Some(store) = self.store_now() else { return };
        let Some(e) = store.get(host) else { return };
        let outline = match kind {
            HostKind::Floor | HostKind::Wall => reviera_core::rebar_systems::outline(&store, e).unwrap_or_default(),
            HostKind::Roof => Vec::new(),
            HostKind::Beam | HostKind::Column => {
                // The long section through the axis: along and up for a beam, across and up for a
                // column, from the host's own plane.
                let plane = reviera_core::rebar_sketch::host_plane(&store, e);
                let corners: Vec<[f64; 2]> = reviera_core::rebar_sketch::host_faces(&store, e, kind)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|f| plane.map(|p| p.to_local(f.point)))
                    .collect();
                let (lo, hi) = corners.iter().fold(([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]), |(l, h), q| ([l[0].min(q[0]), l[1].min(q[1])], [h[0].max(q[0]), h[1].max(q[1])]));
                if corners.is_empty() { Vec::new() } else { vec![lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]] }
            }
        };
        let (bar_ids, bar_names) = self.bar_list();
        let hooks = self.run("rebar_hook_type.list", json!({})).unwrap_or_default();
        let hook_ids: Vec<String> = hooks.as_array().into_iter().flatten().filter_map(|h| h["id"].as_str().map(str::to_owned)).collect();
        let mut hook_names = vec!["None".to_owned()];
        hook_names.extend(hooks.as_array().into_iter().flatten().filter_map(|h| h["name"].as_str().map(str::to_owned)));
        let f = Form::new(&frame, "Sketch Rebar");
        let bar = f.choice("Bar type", &bar_names, 3);
        let style = f.choice("Style", &["Standard".into(), "Stirrup or tie".into()], 0);
        let hook_start = f.choice("Start hook", &hook_names, 0);
        let hook_end = f.choice("End hook", &hook_names, 0);
        let rule = f.choice("Layout", &["Single".into(), "Fixed number".into(), "Maximum spacing".into()], 0);
        let amount = f.text("Number or spacing (mm)", "3", "The set spreads square to the sketch plane, to the far face's cover");
        if !f.show("Then click the bar's corners in the host's plane; it is matched to a shape code (or 99, special)", "Sketch") {
            f.dialog.destroy();
            return;
        }
        let hook = |c: &Choice| at(c).checked_sub(1).and_then(|i| hook_ids.get(i).cloned());
        let v = amount.get_value().trim().parse::<f64>().unwrap_or(0.0);
        let layout = match at(&rule) {
            1 => json!({ "rule": "fixed_number", "n": v.round().max(1.0) as usize }),
            2 => json!({ "rule": "maximum_spacing", "spacing": v / 1000.0 }),
            _ => json!({ "rule": "single" }),
        };
        let mut args = json!({
            "host_id": host,
            "bar_type": bar_ids.get(at(&bar)).cloned().unwrap_or_default(),
            "style": if at(&style) == 1 { "stirrup_tie" } else { "standard" },
            "layout": layout,
        });
        if let Some(h) = hook(&hook_start) {
            args["hook_start"] = json!(h);
        }
        if let Some(h) = hook(&hook_end) {
            args["hook_end"] = json!(h);
        }
        f.dialog.destroy();
        self.start_rebar_sketch(SketchWhat::Bar, host, outline, args);
    }

    /// Concrete › Reinforcement › Free Form (STR-RBR-013): bars over a roof's shell between two
    /// planes, or closed links square to a member's path.
    pub(crate) fn place_free_form_rebar(&mut self) {
        let frame = self.frame;
        let Some(&(host, kind)) = self.rebar_hosts().first() else {
            return frame.set_status_text("Free Form Rebar: select a concrete roof, column, beam, floor or wall first", 0);
        };
        let (bar_ids, bar_names) = self.bar_list();
        let paths: Vec<(&str, &str)> = match kind {
            HostKind::Beam | HostKind::Column => vec![("axis", "Along its axis")],
            HostKind::Wall => vec![("up", "Up the wall, at its middle"), ("along", "Along the wall, at mid-height")],
            _ => Vec::new(),
        };
        let f = Form::new(&frame, "Free Form Rebar");
        let surface = kind == HostKind::Roof;
        let (face, direction, path) = if surface {
            (
                Some(f.choice("Layer", &["Bottom".into(), "Top".into()], 0)),
                Some(f.choice("Bars run", &["Across x (planes square to x)".into(), "Across y (planes square to y)".into()], 0)),
                None,
            )
        } else if paths.is_empty() {
            return frame.set_status_text("Free Form Rebar: a floor takes layers or area reinforcement", 0);
        } else {
            (None, None, Some(f.choice("Path", &paths.iter().map(|p| p.1.to_owned()).collect::<Vec<_>>(), 0)))
        };
        let bar = f.choice("Bar type", &bar_names, if surface { 3 } else { 1 });
        let rule = f.choice("Layout", &["Maximum spacing".into(), "Fixed number".into()], 0);
        let amount = f.text("Spacing (mm) or number", if surface { "300" } else { "200" }, "");
        let note = if surface {
            "Bars follow the shell at the cover, between the planes, trimmed by the edge cover"
        } else {
            "Closed links square to the path, each side inside its face's cover, following the section"
        };
        if !f.show(note, "Place") {
            f.dialog.destroy();
            return;
        }
        let v = amount.get_value().trim().parse::<f64>().unwrap_or(0.0);
        let layout = if at(&rule) == 1 { json!({ "rule": "fixed_number", "n": v.round().max(1.0) as usize }) } else { json!({ "rule": "maximum_spacing", "spacing": v / 1000.0 }) };
        let mut args = json!({ "host_id": host, "bar_type": bar_ids.get(at(&bar)).cloned().unwrap_or_default(), "layout": layout });
        if surface {
            args["kind"] = json!("surface");
            args["face"] = json!(if face.as_ref().map_or(0, at) == 1 { "top" } else { "bottom" });
            args["direction"] = json!(if direction.as_ref().map_or(0, at) == 1 { "y" } else { "x" });
        } else {
            args["kind"] = json!("aligned");
            args["path_ref"] = json!(paths[path.as_ref().map_or(0, at).min(paths.len() - 1)].0);
        }
        f.dialog.destroy();
        let r = self.run("rebar.place_free_form", args);
        self.placed("Free form rebar", r);
    }

    /// Concrete › Reinforcement › Path.
    pub(crate) fn place_path_reinforcement(&mut self) {
        let Some((host, kind, outline)) = self.layer_host("Path Reinforcement") else { return };
        let (bar_ids, bar_names) = self.bar_list();
        let faces = Self::faces_of(kind);
        let frame = self.frame;
        let f = Form::new(&frame, "Path Reinforcement");
        let flip = f.check("Side", "Bars to the right of the path", false);
        let face = f.choice("Face", &faces.map(str::to_owned), 0);
        let spacing = f.text("Spacing (mm)", "150", "Between neighbouring bars along the path");
        let bar = f.choice("Primary bar type", &bar_names, 3);
        let length = f.text("Primary length (mm)", "1000", "");
        let legs = f.text("Legs into the host (mm)", "0", "Segments turned into the host at both ends; 0 for straight bars");
        let alt = f.check("Alternating bars", "Every second bar alternates", false);
        let alt_bar = f.choice("Alternating bar type", &bar_names, 3);
        let alt_length = f.text("Alternating length (mm)", "1000", "");
        let alt_offset = f.text("Alternating offset (mm)", "300", "Where the alternating bars start from the path");
        if !f.show("Then sketch the path in the view; bars run square to it, inside the host's cover", "Sketch Path") {
            f.dialog.destroy();
            return;
        }
        let bar_of = |c: &Choice| bar_ids.get(at(c)).cloned().unwrap_or_default();
        let legs = mm(&legs).unwrap_or(0.0);
        let mut args = json!({
            "host_id": host,
            "flip": flip.get_value(),
            "face": faces[at(&face).min(1)],
            "spacing": mm(&spacing).unwrap_or(0.15),
            "primary": { "bar_type": bar_of(&bar), "length": mm(&length).unwrap_or(1.0), "legs": legs },
        });
        if alt.get_value() {
            args["alternating"] = json!({ "bar_type": bar_of(&alt_bar), "length": mm(&alt_length).unwrap_or(1.0), "legs": legs, "offset": mm(&alt_offset).unwrap_or(0.0) });
        }
        f.dialog.destroy();
        self.start_rebar_sketch(SketchWhat::Path, host, outline, args);
    }

    fn sheet_list(&mut self) -> Vec<(String, String)> {
        self.run("fabric_sheet_type.list", json!({}))
            .unwrap_or_default()
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| (t["id"].as_str().unwrap_or("").to_owned(), format!("{} ({:.2} kg/m²)", t["name"].as_str().unwrap_or(""), t["unit_mass"].as_f64().unwrap_or(0.0))))
            .collect()
    }

    /// Concrete › Reinforcement › Fabric Area.
    pub(crate) fn place_fabric_area(&mut self) {
        let Some((host, kind, outline)) = self.layer_host("Fabric Area") else { return };
        let sheets = self.sheet_list();
        let faces = Self::faces_of(kind);
        let frame = self.frame;
        let f = Form::new(&frame, "Fabric Area");
        let whole = f.choice("Boundary", &[SKETCH.into(), "The whole host".into()], 0);
        let sheet = f.choice("Sheet type", &sheets.iter().map(|s| s.1.clone()).collect::<Vec<_>>(), 2);
        let laps = f.choice("Lap splices", &["Aligned".into(), "Staggered".into()], 0);
        let major_lap = f.text("End lap (mm)", "", "Where sheets meet end to end; blank for a minor spacing plus 50 mm");
        let minor_lap = f.text("Side lap (mm)", "", "Where sheets meet side by side; blank for a major spacing plus 50 mm");
        let face = f.choice("Face", &faces.map(str::to_owned), 1);
        if !f.show("Sheets are cut by the boundary, the host's openings and its cover", "Place") {
            f.dialog.destroy();
            return;
        }
        let mut args = json!({
            "host_id": host,
            "sheet_type_id": sheets.get(at(&sheet)).map(|s| s.0.clone()).unwrap_or_default(),
            "lap_position": if at(&laps) == 1 { "staggered" } else { "aligned" },
            "face": faces[at(&face).min(1)],
        });
        if let Some(v) = mm(&major_lap) {
            args["major_lap"] = json!(v);
        }
        if let Some(v) = mm(&minor_lap) {
            args["minor_lap"] = json!(v);
        }
        let sketch = at(&whole) == 0;
        f.dialog.destroy();
        if sketch {
            return self.start_rebar_sketch(SketchWhat::FabricArea, host, outline, args);
        }
        let r = self.run("fabric_area.create", args);
        self.placed("Fabric area", r);
    }

    /// Concrete › Reinforcement › Fabric Sheet.
    pub(crate) fn place_fabric_sheet(&mut self) {
        let Some((host, kind, outline)) = self.layer_host("Fabric Sheet") else { return };
        let sheets = self.sheet_list();
        let faces = Self::faces_of(kind);
        let frame = self.frame;
        let f = Form::new(&frame, "Fabric Sheet");
        let sheet = f.choice("Sheet type", &sheets.iter().map(|s| s.1.clone()).collect::<Vec<_>>(), 2);
        let cut = f.check("Cut", "Cut by the host's cover", true);
        let face = f.choice("Face", &faces.map(str::to_owned), 1);
        if !f.show("Then click the sheet's first corner and a point along its length", "Place in View") {
            f.dialog.destroy();
            return;
        }
        let args = json!({
            "host_id": host,
            "sheet_type_id": sheets.get(at(&sheet)).map(|s| s.0.clone()).unwrap_or_default(),
            "cut_by_cover": cut.get_value(),
            "face": faces[at(&face).min(1)],
        });
        f.dialog.destroy();
        self.start_rebar_sketch(SketchWhat::FabricSheet, host, outline, args);
    }

    /// Concrete › Reinforcement › Remove Area System.
    pub(crate) fn remove_area_system(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let systems: Vec<ElementId> = self
            .selection()
            .into_iter()
            .filter(|id| store.get(*id).and_then(reviera_core::rebar_systems::area_recipe).is_some())
            .collect();
        if systems.is_empty() {
            return frame.set_status_text("Remove Area System: select area reinforcement first", 0);
        }
        let mut kept = 0;
        for s in systems {
            match self.run("area_reinforcement.remove_system", json!({ "system_id": s })) {
                Ok(v) => kept += v.as_array().map_or(0, Vec::len),
                Err(e) => return error(&frame, &format!("The area system was not removed:\n{e}")),
            }
        }
        self.refresh_scene(&frame);
        self.properties_stale = true;
        frame.set_status_text(&format!("Area system removed; {kept} rebar set(s) kept"), 0);
    }

    /// The rows of an area, path or fabric system; `false` if `id` is not one.
    pub(crate) fn reinforcement_system_rows(&mut self, grid: &PropertyGrid, id: ElementId) -> bool {
        let Some(kind) = self.store_now().and_then(|s| s.get(id).and_then(reviera_core::rebar_systems::system_kind)) else {
            return false;
        };
        let title = match kind {
            "area" => "Area Reinforcement",
            "path" => "Path Reinforcement",
            "fabric_area" => "Fabric Area",
            _ => "Fabric Sheet",
        };
        grid.append(Property::category(title, "c_rsys"));
        let ro = |grid: &PropertyGrid, label: &str, name: &str, value: String| {
            grid.append(Property::string(label, name, value));
            grid.set_property_read_only(name, true);
        };
        let Ok(i) = self.run("reinforcement_system.info", json!({ "system_ids": [id] })) else {
            ro(grid, "Status", "rs_status", "Its bars cannot be worked out (the host changed?)".into());
            return true;
        };
        let i = &i[0];
        for (k, g) in i["groups"].as_array().into_iter().flatten().enumerate() {
            ro(
                grid,
                g["name"].as_str().unwrap_or(""),
                &format!("rs_group{k}"),
                format!("{} × {}", g["count"].as_u64().unwrap_or(0), g["bar_type"].as_str().unwrap_or("")),
            );
        }
        if let Some(t) = i["sheet_type"].as_str() {
            let sheets = i["sheets"].as_array().map_or(0, Vec::len);
            let cut = i["sheets"].as_array().into_iter().flatten().filter(|s| s["cut"] == true).count();
            ro(grid, "Sheet type", "rs_sheet", t.to_owned());
            ro(grid, "Sheets", "rs_sheets", format!("{sheets} ({cut} cut)"));
            // Fabric numbers and partition (STR-RBR-016).
            let mut numbers: Vec<u32> = self.store_now().map(|st| reviera_core::rebar_numbers::number_of(&st, id)).unwrap_or_default();
            numbers.sort_unstable();
            numbers.dedup();
            let list: Vec<String> = numbers.iter().map(u32::to_string).collect();
            ro(grid, "Fabric Numbers", "rs_numbers", list.join(", "));
            let part = self.store_now().and_then(|st| st.get(id).map(reviera_core::rebar_numbers::partition)).unwrap_or_default();
            grid.append(Property::string("Partition", "rs_partition", part));
        }
        ro(grid, "Bars or wires", "rs_count", format!("{}", i["bar_count"].as_u64().unwrap_or(0)));
        ro(grid, "Total length", "rs_total", self.format_length(i["total_length"].as_f64().unwrap_or(0.0)));
        ro(grid, "Mass (kg)", "rs_mass", format!("{:.1}", i["mass"].as_f64().unwrap_or(0.0)));
        ro(grid, "Steel volume (m³)", "rs_volume", format!("{:.4}", i["volume"].as_f64().unwrap_or(0.0)));
        if matches!(kind, "area" | "path") {
            ro(grid, "Real rebar", "rs_real", if i["real_rebar"] == true { "Yes".into() } else { "No".into() });
        }
        if i["orphaned"] == true {
            ro(grid, "Orphaned", "rs_orphan", i["orphan_reason"].as_str().unwrap_or("").to_owned());
        }
        let r = &i["recipe"];
        match kind {
            "area" => {
                let s = r["layers"][0]["spacing"].as_f64().map_or(String::new(), |v| format!("{}", (v * 1e6).round() / 1000.0));
                grid.append(Property::string("Spacing (mm)", "rs_spacing", s));
                grid.set_help_string("rs_spacing", "Every layer's maximum spacing");
            }
            "path" => {
                grid.append(Property::boolean("Bars to the right", "rs_flip", r["flip"] == true));
                let s = r["layout"]["spacing"].as_f64().map_or(String::new(), |v| format!("{}", (v * 1e6).round() / 1000.0));
                grid.append(Property::string("Spacing (mm)", "rs_path_spacing", s));
            }
            "fabric_area" => {
                let laps = vec![("Aligned".to_owned(), 0), ("Staggered".to_owned(), 1)];
                grid.append(Property::enumeration("Lap splices", "rs_laps", laps, i32::from(r["lap_position"] == "staggered")));
                let v = |k: &str| r[k].as_f64().map_or(String::new(), |v| format!("{}", (v * 1e6).round() / 1000.0));
                grid.append(Property::string("End lap (mm)", "rs_major_lap", v("major_lap")));
                grid.append(Property::string("Side lap (mm)", "rs_minor_lap", v("minor_lap")));
            }
            _ => {}
        }
        true
    }

    /// An edit of a system's row; `false` if `property` is not one.
    pub(crate) fn reinforcement_system_edit(&mut self, property: &str, text: Option<String>) -> bool {
        if !property.starts_with("rs_") {
            return false;
        }
        let frame = self.frame;
        let Some(id) = self.selection().first().copied() else { return true };
        let text = text.unwrap_or_default();
        let num = || text.trim().parse::<f64>().ok().map(|v| v / 1000.0);
        let result = match property {
            "rs_partition" => self.run("rebar.set_partition", json!({ "ids": [id], "name": text })),
            "rs_spacing" => {
                let Ok(i) = self.run("reinforcement_system.info", json!({ "system_ids": [id] })) else { return true };
                let Some(s) = num() else { return true };
                let layers: Vec<Value> = i[0]["recipe"]["layers"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|l| {
                        let mut l = l.clone();
                        l["spacing"] = json!(s);
                        l
                    })
                    .collect();
                self.run("area_reinforcement.set", json!({ "system_ids": [id], "layers": layers }))
            }
            "rs_flip" => self.run("path_reinforcement.set", json!({ "system_ids": [id], "flip": text == "true" || text == "1" })),
            "rs_path_spacing" => match num() {
                Some(s) => self.run("path_reinforcement.set", json!({ "system_ids": [id], "layout": { "rule": "maximum_spacing", "spacing": s } })),
                None => return true,
            },
            "rs_laps" => {
                let staggered = text == "Staggered" || text == "1";
                self.run("fabric_area.set", json!({ "system_ids": [id], "lap_position": if staggered { "staggered" } else { "aligned" } }))
            }
            "rs_major_lap" | "rs_minor_lap" => match num() {
                Some(v) => {
                    let mut args = json!({ "system_ids": [id] });
                    args[&property[3..]] = json!(v);
                    self.run("fabric_area.set", args)
                }
                None => return true,
            },
            _ => return true,
        };
        if let Err(e) = result {
            error(&frame, &format!("The reinforcement was not changed:\n{e}"));
        }
        self.refresh_scene(&frame);
        self.properties_stale = true;
        true
    }
}

impl Gui {
    fn start_rebar_sketch(&mut self, what: SketchWhat, host: ElementId, outline: Vec<[f64; 2]>, args: Value) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let plane = store.get(host).and_then(|e| {
            if what == SketchWhat::Bar { reviera_core::rebar_sketch::host_plane(&store, e) } else { reviera_core::rebar_systems::sketch_plane(&store, e) }
        });
        let Some(plane) = plane else {
            return frame.set_status_text("The host's plane cannot be worked out", 0);
        };
        if !plane.is_horizontal() && self.active_view().is_some_and(|v| v.plan.is_some()) {
            frame.set_status_text(if what == SketchWhat::Bar {
                "This host's plane is upright: sketch in a 3D view; the Coordinates dialog (XY) works anywhere"
            } else {
"A wall's face is sketched in a 3D view; the Coordinates dialog (XY) works anywhere"
            }, 0);
        }
        let sheet = (what == SketchWhat::FabricSheet)
            .then(|| {
                let id = args["sheet_type_id"].as_str()?.to_owned();
                let list = self.run("fabric_sheet_type.list", json!({})).ok()?;
                let t = list.as_array()?.iter().find(|t| t["id"] == id.as_str())?.clone();
                Some((t["length"].as_f64()?, t["width"].as_f64()?))
            })
            .flatten();
        self.end_tool(&frame);
        self.rebar_sketch = Some(RebarSketch {
            what,
            args,
            plane,
            outline,
            points: Vec::new(),
            cursor: None,
            rectangle: false,
            major: 0,
            sheet,
        });
        self.rebar_sketch_prompt();
        self.rebar_sketch_preview();
    }

    pub(crate) fn end_rebar_sketch(&mut self) -> bool {
        if self.rebar_sketch.take().is_none() {
            return false;
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        true
    }

    fn rebar_sketch_prompt(&mut self) {
        let Some(t) = &self.rebar_sketch else { return };
        let n = t.points.len();
        let text = match t.what {
            SketchWhat::Area | SketchWhat::FabricArea if t.rectangle => {
                format!("Boundary (rectangle): click {} corner; R draws by corners, M picks the major edge, XY types coordinates, Esc ends", if n == 0 { "the first" } else { "the opposite" })
            }
            SketchWhat::Area | SketchWhat::FabricArea => format!(
                "Boundary: {n} corner(s); click corners, Enter or the first corner closes; R draws a rectangle, M picks the major edge (now edge {}), Backspace undoes, XY types coordinates",
                t.major
            ),
            SketchWhat::Path => format!(
                "Path: {n} point(s); click along the edge, Enter finishes; F flips the bars' side, Backspace undoes, XY types coordinates"
            ),
            SketchWhat::FabricSheet if n == 0 => "Fabric Sheet: click the sheet's first corner (XY types it)".to_owned(),
            SketchWhat::FabricSheet => "Fabric Sheet: click a point along its length, or Enter for the plane's x direction".to_owned(),
            SketchWhat::Bar => format!(
                "Sketch Rebar: {n} corner(s); click the bar's corners, Enter finishes, the first corner closes a link; Backspace undoes, XY types coordinates"
            ),
        };
        self.frame.set_status_text(&text, 0);
    }

    /// Where pixel (`x`, `y`) meets the sketch plane, snapped to the host's corners and edges and
    /// the sketch's first point (or the typed point fed by the Coordinates dialog).
    fn rebar_sketch_point(&self, x: i32, y: i32) -> Option<[f64; 2]> {
        let t = self.rebar_sketch.as_ref()?;
        if let Some(p) = crate::coords::typed() {
            return Some(p);
        }
        let v = self.active_view()?;
        let size = v.panel.get_client_size();
        let (o, d) = v.camera.ray(x as f32, y as f32, size.width as f32, size.height as f32);
        let (o, d) = ([o.x as f64, o.y as f64, o.z as f64], [d.x as f64, d.y as f64, d.z as f64]);
        let n = t.plane.normal;
        let den = d[0] * n[0] + d[1] * n[1] + d[2] * n[2];
        if den.abs() < 1e-9 {
            return None;
        }
        let w = [t.plane.origin[0] - o[0], t.plane.origin[1] - o[1], t.plane.origin[2] - o[2]];
        let s = (w[0] * n[0] + w[1] * n[1] + w[2] * n[2]) / den;
        let q = t.plane.to_local([o[0] + d[0] * s, o[1] + d[1] * s, o[2] + d[2] * s]);
        let screen = |p: [f64; 2]| {
            let w = t.plane.to_world(p);
            v.camera.project(Vec3::new(w[0] as f32, w[1] as f32, w[2] as f32), size.width as f32, size.height as f32)
        };
        let px = |p: [f64; 2]| screen(p).map_or(f32::MAX, |s| (s.0 - x as f32).hypot(s.1 - y as f32));
        // Corners first, then the nearest point on an edge, then free to the millimetre.
        let corners = t.outline.iter().chain(t.points.first()).copied();
        if let Some(c) = corners.map(|c| (px(c), c)).filter(|(d, _)| *d <= crate::snap::RADIUS_PX).min_by(|a, b| a.0.total_cmp(&b.0)) {
            return Some(c.1);
        }
        let k = t.outline.len();
        let on_edge = (0..k)
            .map(|i| {
                let (a, b) = (t.outline[i], t.outline[(i + 1) % k]);
                let e = [b[0] - a[0], b[1] - a[1]];
                let l2 = (e[0] * e[0] + e[1] * e[1]).max(1e-12);
                let u = (((q[0] - a[0]) * e[0] + (q[1] - a[1]) * e[1]) / l2).clamp(0.0, 1.0);
                [a[0] + e[0] * u, a[1] + e[1] * u]
            })
            .map(|p| (px(p), p))
            .filter(|(d, _)| *d <= crate::snap::RADIUS_PX)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let p = on_edge.map_or(q, |e| e.1);
        Some([(p[0] * 1000.0).round() / 1000.0, (p[1] * 1000.0).round() / 1000.0])
    }

    pub(crate) fn rebar_sketch_motion(&mut self, x: i32, y: i32) {
        let p = self.rebar_sketch_point(x, y);
        if let Some(t) = &mut self.rebar_sketch {
            t.cursor = p;
        }
        self.rebar_sketch_preview();
    }

    pub(crate) fn rebar_sketch_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        let Some(p) = self.rebar_sketch_point(x, y) else {
            return frame.set_status_text("Click on the host's plane: its top in a plan or 3D view, a wall's face in a 3D view", 0);
        };
        let Some(t) = &mut self.rebar_sketch else { return };
        // A click on the first corner closes a boundary.
        if t.closed() && !t.rectangle && t.points.len() >= 3 && t.points[0] == p && crate::coords::typed().is_none() {
            return self.finish_rebar_sketch();
        }
        // A click on the first corner closes a link.
        let closes = t.what == SketchWhat::Bar && t.points.len() >= 3 && t.points[0] == p;
        t.points.push(p);
        let done = closes || (t.rectangle && t.points.len() == 2) || (t.what == SketchWhat::FabricSheet && t.points.len() == 2);
        if done {
            return self.finish_rebar_sketch();
        }
        self.rebar_sketch_prompt();
        self.rebar_sketch_preview();
    }

    /// Keys: Enter finishes, Backspace takes the last point back, R switches rectangles, M moves
    /// the major direction to the next edge, F flips a path's side.
    pub(crate) fn rebar_sketch_key(&mut self, key: i32) -> bool {
        let Some(t) = &mut self.rebar_sketch else { return false };
        match key {
            WXK_RETURN | WXK_NUMPAD_ENTER => {
                self.finish_rebar_sketch();
                return true;
            }
            WXK_BACK => {
                t.points.pop();
            }
            82 if t.closed() => {
                t.rectangle = !t.rectangle;
                t.points.clear();
                t.major = 0;
            }
            77 if t.closed() => {
                let n = t.shape().len().max(1);
                t.major = (t.major + 1) % n;
            }
            70 if t.what == SketchWhat::Path => {
                let flip = t.args["flip"] == true;
                t.args["flip"] = json!(!flip);
            }
            _ => return false,
        }
        self.rebar_sketch_prompt();
        self.rebar_sketch_preview();
        true
    }

    /// Makes the system from the sketch; a refusal keeps the sketch open to fix.
    pub(crate) fn finish_rebar_sketch(&mut self) {
        let frame = self.frame;
        let Some(t) = self.rebar_sketch.clone() else { return };
        let pts = t.shape();
        let mut args = t.args.clone();
        let (command, what) = match t.what {
            SketchWhat::Area | SketchWhat::FabricArea => {
                if pts.len() < 3 {
                    return frame.set_status_text("A boundary needs at least three corners", 0);
                }
                args["boundary"] = json!(pts);
                args["major_edge"] = json!(t.major.min(pts.len() - 1));
                if t.what == SketchWhat::Area { ("area_reinforcement.create", "Area reinforcement") } else { ("fabric_area.create", "Fabric area") }
            }
            SketchWhat::Path => {
                if pts.len() < 2 {
                    return frame.set_status_text("A path needs at least two points", 0);
                }
                args["path"] = json!(pts);
                ("path_reinforcement.create", "Path reinforcement")
            }
            SketchWhat::FabricSheet => {
                let Some(a) = pts.first() else {
                    return frame.set_status_text("Click the sheet's first corner", 0);
                };
                args["point"] = json!(a);
                args["rotation"] = json!(pts.get(1).map_or(0.0, |b| (b[1] - a[1]).atan2(b[0] - a[0]).to_degrees()));
                ("fabric_sheet.place", "Fabric sheet")
            }
            SketchWhat::Bar => {
                if pts.len() < 2 {
                    return frame.set_status_text("A bar needs at least two corners", 0);
                }
                args["curves"] = json!(pts.windows(2).map(|w| json!({ "line": { "start": w[0], "end": w[1] } })).collect::<Vec<_>>());
                ("rebar.sketch", "Sketched rebar")
            }
        };
        match self.run(command, args) {
            Ok(v) => {
                self.end_rebar_sketch();
                self.refresh_scene(&frame);
                if let Some(id) = v.as_u64() {
                    let _ = self.run("selection.set", json!({ "ids": [id] }));
                    self.selection_changed();
                }
                frame.set_status_text(&format!("{what}: placed as element {v}"), 0);
            }
            Err(e) => {
                // A sheet's direction point is taken back so it can be clicked again.
                if let Some(t) = &mut self.rebar_sketch
                    && t.what == SketchWhat::FabricSheet
                {
                    t.points.truncate(1);
                }
                frame.set_status_text(&format!("{e} — the sketch stays open to fix it"), 0);
            }
        }
    }

    fn rebar_sketch_preview(&mut self) {
        let Some(t) = &self.rebar_sketch else { return };
        let plan_z = self.active_view().and_then(|v| v.plan.as_ref()).map(|pl| pl.range.cut as f32 + 0.004);
        let thin = if plan_z.is_some() { 0.5 } else { 0.08 };
        let f = |p: [f64; 2]| {
            let w = t.plane.to_world(p);
            [w[0] as f32, w[1] as f32, plan_z.unwrap_or(w[2] as f32 + 0.002)]
        };
        let mut pts = t.shape();
        if t.rectangle && t.points.len() == 1
            && let Some(c) = t.cursor
        {
            let a = t.points[0];
            pts = vec![a, [c[0], a[1]], c, [a[0], c[1]]];
        } else if let Some(c) = t.cursor
            && t.what != SketchWhat::FabricSheet
        {
            pts.push(c);
        }
        let mut segs: Vec<[[f32; 3]; 2]> = pts.windows(2).map(|w| [f(w[0]), f(w[1])]).collect();
        if t.closed() && pts.len() >= 3 {
            segs.push([f(pts[pts.len() - 1]), f(pts[0])]);
        }
        let mut major = Vec::new();
        if t.closed() && pts.len() >= 2 {
            // The major direction: its edge doubled on the inside.
            let k = t.major.min(pts.len() - 1);
            let (a, b) = (pts[k], pts[(k + 1) % pts.len()]);
            let l = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-9);
            let area: f64 = (0..pts.len()).map(|i| {
                let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
                p[0] * q[1] - q[0] * p[1]
            }).sum();
            let side = if area >= 0.0 { 1.0 } else { -1.0 };
            let o = 0.08 * side;
            let n = [-(b[1] - a[1]) / l * o, (b[0] - a[0]) / l * o];
            major.push([f(a), f(b)]);
            major.push([f([a[0] + n[0], a[1] + n[1]]), f([b[0] + n[0], b[1] + n[1]])]);
        }
        if t.what == SketchWhat::Path && pts.len() >= 2 {
            // Ticks on the side the bars go.
            let side = if t.args["flip"] == true { -1.0 } else { 1.0 };
            for w in pts.windows(2) {
                let (a, b) = (w[0], w[1]);
                let l = (b[0] - a[0]).hypot(b[1] - a[1]);
                if l < 1e-6 {
                    continue;
                }
                let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
                let n = [-(b[1] - a[1]) / l * 0.3 * side, (b[0] - a[0]) / l * 0.3 * side];
                major.push([f(m), f([m[0] + n[0], m[1] + n[1]])]);
            }
        }
        if t.what == SketchWhat::FabricSheet
            && let (Some(a), Some((sl, sw))) = (t.points.first().copied().or(t.cursor), t.sheet)
        {
            let toward = if t.points.is_empty() { None } else { t.cursor };
            let ang = toward.map_or(0.0, |c| (c[1] - a[1]).atan2(c[0] - a[0]));
            let (u, v) = ([ang.cos(), ang.sin()], [-ang.sin(), ang.cos()]);
            let c = [a, [a[0] + u[0] * sl, a[1] + u[1] * sl], [a[0] + u[0] * sl + v[0] * sw, a[1] + u[1] * sl + v[1] * sw], [a[0] + v[0] * sw, a[1] + v[1] * sw]];
            for i in 0..4 {
                segs.push([f(c[i]), f(c[(i + 1) % 4])]);
            }
        }
        let lines = vec![
            Lines { color: PREVIEW, width: thin, segments: segs },
            Lines { color: MAJOR, width: thin * 1.6, segments: major },
        ];
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&lines);
        }
        if let Some(v) = self.active_view() {
            v.panel.refresh(false, None);
        }
    }
}
