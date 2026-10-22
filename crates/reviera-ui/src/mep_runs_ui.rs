// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The Duct and Pipe tools (MEP-DUCT-001, MEP-PIPE-001): Systems › HVAC & Piping › Duct (`DT`)
//! and Pipe (`PL`).
//!
//! A form takes the run's type, shape and size, its middle elevation above the level, its system
//! type and, for pipes, a slope. Then each click in a plan adds a point at the current elevation
//! (snapping as other tools do; typed coordinates work through Coordinates); **E** asks for a new
//! elevation and puts a riser at the last point; **Enter** or **Esc** makes the run (`duct.create`
//! or `pipe.create`: elbows at corners, a tee or tap where it ends on a duct or pipe) and the tool
//! stays for the next one. Cap Open Ends caps the selection's open ends.
//!
//! Systems › Electrical › Cable Tray (`CT`) and Conduit (`CN`) draw the same way (tray: width
//! and height; conduit: a diameter of the type's standard; both an optional bend radius). Cable
//! Tray Fitting (`TF`) and Conduit Fitting (`NF`) ask for the part (or Split) and then place it
//! where each click falls on a tray or conduit: on an open end, or along one (splitting it).
//! "+" and "−" upgrade and downgrade the selected fitting; Parallel Conduits copies the selected
//! conduit run.
//!
//! Systems › HVAC & Piping › Flex Duct (`FD`, MEP-DUCT-005) draws the same way, the points
//! being a spline's: the first click on an open duct connector or a duct's centreline (a tap is
//! put there) starts from it, the last on a connector ends on it (`flex_duct.create` with
//! `snap`). Each pick is written to `<data dir>/Reviera/sessions/flex-duct.json`, so a drawing
//! cut short by a crash is offered for resumption the next time the tool starts. **Convert to
//! Flex Duct** (`CV`) asks for the maximum length and converts the selected terminals' final
//! segments.
//!
//! Systems › Risers (MEP-DUCT-011): **Create Riser** rises from an open end of the selected run
//! (the open connector chosen when there are several) to a level and offset, bound to the level
//! by default; **Bind Riser** binds or unbinds a selected vertical segment's end.

use reviera_core::ElementId;
use reviera_core::mep::{self, Domain};
use reviera_core::mep_systems;
use reviera_render::RenderBackend;
use reviera_render::plan::Lines;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::Gui;

/// A Flex Duct drawing in progress, kept on disk so it survives a crash.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct FlexSession {
    /// The model's file.
    model: String,
    level: ElementId,
    type_id: Option<ElementId>,
    shape: mep::Shape,
    diameter: f64,
    width: f64,
    height: f64,
    elevation: f64,
    system_type: Option<String>,
    size_from_connector: bool,
    points: Vec<[f64; 3]>,
}

fn flex_session_path() -> std::path::PathBuf {
    super::crash::dir().with_file_name("sessions").join("flex-duct.json")
}

fn forget_flex_session() {
    let _ = std::fs::remove_file(flex_session_path());
}

/// The Duct and Pipe tools: the run's settings and the points clicked so far.
#[derive(Debug, Clone)]
pub(crate) struct RunTool {
    pub domain: Domain,
    pub level: ElementId,
    pub type_id: Option<ElementId>,
    pub shape: mep::Shape,
    /// Metres.
    pub diameter: f64,
    pub width: f64,
    pub height: f64,
    /// The centreline's height above the level now (metres).
    pub elevation: f64,
    pub system_type: Option<String>,
    /// Pipes: rise over run.
    pub slope: f64,
    /// Plan points with their height above the level.
    pub points: Vec<[f64; 3]>,
    pub cursor: Option<[f64; 2]>,
    /// Tray and conduit: the bend radius (metres); the type's when none.
    pub bend_radius: Option<f64>,
    /// Placing fittings by clicking (Cable Tray Fitting, Conduit Fitting) instead of drawing.
    pub placing: Option<Placing>,
    /// Flex Duct (MEP-DUCT-005): the points are a spline's; the size may come from the connector.
    pub flex: bool,
    pub size_from_connector: bool,
}

/// What a click places: a fitting's part, or a split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Placing {
    Part(reviera_core::mep_runs::Part),
    Split,
    /// A pipe accessory (MEP-PIPE-004).
    Accessory(reviera_core::mep_runs::AccessoryKind),
    /// A duct accessory (MEP-DUCT-004).
    DuctAccessory(reviera_core::mep_runs::DuctAccessoryKind),
}

fn domain_title(d: Domain) -> &'static str {
    match d {
        Domain::Duct => "Duct",
        Domain::Pipe => "Pipe",
        Domain::CableTray => "Cable Tray",
        Domain::Conduit => "Conduit",
        Domain::Electrical => "Electrical",
    }
}

const PREVIEW: [f32; 4] = [0.05, 0.35, 0.95, 1.0];

fn number(t: &TextCtrl) -> Option<f64> {
    t.get_value().trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite())
}

impl Gui {
    /// Systems › Duct and Pipe: asks for the run's settings, then takes clicked points.
    pub(crate) fn start_run_tool(&mut self, domain: Domain) {
        let frame = self.frame;
        if !self.ensure_plan(false) {
            return;
        }
        let Some(level) = self.active_view().and_then(|v| v.plan.as_ref()).map(|p| p.level) else { return };
        let Some(store) = self.store_now() else { return };
        let keep = self.run_tool.as_ref().filter(|t| t.domain == domain).cloned();
        let types: Vec<(ElementId, String, mep::Shape)> = store
            .iter()
            .filter_map(|e| {
                let t = reviera_core::mep_runs::segment_type(e)?;
                (t.domain == domain).then(|| (e.id(), e.name.clone().unwrap_or_default(), t.shape))
            })
            .collect();
        let system_types: Vec<mep_systems::SystemType> = mep_systems::system_types(&store)
            .into_iter()
            .filter(|t| t.classification.fits(domain))
            .collect();
        let duct = domain == Domain::Duct;
        let tray = domain == Domain::CableTray;
        let conduit = domain == Domain::Conduit;
        let carrier = tray || conduit;
        let title = domain_title(domain);
        let f = super::reinforcement_ui::Form::new(&frame, title);
        let mut type_labels = vec![match domain {
            Domain::Duct => "Built-in (by shape)".to_owned(),
            Domain::Pipe => "Standard Pipe (built-in)".to_owned(),
            Domain::CableTray => "Ladder Cable Tray (built-in)".to_owned(),
            _ => "Conduit, IEC 61386 (built-in)".to_owned(),
        }];
        type_labels.extend(types.iter().map(|t| t.1.clone()));
        let type_at = keep
            .as_ref()
            .and_then(|k| k.type_id)
            .and_then(|id| types.iter().position(|t| t.0 == id))
            .map_or(0, |i| i + 1);
        let type_choice = f.choice("Type", &type_labels, type_at);
        let shapes = [mep::Shape::Rectangular, mep::Shape::Round, mep::Shape::Oval];
        let shape_choice = duct.then(|| {
            let at = keep.as_ref().and_then(|k| shapes.iter().position(|s| *s == k.shape)).unwrap_or(0);
            f.choice("Shape", &["Rectangular".to_owned(), "Round".to_owned(), "Oval".to_owned()], at)
        });
        let mm = |v: f64| format!("{:.0}", v * 1000.0);
        let k = keep.as_ref();
        let (w0, h0) = if tray { (0.3, 0.1) } else { (0.6, 0.3) };
        let width = (duct || tray).then(|| f.text("Width (mm)", &mm(k.map_or(w0, |k| k.width)), if tray { "The tray's width" } else { "Rectangular and oval ducts" }));
        let height = (duct || tray).then(|| f.text("Height (mm)", &mm(k.map_or(h0, |k| k.height)), if tray { "The tray's side height" } else { "Rectangular and oval ducts" }));
        let standard_sizes = {
            let s = reviera_core::mep_electrical::settings(&store);
            s.conduit_standards
                .iter()
                .map(|c| format!("{}: {}", c.name, c.sizes.iter().map(|z| format!("{:.1}", z.diameter * 1000.0).trim_end_matches(".0").to_owned()).collect::<Vec<_>>().join(", ")))
                .collect::<Vec<_>>()
                .join("; ")
        };
        let diameter = (!tray).then(|| {
            let tip = if conduit { format!("Outside diameter, one of its standard's sizes (mm) — {standard_sizes}") } else { "Round ducts and pipes".to_owned() };
            f.text("Diameter (mm)", &mm(k.map_or(if duct { 0.25 } else if conduit { 0.025 } else { 0.05 }, |k| k.diameter)), &tip)
        });
        let default_elevation = match domain {
            Domain::Duct => 2.7,
            Domain::CableTray => 3.0,
            Domain::Conduit => 2.8,
            _ => 2.4,
        };
        let elevation = f.text(
            "Middle elevation (m)",
            &format!("{:.3}", k.map_or(default_elevation, |k| k.elevation)),
            "The centreline's height above the level; E while drawing changes it (a riser)",
        );
        let radius = carrier.then(|| {
            f.text(
                "Bend radius (mm)",
                &k.and_then(|k| k.bend_radius).map_or_else(String::new, mm),
                "Empty: the type's (tray: width × its multiplier; conduit: the standard's radius, else diameter × its multiplier)",
            )
        });
        let mut st_labels = vec!["None".to_owned()];
        st_labels.extend(system_types.iter().map(|t| t.name.clone()));
        let st_at = k
            .and_then(|k| k.system_type.as_ref())
            .and_then(|id| system_types.iter().position(|t| &t.id == id))
            .map_or(0, |i| i + 1);
        let st_choice = (!carrier).then(|| f.choice("System type", &st_labels, st_at));
        let slope = (domain == Domain::Pipe).then(|| {
            f.text("Slope (%)", &format!("{}", k.map_or(0.0, |k| k.slope * 100.0)), "Level legs fall by this much from the first point")
        });
        if !f.show("Click the run's points; E changes the elevation (a riser); Enter or Esc finishes", "Draw") {
            f.dialog.destroy();
            return;
        }
        let read = |t: &Option<TextCtrl>, d: f64| t.as_ref().and_then(number).map_or(d, |v| v / 1000.0);
        let ti = type_choice.get_selection().map_or(0, |i| i as usize);
        let picked = ti.checked_sub(1).and_then(|i| types.get(i));
        let shape = match (&shape_choice, picked) {
            (Some(c), _) => shapes[c.get_selection().map_or(0, |i| i as usize).min(2)],
            (None, Some(t)) => t.2,
            (None, None) if tray => mep::Shape::Rectangular,
            (None, None) => mep::Shape::Round,
        };
        let tool = RunTool {
            domain,
            level,
            type_id: picked.map(|t| t.0),
            shape,
            diameter: diameter.as_ref().and_then(number).map_or(0.25, |v| v / 1000.0),
            width: read(&width, w0),
            height: read(&height, h0),
            elevation: number(&elevation).unwrap_or(default_elevation),
            system_type: st_choice
                .as_ref()
                .and_then(|c| c.get_selection())
                .and_then(|i| (i as usize).checked_sub(1))
                .and_then(|i| system_types.get(i))
                .map(|t| t.id.clone()),
            slope: slope.as_ref().and_then(number).map_or(0.0, |v| v / 100.0),
            points: Vec::new(),
            cursor: None,
            bend_radius: radius.as_ref().and_then(number).map(|v| v / 1000.0),
            placing: None,
            flex: false,
            size_from_connector: false,
        };
        f.dialog.destroy();
        self.end_tool(&frame);
        self.run_tool = Some(tool);
        frame.set_status_text(&format!("{title}: click the run's points; E changes the elevation; Enter or Esc finishes"), 0);
    }

    pub(crate) fn end_run_tool(&mut self) -> bool {
        if self.run_tool.take().is_none() {
            return false;
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        true
    }

    pub(crate) fn run_motion(&mut self, x: i32, y: i32) {
        let from = self.run_tool.as_ref().and_then(|t| t.points.last().map(|p| [p[0], p[1]]));
        let lines = self.snap_lines();
        let p = self.snapped_at(x, y, from, &lines).map(|s| s.point);
        if let Some(t) = &mut self.run_tool {
            t.cursor = p;
        }
        self.run_preview();
    }

    pub(crate) fn run_click(&mut self, x: i32, y: i32) {
        let from = self.run_tool.as_ref().and_then(|t| t.points.last().map(|p| [p[0], p[1]]));
        let lines = self.snap_lines();
        let Some(p) = self.snapped_at(x, y, from, &lines).map(|s| s.point) else { return };
        if let Some((domain, placing)) = self.run_tool.as_ref().and_then(|t| Some((t.domain, t.placing?))) {
            return self.place_fitting_at(domain, placing, p);
        }
        let Some(t) = &mut self.run_tool else { return };
        let q = [p[0], p[1], t.elevation];
        if t.points.last() != Some(&q) {
            t.points.push(q);
        }
        if t.flex {
            self.save_flex_session();
        }
        self.run_preview();
    }

    /// Writes the Flex Duct picks so far (the micro-operations of the drawing session).
    fn save_flex_session(&self) {
        let Some(t) = self.run_tool.as_ref().filter(|t| t.flex) else { return };
        let session = FlexSession {
            model: self.current_path().map(|p| p.display().to_string()).unwrap_or_default(),
            level: t.level,
            type_id: t.type_id,
            shape: t.shape,
            diameter: t.diameter,
            width: t.width,
            height: t.height,
            elevation: t.elevation,
            system_type: t.system_type.clone(),
            size_from_connector: t.size_from_connector,
            points: t.points.clone(),
        };
        let path = flex_session_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string(&session) {
            let _ = std::fs::write(path, text);
        }
    }

    /// Systems › HVAC & Piping › Flex Duct: resumes a drawing cut short, or asks for the type
    /// and size, then takes clicked points.
    pub(crate) fn start_flex_tool(&mut self) {
        let frame = self.frame;
        if !self.ensure_plan(false) {
            return;
        }
        let Some(level) = self.active_view().and_then(|v| v.plan.as_ref()).map(|p| p.level) else { return };
        let Some(store) = self.store_now() else { return };
        // A drawing cut short (by a crash) in this model.
        let model = self.current_path().map(|p| p.display().to_string()).unwrap_or_default();
        if let Some(sess) = std::fs::read_to_string(flex_session_path())
            .ok()
            .and_then(|t| serde_json::from_str::<FlexSession>(&t).ok())
            .filter(|s| s.model == model && !s.points.is_empty() && store.get(s.level).is_some())
        {
            let d = MessageDialog::builder(
                &frame,
                &format!("A flex duct was being drawn when Reviera last closed ({} point(s) picked).\n\nResume it?", sess.points.len()),
                "Flex Duct",
            )
            .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
            .build();
            if d.show_modal() == ID_YES {
                self.end_tool(&frame);
                let n = sess.points.len();
                self.run_tool = Some(RunTool {
                    domain: Domain::Duct,
                    level: sess.level,
                    type_id: sess.type_id.filter(|t| store.get(*t).is_some()),
                    shape: sess.shape,
                    diameter: sess.diameter,
                    width: sess.width,
                    height: sess.height,
                    elevation: sess.elevation,
                    system_type: sess.system_type,
                    slope: 0.0,
                    points: sess.points,
                    cursor: None,
                    bend_radius: None,
                    placing: None,
                    flex: true,
                    size_from_connector: sess.size_from_connector,
                });
                self.run_preview();
                frame.set_status_text(&format!("Flex Duct: resumed with {n} point(s); click the next points, Enter or Esc finishes"), 0);
                return;
            }
            forget_flex_session();
        }
        let types: Vec<(ElementId, String, mep::Shape)> = store
            .iter()
            .filter_map(|e| {
                let t = reviera_core::mep_runs::segment_type(e)?;
                t.flexible.then(|| (e.id(), e.name.clone().unwrap_or_default(), t.shape))
            })
            .collect();
        let system_types: Vec<mep_systems::SystemType> = mep_systems::system_types(&store).into_iter().filter(|t| t.classification.fits(Domain::Duct)).collect();
        let keep = self.run_tool.as_ref().filter(|t| t.flex).cloned();
        let k = keep.as_ref();
        let f = super::reinforcement_ui::Form::new(&frame, "Flex Duct");
        let mut type_labels = vec!["Built-in Round or Rectangular Flex Duct (by shape)".to_owned()];
        type_labels.extend(types.iter().map(|t| t.1.clone()));
        let type_choice = f.choice("Type", &type_labels, k.and_then(|k| k.type_id).and_then(|id| types.iter().position(|t| t.0 == id)).map_or(0, |i| i + 1));
        let shapes = [mep::Shape::Round, mep::Shape::Rectangular];
        let shape_choice = f.choice("Shape", &["Round".to_owned(), "Rectangular".to_owned()], k.map_or(0, |k| usize::from(k.shape != mep::Shape::Round)));
        let from_connector = f.check("Size", "From the connector it starts or ends on", k.is_none_or(|k| k.size_from_connector));
        let mm = |v: f64| format!("{:.0}", v * 1000.0);
        let diameter = f.text("Diameter (mm)", &mm(k.map_or(0.25, |k| k.diameter)), "Round flex; used when the size is not taken from a connector");
        let width = f.text("Width (mm)", &mm(k.map_or(0.4, |k| k.width)), "Rectangular flex");
        let height = f.text("Height (mm)", &mm(k.map_or(0.2, |k| k.height)), "Rectangular flex");
        let elevation = f.text("Middle elevation (m)", &format!("{:.3}", k.map_or(2.7, |k| k.elevation)), "Height above the level of points not on a connector or duct");
        let mut st_labels = vec!["From what it connects to".to_owned()];
        st_labels.extend(system_types.iter().map(|t| t.name.clone()));
        let st_choice = f.choice("System type", &st_labels, 0);
        if !f.show("Click a connector or a duct's centreline, then the points, then the end (a connector ends it); Enter or Esc finishes", "Draw") {
            f.dialog.destroy();
            return;
        }
        let picked = type_choice.get_selection().and_then(|i| (i as usize).checked_sub(1)).and_then(|i| types.get(i));
        let shape = picked.map_or_else(|| shapes[shape_choice.get_selection().map_or(0, |i| i as usize).min(1)], |t| t.2);
        let tool = RunTool {
            domain: Domain::Duct,
            level,
            type_id: picked.map(|t| t.0),
            shape,
            diameter: number(&diameter).map_or(0.25, |v| v / 1000.0),
            width: number(&width).map_or(0.4, |v| v / 1000.0),
            height: number(&height).map_or(0.2, |v| v / 1000.0),
            elevation: number(&elevation).unwrap_or(2.7),
            system_type: st_choice.get_selection().and_then(|i| (i as usize).checked_sub(1)).and_then(|i| system_types.get(i)).map(|t| t.id.clone()),
            slope: 0.0,
            points: Vec::new(),
            cursor: None,
            bend_radius: None,
            placing: None,
            flex: true,
            size_from_connector: from_connector.is_checked(),
        };
        f.dialog.destroy();
        self.end_tool(&frame);
        self.run_tool = Some(tool);
        frame.set_status_text("Flex Duct: click a connector or a duct's centreline, then the points; Enter or Esc finishes", 0);
    }

    /// Convert to Flex Duct: the selected terminals' final segments, up to a maximum length.
    pub(crate) fn convert_to_flex(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let terminals: Vec<ElementId> = self
            .selection()
            .into_iter()
            .filter(|i| store.get(*i).is_some_and(|e| reviera_core::mep::ports(&store, e).iter().any(|p| p.connector.domain == Domain::Duct) && !reviera_core::mep_runs::is_run_element(e)))
            .collect();
        if terminals.is_empty() {
            return frame.set_status_text("Convert to Flex Duct: select air terminals (or equipment) at the ends of duct runs", 0);
        }
        let d = TextEntryDialog::builder(&frame, "Convert final segments no longer than (m):", "Convert to Flex Duct").with_default_value("1.5").build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some(max) = d.get_value().and_then(|s| s.trim().replace(',', ".").parse::<f64>().ok()) else { return };
        match self.run("duct.convert_to_flex", json!({ "terminal_ids": terminals, "max_length": max })) {
            Ok(v) => {
                self.refresh_scene(&frame);
                let rep: reviera_core::mep_flex::ConvertReport = serde_json::from_value(v).unwrap_or_default();
                let mut text = format!("{} converted to flex duct.", rep.ids.len());
                for s in &rep.skipped {
                    let name = store.get(s.terminal).and_then(|e| e.name.clone()).unwrap_or_else(|| format!("Element {}", s.terminal.0));
                    text.push_str(&format!("\n{name}: skipped, {}", s.reason));
                }
                if rep.skipped.is_empty() {
                    frame.set_status_text(&text, 0);
                } else {
                    MessageDialog::builder(&frame, &text, "Convert to Flex Duct").with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation).build().show_modal();
                }
            }
            Err(e) => error(&frame, &e),
        }
    }

    /// E: a new elevation (the run rises or drops there); Enter: finish.
    pub(crate) fn run_key(&mut self, key: i32) -> bool {
        if self.run_tool.is_none() {
            return false;
        }
        if self.run_tool.as_ref().is_some_and(|t| t.placing.is_some()) {
            return false;
        }
        match key {
            WXK_RETURN | WXK_NUMPAD_ENTER => {
                self.finish_run();
                true
            }
            k if k == 'E' as i32 || k == 'e' as i32 => {
                let frame = self.frame;
                let now = self.run_tool.as_ref().map_or(2.7, |t| t.elevation);
                let d = TextEntryDialog::builder(&frame, "New middle elevation above the level (m):", "Elevation")
                    .with_default_value(&format!("{now:.3}"))
                    .build();
                if d.show_modal() == ID_OK
                    && let Some(v) = d.get_value().and_then(|s| s.trim().replace(',', ".").parse::<f64>().ok())
                    && let Some(t) = &mut self.run_tool
                {
                    t.elevation = v;
                    // A riser at the last point.
                    if let Some(last) = t.points.last().copied()
                        && (last[2] - v).abs() > 1e-6
                    {
                        t.points.push([last[0], last[1], v]);
                    }
                    frame.set_status_text(&format!("Middle elevation {v:.3} m"), 0);
                }
                true
            }
            _ => false,
        }
    }

    /// Esc: a run of two or more points is made; otherwise the tool ends.
    pub(crate) fn run_escape(&mut self) {
        if self.run_tool.as_ref().is_some_and(|t| t.placing.is_none() && t.points.len() >= 2) {
            self.finish_run();
        } else {
            if self.run_tool.as_ref().is_some_and(|t| t.flex) {
                forget_flex_session();
            }
            self.end_run_tool();
            self.frame.set_status_text("Ready", 0);
        }
    }

    fn finish_run(&mut self) {
        let frame = self.frame;
        let Some(t) = self.run_tool.clone() else { return };
        if t.points.len() < 2 {
            return frame.set_status_text("Click at least two points", 0);
        }
        if t.flex {
            let mut args = json!({ "points": t.points, "level": t.level, "shape": t.shape, "snap": true });
            if !t.size_from_connector {
                if t.shape == mep::Shape::Round {
                    args["diameter"] = json!(t.diameter);
                } else {
                    args["width"] = json!(t.width);
                    args["height"] = json!(t.height);
                }
            }
            if let Some(id) = t.type_id {
                args["type_id"] = json!(id);
            }
            if let Some(st) = &t.system_type {
                args["system_type_id"] = json!(st);
            }
            // A size from the connector needs one; else the form's.
            let mut out = self.app.execute("flex_duct.create", args.clone(), &reviera_core::Caller::user());
            if t.size_from_connector && out.as_ref().is_err_and(|e| e.code == "mep_run.size") {
                if t.shape == mep::Shape::Round {
                    args["diameter"] = json!(t.diameter);
                } else {
                    args["width"] = json!(t.width);
                    args["height"] = json!(t.height);
                }
                out = self.app.execute("flex_duct.create", args, &reviera_core::Caller::user());
            }
            return match out {
                Ok(o) => {
                    forget_flex_session();
                    if let Some(tool) = &mut self.run_tool {
                        tool.points.clear();
                        tool.cursor = None;
                    }
                    if let Some(v) = self.viewport() {
                        v.set_overlay_lines(&[]);
                    }
                    self.refresh_scene(&frame);
                    let r = &o.result;
                    let mut text = "Flex duct made".to_owned();
                    if r["fittings"].as_array().is_some_and(|f| !f.is_empty()) {
                        text.push_str(" with a tap");
                    }
                    if r["connected_to"].as_array().is_some_and(|c| !c.is_empty()) {
                        text.push_str(", connected");
                    }
                    if let Some(w) = o.warnings.first() {
                        text.push_str(&format!("; {}", w.message));
                    }
                    frame.set_status_text(&format!("{text}. Click the next, or Esc"), 0);
                }
                Err(e) => error(&frame, &format!("The flex duct was not made:\n{e}")),
            };
        }
        let mut args = json!({ "points": t.points, "level": t.level, "shape": t.shape });
        if let Some(r) = t.bend_radius {
            args["bend_radius"] = json!(r);
        }
        if t.shape == mep::Shape::Round {
            args["diameter"] = json!(t.diameter);
        } else {
            args["width"] = json!(t.width);
            args["height"] = json!(t.height);
        }
        if let Some(id) = t.type_id {
            args["type_id"] = json!(id);
        }
        if let Some(st) = &t.system_type {
            args["system_type_id"] = json!(st);
        }
        if t.domain == Domain::Pipe && t.slope != 0.0 {
            args["slope"] = json!(t.slope);
        }
        let command = match t.domain {
            Domain::Duct => "duct.create",
            Domain::CableTray => "cable_tray.create",
            Domain::Conduit => "conduit.create",
            _ => "pipe.create",
        };
        match self.app.execute(command, args, &reviera_core::Caller::user()) {
            Ok(o) => {
                if let Some(tool) = &mut self.run_tool {
                    // The next run starts afresh at the same settings.
                    tool.points.clear();
                    tool.cursor = None;
                }
                if let Some(v) = self.viewport() {
                    v.set_overlay_lines(&[]);
                }
                self.refresh_scene(&frame);
                let r = &o.result;
                let mut text = format!(
                    "{} segment(s), {} fitting(s)",
                    r["segments"].as_array().map_or(0, Vec::len),
                    r["fittings"].as_array().map_or(0, Vec::len)
                );
                if !r["system"].is_null() {
                    text.push_str("; joined its system");
                }
                if let Some(w) = o.warnings.first() {
                    text.push_str(&format!("; {}", w.message));
                }
                frame.set_status_text(&format!("{text}. Click the next run, or Esc"), 0);
            }
            Err(e) => error(&frame, &format!("The run was not made:\n{e}")),
        }
    }

    fn run_preview(&mut self) {
        let Some(t) = self.run_tool.clone().filter(|t| t.placing.is_none()) else { return };
        let Some(plan) = self.active_view().and_then(|v| v.plan.clone()) else { return };
        let z = plan.range.cut as f32 + 0.004;
        let mut pts: Vec<[f64; 2]> = t.points.iter().map(|p| [p[0], p[1]]).collect();
        if let Some(c) = t.cursor {
            pts.push(c);
        }
        // Flex: the spline through the points.
        if t.flex && pts.len() >= 2 {
            let p3: Vec<[f64; 3]> = pts.iter().map(|p| [p[0], p[1], 0.0]).collect();
            let dir = |a: [f64; 3], b: [f64; 3]| {
                let d = [b[0] - a[0], b[1] - a[1], 0.0];
                let l = (d[0] * d[0] + d[1] * d[1]).sqrt().max(1e-9);
                [d[0] / l, d[1] / l, 0.0]
            };
            let n = p3.len();
            let path = reviera_core::mep_flex::FlexPath { start_tangent: dir(p3[0], p3[1]), end_tangent: dir(p3[n - 2], p3[n - 1]), points: p3 };
            pts = path.samples(12).into_iter().map(|(p, _)| [p[0], p[1]]).collect();
        }
        let lines = Lines {
            color: PREVIEW,
            width: 0.5,
            segments: pts
                .windows(2)
                .map(|w| [[w[0][0] as f32, w[0][1] as f32, z], [w[1][0] as f32, w[1][1] as f32, z]])
                .collect(),
        };
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[lines]);
        }
    }

    /// Systems › Cap Open Ends for the selected ducts, pipes and fittings.
    pub(crate) fn cap_open_ends(&mut self) {
        let frame = self.frame;
        let ids = self.selection();
        match self.run("duct.cap_open_ends", json!({ "element_ids": ids })) {
            Ok(v) => {
                self.refresh_scene(&frame);
                frame.set_status_text(&format!("{} cap(s) placed", v.as_array().map_or(0, Vec::len)), 0);
            }
            Err(e) => error(&frame, &format!("No caps were placed:\n{e}")),
        }
    }
}

impl Gui {
    /// Systems › Electrical › Cable Tray Fitting and Conduit Fitting: the part (or Split), then
    /// clicks place it.
    pub(crate) fn start_fitting_tool(&mut self, domain: Domain) {
        use reviera_core::mep_runs::Part;
        let frame = self.frame;
        if !self.ensure_plan(false) {
            return;
        }
        let Some(level) = self.active_view().and_then(|v| v.plan.as_ref()).map(|p| p.level) else { return };
        let mut parts = vec![
            (Placing::Part(Part::Tee), "Tee"),
            (Placing::Part(Part::Cross), "Cross"),
            (Placing::Part(Part::Elbow), "Elbow (on an open end)"),
            (Placing::Part(Part::Coupling), "Coupling"),
            (Placing::Part(Part::Cap), "Cap (on an open end)"),
        ];
        if domain == Domain::Conduit {
            parts.push((Placing::Part(Part::Box), "Junction box"));
        }
        if domain != Domain::Pipe {
            parts.push((Placing::Split, "Split (a coupling where the type has one)"));
        }
        let labels: Vec<&str> = parts.iter().map(|p| p.1).collect();
        let title = format!("{} Fitting", domain_title(domain));
        let d = SingleChoiceDialog::builder(&frame, "Place:", &title, &labels).build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some((placing, label)) = usize::try_from(d.get_selection()).ok().and_then(|k| parts.get(k)).copied() else { return };
        self.end_tool(&frame);
        self.run_tool = Some(RunTool {
            domain,
            level,
            type_id: None,
            shape: mep::Shape::Round,
            diameter: 0.0,
            width: 0.0,
            height: 0.0,
            elevation: 0.0,
            system_type: None,
            slope: 0.0,
            points: Vec::new(),
            cursor: None,
            bend_radius: None,
            placing: Some(placing),
            flex: false,
            size_from_connector: false,
        });
        frame.set_status_text(&format!("{title}: click on a {} or its open end to place: {label}; Esc ends", domain_title(domain).to_lowercase()), 0);
    }

    fn place_fitting_at(&mut self, domain: Domain, placing: Placing, p: [f64; 2]) {
        let frame = self.frame;
        let (command, args) = match placing {
            Placing::Part(part) => {
                let command = match domain {
                    Domain::Conduit => "conduit_fitting.place",
                    Domain::Pipe => "pipe_fitting.place",
                    _ => "cable_tray_fitting.place",
                };
                (command, json!({ "part": part, "point": p }))
            }
            Placing::Accessory(kind) => ("pipe_accessory.place", json!({ "kind": kind, "point": p })),
            Placing::DuctAccessory(kind) => ("duct_accessory.place", json!({ "kind": kind, "point": p })),
            Placing::Split => {
                let Some(store) = self.store_now() else { return };
                // The straight segment under the click.
                let host = store.iter().find_map(|e| {
                    let s = reviera_core::mep_runs::segment_recipe(e)?;
                    if s.domain != domain || s.corner.is_some() {
                        return None;
                    }
                    let (a, b) = ([s.start[0], s.start[1]], [s.end[0], s.end[1]]);
                    let d = [b[0] - a[0], b[1] - a[1]];
                    let l2 = d[0] * d[0] + d[1] * d[1];
                    if l2 < 1e-12 {
                        return None;
                    }
                    let t = ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2;
                    let q = [a[0] + d[0] * t, a[1] + d[1] * t];
                    let off = ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt();
                    ((0.0..=1.0).contains(&t) && off <= s.size.nominal() / 2.0 + 0.1).then(|| e.id())
                });
                let Some(h) = host else {
                    return frame.set_status_text(&format!("Split: click on a straight {}", domain_title(domain).to_lowercase()), 0);
                };
                (if domain == Domain::Conduit { "conduit.split" } else { "cable_tray.split" }, json!({ "id": h, "point": p }))
            }
        };
        match self.app.execute(command, args, &reviera_core::Caller::user()) {
            Ok(o) => {
                self.refresh_scene(&frame);
                let what = if let Placing::Accessory(k) = placing {
                    format!("{} inserted; the pipe is split{}", k.label(), if o.result["flanges"].as_array().is_some_and(|f| !f.is_empty()) { ", with flanges" } else { "" })
                } else if let Placing::DuctAccessory(k) = placing {
                    if o.result["split"].is_null() { format!("{} placed on the open end", k.label()) } else { format!("{} inserted; the duct is split", k.label()) }
                } else if placing == Placing::Split {
                    if o.result["coupling"].is_null() { "Split".to_owned() } else { "Split with a coupling".to_owned() }
                } else if o.result["split"].is_null() {
                    "Placed on the open end".to_owned()
                } else {
                    "Placed; the run is split and connected".to_owned()
                };
                frame.set_status_text(&format!("{what}. Click again, or Esc"), 0);
            }
            Err(e) => error(&frame, &e.message),
        }
    }

    /// Systems › Plumbing & Piping › Pipe Accessory (`PA`): the kind, then each click on a pipe
    /// inserts one.
    pub(crate) fn start_accessory_tool(&mut self) {
        use reviera_core::mep_runs::AccessoryKind as K;
        let frame = self.frame;
        if !self.ensure_plan(false) {
            return;
        }
        let Some(level) = self.active_view().and_then(|v| v.plan.as_ref()).map(|p| p.level) else { return };
        let kinds = [K::GateValve, K::BallValve, K::ButterflyValve, K::GlobeValve, K::CheckValve, K::PressureReducingValve, K::ReliefValve, K::Strainer, K::WaterMeter];
        let labels: Vec<&str> = kinds.iter().map(|k| k.label()).collect();
        let d = SingleChoiceDialog::builder(&frame, "Insert:", "Pipe Accessory", &labels).build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some(kind) = usize::try_from(d.get_selection()).ok().and_then(|k| kinds.get(k)).copied() else { return };
        self.end_tool(&frame);
        self.run_tool = Some(RunTool {
            domain: Domain::Pipe,
            level,
            type_id: None,
            shape: mep::Shape::Round,
            diameter: 0.0,
            width: 0.0,
            height: 0.0,
            elevation: 0.0,
            system_type: None,
            slope: 0.0,
            points: Vec::new(),
            cursor: None,
            bend_radius: None,
            placing: Some(Placing::Accessory(kind)),
            flex: false,
            size_from_connector: false,
        });
        frame.set_status_text(&format!("Pipe Accessory: click on a pipe to insert a {}; flanged from 50 mm; Esc ends", kind.label().to_lowercase()), 0);
    }

    /// Systems › Insulation & Accessories › Duct Accessory (`DA`): the kind, then each click on
    /// a duct inserts one, or on an open duct end places one there (MEP-DUCT-004).
    pub(crate) fn start_duct_accessory_tool(&mut self) {
        use reviera_core::mep_runs::DuctAccessoryKind as K;
        let frame = self.frame;
        if !self.ensure_plan(false) {
            return;
        }
        let Some(level) = self.active_view().and_then(|v| v.plan.as_ref()).map(|p| p.level) else { return };
        let kinds = [K::FireDamper, K::SmokeDamper, K::ControlDamper, K::BalancingDamper, K::BackdraftDamper, K::Filter, K::HeatingCoil, K::CoolingCoil, K::SoundAttenuator];
        let labels: Vec<&str> = kinds.iter().map(|k| k.label()).collect();
        let d = SingleChoiceDialog::builder(&frame, "Insert:", "Duct Accessory", &labels).build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some(kind) = usize::try_from(d.get_selection()).ok().and_then(|k| kinds.get(k)).copied() else { return };
        self.end_tool(&frame);
        self.run_tool = Some(RunTool {
            domain: Domain::Duct,
            level,
            type_id: None,
            shape: mep::Shape::Round,
            diameter: 0.0,
            width: 0.0,
            height: 0.0,
            elevation: 0.0,
            system_type: None,
            slope: 0.0,
            points: Vec::new(),
            cursor: None,
            bend_radius: None,
            placing: Some(Placing::DuctAccessory(kind)),
            flex: false,
            size_from_connector: false,
        });
        frame.set_status_text(&format!("Duct Accessory: click on a duct to insert a {} ({:.0} mm long), or on an open end; Esc ends", kind.label().to_lowercase(), kind.default_length() * 1000.0), 0);
    }

    /// Insulation or lining › Add / Edit (MEP-DUCT-004): the type and thickness for the selected
    /// ducts, pipes, fittings and accessories.
    pub(crate) fn mep_covering(&mut self, kind: reviera_core::mep_coverings::CoveringKind, edit: bool) {
        use reviera_core::mep_coverings::{CoveringKind, covering_type};
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let ids = self.selection();
        let title = match (kind, edit) {
            (CoveringKind::Insulation, false) => "Add Insulation",
            (CoveringKind::Insulation, true) => "Edit Insulation",
            (CoveringKind::Lining, false) => "Add Lining",
            (CoveringKind::Lining, true) => "Edit Lining",
        };
        if ids.is_empty() {
            return frame.set_status_text(&format!("{title}: select ducts, pipes, fittings or accessories (Tab selects a run)"), 0);
        }
        // The types of this kind; the built-in one is made on first use.
        let mut types: Vec<(Option<ElementId>, String)> = store.iter().filter(|e| covering_type(e).is_some_and(|t| t.covering == kind)).map(|e| (Some(e.id()), e.name.clone().unwrap_or_default())).collect();
        if types.is_empty() {
            types.push((None, if kind == CoveringKind::Insulation { "Duct Insulation".into() } else { "Duct Lining".into() }));
        }
        let names: Vec<String> = types.iter().map(|t| t.1.clone()).collect();
        let f = super::reinforcement_ui::Form::new(&frame, title);
        let ty = f.choice("Type", &names, 0);
        let thickness = f.text("Thickness (mm)", "25", if kind == CoveringKind::Lining { "Above 0 and less than half the duct's smallest side" } else { "Above 0" });
        let note = if kind == CoveringKind::Lining { "Lining narrows the section the pressure-drop calculation uses." } else { "Insulation follows its host and is included in clash checks." };
        let ok = f.show(note, if edit { "Apply" } else { "Add" });
        let t = thickness.get_value().trim().replace(',', ".").parse::<f64>().map(|v| v / 1000.0);
        let chosen = ty.get_selection().and_then(|k| types.get(k as usize)).and_then(|t| t.0);
        f.dialog.destroy();
        if !ok {
            return;
        }
        let Ok(t) = t else { return error(&frame, "The thickness must be a number of millimetres") };
        let command = match (kind, edit) {
            (CoveringKind::Insulation, false) => "mep_insulation.add",
            (CoveringKind::Insulation, true) => "mep_insulation.set",
            (CoveringKind::Lining, false) => "duct_lining.add",
            (CoveringKind::Lining, true) => "duct_lining.set",
        };
        let mut args = json!({ "element_ids": ids, "thickness": t });
        if let Some(c) = chosen {
            args["type_id"] = json!(c);
        }
        match self.app.execute(command, args, &reviera_core::Caller::user()) {
            Ok(o) => {
                self.refresh_scene(&frame);
                let n = o.result.as_array().map_or(0, Vec::len);
                let skipped = o.warnings.iter().map(|w| w.message.clone()).collect::<Vec<_>>().join("; ");
                frame.set_status_text(&format!("{title}: {n} element(s){}", if skipped.is_empty() { String::new() } else { format!(". {skipped}") }), 0);
            }
            Err(e) => error(&frame, &e.message),
        }
    }

    /// Insulation or lining › Remove on the selection (MEP-DUCT-004).
    pub(crate) fn remove_mep_covering(&mut self, kind: reviera_core::mep_coverings::CoveringKind) {
        let frame = self.frame;
        let ids = self.selection();
        let command = if kind == reviera_core::mep_coverings::CoveringKind::Insulation { "mep_insulation.remove" } else { "duct_lining.remove" };
        match self.run(command, json!({ "element_ids": ids })) {
            Ok(v) => {
                self.refresh_scene(&frame);
                frame.set_status_text(&format!("{} removed from {} element(s)", kind.label(), v.as_array().map_or(0, Vec::len)), 0);
            }
            Err(e) => error(&frame, &e),
        }
    }

    /// Heal: the two selected segments, in line with open facing ends, joined into one.
    pub(crate) fn heal_segments(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let segs: Vec<ElementId> = self.selection().into_iter().filter(|i| store.get(*i).and_then(reviera_core::mep_runs::segment_recipe).is_some()).collect();
        let [a, b] = segs[..] else {
            return frame.set_status_text("Heal: select the two duct or pipe segments either side of the gap", 0);
        };
        match self.run("mep_segment.heal", json!({ "first_id": a, "second_id": b })) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text("Healed into one segment", 0);
            }
            Err(e) => error(&frame, &e),
        }
    }

    /// Justify (MEP-DUCT-006): the selected run aligned on a face or edge, from its start or end
    /// segment.
    pub(crate) fn justify_run(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let run: Vec<ElementId> = self.selection().into_iter().filter(|i| store.get(*i).is_some_and(reviera_core::mep_runs::is_run_element)).collect();
        if run.is_empty() {
            return frame.set_status_text("Justify: select a run's segments and fittings (Tab selects a run)", 0);
        }
        use reviera_core::mep_justify::Alignment as A;
        let f = super::reinforcement_ui::Form::new(&frame, "Justify");
        let control = f.choice("Control point", &["Start segment".to_owned(), "End segment".to_owned()], 0);
        let up = f.choice("Vertical", &["Top".to_owned(), "Middle".to_owned(), "Bottom".to_owned()], 2);
        let across = f.choice("Horizontal", &["Left".to_owned(), "Centre".to_owned(), "Right".to_owned()], 1);
        if !f.show("The control segment stays; the others align on its face or edge (left and right as seen from it)", "Finish") {
            f.dialog.destroy();
            return;
        }
        let pick = |c: &Choice| c.get_selection().map_or(0, |i| (i as usize).min(2));
        let alignment = [[A::TL, A::TC, A::TR], [A::ML, A::MC, A::MR], [A::BL, A::BC, A::BR]][pick(&up)][pick(&across)];
        let end = if pick(&control) == 1 { "end" } else { "start" };
        f.dialog.destroy();
        match self.run("mep_run.justify", json!({ "element_ids": run, "control_end": end, "alignment": alignment })) {
            Ok(v) => {
                self.refresh_scene(&frame);
                let n = v["moved"].as_array().map_or(0, Vec::len);
                frame.set_status_text(&format!("Justified {}: {n} element(s) moved", alignment.label()), 0);
            }
            Err(e) => error(&frame, &e),
        }
    }

    /// Change Tee to Cross on the selected tees.
    pub(crate) fn tee_to_cross(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let tees: Vec<ElementId> = self
            .selection()
            .into_iter()
            .filter(|i| store.get(*i).and_then(reviera_core::mep_runs::fitting_recipe).is_some_and(|f| f.part == reviera_core::mep_runs::Part::Tee))
            .collect();
        if tees.is_empty() {
            return frame.set_status_text("Change Tee to Cross: select a tee", 0);
        }
        for t in &tees {
            if let Err(e) = self.run("mep_fitting.tee_to_cross", json!({ "fitting_id": t })) {
                return error(&frame, &e);
            }
        }
        self.refresh_scene(&frame);
        frame.set_status_text(&format!("{} tee(s) changed to crosses; the new legs are open", tees.len()), 0);
    }

    /// Systems › HVAC & Piping › Hydronic Analysis (MEP-PIPE-008): turns the analysis on if it
    /// is off (asking first), then shows each loop's pump flow and head and what cannot be analysed.
    pub(crate) fn hydronic_analysis(&mut self) {
        let frame = self.frame;
        let mut result = self.run("hydronic.analyze", json!({}));
        if result.as_ref().is_err_and(|e| e.contains("is off")) {
            let d = MessageDialog::builder(&frame, "Hydronic network analysis is off (Mechanical Settings › Pipe › Calculation).\n\nTurn it on?", "Hydronic Analysis")
                .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
                .build();
            if d.show_modal() != ID_YES {
                return;
            }
            if let Err(e) = self.run("hydronic.set_enabled", json!({ "enabled": true })) {
                return error(&frame, &e);
            }
            result = self.run("hydronic.analyze", json!({}));
        }
        let rep: reviera_core::mep_hydronic::HydronicReport = match result.map(serde_json::from_value) {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => return error(&frame, &e.to_string()),
            Err(e) => return error(&frame, &e),
        };
        let Some(store) = self.store_now() else { return };
        let name = |id: ElementId| {
            let e = store.get(id);
            let ty = e.and_then(|e| reviera_core::mesh_objects::type_of(&store, e)).and_then(|(t, _)| store.get(t)).and_then(|t| t.name.clone());
            format!("{} {}", ty.or_else(|| e.and_then(|e| e.name.clone())).unwrap_or_else(|| "Element".into()), id.0)
        };
        let mut text = String::new();
        if rep.loops.is_empty() && rep.problems.is_empty() {
            text.push_str("No hydronic loops: place pumps and connect them with pipes.\n");
        }
        for l in &rep.loops {
            let mut role = l.role.clone();
            if let Some(c) = role.get_mut(0..1) {
                c.make_ascii_uppercase();
            }
            text.push_str(&format!(
                "{role} loop {}: {}\n  flow {:.2} L/s, head {:.1} kPa{}\n",
                l.id,
                name(l.pump),
                l.flow * 1000.0,
                l.head / 1000.0,
                l.critical.map(|c| format!(" (critical: {})", name(c))).unwrap_or_default()
            ));
        }
        if !rep.problems.is_empty() {
            text.push_str(&format!("\nNot analysed:\n{}", rep.problems.join("\n")));
        }
        frame.set_status_text(&format!("Hydronic analysis: {} loop(s)", rep.loops.len()), 0);
        MessageDialog::builder(&frame, &text, "Hydronic Analysis").with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation).build().show_modal();
    }

    /// Systems › HVAC & Piping › Hydraulic Separation Add / Remove on the selected pipe, fitting
    /// or tank (MEP-PIPE-008).
    pub(crate) fn hydraulic_separation(&mut self, add: bool) {
        let frame = self.frame;
        let sel = self.selection();
        if sel.is_empty() {
            return frame.set_status_text("Hydraulic Separation: select a pipe, pipe fitting or tank", 0);
        }
        let command = if add { "hydraulic_separation.add" } else { "hydraulic_separation.remove" };
        for id in &sel {
            if let Err(e) = self.run(command, json!({ "element_id": id })) {
                return error(&frame, &e);
            }
        }
        self.properties_stale = true;
        frame.set_status_text(&format!("{} {} hydraulic separation(s)", if add { "Added" } else { "Removed" }, sel.len()), 0);
    }

    /// "+" and "−" on the selected fitting.
    pub(crate) fn fitting_grade(&mut self, up: bool) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let fittings: Vec<ElementId> =
            self.selection().into_iter().filter(|i| store.get(*i).and_then(reviera_core::mep_runs::fitting_recipe).is_some()).collect();
        let [f] = fittings[..] else {
            return frame.set_status_text("Select one fitting first", 0);
        };
        let command = if up { "mep_fitting.upgrade" } else { "mep_fitting.downgrade" };
        match self.run(command, json!({ "fitting_id": f })) {
            Ok(v) => {
                self.refresh_scene(&frame);
                frame.set_status_text(&format!("The fitting is now a {}", v.as_str().unwrap_or("")), 0);
            }
            Err(e) => error(&frame, &e),
        }
    }

    /// Systems › Electrical › Parallel Conduits for the selected conduit run.
    pub(crate) fn parallel_conduits(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let run = self.selection().into_iter().find(|i| {
            store.get(*i).and_then(reviera_core::mep_runs::segment_recipe).is_some_and(|s| s.domain == Domain::Conduit)
        });
        let Some(run) = run else {
            return frame.set_status_text("Parallel Conduits: select a conduit of the run first", 0);
        };
        let f = super::reinforcement_ui::Form::new(&frame, "Parallel Conduits");
        let across = f.text("Across", "3", "Conduits side by side, the selected one included");
        let h = f.text("Offset across (mm)", "60", "Centre to centre, to the left of the run's direction (negative: right)");
        let up = f.text("Up", "1", "Rows, the selected one included");
        let v = f.text("Offset up (mm)", "60", "Centre to centre between rows");
        let bend = f.choice("Bends", &["Same bend radius".to_owned(), "Concentric bend radius".to_owned()], 0);
        let ok = f.show("Concentric bends are for conduit types without fittings.", "Make");
        let int = |t: &TextCtrl| t.get_value().trim().parse::<usize>().ok();
        let args = json!({
            "run_ids": [run],
            "h_count": int(&across).unwrap_or(1),
            "v_count": int(&up).unwrap_or(1),
            "h_offset": number(&h).unwrap_or(60.0) / 1000.0,
            "v_offset": number(&v).unwrap_or(60.0) / 1000.0,
            "bend": if bend.get_selection() == Some(1) { "concentric" } else { "same" },
        });
        f.dialog.destroy();
        if !ok {
            return;
        }
        match self.run("conduit.create_parallel", args) {
            Ok(v) => {
                self.refresh_scene(&frame);
                frame.set_status_text(&format!("{} element(s) made", v.as_array().map_or(0, Vec::len)), 0);
            }
            Err(e) => error(&frame, &format!("No parallel conduits were made:\n{e}")),
        }
    }
}

impl Gui {
    /// Systems › Plumbing & Piping › Parallel Pipes: copies of the selected pipe's run.
    pub(crate) fn parallel_pipes(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let run = self.selection().into_iter().find(|i| {
            store.get(*i).and_then(reviera_core::mep_runs::segment_recipe).is_some_and(|s| s.domain == Domain::Pipe)
        });
        let Some(run) = run else {
            return frame.set_status_text("Parallel Pipes: select a pipe of the run first", 0);
        };
        let f = super::reinforcement_ui::Form::new(&frame, "Parallel Pipes");
        let across = f.text("Horizontal number", "2", "Pipes side by side, the selected one included (1–20)");
        let h = f.text("Horizontal offset (mm)", "150", "Centre to centre, to the left of the run's direction (negative: right); at least the pipe's outside size");
        let up = f.text("Vertical number", "1", "Rows, the selected one included (1–20)");
        let v = f.text("Vertical offset (mm)", "150", "Centre to centre between rows");
        let ok = f.show("Copies follow the whole run with the same type, size, system type and slope; a run with tees, crosses or taps is refused.", "Make");
        let int = |t: &TextCtrl| t.get_value().trim().parse::<usize>().ok();
        let args = json!({
            "run_ids": [run],
            "h_count": int(&across).unwrap_or(1),
            "v_count": int(&up).unwrap_or(1),
            "h_offset": number(&h).unwrap_or(150.0) / 1000.0,
            "v_offset": number(&v).unwrap_or(150.0) / 1000.0,
        });
        f.dialog.destroy();
        if !ok {
            return;
        }
        match self.run("pipe.create_parallel", args) {
            Ok(v) => {
                self.refresh_scene(&frame);
                frame.set_status_text(&format!("{} element(s) made; they join a system when connected to its equipment", v.as_array().map_or(0, Vec::len)), 0);
            }
            Err(e) => error(&frame, &format!("No parallel pipes were made:\n{e}")),
        }
    }

    /// Systems › Plumbing & Piping › Slope: the Slope Editor for the selected section of pipe.
    pub(crate) fn slope_editor(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let section: Vec<ElementId> = self
            .selection()
            .into_iter()
            .filter(|i| {
                store.get(*i).is_some_and(|e| {
                    reviera_core::mep_runs::segment_recipe(e).is_some_and(|s| s.domain == Domain::Pipe)
                        || reviera_core::mep_runs::fitting_recipe(e).is_some_and(|f| f.domain == Domain::Pipe)
                })
            })
            .collect();
        if section.is_empty() {
            return frame.set_status_text("Slope: select the section's pipes and fittings (Tab selects a whole run)", 0);
        }
        let allowed: Vec<f64> = self.run("mep_settings.slopes", json!({})).ok().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
        let labels: Vec<String> = allowed.iter().map(|a| format!("{:.2} %", a * 100.0)).collect();
        let f = super::reinforcement_ui::Form::new(&frame, "Slope Editor");
        let value = f.choice("Slope value", &labels, allowed.iter().position(|a| (*a - 0.02).abs() < 1e-9).unwrap_or(0));
        let ends = ["The lowest end (falls toward it)".to_owned(), "The other end".to_owned()];
        let control = f.choice("Slope control point", &ends, 0);
        let ok = f.show("The control point stays where it is; the pipe rises away from it. Fittings follow, and branches joined to the section are kept connected.", "Finish");
        let k = value.get_selection().map_or(0, |k| k as usize);
        let other = control.get_selection() == Some(1);
        f.dialog.destroy();
        let Some(slope) = allowed.get(k).copied().filter(|_| ok) else { return };
        // The other end: the lowest end found by the command, then the one it did not choose.
        let mut args = json!({ "element_ids": section, "slope": slope });
        if other {
            let ends: Vec<Value> = section
                .iter()
                .flat_map(|id| {
                    let n = store.get(*id).map_or(0, reviera_core::mep_runs::port_count);
                    (0..n).map(move |k| (*id, k))
                })
                .filter(|(id, k)| {
                    let partners = reviera_core::mep::partners(&store);
                    !partners.get(&(*id, *k)).into_iter().flatten().any(|(o, _)| section.contains(o))
                })
                .filter(|(id, k)| store.get(*id).and_then(reviera_core::mep_runs::segment_recipe).is_some() || *k < 2)
                .map(|(id, k)| json!({ "element_id": id, "port": k }))
                .collect();
            let z = |v: &Value| {
                reviera_core::mep::port(&store, ElementId(v["element_id"].as_u64().unwrap_or(0)), v["port"].as_u64().unwrap_or(0) as usize).map_or(0.0, |p| p.position[2])
            };
            if let Some(high) = ends.iter().max_by(|a, b| z(a).total_cmp(&z(b))) {
                args["control_point"] = high.clone();
            }
        }
        match self.run("mep_run.set_slope", args) {
            Ok(r) => {
                self.refresh_scene(&frame);
                frame.set_status_text(
                    &format!("{} segment(s) at {:.2} %; the far end rose {:.0} mm", r["segments"], slope * 100.0, r["rise"].as_f64().unwrap_or(0.0) * 1000.0),
                    0,
                );
            }
            Err(e) => {
                // A branch that cannot follow: shown selected.
                let ids: Vec<ElementId> = e.split(|c: char| !c.is_ascii_digit()).filter_map(|t| t.parse::<u64>().ok()).map(ElementId).filter(|i| store.get(*i).is_some() && !section.contains(i)).collect();
                if !ids.is_empty() && e.contains("cannot be kept connected") {
                    self.select_ids(&ids, super::select::Combine::Replace);
                }
                error(&frame, &format!("The slope was not changed:\n{e}"));
            }
        }
    }
}

fn error(frame: &Frame, message: &str) {
    let d = MessageDialog::builder(frame, message, "BIM Reviera")
        .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconWarning)
        .build();
    d.show_modal();
}

impl Gui {
    /// The model's levels, lowest first: (GlobalId's element, label).
    fn level_choices(store: &reviera_core::Store) -> Vec<(ElementId, String)> {
        reviera_core::plans::levels(store)
            .into_iter()
            .map(|(z, _, id)| (id, format!("{} ({:.0} mm)", store.get(id).and_then(|e| e.name.clone()).unwrap_or_default(), z * 1000.0)))
            .collect()
    }

    /// Systems › Risers › Create Riser.
    pub(crate) fn create_riser(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let sel = self.selection();
        let partners = mep::partners(&store);
        // The selection's open connectors of the four routing domains.
        let open: Vec<(ElementId, mep::Port)> = sel
            .iter()
            .filter_map(|id| store.get(*id).map(|e| (*id, e)))
            .flat_map(|(id, e)| mep::ports(&store, e).into_iter().map(move |p| (id, p)))
            .filter(|(id, p)| {
                matches!(p.connector.domain, Domain::Duct | Domain::Pipe | Domain::CableTray | Domain::Conduit)
                    && !p.connector.surface
                    && !partners.contains_key(&(*id, p.index))
            })
            .collect();
        if open.is_empty() {
            return frame.set_status_text("Create Riser: select a duct, pipe, cable tray or conduit with an open end", 0);
        }
        let (element, port) = if open.len() == 1 {
            open[0].clone()
        } else {
            let labels: Vec<String> = open
                .iter()
                .map(|(id, p)| format!("Element {} connector {} at {:.2}, {:.2}, {:.2} m", id.0, p.index, p.position[0], p.position[1], p.position[2]))
                .collect();
            let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
            let d = SingleChoiceDialog::builder(&frame, "Rise from:", "Create Riser", &refs).build();
            if d.show_modal() != ID_OK {
                return;
            }
            open[d.get_selection().max(0) as usize].clone()
        };
        let levels = Self::level_choices(&store);
        let f = super::reinforcement_ui::Form::new(&frame, "Create Riser");
        let names: Vec<String> = levels.iter().map(|l| l.1.clone()).collect();
        let level = f.choice("Target level", &names, levels.len().saturating_sub(1));
        let offset = f.text("Offset (mm)", "0", "Above the target level; a target below the start makes a drop");
        let bind = f.check("", "Bind top to level", true);
        let ok = f.show("An elbow from the routing preferences and one vertical segment of the same type, size and system; its far end is left open.", "Finish");
        let k = level.get_selection().map_or(0, |k| k as usize);
        let off = offset.get_value().trim().replace(',', ".").parse::<f64>().unwrap_or(0.0) / 1000.0;
        let b = bind.get_value();
        f.dialog.destroy();
        if !ok {
            return;
        }
        let Some((level_id, _)) = levels.get(k) else { return };
        let args = json!({ "element_id": element, "connector_index": port.index, "top_level": level_id, "top_offset": off, "bind": b });
        match self.run("mep_run.create_riser", args) {
            Ok(r) => {
                self.refresh_scene(&frame);
                let made: Vec<ElementId> = ["segments", "fittings"]
                    .iter()
                    .flat_map(|k| r[*k].as_array().cloned().unwrap_or_default())
                    .filter_map(|v| v.as_u64().map(ElementId))
                    .collect();
                self.select_ids(&made, super::select::Combine::Replace);
                frame.set_status_text("Riser created", 0);
            }
            Err(e) => super::error(&frame, &format!("The riser was not created:\n{e}")),
        }
    }

    /// Systems › Risers › Bind Riser: the selected vertical segment's top level and offset.
    pub(crate) fn bind_riser(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let sel = self.selection();
        let Some((id, s)) = sel.first().and_then(|id| store.get(*id).and_then(reviera_core::mep_runs::segment_recipe).map(|s| (*id, s))) else {
            return frame.set_status_text("Bind Riser: select a vertical segment", 0);
        };
        let levels = Self::level_choices(&store);
        let mut names = vec!["(not bound)".to_owned()];
        names.extend(levels.iter().map(|l| l.1.clone()));
        let current = s.riser.as_ref().and_then(|r| levels.iter().position(|(lid, _)| store.get(*lid).is_some_and(|e| e.guid().to_string() == r.level)));
        let f = super::reinforcement_ui::Form::new(&frame, "Bind Riser");
        let level = f.choice("Top level", &names, current.map_or(0, |k| k + 1));
        let offset = f.text("Top offset (mm)", &format!("{:.0}", s.riser.as_ref().map_or(0.0, |r| r.offset) * 1000.0), "Above the level");
        let ok = f.show("A bound riser's end keeps to its level when the level moves; the runs beyond it move with it. Unbinding keeps the geometry.", "OK");
        let k = level.get_selection().map_or(0, |k| k as usize);
        let off = offset.get_value().trim().replace(',', ".").parse::<f64>().unwrap_or(0.0) / 1000.0;
        f.dialog.destroy();
        if !ok {
            return;
        }
        let args = match k.checked_sub(1).and_then(|k| levels.get(k)) {
            Some((lid, _)) => json!({ "element_id": id, "top_level": lid, "top_offset": off }),
            None => json!({ "element_id": id }),
        };
        match self.run("mep_run.bind_riser", args) {
            Ok(_) => self.refresh_scene(&frame),
            Err(e) => super::error(&frame, &format!("The riser was not bound:\n{e}")),
        }
    }
}
