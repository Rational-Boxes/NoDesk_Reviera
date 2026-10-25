// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The MEP foundations in the window (Ch 4 §4.6): the Systems tab, the System Browser, connector
//! glyphs and the system check markers.
//!
//! - **Systems › Components**: Load MEP Types (the starter component types) and Place MEP
//!   Component (a type with connectors, placed with the Place Component tool, so its ports
//!   connect to those they meet). **Connect** joins the selection's ports that meet.
//! - **Systems › Systems**: Create System from the selected terminals, fixtures and equipment
//!   (the kind and the type from their connectors; a name proposed), Add To and Remove From
//!   System, Select Equipment, Disconnect Equipment, Divide System and System Types (colour,
//!   abbreviation; Duplicate).
//! - **Systems › Check Systems**: Check Duct Systems, Check Pipe Systems and Show Disconnects
//!   toggle markers in the active view (per view, for the session): a "!" marker on a stalk
//!   over each issue's connectors; a red ring on each open connector. The issues are listed in
//!   the status bar as they are counted.
//! - **System Browser** (`F9`): systems by discipline and classification, then system, its
//!   equipment and members; and Unassigned connectors by domain. Selecting a node selects its
//!   elements. It refreshes when the model changes.
//! - The selected elements' connectors are drawn as a ring square to their direction with a
//!   stalk outward, coloured by domain (red when open).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use reviera_core::mep::{self, Domain};
use reviera_core::mep_systems;
use reviera_app::DocumentId;
use reviera_core::ElementId;
use reviera_render::plan::Lines;
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, with};

/// Markers turned on in one view.
#[derive(Debug, Clone, Default)]
pub(crate) struct MepMarks {
    pub duct: bool,
    pub pipe: bool,
    pub circuit: bool,
    /// Show Disconnects, for these domains.
    pub disconnects: Option<Vec<Domain>>,
}

pub(crate) struct SystemBrowser {
    tree: TreeCtrl,
    /// Each node's elements, by its custom data index.
    nodes: Vec<Vec<ElementId>>,
    /// The model state it shows.
    shown: Option<(DocumentId, reviera_core::ModelVersion)>,
    quiet: bool,
}

fn domain_colour(d: Domain) -> [f32; 4] {
    match d {
        Domain::Duct => [0.15, 0.45, 0.90, 1.0],
        Domain::Pipe => [0.10, 0.65, 0.35, 1.0],
        Domain::Electrical => [0.95, 0.60, 0.10, 1.0],
        Domain::CableTray | Domain::Conduit => [0.55, 0.40, 0.75, 1.0],
    }
}

/// A line segment in model coordinates.
type Segment = [[f32; 3]; 2];

const OPEN: [f32; 4] = [0.90, 0.10, 0.10, 1.0];
/// Analytical pipe connections.
const ANALYTICAL: [f32; 4] = [0.10, 0.55, 0.30, 1.0];
/// A circuit's path.
const PATH: [f32; 4] = [0.85, 0.35, 0.05, 1.0];
pub(crate) const WARN: [f32; 4] = [0.95, 0.65, 0.05, 1.0];

/// "duct systems", "pipe systems", "circuits".
fn plural(label: &str) -> String {
    if label == "circuit" { "circuits".into() } else { format!("{label} systems") }
}

/// A dashed line from `a` to `b`.
fn dashed(a: [f64; 3], b: [f64; 3]) -> Vec<Segment> {
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let n = ((l / 0.3).ceil() as usize).max(1);
    (0..n)
        .filter(|k| k % 2 == 0)
        .map(|k| {
            let t0 = k as f64 / n as f64;
            let t1 = ((k + 1) as f64 / n as f64).min(1.0);
            let at = |t: f64| [a[0] + d[0] * t, a[1] + d[1] * t, a[2] + d[2] * t];
            [f32s(at(t0)), f32s(at(t1))]
        })
        .collect()
}

/// A ring of radius `r` at `c` square to `n`, as segments.
fn ring(c: [f64; 3], n: [f64; 3], r: f64) -> Vec<Segment> {
    let a = if n[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] };
    let cross = |u: [f64; 3], v: [f64; 3]| [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    let unit = |v: [f64; 3]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-12);
        v.map(|x| x / l)
    };
    let u = unit(cross(n, a));
    let w = cross(n, u);
    let at = |t: f64| {
        let (s, k) = t.sin_cos();
        [0, 1, 2].map(|i| (c[i] + r * (u[i] * k + w[i] * s)) as f32)
    };
    let steps = 20;
    (0..steps)
        .map(|i| {
            let t0 = std::f64::consts::TAU * i as f64 / steps as f64;
            let t1 = std::f64::consts::TAU * (i + 1) as f64 / steps as f64;
            [at(t0), at(t1)]
        })
        .collect()
}

fn f32s(p: [f64; 3]) -> [f32; 3] {
    p.map(|v| v as f32)
}

/// A "!" marker on a stalk above `p`.
fn marker(p: [f64; 3]) -> Vec<Segment> {
    let top = [p[0], p[1], p[2] + 0.6];
    let mut s = vec![[f32s(p), f32s(top)]];
    // A triangle around a bar and a dot, in the vertical plane through x.
    let (h, w) = (0.32, 0.2);
    let base = [top[0], top[1], top[2]];
    let a = [base[0] - w, base[1], base[2]];
    let b = [base[0] + w, base[1], base[2]];
    let c = [base[0], base[1], base[2] + h];
    s.extend([[f32s(a), f32s(b)], [f32s(b), f32s(c)], [f32s(c), f32s(a)]]);
    s.push([f32s([base[0], base[1], base[2] + 0.1]), f32s([base[0], base[1], base[2] + 0.22])]);
    s.push([f32s([base[0], base[1], base[2] + 0.05]), f32s([base[0], base[1], base[2] + 0.065])]);
    // The same triangle lying flat beside the connector, for plans.
    let (x, y, z) = (p[0] + 0.25, p[1], p[2]);
    let a = [x - w, y - h / 2.0, z];
    let b = [x + w, y - h / 2.0, z];
    let c = [x, y + h / 2.0, z];
    s.extend([[f32s(a), f32s(b)], [f32s(b), f32s(c)], [f32s(c), f32s(a)]]);
    s.push([f32s([x, y - 0.02, z]), f32s([x, y + 0.1, z])]);
    s.push([f32s([x, y - 0.1, z]), f32s([x, y - 0.075, z])]);
    s.push([f32s(p), f32s([x - w / 2.0, y, z])]);
    s
}

fn is_equipment(store: &reviera_core::Store, id: ElementId) -> bool {
    store
        .get(id)
        .filter(|e| reviera_core::mesh_objects::mesh_object_recipe(e).is_some())
        .and_then(|e| reviera_core::mesh_objects::type_of(store, e))
        .is_some_and(|(_, t)| t.category.is_mechanical_equipment())
}

fn unit_label(store: &reviera_core::Store, id: ElementId) -> String {
    let ty = store.get(id).and_then(|e| reviera_core::mesh_objects::type_of(store, e)).and_then(|(t, _)| store.get(t)).and_then(|t| t.name.clone()).unwrap_or_default();
    format!("{ty} ({})", id.0)
}

/// A placeholder outside any system.
const PLACEHOLDER: [f32; 4] = [0.45, 0.3, 0.65, 1.0];

impl Gui {
    /// The placeholder (MEP-DUCT-007) whose line is within a few pixels of (`x`, `y`) in the
    /// active view.
    pub(crate) fn placeholder_at(&self, x: i32, y: i32) -> Option<ElementId> {
        let v = self.active_view()?;
        let store = self.app.document(v.doc)?.doc.store();
        let size = v.panel.get_client_size();
        let (w, h) = (size.width as f32, size.height as f32);
        let (x, y) = (x as f32, y as f32);
        let hidden = self.view_visibility(v).hidden;
        store
            .iter()
            .filter(|e| !hidden.contains(&e.id()))
            .filter_map(|e| {
                let s = reviera_core::mep_runs::segment_recipe(e).filter(|s| s.placeholder)?;
                let a = v.camera.project(glam::Vec3::from(f32s(s.start)), w, h)?;
                let b = v.camera.project(glam::Vec3::from(f32s(s.end)), w, h)?;
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let t = (((x - a.0) * dx + (y - a.1) * dy) / (dx * dx + dy * dy).max(1e-6)).clamp(0.0, 1.0);
                let d = (x - (a.0 + dx * t)).hypot(y - (a.1 + dy * t));
                (d < 5.0).then_some((e.id(), d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// The placeholders joined to `start`, directly or through others, `start` first.
    pub(crate) fn placeholder_network(&self, start: ElementId) -> Vec<ElementId> {
        let Some(store) = self.store_now() else { return vec![start] };
        let partners = mep::partners(&store);
        let mut out = vec![start];
        let mut k = 0;
        while k < out.len() {
            let at = out[k];
            k += 1;
            for ((e, _), list) in &partners {
                if *e != at {
                    continue;
                }
                for (o, _) in list {
                    if !out.contains(o) && reviera_core::mep_placeholders::is_placeholder(&store, *o) {
                        out.push(*o);
                    }
                }
            }
        }
        out
    }

    /// The connector glyphs of the selected elements and the active view's check markers.
    pub(crate) fn mep_lines(&mut self, index: usize) -> Vec<Lines> {
        let Some(store) = self.store_now() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let partners = mep::partners(&store);
        let mut by_colour: HashMap<[u32; 4], (Vec<Segment>, [f32; 4])> = HashMap::new();
        let mut add = |c: [f32; 4], segs: Vec<Segment>| {
            by_colour.entry(c.map(f32::to_bits)).or_insert_with(|| (Vec::new(), c)).0.extend(segs);
        };
        for id in self.selection() {
            let Some(e) = store.get(id) else { continue };
            for p in mep::ports(&store, e) {
                let size = match p.connector.shape {
                    mep::Shape::Round => p.connector.diameter,
                    _ => p.connector.width.max(p.connector.height),
                };
                let r = (size / 2.0).max(0.04);
                let open = !p.connector.surface
                    && p.connector.domain != Domain::Electrical
                    && !partners.contains_key(&(p.element, p.index));
                let colour = if open { OPEN } else { domain_colour(p.connector.domain) };
                let mut segs = ring(p.position, p.direction, r);
                let tip = [0, 1, 2].map(|k| p.position[k] + p.direction[k] * (r + 0.1));
                segs.push([f32s(p.position), f32s(tip)]);
                add(colour, segs);
            }
        }
        // Analytical pipe connections (MEP-PIPE-009): dashed from the connector to where it
        // attaches on the pipe, in every view.
        let hidden = self.views.get(index).map(|v| self.view_visibility(v).hidden).unwrap_or_default();
        for c in reviera_core::mep_analytical::all(&store) {
            if !hidden.contains(&c.id) {
                add(ANALYTICAL, dashed(c.from, c.point));
            }
        }
        // Placeholders (MEP-DUCT-007): a line each, in their system's colour, in every view and
        // detail level.
        let run_colours = mep_systems::element_colors(&store);
        let selected = self.selection();
        for e in store.iter() {
            let Some(s) = reviera_core::mep_runs::segment_recipe(e).filter(|s| s.placeholder) else { continue };
            if hidden.contains(&e.id()) {
                continue;
            }
            let colour = if selected.contains(&e.id()) { reviera_render::HIGHLIGHT } else { run_colours.get(&e.id()).copied().unwrap_or(PLACEHOLDER) };
            out.push(Lines { color: colour, width: 1.5, segments: vec![[f32s(s.start), f32s(s.end)]] });
        }
        // Circuits of the selection: dashed from the panel's connector to each device's.
        let sel = self.selection();
        for c in reviera_core::mep_electrical::circuits(&store) {
            if !sel.iter().any(|s| c.elements.contains(s) || c.panel == Some(*s) || c.id == *s) {
                continue;
            }
            // Its path (MEP-ELEC-004), and straight dashes to devices off it.
            let def = store.get(c.id).and_then(reviera_core::mep_electrical::recipe).and_then(|r| r.path).unwrap_or_default();
            let (path, _) = reviera_core::mep_cables::path(&store, &c, &def, &reviera_core::mep_electrical::settings(&store));
            for w in path.windows(2) {
                add(PATH, dashed(w[0], w[1]));
            }
            let home = path.first().copied();
            let on_path = |q: [f64; 3]| path.iter().any(|p| (p[0] - q[0]).abs() + (p[1] - q[1]).abs() + (p[2] - q[2]).abs() < 1e-6);
            for (e, k) in &c.ports {
                let Some(p) = mep::port(&store, *e, *k) else { continue };
                match home {
                    Some(_) if on_path(p.position) => {}
                    Some(h) => add(domain_colour(Domain::Electrical), dashed(h, p.position)),
                    // No panel: a short home-run arrow up from the device.
                    None => {
                        let up = [p.position[0], p.position[1], p.position[2] + 0.4];
                        let mut segs = vec![[f32s(p.position), f32s(up)]];
                        segs.push([f32s(up), f32s([up[0] - 0.08, up[1], up[2] - 0.1])]);
                        segs.push([f32s(up), f32s([up[0] + 0.08, up[1], up[2] - 0.1])]);
                        add(OPEN, segs);
                    }
                }
            }
        }
        let Some(view) = self.views.get(index) else {
            return Vec::new();
        };
        if let Some(marks) = self.mep_marks.get(&view.id) {
            for (on, kind) in [
                (marks.duct, mep_systems::CheckKind::Duct),
                (marks.pipe, mep_systems::CheckKind::Pipe),
                (marks.circuit, mep_systems::CheckKind::Circuit),
            ] {
                if !on {
                    continue;
                }
                let issues = mep_systems::check(&store, mep_systems::CheckParams { kind, view_id: None, on: true }).unwrap_or_default();
                for i in issues.iter().take(5000) {
                    for (e, k) in &i.connector_refs {
                        if let Some(p) = mep::port(&store, *e, *k) {
                            add(WARN, marker(p.position));
                        }
                    }
                }
            }
            if let Some(domains) = &marks.disconnects {
                for p in mep::open_ports(&store, domains) {
                    let size = p.connector.diameter.max(p.connector.width).max(0.1);
                    let mut segs = ring(p.position, p.direction, size / 2.0 + 0.08);
                    segs.extend(ring(p.position, p.direction, size / 2.0 + 0.12));
                    add(OPEN, segs);
                }
            }
        }
        for (_, (segments, color)) in by_colour {
            out.push(Lines { color, width: 0.5, segments });
        }
        out.extend(self.inspector_lines(index));
        out
    }

    // ---- Components ----

    /// Loads the starter MEP types (Systems › Load MEP Types).
    pub(crate) fn load_mep_types(&mut self) {
        let frame = self.frame;
        match self.run("mep_type.load_starters", json!({})) {
            Ok(v) => frame.set_status_text(
                &format!("{} MEP component types are in the model: Place MEP Component places them", v.as_object().map_or(0, |m| m.len())),
                0,
            ),
            Err(e) => error(&frame, &format!("The MEP types were not loaded:\n{e}")),
        }
    }

    /// Places a component type with connectors (Systems › Place MEP Component).
    pub(crate) fn place_mep_component(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else {
            return frame.set_status_text("Open or create a project first (File > New)", 0);
        };
        let mut types: Vec<(ElementId, String)> = store
            .iter()
            .filter_map(|e| {
                let t = reviera_core::mesh_objects::mesh_object_type(e)?;
                (!t.connectors.is_empty()).then(|| (e.id(), format!("{} ({})", e.name.clone().unwrap_or_default(), t.category.label())))
            })
            .collect();
        if types.is_empty() {
            self.load_mep_types();
            let Some(store) = self.store_now() else { return };
            types = store
                .iter()
                .filter_map(|e| {
                    let t = reviera_core::mesh_objects::mesh_object_type(e)?;
                    (!t.connectors.is_empty()).then(|| (e.id(), format!("{} ({})", e.name.clone().unwrap_or_default(), t.category.label())))
                })
                .collect();
        }
        types.sort_by(|a, b| a.1.cmp(&b.1));
        let labels: Vec<&str> = types.iter().map(|t| t.1.as_str()).collect();
        let d = SingleChoiceDialog::builder(&frame, "Component type:", "Place MEP Component", &labels).build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some((id, label)) = usize::try_from(d.get_selection()).ok().and_then(|k| types.get(k)).cloned() else {
            return;
        };
        let name = label.split(" (").next().unwrap_or(&label).to_owned();
        self.start_place_tool(super::components::Source::Type(id), name);
    }

    /// Systems › Mechanical › Mechanical Control Device: a thermostat, humidistat, sensor or
    /// controller type and its mounting height; each click puts one on the nearest wall face (or
    /// on the work plane away from walls).
    pub(crate) fn place_control_device(&mut self) {
        let names = ["Thermostat", "Humidistat", "Temperature Sensor", "CO2 Sensor", "Humidity Sensor", "Occupancy Sensor", "Pressure Sensor", "DDC Controller"];
        self.place_wall_component(reviera_core::mesh_objects::Category::is_control_device, &names, "Mechanical Control Device", Some(1500.0));
    }

    /// Systems › Plumbing & Piping › Plumbing Fixture (`PX`, MEP-PIPE-006): a fixture type and its
    /// height; each click puts one against the nearest wall face (or on the work plane).
    pub(crate) fn place_plumbing_fixture(&mut self) {
        let names = ["Lavatory", "Water Closet", "Kitchen Sink", "Shower", "Urinal", "Floor Drain"];
        self.place_wall_component(reviera_core::mesh_objects::Category::is_plumbing_fixture, &names, "Plumbing Fixture", Some(0.0));
    }

    /// Systems › Plumbing & Piping › Sprinkler (`SK`, MEP-PIPE-007): a head type; each click puts
    /// a pendent head under the ceiling there (following it), an upright head on its pipe height,
    /// a sidewall head on the nearest wall face. A height overrides the automatic one.
    pub(crate) fn place_sprinkler(&mut self) {
        let names = ["Pendent Sprinkler", "Upright Sprinkler", "Sidewall Sprinkler"];
        self.place_wall_component(|c| c == reviera_core::mesh_objects::Category::Sprinkler, &names, "Sprinkler", None);
    }

    /// A type of the categories `accepts` and a height, then the Place Component tool (clicks
    /// near a wall go on its face).
    /// With no default height, a blank height is automatic (`sprinkler.place` chooses it).
    fn place_wall_component(&mut self, accepts: fn(reviera_core::mesh_objects::Category) -> bool, starters: &[&str], title: &str, default_mm: Option<f64>) {
        self.place_typed_component(accepts, starters, title, default_mm, None);
    }

    /// As [`Self::place_wall_component`], with a hint of its own; `flow` asks for an air
    /// terminal's flow.
    fn place_typed_component(&mut self, accepts: fn(reviera_core::mesh_objects::Category) -> bool, starters: &[&str], title: &str, default_mm: Option<f64>, own: Option<(&str, bool)>) {
        let frame = self.frame;
        let devices = |store: &reviera_core::Store| -> Vec<(ElementId, String)> {
            let mut v: Vec<(ElementId, String)> = store
                .iter()
                .filter_map(|e| {
                    let t = reviera_core::mesh_objects::mesh_object_type(e)?;
                    accepts(t.category).then(|| (e.id(), format!("{} ({})", e.name.clone().unwrap_or_default(), t.category.label())))
                })
                .collect();
            v.sort_by(|a, b| a.1.cmp(&b.1));
            v
        };
        let Some(store) = self.store_now() else {
            return frame.set_status_text("Open or create a project first (File > New)", 0);
        };
        let mut types = devices(&store);
        if types.is_empty() {
            if let Err(e) = self.run("mep_type.load_starters", json!({ "names": starters })) {
                return error(&frame, &e);
            }
            let Some(store) = self.store_now() else { return };
            types = devices(&store);
        }
        let f = super::reinforcement_ui::Form::new(&frame, title);
        let names: Vec<String> = types.iter().map(|t| t.1.clone()).collect();
        let ty = f.choice("Type", &names, 0);
        let height = match default_mm {
            Some(mm) => f.text("Mounting height (mm)", &format!("{mm:.0}"), "Above the level"),
            None if own.is_some() => f.text("Height (mm)", "", "Blank: under the ceiling over each click (following it)"),
            None => f.text("Height (mm)", "", "Blank: under the ceiling (pendent), 3000 (upright), 2450 (sidewall)"),
        };
        let flow = own.is_some_and(|o| o.1).then(|| f.text("Flow (L/s)", "", "Each terminal's preset flow; blank: the type's"));
        let hint = if let Some((h, _)) = own {
            h
        } else if default_mm.is_some() {
            "Click near a wall to put it on that face (it follows the wall); away from walls it stands on the work plane. Spacebar turns it there."
        } else {
            "Click under a ceiling to hang a pendent head from it (it follows the ceiling); sidewall heads go on the nearest wall face."
        };
        let ok = f.show(hint, "Place");
        let k = ty.get_selection().map_or(0, |k| k as usize);
        // Automatic: NaN, which the place tool leaves out.
        let h = height.get_value().trim().replace(',', ".").parse::<f64>().ok().or(default_mm).map_or(f64::NAN, |mm| mm / 1000.0);
        let flow = flow.and_then(|t| t.get_value().trim().replace(',', ".").parse::<f64>().ok()).filter(|v| v.is_finite() && *v >= 0.0).map(|ls| ls / 1000.0);
        f.dialog.destroy();
        let Some((id, label)) = types.get(k).cloned().filter(|_| ok) else { return };
        let name = label.split(" (").next().unwrap_or(&label).to_owned();
        self.start_place_tool(super::components::Source::Type(id), name);
        self.set_place_offset(h);
        self.set_place_flow(flow);
    }

    /// Systems › Air Terminal (`AT`, MEP-DUCT-009): a terminal type, its flow and height; each
    /// click places one, on a duct's bottom face when clicked over a duct.
    pub(crate) fn place_air_terminal(&mut self) {
        let names = ["Supply Diffuser 600", "Return Grille 600"];
        self.place_typed_component(
            |c| c == reviera_core::mesh_objects::Category::AirTerminal,
            &names,
            "Air Terminal",
            Some(2700.0),
            Some(("Click to place a terminal at the height; over a duct it goes on the duct's bottom face, joined to it. Spacebar turns it. Its flow counts toward the room's airflow.", true)),
        );
    }

    /// Systems › Mechanical Equipment (`ME`, MEP-DUCT-009): an air handler, VAV box, fan,
    /// boiler, chiller, pump or radiator type and its height; each click places one.
    pub(crate) fn place_mech_equipment(&mut self) {
        let names = ["Air Handling Unit", "VAV Box", "Exhaust Fan", "Boiler", "Chiller", "Circulation Pump", "Radiator", "Fan Coil Unit"];
        self.place_typed_component(
            reviera_core::mesh_objects::Category::is_mechanical_equipment,
            &names,
            "Mechanical Equipment",
            Some(0.0),
            Some(("Click to place it; Spacebar turns the next copy 90°.", false)),
        );
    }

    /// Systems › Electrical › Electrical Equipment (`EE`, MEP-ELEC-001): panelboards,
    /// switchboards, motor control centres and transformers, on a wall face or free.
    pub(crate) fn place_electrical_equipment(&mut self) {
        let names = ["Distribution Board", "Switchboard", "Transformer"];
        self.place_typed_component(
            reviera_core::mesh_objects::Category::is_electrical_equipment,
            &names,
            "Electrical Equipment",
            Some(0.0),
            Some(("Click near a wall to put it against that face (it follows the wall); away from walls it stands free. Spacebar turns it.", false)),
        );
    }

    /// Systems › Electrical › Device (`DV`, MEP-ELEC-001): receptacles, data and telephone
    /// outlets, switches, alarms and detectors on a wall face.
    pub(crate) fn place_device(&mut self) {
        let names = ["Receptacle", "Data Outlet", "Telephone Outlet", "Light Switch", "Manual Call Point", "Smoke Detector"];
        self.place_typed_component(
            reviera_core::mesh_objects::Category::is_electrical_device,
            &names,
            "Device",
            Some(300.0),
            Some(("Click near a wall to put it on that face at the height (it follows the wall).", false)),
        );
    }

    /// Systems › Electrical › Lighting Fixture (`LF`, MEP-ELEC-001): under the ceiling over each
    /// click, or at a height.
    pub(crate) fn place_lighting_fixture(&mut self) {
        let names = ["Ceiling Luminaire", "Wall Luminaire"];
        self.place_typed_component(
            |c| c == reviera_core::mesh_objects::Category::LightingFixture,
            &names,
            "Lighting Fixture",
            None,
            Some(("Click to hang one under the ceiling there (it follows the ceiling). The room it belongs to is the one its type's calculation point is in.", false)),
        );
    }

    /// Systems › Electrical › Photometry (MEP-ELEC-001): an IES or EULUMDAT file for the
    /// selected lighting fixture's type.
    pub(crate) fn fixture_photometry(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let ty = self.selection().into_iter().find_map(|id| {
            let e = store.get(id)?;
            let (t, mt) = reviera_core::mesh_objects::type_of(&store, e)?;
            (mt.category == reviera_core::mesh_objects::Category::LightingFixture).then_some(t)
        });
        let Some(ty) = ty else {
            return frame.set_status_text("Photometry: select a lighting fixture", 0);
        };
        let d = FileDialog::builder(&frame)
            .with_message("Photometric File")
            .with_wildcard("Photometric files (*.ies;*.ldt)|*.ies;*.IES;*.ldt;*.LDT|All files (*.*)|*.*")
            .with_style(FileDialogStyle::Open | FileDialogStyle::FileMustExist)
            .build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some(file) = d.get_path() else { return };
        match self.run("lighting_fixture_type.set_photometry", json!({ "type_id": ty, "file": file })) {
            Ok(v) => frame.set_status_text(
                &format!(
                    "Photometry: {} lm, peak {:.0} cd",
                    v["lumens"].as_f64().map_or("absolute".to_owned(), |l| format!("{l:.0}")),
                    v["max_candela"].as_f64().unwrap_or(0.0)
                ),
                0,
            ),
            Err(e) => error(&frame, &e),
        }
    }

    /// Systems › Equipment Set (MEP-DUCT-009): the selected units become a set; the chosen ones
    /// are on duty, the rest stand by.
    pub(crate) fn create_equipment_set(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let units: Vec<ElementId> = self.selection().into_iter().filter(|id| is_equipment(&store, *id)).collect();
        if units.len() < 2 {
            return frame.set_status_text("Equipment Set: select two or more pumps, fans or other mechanical equipment", 0);
        }
        let labels: Vec<String> = units.iter().map(|id| unit_label(&store, *id)).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let d = MultiChoiceDialog::builder(&frame, "Duty units (the others stand by):", "Equipment Set", &refs).build();
        d.set_selections(&[0]);
        if d.show_modal() != ID_OK {
            return;
        }
        let duty: Vec<ElementId> = d.get_selections().iter().filter_map(|k| units.get(*k as usize).copied()).collect();
        match self.run("equipment_set.create", json!({ "equipment_ids": units, "duty_ids": duty })) {
            Ok(_) => frame.set_status_text(&format!("Equipment set made: {} on duty, {} standing by", duty.len(), units.len() - duty.len()), 0),
            Err(e) => error(&frame, &e),
        }
    }

    /// Systems › Add to Set and Remove from Set: the selected units join the set one of them is
    /// in (standing by), or leave theirs.
    pub(crate) fn edit_equipment_set(&mut self, add: bool) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let units: Vec<ElementId> = self.selection().into_iter().filter(|id| is_equipment(&store, *id)).collect();
        let sets = reviera_core::mep_equipment::sets(&store);
        let in_set = |id: &ElementId| sets.iter().find(|s| s.duty.contains(id) || s.standby.contains(id)).map(|s| s.id);
        let result = if add {
            let Some(set) = units.iter().find_map(in_set) else {
                return frame.set_status_text("Add to Set: select a unit of the set and the units to add", 0);
            };
            let new: Vec<ElementId> = units.iter().filter(|u| in_set(u).is_none()).copied().collect();
            if new.is_empty() {
                return frame.set_status_text("Add to Set: select units that are in no set too", 0);
            }
            self.run("equipment_set.add", json!({ "set_id": set, "equipment_ids": new }))
        } else {
            let Some(set) = units.iter().find_map(in_set) else {
                return frame.set_status_text("Remove from Set: select units of an equipment set", 0);
            };
            let out: Vec<ElementId> = units.iter().filter(|u| in_set(u) == Some(set)).copied().collect();
            self.run("equipment_set.remove", json!({ "set_id": set, "equipment_ids": out }))
        };
        match result {
            Ok(_) => frame.set_status_text(if add { "Added to the set, standing by" } else { "Removed from the set" }, 0),
            Err(e) => error(&frame, &e),
        }
    }

    /// Systems › Connect Into (`CI`, MEP-DUCT-009): the selected component's open duct or pipe
    /// connector is routed into the selected main.
    pub(crate) fn connect_into(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let sel = self.selection();
        let main = sel.iter().copied().find(|id| store.get(*id).and_then(reviera_core::mep_runs::segment_recipe).is_some_and(|s| !s.placeholder));
        let comp = sel.iter().copied().find(|id| store.get(*id).is_some_and(|e| reviera_core::mesh_objects::mesh_object_recipe(e).is_some()));
        let (Some(main), Some(comp)) = (main, comp) else {
            return frame.set_status_text("Connect Into: select a component and the duct or pipe main to connect it into", 0);
        };
        let domain = store.get(main).and_then(reviera_core::mep_runs::segment_recipe).map(|s| s.domain);
        let partners = mep::partners(&store);
        let open: Vec<mep::Port> = mep::ports(&store, store.get(comp).expect("selected"))
            .into_iter()
            .filter(|p| Some(p.connector.domain) == domain && !partners.contains_key(&(comp, p.index)))
            .collect();
        let index = match open.len() {
            0 => return frame.set_status_text("Connect Into: the component has no open connector of the main's kind", 0),
            1 => open[0].index,
            _ => {
                let labels: Vec<String> = open.iter().map(|p| p.connector.name.clone()).collect();
                let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
                let d = SingleChoiceDialog::builder(&frame, "Connector:", "Connect Into", &refs).build();
                if d.show_modal() != ID_OK {
                    return;
                }
                let Some(p) = usize::try_from(d.get_selection()).ok().and_then(|k| open.get(k)) else { return };
                p.index
            }
        };
        match self.run("mep.connect_into", json!({ "element_id": comp, "connector_index": index, "target_id": main })) {
            Ok(v) => {
                self.refresh_scene(&frame);
                frame.set_status_text(&format!("Connected: {} segment(s), {} fitting(s)", v["segments"].as_array().map_or(0, Vec::len), v["fittings"].as_array().map_or(0, Vec::len)), 0);
            }
            Err(e) => error(&frame, &e),
        }
    }

    /// Systems › Mechanical › Assign Controlled Element: the selected control devices control the
    /// rest of the selection.
    pub(crate) fn assign_controlled(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let sel = self.selection();
        let (devices, others): (Vec<ElementId>, Vec<ElementId>) =
            sel.iter().partition(|id| store.get(**id).is_some_and(|e| reviera_core::mep_controls::is_device(&store, e)));
        if devices.is_empty() {
            return frame.set_status_text("Assign Controlled Element: select a control device and what it controls", 0);
        }
        for d in &devices {
            if let Err(e) = self.run("mech_control_device.assign", json!({ "device_id": d, "controlled_ids": others, "add": !others.is_empty() })) {
                return error(&frame, &e);
            }
        }
        frame.set_status_text(
            &if others.is_empty() {
                "The device controls nothing now".to_owned()
            } else {
                format!("{} device(s) control {} element(s)", devices.len(), others.len())
            },
            0,
        );
    }

    /// Connects the selection's ports that meet (Systems › Connect).
    pub(crate) fn connect_touching(&mut self) {
        let frame = self.frame;
        let ids = self.selection();
        match self.run("mep.connect_touching", json!({ "element_ids": ids })) {
            Ok(v) => {
                let n = v.as_array().map_or(0, Vec::len);
                frame.set_status_text(&format!("{n} connection(s) made"), 0);
                self.refresh_active();
            }
            Err(e) => error(&frame, &format!("Not connected:\n{e}")),
        }
    }

    /// Analytical Connections (MEP-PIPE-009): each open pipe connector of the selected equipment
    /// joined logically to the nearest pipe of its classification within a search radius.
    pub(crate) fn analytical_connections(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let equipment: Vec<ElementId> = self
            .selection()
            .into_iter()
            .filter(|id| store.get(*id).is_some_and(|e| reviera_core::mesh_objects::mesh_object_recipe(e).is_some() && mep::ports(&store, e).iter().any(|p| p.connector.domain == Domain::Pipe)))
            .collect();
        if equipment.is_empty() {
            return frame.set_status_text("Analytical Connections: select equipment with pipe connectors", 0);
        }
        let f = super::reinforcement_ui::Form::new(&frame, "Analytical Connections");
        let radius = f.text("Search radius (mm)", "3000", "Each open connector joins the nearest pipe of its classification within it");
        let ok = f.show("Connectors are joined logically, without modelling the pipe: they count as connected and carry flow in the hydronic analysis.", "Connect");
        let r = radius.get_value().trim().replace(',', ".").parse::<f64>().unwrap_or(3000.0) / 1000.0;
        f.dialog.destroy();
        if !ok {
            return;
        }
        match self.run("analytical_pipe_connection.create_auto", json!({ "equipment_ids": equipment, "radius": r })) {
            Ok(v) => {
                let made = v["created"].as_array().map_or(0, Vec::len);
                let skipped: Vec<&str> = v["skipped"].as_array().into_iter().flatten().filter_map(serde_json::Value::as_str).collect();
                self.refresh_active();
                if skipped.is_empty() {
                    frame.set_status_text(&format!("{made} analytical connection(s) made"), 0);
                } else {
                    error(&frame, &format!("{made} analytical connection(s) made. Not connected:\n{}", skipped.join("\n")));
                }
            }
            Err(e) => error(&frame, &e),
        }
    }

    // ---- Systems ----

    /// The systems, labelled, optionally only those whose classification `keep` accepts.
    fn system_choices(&self, keep: &dyn Fn(&mep_systems::SystemInfo) -> bool) -> Vec<(ElementId, String)> {
        let Some(store) = self.store_now() else { return Vec::new() };
        mep_systems::systems(&store)
            .into_iter()
            .filter(|s| keep(s))
            .map(|s| (s.id, format!("{} ({}, {} elements)", s.name, s.type_name, s.elements.len())))
            .collect()
    }

    fn choose_system(&self, title: &str, keep: &dyn Fn(&mep_systems::SystemInfo) -> bool) -> Option<ElementId> {
        let frame = self.frame;
        let systems = self.system_choices(keep);
        if systems.is_empty() {
            frame.set_status_text(&format!("{title}: there is no such system yet (Create System makes one)"), 0);
            return None;
        }
        let labels: Vec<&str> = systems.iter().map(|s| s.1.as_str()).collect();
        let d = SingleChoiceDialog::builder(&frame, "System:", title, &labels).build();
        if d.show_modal() != ID_OK {
            return None;
        }
        usize::try_from(d.get_selection()).ok().and_then(|k| systems.get(k)).map(|s| s.0)
    }

    /// Makes a system of the selection (Systems › Create System).
    pub(crate) fn create_system(&mut self) {
        let frame = self.frame;
        let ids = self.selection();
        let Some(store) = self.store_now() else { return };
        // The classifications the selection's connectors offer.
        let mut classes: Vec<mep::Classification> = Vec::new();
        for id in &ids {
            if let Some(e) = store.get(*id) {
                for p in mep::ports(&store, e) {
                    if let Some(c) = p.connector.classification
                        && !c.is_wildcard()
                        && !classes.contains(&c)
                    {
                        classes.push(c);
                    }
                }
            }
        }
        if classes.is_empty() {
            return frame.set_status_text("Create System: select terminals, fixtures or equipment with duct or pipe connectors first", 0);
        }
        let types: Vec<mep_systems::SystemType> = mep_systems::system_types(&store)
            .into_iter()
            .filter(|t| classes.contains(&t.classification))
            .collect();
        let f = super::reinforcement_ui::Form::new(&frame, "Create System");
        let labels: Vec<String> = types.iter().map(|t| format!("{} ({})", t.name, t.classification.label())).collect();
        let choice = f.choice("System type", &labels, 0);
        let name = f.text("Name", "", "Leave empty for the type's name and the next number");
        if !f.show(&format!("{} element(s) join by their connectors of the type's classification", ids.len()), "Create") {
            f.dialog.destroy();
            return;
        }
        let t = types[choice.get_selection().map_or(0, |i| i as usize).min(types.len() - 1)].clone();
        let n = name.get_value();
        f.dialog.destroy();
        let kind = if t.classification.is_air() { "duct" } else { "piping" };
        // Elements without a connector of the type's classification are left out and listed.
        let mut args = json!({ "kind": kind, "element_ids": ids, "system_type_id": t.id, "skip_incompatible": true });
        if !n.trim().is_empty() {
            args["name"] = json!(n.trim());
        }
        match self.app.execute("system.create", args, &reviera_core::Caller::user()) {
            Ok(o) => {
                let name = self
                    .store_now()
                    .and_then(|s| s.get(ElementId(o.result.as_u64().unwrap_or(0))).and_then(|e| e.name.clone()))
                    .unwrap_or_default();
                self.refresh_scene(&frame);
                match o.warnings.iter().find(|w| w.id == "system.skipped") {
                    Some(w) => error(&frame, &format!("Made the system {name}.\n\n{}", w.message)),
                    None => frame.set_status_text(&format!("Made the system {name}: Select Equipment sets what serves it"), 0),
                }
            }
            Err(e) => error(&frame, &format!("No system was made:\n{}", e.message)),
        }
    }

    /// Adds the selection to a system, or takes it out (Systems › Add To / Remove From System).
    pub(crate) fn system_members(&mut self, add: bool) {
        let frame = self.frame;
        let ids = self.selection();
        if ids.is_empty() {
            return frame.set_status_text("Select the elements first", 0);
        }
        let title = if add { "Add To System" } else { "Remove From System" };
        let keep = |s: &mep_systems::SystemInfo| add || ids.iter().any(|i| s.elements.contains(i));
        let Some(system) = self.choose_system(title, &keep) else { return };
        let command = if add { "system.add" } else { "system.remove" };
        match self.run(command, json!({ "system_id": system, "element_ids": ids })) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text(if add { "Added to the system" } else { "Removed from the system" }, 0);
            }
            Err(e) => error(&frame, &format!("{title}:\n{e}")),
        }
    }

    /// Sets the selected element as a system's equipment (Systems › Select Equipment).
    pub(crate) fn select_system_equipment(&mut self) {
        let frame = self.frame;
        let ids = self.selection();
        let [eq] = ids[..] else {
            return frame.set_status_text("Select Equipment: select one piece of equipment first", 0);
        };
        let Some(system) = self.choose_system("Select Equipment", &|_| true) else { return };
        match self.run("system.select_equipment", json!({ "system_id": system, "equipment_id": eq })) {
            Ok(_) => {
                self.refresh_scene(&frame);
                frame.set_status_text("The equipment serves the system", 0);
            }
            Err(e) => error(&frame, &format!("Select Equipment:\n{e}")),
        }
    }

    /// Systems › Disconnect Equipment and Divide System.
    pub(crate) fn system_action(&mut self, command: &str, title: &str) {
        let frame = self.frame;
        let sel = self.selection();
        let keep = |s: &mep_systems::SystemInfo| sel.is_empty() || sel.iter().any(|i| s.elements.contains(i));
        let Some(system) = self.choose_system(title, &keep) else { return };
        match self.run(command, json!({ "system_id": system })) {
            Ok(v) => {
                self.refresh_scene(&frame);
                let text = match v.as_array() {
                    Some(parts) => format!("Divided into {} systems", parts.len()),
                    None => format!("{title}: done"),
                };
                frame.set_status_text(&text, 0);
            }
            Err(e) => error(&frame, &format!("{title}:\n{e}")),
        }
    }

    /// System types: colour and abbreviation, or Duplicate (Systems › System Types).
    pub(crate) fn edit_system_types(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let types = mep_systems::system_types(&store);
        let labels: Vec<String> = types.iter().map(|t| format!("{} ({}, {})", t.name, t.abbreviation, t.classification.label())).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let d = SingleChoiceDialog::builder(&frame, "System type:", "System Types", &refs).build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some(t) = usize::try_from(d.get_selection()).ok().and_then(|k| types.get(k)).cloned() else { return };
        let f = super::reinforcement_ui::Form::new(&frame, "System Type");
        let action = f.choice("Change", &["This type".to_owned(), "A duplicate of it".to_owned()], 0);
        let name = f.text("Name", &t.name, "The duplicate's name, or this type's new name");
        let abbr = f.text("Abbreviation", &t.abbreviation, "Shown in tags and the System Browser");
        let colour = f.check("Colour", "Choose a new colour after OK", false);
        if !f.show(&format!("{}: the colour draws every member of its systems", t.classification.label()), "OK") {
            f.dialog.destroy();
            return;
        }
        let duplicate = action.get_selection() == Some(1);
        let (name, abbr, pick) = (name.get_value(), abbr.get_value(), colour.is_checked());
        f.dialog.destroy();
        let mut params = json!({ "abbreviation": abbr.trim() });
        if pick {
            let [r, g, b, _] = t.color.map(|c| (c * 255.0).round() as u8);
            let cd = ColourDialog::builder(&frame)
                .with_title("System Type Colour")
                .with_initial_colour(Colour::new(r, g, b, 255))
                .build();
            if cd.show_modal() == ID_OK
                && let Some(c) = cd.get_colour()
            {
                params["color"] = json!([c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0, 1.0]);
            }
        }
        let result = if duplicate {
            params["classification"] = json!(t.classification);
            params["name"] = json!(name.trim());
            self.run("system_type.create", params).map(|_| ())
        } else {
            if name.trim() != t.name {
                params["name"] = json!(name.trim());
            }
            params["type_id"] = json!(t.id);
            self.run("system_type.set", params).map(|_| ())
        };
        match result {
            Ok(()) => {
                self.refresh_scene(&frame);
                frame.set_status_text("System type saved", 0);
            }
            Err(e) => error(&frame, &format!("The system type was not saved:\n{e}")),
        }
    }

    // ---- Check Systems ----

    /// Turns a check's markers on or off in the active view.
    pub(crate) fn toggle_mep_check(&mut self, kind: mep_systems::CheckKind) {
        let frame = self.frame;
        let Some(view) = self.active else { return };
        let Some(store) = self.store_now() else { return };
        let marks = self.mep_marks.entry(view).or_default();
        let (on, label) = match kind {
            mep_systems::CheckKind::Duct => {
                marks.duct = !marks.duct;
                (marks.duct, "duct")
            }
            mep_systems::CheckKind::Pipe => {
                marks.pipe = !marks.pipe;
                (marks.pipe, "pipe")
            }
            mep_systems::CheckKind::Circuit => {
                marks.circuit = !marks.circuit;
                (marks.circuit, "circuit")
            }
        };
        if on {
            let issues = mep_systems::check(&store, mep_systems::CheckParams { kind, view_id: None, on }).unwrap_or_default();
            let text = match issues.first() {
                None => format!("Check {}: no issues", plural(label)),
                Some(i) => format!("Check {}: {} issue(s); first: {}", plural(label), issues.len(), i.message),
            };
            frame.set_status_text(&text, 0);
        } else {
            frame.set_status_text(&format!("Check {} off", plural(label)), 0);
        }
        self.refresh_active();
    }

    /// Show Disconnects: chooses the domains, or turns the markers off.
    pub(crate) fn show_disconnects(&mut self) {
        let frame = self.frame;
        let Some(view) = self.active else { return };
        if self.mep_marks.get(&view).is_some_and(|m| m.disconnects.is_some()) {
            if let Some(m) = self.mep_marks.get_mut(&view) {
                m.disconnects = None;
            }
            frame.set_status_text("Show Disconnects off", 0);
            return self.refresh_active();
        }
        let f = super::reinforcement_ui::Form::new(&frame, "Show Disconnects");
        let boxes: Vec<(Domain, CheckBox)> = [Domain::Duct, Domain::Pipe, Domain::CableTray, Domain::Conduit]
            .into_iter()
            .map(|d| (d, f.check(d.label(), "Show open connectors", true)))
            .collect();
        if !f.show("Open connectors are marked with a red ring until they are connected", "Show") {
            f.dialog.destroy();
            return;
        }
        let domains: Vec<Domain> = boxes.iter().filter(|(_, b)| b.is_checked()).map(|(d, _)| *d).collect();
        f.dialog.destroy();
        let n = self.store_now().map_or(0, |s| mep::open_ports(&s, &domains).len());
        self.mep_marks.entry(view).or_default().disconnects = Some(domains);
        frame.set_status_text(&format!("Show Disconnects: {n} open connector(s)"), 0);
        self.refresh_active();
    }

    // ---- System Browser ----

    /// Shows or hides the System Browser (F9), making it on first use.
    pub(crate) fn toggle_system_browser(gui: &Rc<RefCell<Gui>>) {
        if gui.borrow().system_browser.is_some() {
            with(gui, |g| {
                g.toggle_pane("system_browser");
                g.rebuild_system_browser();
            });
            return;
        }
        let (frame, aui) = {
            let g = gui.borrow();
            (g.frame, g.aui)
        };
        let tree = TreeCtrl::builder(&frame)
            .with_style(TreeCtrlStyle::Default | TreeCtrlStyle::HasButtons | TreeCtrlStyle::LinesAtRoot | TreeCtrlStyle::HideRoot)
            .build();
        aui.add_pane_with_info(
            &tree,
            AuiPaneInfo::new()
                .with_name("system_browser")
                .with_caption("System Browser")
                .right()
                .layer(1)
                .best_size(320, 500)
                .min_size(220, 160)
                .close_button(true)
                .floatable(true),
        );
        aui.update();
        {
            let gui = gui.clone();
            tree.on_selection_changed(move |e| {
                if let Some(item) = e.get_item()
                    && let Ok(mut g) = gui.try_borrow_mut()
                {
                    g.system_browser_selected(&item);
                }
            });
        }
        with(gui, |g| {
            g.system_browser = Some(SystemBrowser { tree, nodes: Vec::new(), shown: None, quiet: false });
            g.rebuild_system_browser();
        });
    }

    /// Refreshes the System Browser when the model has changed since it was filled.
    pub(crate) fn system_browser_idle(&mut self) {
        let Some(b) = &self.system_browser else { return };
        if !self.aui.is_pane_shown("system_browser") {
            return;
        }
        let now = self.doc.and_then(|d| Some((d, self.app.document(d)?.doc.version())));
        if now != b.shown {
            self.rebuild_system_browser();
        }
    }

    fn rebuild_system_browser(&mut self) {
        let now = self.doc.and_then(|d| Some((d, self.app.document(d)?.doc.version())));
        let store = self.store_now();
        let Some(b) = &mut self.system_browser else { return };
        b.shown = now;
        b.quiet = true;
        let t = b.tree;
        t.delete_all_items();
        b.nodes.clear();
        let Some(store) = store else {
            b.quiet = false;
            return;
        };
        let Some(root) = t.add_root("Systems", None, None) else {
            b.quiet = false;
            return;
        };
        // A copy by its own name, else its type's, with its id.
        let label = |id: ElementId| {
            store
                .get(id)
                .map(|e| {
                    let name = e
                        .name
                        .clone()
                        .filter(|n| !n.is_empty())
                        .or_else(|| {
                            let (t, _) = reviera_core::mesh_objects::type_of(&store, e)?;
                            store.get(t)?.name.clone()
                        })
                        .unwrap_or_else(|| e.class.trim_start_matches("Ifc").to_owned());
                    format!("{name} {}", id.0)
                })
                .unwrap_or_default()
        };
        let push = |nodes: &mut Vec<Vec<ElementId>>, parent: &TreeItemId, text: &str, ids: Vec<ElementId>| {
            nodes.push(ids);
            t.append_item_with_data(parent, text, (nodes.len() - 1) as u64, None, None)
        };
        let systems = mep_systems::systems(&store);
        for (discipline, air) in [("Mechanical", true), ("Piping", false)] {
            let mine: Vec<&mep_systems::SystemInfo> = systems.iter().filter(|s| s.classification.is_air() == air).collect();
            if mine.is_empty() {
                continue;
            }
            let all: Vec<ElementId> = mine.iter().flat_map(|s| s.elements.iter().copied()).collect();
            let Some(d) = push(&mut b.nodes, &root, &format!("{discipline} ({})", mine.len()), all) else { continue };
            let mut classes: Vec<mep::Classification> = mine.iter().map(|s| s.classification).collect();
            classes.sort();
            classes.dedup();
            for c in classes {
                let of: Vec<&&mep_systems::SystemInfo> = mine.iter().filter(|s| s.classification == c).collect();
                let all: Vec<ElementId> = of.iter().flat_map(|s| s.elements.iter().copied()).collect();
                let Some(ci) = push(&mut b.nodes, &d, c.label(), all) else { continue };
                for s in of {
                    let text = format!("{} ({}, {})", s.name, s.type_name, s.elements.len());
                    let Some(si) = push(&mut b.nodes, &ci, &text, s.elements.clone()) else { continue };
                    if let Some(eq) = s.equipment {
                        push(&mut b.nodes, &si, &format!("Equipment: {}", label(eq)), vec![eq]);
                    } else {
                        push(&mut b.nodes, &si, "No equipment", Vec::new());
                    }
                    for e in s.elements.iter().filter(|e| Some(**e) != s.equipment) {
                        push(&mut b.nodes, &si, &label(*e), vec![*e]);
                    }
                    t.expand(&si);
                }
                t.expand(&ci);
            }
            t.expand(&d);
        }
        // Electrical: panels, their circuits and devices; circuits with no panel.
        let circuits = reviera_core::mep_electrical::circuits(&store);
        if !circuits.is_empty() || store.iter().any(|e| reviera_core::mep_electrical::is_panel(&store, e)) {
            let all: Vec<ElementId> = circuits.iter().flat_map(|c| c.elements.iter().copied()).collect();
            if let Some(el) = push(&mut b.nodes, &root, &format!("Electrical ({})", circuits.len()), all) {
                let mut panels: Vec<(Option<ElementId>, String)> = store
                    .iter()
                    .filter(|e| reviera_core::mep_electrical::is_panel(&store, e))
                    .map(|e| (Some(e.id()), reviera_core::mep_electrical::panel_name(e)))
                    .collect();
                panels.push((None, "No panel".into()));
                for (pid, pname) in panels {
                    let on: Vec<&reviera_core::mep_electrical::CircuitInfo> = circuits.iter().filter(|c| c.panel == pid).collect();
                    if pid.is_none() && on.is_empty() {
                        continue;
                    }
                    let mut ids: Vec<ElementId> = pid.into_iter().collect();
                    ids.extend(on.iter().flat_map(|c| c.elements.iter().copied()));
                    let load: f64 = on.iter().map(|c| c.load).sum();
                    let Some(pi) = push(&mut b.nodes, &el, &format!("{pname} ({} circuits, {:.1} kVA)", on.len(), load / 1000.0), ids) else { continue };
                    for c in on {
                        let num = if c.name.is_empty() { "(no number)".to_owned() } else { c.name.clone() };
                        let text = format!("{num}: {} {:.0} V {}P, {:.0} VA", c.kind.label(), c.voltage, c.poles, c.load);
                        let Some(ci) = push(&mut b.nodes, &pi, &text, c.elements.clone()) else { continue };
                        for e in &c.elements {
                            push(&mut b.nodes, &ci, &label(*e), vec![*e]);
                        }
                    }
                    t.expand(&pi);
                }
                t.expand(&el);
            }
        }
        // Zones (MEP-ZONE-001): under what serves them, each with its spaces.
        let zones = reviera_core::system_zones::infos(&store);
        if !zones.is_empty() {
            let all: Vec<ElementId> = zones.iter().flat_map(|z| std::iter::once(z.id).chain(z.spaces.iter().copied())).collect();
            if let Some(zi) = push(&mut b.nodes, &root, &format!("Zones ({})", zones.len()), all) {
                let mut servers: Vec<Option<ElementId>> = zones.iter().map(|z| z.served_by).collect();
                servers.sort();
                servers.dedup();
                for srv in servers {
                    let of: Vec<&reviera_core::system_zones::ZoneInfo> = zones.iter().filter(|z| z.served_by == srv).collect();
                    let text = match srv {
                        Some(id) => label(id),
                        None => "Unassigned".into(),
                    };
                    let mut ids: Vec<ElementId> = srv.into_iter().collect();
                    ids.extend(of.iter().map(|z| z.id));
                    let Some(si) = push(&mut b.nodes, &zi, &text, ids) else { continue };
                    for z in of {
                        let Some(zn) = push(&mut b.nodes, &si, &format!("{} ({} spaces)", z.name, z.spaces.len()), vec![z.id]) else { continue };
                        for (sp, n) in z.spaces.iter().zip(&z.space_numbers) {
                            push(&mut b.nodes, &zn, &format!("Space {n}"), vec![*sp]);
                        }
                    }
                    t.expand(&si);
                }
                t.expand(&zi);
            }
        }
        let unassigned = mep_systems::unassigned(&store);
        let on_circuit = reviera_core::mep_electrical::port_circuits(&store);
        let electrical: Vec<mep::Port> = mep::all_ports(&store)
            .into_iter()
            .filter(|p| p.connector.domain == Domain::Electrical && p.connector.flow_direction != mep::FlowDirection::Out)
            .filter(|p| !on_circuit.contains_key(&(p.element, p.index)))
            .collect();
        let mut by_domain: Vec<(Domain, Vec<ElementId>)> = Vec::new();
        for p in unassigned.iter().chain(&electrical) {
            match by_domain.iter_mut().find(|(d, _)| *d == p.connector.domain) {
                Some((_, v)) => {
                    if !v.contains(&p.element) {
                        v.push(p.element);
                    }
                }
                None => by_domain.push((p.connector.domain, vec![p.element])),
            }
        }
        let all: Vec<ElementId> = by_domain.iter().flat_map(|(_, v)| v.iter().copied()).collect();
        if let Some(u) = push(&mut b.nodes, &root, &format!("Unassigned ({})", all.len()), all) {
            for (d, ids) in by_domain {
                if let Some(di) = push(&mut b.nodes, &u, &format!("{} ({})", d.label(), ids.len()), ids.clone()) {
                    for e in ids {
                        push(&mut b.nodes, &di, &label(e), vec![e]);
                    }
                }
            }
            t.expand(&u);
        }
        b.quiet = false;
    }

    /// Selecting a node selects its elements.
    fn system_browser_selected(&mut self, item: &TreeItemId) {
        let Some(b) = &self.system_browser else { return };
        if b.quiet {
            return;
        }
        let Some(ids) = b
            .tree
            .get_custom_data(item)
            .and_then(|d| d.downcast_ref::<u64>().copied())
            .and_then(|i| b.nodes.get(i as usize))
            .cloned()
        else {
            return;
        };
        if ids.is_empty() {
            return;
        }
        if self.run("selection.set", json!({ "ids": ids })).is_ok() {
            self.selection_changed();
            let n = ids.len();
            self.frame.set_status_text(&format!("{n} element(s) selected"), 0);
            self.refresh_active();
        }
    }
}

fn error(frame: &Frame, message: &str) {
    let d = MessageDialog::builder(frame, message, "BIM Reviera")
        .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconWarning)
        .build();
    d.show_modal();
}
